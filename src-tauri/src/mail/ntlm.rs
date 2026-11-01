//! NTLMv2 kimlik doğrulaması (Exchange/IIS için; Kerberos kullanılamadığında).

use hmac::{Hmac, Mac};
use md4::{Digest, Md4};
use md5::Md5;

type HmacMd5 = Hmac<Md5>;

const SIG: &[u8; 8] = b"NTLMSSP\0";
const FLAGS: u32 = 0x0000_0001 // UNICODE
    | 0x0000_0004 // REQUEST_TARGET
    | 0x0000_0200 // NTLM
    | 0x0000_8000 // ALWAYS_SIGN
    | 0x0008_0000 // EXTENDED_SESSIONSECURITY
    | 0x0080_0000 // TARGET_INFO
    | 0x2000_0000 // 128
    | 0x8000_0000; // 56

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut m = <HmacMd5 as Mac>::new_from_slice(key).expect("hmac");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

/// Tip 1 (negotiate) iletisi
pub fn negotiate() -> Vec<u8> {
    let mut v = Vec::with_capacity(32);
    v.extend_from_slice(SIG);
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&FLAGS.to_le_bytes());
    v.extend_from_slice(&[0u8; 16]); // etki alanı + iş istasyonu (boş)
    v
}

pub struct Challenge {
    pub flags: u32,
    pub server_challenge: [u8; 8],
    pub target_info: Vec<u8>,
}

/// Tip 2 (challenge) iletisini çözer.
pub fn parse_challenge(data: &[u8]) -> Option<Challenge> {
    if data.len() < 32 || &data[..8] != SIG || u32::from_le_bytes(data[8..12].try_into().ok()?) != 2 {
        return None;
    }
    let flags = u32::from_le_bytes(data[20..24].try_into().ok()?);
    let mut server_challenge = [0u8; 8];
    server_challenge.copy_from_slice(&data[24..32]);
    let mut target_info = Vec::new();
    if data.len() >= 48 {
        let len = u16::from_le_bytes([data[40], data[41]]) as usize;
        let off = u32::from_le_bytes(data[44..48].try_into().ok()?) as usize;
        if off + len <= data.len() {
            target_info = data[off..off + len].to_vec();
        }
    }
    Some(Challenge { flags, server_challenge, target_info })
}

fn av_timestamp(target_info: &[u8]) -> Option<[u8; 8]> {
    let mut i = 0;
    while i + 4 <= target_info.len() {
        let id = u16::from_le_bytes([target_info[i], target_info[i + 1]]);
        let len = u16::from_le_bytes([target_info[i + 2], target_info[i + 3]]) as usize;
        if id == 0 {
            break;
        }
        if id == 7 && len == 8 && i + 12 <= target_info.len() {
            return target_info[i + 4..i + 12].try_into().ok();
        }
        i += 4 + len;
    }
    None
}

/// "ETKIALANI\kullanici" veya "kullanici@etkialani" biçimini ayırır.
pub fn split_user(login: &str) -> (String, String) {
    if let Some((d, u)) = login.split_once('\\') {
        (u.to_string(), d.to_string())
    } else {
        (login.to_string(), String::new())
    }
}

/// Tip 3 (authenticate) iletisi
pub fn authenticate(ch: &Challenge, login: &str, password: &str, workstation: &str) -> Vec<u8> {
    authenticate_with(ch, login, password, workstation, rand8(), None)
}

fn rand8() -> [u8; 8] {
    let u = uuid::Uuid::new_v4();
    u.as_bytes()[..8].try_into().unwrap()
}

fn authenticate_with(ch: &Challenge, login: &str, password: &str, workstation: &str, client_nonce: [u8; 8], now: Option<[u8; 8]>) -> Vec<u8> {
    let (user, domain) = split_user(login);
    let nt_hash: [u8; 16] = Md4::digest(utf16(password)).into();
    let ntowf = hmac_md5(&nt_hash, &[&utf16(&format!("{}{}", user.to_uppercase(), domain))]);
    let ts = now.or_else(|| av_timestamp(&ch.target_info)).unwrap_or_else(|| {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        ((secs.as_nanos() / 100) as u64 + 116_444_736_000_000_000).to_le_bytes()
    });
    let mut temp = vec![1u8, 1, 0, 0, 0, 0, 0, 0];
    temp.extend_from_slice(&ts);
    temp.extend_from_slice(&client_nonce);
    temp.extend_from_slice(&[0u8; 4]);
    temp.extend_from_slice(&ch.target_info);
    temp.extend_from_slice(&[0u8; 4]);
    let proof = hmac_md5(&ntowf, &[&ch.server_challenge, &temp]);
    let mut nt = proof.to_vec();
    nt.extend_from_slice(&temp);
    let mut lm = hmac_md5(&ntowf, &[&ch.server_challenge, &client_nonce]).to_vec();
    lm.extend_from_slice(&client_nonce);

    let fields: [Vec<u8>; 6] = [lm, nt, utf16(&domain), utf16(&user), utf16(workstation), Vec::new()];
    let header_len = 64usize;
    let mut out = Vec::new();
    out.extend_from_slice(SIG);
    out.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = header_len;
    let mut payload = Vec::new();
    for f in &fields {
        out.extend_from_slice(&(f.len() as u16).to_le_bytes());
        out.extend_from_slice(&(f.len() as u16).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += f.len();
        payload.extend_from_slice(f);
    }
    let flags = FLAGS & ch.flags | 0x0000_0001 | 0x0000_0200;
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MS-NLMP 4.2.4 NTLMv2 test vektörü (NTProofStr)
    #[test]
    fn ntlmv2_vector() {
        let target_info = [
            0x02u8, 0x00, 0x0c, 0x00, 0x44, 0x00, 0x6f, 0x00, 0x6d, 0x00, 0x61, 0x00, 0x69, 0x00, 0x6e, 0x00, 0x01, 0x00, 0x0c,
            0x00, 0x53, 0x00, 0x65, 0x00, 0x72, 0x00, 0x76, 0x00, 0x65, 0x00, 0x72, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let ch = Challenge { flags: 0xe28a8233, server_challenge: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef], target_info: target_info.to_vec() };
        let msg = authenticate_with(&ch, "Domain\\User", "Password", "COMPUTER", [0xaa; 8], Some([0; 8]));
        // NT yanıtının ilk 16 baytı NTProofStr'dir
        let nt_off = u32::from_le_bytes(msg[24..28].try_into().unwrap()) as usize;
        assert_eq!(
            &msg[nt_off..nt_off + 16],
            &[0x68, 0xcd, 0x0a, 0xb8, 0x51, 0xe5, 0x1c, 0x96, 0xaa, 0xbc, 0x92, 0x7b, 0xeb, 0xef, 0x6a, 0x1c]
        );
        let c = parse_challenge(&{
            let mut v = SIG.to_vec();
            v.extend_from_slice(&2u32.to_le_bytes());
            v.extend_from_slice(&[0; 8]);
            v.extend_from_slice(&0x8201u32.to_le_bytes());
            v.extend_from_slice(&[9; 8]);
            v.extend_from_slice(&[0; 8]);
            v.extend_from_slice(&(4u16).to_le_bytes());
            v.extend_from_slice(&(4u16).to_le_bytes());
            v.extend_from_slice(&48u32.to_le_bytes());
            v.extend_from_slice(&[0, 0, 0, 0]);
            v
        })
        .unwrap();
        assert_eq!(c.server_challenge, [9; 8]);
        assert_eq!(c.target_info, vec![0, 0, 0, 0]);
    }
}
