//! TNEF (winmail.dat, application/ms-tnef) çözücü.
//!
//! Outlook "zengin metin" iletilerinde ekleri, gövdeyi ve alıcı bilgisini (MAPI
//! özellikleri) winmail.dat içine paketler. Outlook bunu açıp gösterir; burada da ekler
//! ayrı dosyalar olarak, gövde (HTML/RTF/düz metin) ve alıcılar okunur.

use std::collections::HashMap;

const SIGNATURE: u32 = 0x223E_9F78;
const LVL_ATTACHMENT: u8 = 2;

const ATT_BODY: u32 = 0x0002_800C;
const ATT_OEM_CODEPAGE: u32 = 0x0006_9007;
const ATT_MAPI_PROPS: u32 = 0x0006_9003;
const ATT_RECIP_TABLE: u32 = 0x0006_9004;
const ATT_ATTACH_REND_DATA: u32 = 0x0006_9002;
const ATT_ATTACH_TITLE: u32 = 0x0001_8010;
const ATT_ATTACH_DATA: u32 = 0x0006_800F;
const ATT_ATTACHMENT: u32 = 0x0006_9005;

#[derive(Clone, Debug)]
enum Val {
    Str(String),
    Bin(Vec<u8>),
    Int(i64),
}

type Props = HashMap<u16, Val>;

#[derive(Default, Debug)]
pub struct TnefAttachment {
    pub name: String,
    pub mime: String,
    pub cid: String,
    pub data: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct Tnef {
    pub attachments: Vec<TnefAttachment>,
    pub text: String,
    pub html: String,
    /// (ad, adres, tür: 1 Kime, 2 Bilgi, 3 Gizli)
    pub recipients: Vec<(String, String, i64)>,
}

pub fn is_tnef(data: &[u8]) -> bool {
    data.len() >= 6 && u32::from_le_bytes([data[0], data[1], data[2], data[3]]) == SIGNATURE
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn pad4(&mut self, n: usize) {
        let r = (4 - n % 4) % 4;
        self.p = (self.p + r).min(self.b.len());
    }
}

fn utf16(b: &[u8]) -> String {
    let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&u).trim_end_matches('\0').to_string()
}

/// MAPI özellik listesi (TNEF kodlaması: sayı + [tür, kimlik, (adlı), değer(ler)])
fn read_props(r: &mut Rd, codepage: u32) -> Option<Props> {
    let count = r.u32()?;
    let mut out = Props::new();
    for _ in 0..count.min(10_000) {
        let ty = r.u16()?;
        let id = r.u16()?;
        if id >= 0x8000 {
            // Adlı özellik: GUID + tür (0: sayı, 1: ad)
            r.take(16)?;
            if r.u32()? == 1 {
                let n = r.u32()? as usize;
                r.take(n)?;
                r.pad4(n);
            } else {
                r.u32()?;
            }
        }
        let multi = ty & 0x1000 != 0;
        let base = ty & 0x0FFF;
        let var = matches!(base, 0x001E | 0x001F | 0x0102 | 0x000D);
        let n = if multi || var { r.u32()? as usize } else { 1 };
        let mut last: Option<Val> = None;
        for _ in 0..n.min(100_000) {
            let v = match base {
                0x0002 => {
                    let v = r.u16()? as i16 as i64;
                    r.take(2)?;
                    Val::Int(v)
                }
                0x0003 | 0x000A | 0x000B | 0x0004 => Val::Int(r.u32()? as i32 as i64),
                0x0005 | 0x0006 | 0x0007 | 0x0014 | 0x0040 => {
                    let s = r.take(8)?;
                    Val::Int(i64::from_le_bytes(s.try_into().ok()?))
                }
                0x0048 => Val::Bin(r.take(16)?.to_vec()),
                0x001E | 0x001F | 0x0102 | 0x000D => {
                    let len = r.u32()? as usize;
                    let d = r.take(len)?;
                    r.pad4(len);
                    match base {
                        0x001F => Val::Str(utf16(d)),
                        0x001E => Val::Str(crate::msgfile::codepage_encoding(codepage).decode(d).0.trim_end_matches('\0').to_string()),
                        // Gömülü nesne: başta 16 baytlık arabirim kimliği
                        0x000D => Val::Bin(d.get(16..).unwrap_or_default().to_vec()),
                        _ => Val::Bin(d.to_vec()),
                    }
                }
                _ => return Some(out), // bilinmeyen tür: kalan okunamaz
            };
            last = Some(v);
        }
        if let (Some(v), false) = (last, multi) {
            out.insert(id, v);
        }
    }
    Some(out)
}

fn s(p: &Props, ids: &[u16]) -> String {
    ids.iter()
        .find_map(|id| match p.get(id) {
            Some(Val::Str(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn parse(data: &[u8]) -> Option<Tnef> {
    if !is_tnef(data) {
        return None;
    }
    let mut r = Rd { b: data, p: 6 };
    let mut out = Tnef::default();
    let mut codepage = 1254u32;
    let mut msg_props = Props::new();
    let mut cur: Option<TnefAttachment> = None;
    while r.p < data.len() {
        let Some(level) = r.u8() else { break };
        let Some(id) = r.u32() else { break };
        let Some(len) = r.u32() else { break };
        let Some(body) = r.take(len as usize) else { break };
        let _ = r.u16(); // sağlama toplamı
        match id {
            ATT_OEM_CODEPAGE if body.len() >= 4 => {
                codepage = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            }
            ATT_BODY if level != LVL_ATTACHMENT => {
                out.text = crate::msgfile::codepage_encoding(codepage).decode(body).0.trim_end_matches('\0').to_string();
            }
            ATT_MAPI_PROPS => {
                if let Some(p) = read_props(&mut Rd { b: body, p: 0 }, codepage) {
                    msg_props.extend(p);
                }
            }
            ATT_RECIP_TABLE => {
                let mut rr = Rd { b: body, p: 0 };
                let rows = rr.u32().unwrap_or(0);
                for _ in 0..rows.min(10_000) {
                    let Some(p) = read_props(&mut rr, codepage) else { break };
                    let mut email = s(&p, &[0x39FE]);
                    if email.is_empty() {
                        email = s(&p, &[0x3003]);
                    }
                    let kind = match p.get(&0x0C15) {
                        Some(Val::Int(k)) => *k,
                        _ => 1,
                    };
                    if !email.is_empty() {
                        out.recipients.push((s(&p, &[0x3001]), email, kind));
                    }
                }
            }
            ATT_ATTACH_REND_DATA => {
                if let Some(a) = cur.take() {
                    out.attachments.push(a);
                }
                cur = Some(TnefAttachment::default());
            }
            ATT_ATTACH_TITLE => {
                if let Some(a) = cur.as_mut() {
                    a.name = crate::msgfile::codepage_encoding(codepage).decode(body).0.trim_end_matches('\0').to_string();
                }
            }
            ATT_ATTACH_DATA => {
                if let Some(a) = cur.as_mut() {
                    a.data = body.to_vec();
                }
            }
            ATT_ATTACHMENT => {
                if let (Some(a), Some(p)) = (cur.as_mut(), read_props(&mut Rd { b: body, p: 0 }, codepage)) {
                    // Uzun dosya adı, MIME türü, içerik kimliği; veri burada da olabilir
                    let long = s(&p, &[0x3707, 0x3704, 0x3001]);
                    if !long.is_empty() {
                        a.name = long;
                    }
                    a.mime = s(&p, &[0x370E]);
                    a.cid = s(&p, &[0x3712]);
                    if a.data.is_empty() {
                        if let Some(Val::Bin(d)) = p.get(&0x3701) {
                            a.data = d.clone();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(a) = cur.take() {
        out.attachments.push(a);
    }
    out.attachments.retain(|a| !a.data.is_empty());
    for (i, a) in out.attachments.iter_mut().enumerate() {
        if a.name.is_empty() {
            a.name = format!("ek-{}", i + 1);
        }
        if a.mime.is_empty() {
            a.mime = "application/octet-stream".into();
        }
    }
    // Gövde: HTML, yoksa sıkıştırılmış RTF (HTML ya da düz metin), yoksa düz metin
    match msg_props.get(&0x1013) {
        Some(Val::Bin(b)) if !b.is_empty() => out.html = crate::msgfile::decode_html_bytes(b, Some(codepage)),
        Some(Val::Str(h)) => out.html = h.clone(),
        _ => {}
    }
    if out.html.is_empty() {
        if let Some(Val::Bin(rtf)) = msg_props.get(&0x1009) {
            let (is_html, content) = crate::msgfile::rtf_extract(&crate::msgfile::decompress_rtf(rtf));
            if is_html {
                out.html = content;
            } else if out.text.trim().is_empty() {
                out.text = content.trim().to_string();
            }
        }
    }
    if out.text.trim().is_empty() {
        out.text = s(&msg_props, &[0x1000]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(out: &mut Vec<u8>, level: u8, id: u32, data: &[u8]) {
        out.push(level);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        let sum: u32 = data.iter().map(|b| *b as u32).sum();
        out.extend_from_slice(&((sum & 0xFFFF) as u16).to_le_bytes());
    }

    fn props_str(list: &[(u16, &str)]) -> Vec<u8> {
        let mut p = (list.len() as u32).to_le_bytes().to_vec();
        for (id, v) in list {
            p.extend_from_slice(&0x001Fu16.to_le_bytes());
            p.extend_from_slice(&id.to_le_bytes());
            p.extend_from_slice(&1u32.to_le_bytes());
            let mut d: Vec<u8> = v.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            d.extend_from_slice(&[0, 0]);
            p.extend_from_slice(&(d.len() as u32).to_le_bytes());
            let n = d.len();
            p.extend_from_slice(&d);
            p.extend(std::iter::repeat_n(0, (4 - n % 4) % 4));
        }
        p
    }

    #[test]
    fn winmail_dat() {
        let mut t = SIGNATURE.to_le_bytes().to_vec();
        t.extend_from_slice(&0x1234u16.to_le_bytes());
        attr(&mut t, 1, ATT_OEM_CODEPAGE, &[0xE6, 0x04, 0, 0, 0, 0, 0, 0]);
        attr(&mut t, 1, ATT_BODY, "Merhaba \u{fe}".as_bytes());
        let mut rt = 1u32.to_le_bytes().to_vec();
        rt.extend(props_str(&[(0x3001, "Ayşe Kaya"), (0x39FE, "ayse@ornek.com")]));
        attr(&mut t, 1, ATT_RECIP_TABLE, &rt);
        attr(&mut t, 2, ATT_ATTACH_REND_DATA, &[0; 14]);
        attr(&mut t, 2, ATT_ATTACH_TITLE, b"RAPOR~1.PDF\0");
        attr(&mut t, 2, ATT_ATTACH_DATA, b"%PDF-1.4");
        attr(&mut t, 2, ATT_ATTACHMENT, &props_str(&[(0x3707, "Aylık Rapor.pdf"), (0x370E, "application/pdf")]));
        let r = parse(&t).expect("tnef");
        assert_eq!(r.attachments.len(), 1);
        assert_eq!(r.attachments[0].name, "Aylık Rapor.pdf");
        assert_eq!(r.attachments[0].mime, "application/pdf");
        assert_eq!(r.attachments[0].data, b"%PDF-1.4");
        assert_eq!(r.recipients, vec![("Ayşe Kaya".to_string(), "ayse@ornek.com".to_string(), 1)]);
        assert!(r.text.starts_with("Merhaba"));
    }
}
