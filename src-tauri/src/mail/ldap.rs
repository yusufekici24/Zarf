//! Active Directory'den oturum açmış kullanıcının gerçek e-posta adresini okur
//! (Outlook'un yaptığı gibi). Etki alanı denetleyicisi DNS SRV ile bulunur,
//! oturum Kerberos biletiyle (SASL GSSAPI) parolasız açılır. AD "LDAP imzalama
//! zorunlu" ise bütünlük katmanı (imzalı iletiler) kullanılır.

use super::gssapi::{Client, Mech};
use super::{dns, MailError, MailResult};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DirectoryUser {
    pub email: String,
    pub display_name: String,
    /// proxyAddresses içindeki diğer SMTP adresleri
    pub aliases: Vec<String>,
    pub server: String,
}

// ------------------------------------------------------------------ BER
fn ber_len(n: usize, out: &mut Vec<u8>) {
    if n < 0x80 {
        out.push(n as u8);
    } else {
        let b = n.to_be_bytes();
        let skip = b.iter().take_while(|x| **x == 0).count();
        out.push(0x80 | (b.len() - skip) as u8);
        out.extend_from_slice(&b[skip..]);
    }
}

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    ber_len(content.len(), &mut v);
    v.extend_from_slice(content);
    v
}

fn int(tag: u8, n: i64) -> Vec<u8> {
    let b = n.to_be_bytes();
    let mut i = 0;
    while i < 7 && ((b[i] == 0 && b[i + 1] & 0x80 == 0) || (b[i] == 0xff && b[i + 1] & 0x80 != 0)) {
        i += 1;
    }
    tlv(tag, &b[i..])
}

fn octets(s: &[u8]) -> Vec<u8> {
    tlv(0x04, s)
}

fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

/// Çözülmüş BER öğesi
#[derive(Debug)]
struct Ber<'a> {
    tag: u8,
    data: &'a [u8],
}

fn ber_read(buf: &[u8]) -> Option<(Ber<'_>, usize)> {
    let tag = *buf.first()?;
    let l0 = *buf.get(1)? as usize;
    let (len, hdr) = if l0 < 0x80 {
        (l0, 2)
    } else {
        let n = l0 & 0x7f;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *buf.get(2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let data = buf.get(hdr..hdr + len)?;
    Some((Ber { tag, data }, hdr + len))
}

fn ber_children(mut buf: &[u8]) -> Vec<Ber<'_>> {
    let mut v = Vec::new();
    while let Some((b, n)) = ber_read(buf) {
        v.push(b);
        buf = &buf[n..];
    }
    v
}

fn ber_int(data: &[u8]) -> i64 {
    let mut n: i64 = if data.first().map(|b| b & 0x80 != 0).unwrap_or(false) { -1 } else { 0 };
    for b in data {
        n = (n << 8) | *b as i64;
    }
    n
}

// ------------------------------------------------------------------ filtreler
fn f_eq(attr: &str, val: &str) -> Vec<u8> {
    tlv(0xa3, &[octets(attr.as_bytes()), octets(val.as_bytes())].concat())
}

fn f_or(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(0xa1, &parts.concat())
}

fn f_present(attr: &str) -> Vec<u8> {
    tlv(0x87, attr.as_bytes())
}

// ------------------------------------------------------------------ bağlantı
struct Conn {
    stream: TcpStream,
    next_id: i64,
    /// Bağlantı sonrası bütünlük katmanı (imzalı iletiler)
    layer: Option<Client>,
    inbuf: Vec<u8>,
}

impl Conn {
    fn send_op(&mut self, op: Vec<u8>) -> MailResult<i64> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = seq(&[int(0x02, id), op]);
        let data = match &self.layer {
            Some(c) => {
                let w = c.wrap(&msg)?;
                let mut v = (w.len() as u32).to_be_bytes().to_vec();
                v.extend_from_slice(&w);
                v
            }
            None => msg,
        };
        self.stream.write_all(&data).map_err(|e| MailError(format!("LDAP: {e}")))?;
        Ok(id)
    }

    /// Bir sonraki LDAPMessage'ın (ileti kimliği, işlem) içeriğini okur.
    fn read_msg(&mut self) -> MailResult<(i64, u8, Vec<u8>)> {
        loop {
            if let Some((b, n)) = ber_read(&self.inbuf) {
                let kids = ber_children(b.data);
                let out = match (kids.first(), kids.get(1)) {
                    (Some(id), Some(op)) => (ber_int(id.data), op.tag, op.data.to_vec()),
                    _ => return Err(MailError("LDAP: bozuk yanıt".into())),
                };
                self.inbuf.drain(..n);
                return Ok(out);
            }
            let chunk = self.read_chunk()?;
            self.inbuf.extend_from_slice(&chunk);
        }
    }

    fn read_chunk(&mut self) -> MailResult<Vec<u8>> {
        let io = |e: std::io::Error| MailError(format!("LDAP: {e}"));
        if let Some(c) = &self.layer {
            let mut len = [0u8; 4];
            self.stream.read_exact(&mut len).map_err(io)?;
            let n = u32::from_be_bytes(len) as usize;
            if n > 16 << 20 {
                return Err(MailError("LDAP: çok büyük ileti".into()));
            }
            let mut tok = vec![0u8; n];
            self.stream.read_exact(&mut tok).map_err(io)?;
            c.unwrap(&tok)
        } else {
            let mut buf = [0u8; 8192];
            let n = self.stream.read(&mut buf).map_err(io)?;
            if n == 0 {
                return Err(MailError("LDAP: bağlantı kapandı".into()));
            }
            Ok(buf[..n].to_vec())
        }
    }

    /// SASL GSSAPI bağlanması (RFC 4752). Sunucu sunarsa bütünlük katmanı seçilir.
    fn bind_gssapi(&mut self, host: &str) -> MailResult<()> {
        let mut gss = Client::new("ldap", host, Mech::Kerberos)?;
        let mut token = gss.step(&[])?;
        for _ in 0..10 {
            let creds = if token.is_empty() && !gss.complete { None } else { Some(token.clone()) };
            let mut sasl = vec![octets(b"GSSAPI")];
            if let Some(c) = creds {
                sasl.push(octets(&c));
            }
            let op = tlv(0x60, &[int(0x02, 3), octets(b""), tlv(0xa3, &sasl.concat())].concat());
            self.send_op(op)?;
            let (_, tag, data) = self.read_msg()?;
            if tag != 0x61 {
                return Err(MailError("LDAP: beklenmeyen bağlanma yanıtı".into()));
            }
            let kids = ber_children(&data);
            let code = kids.first().map(|k| ber_int(k.data)).unwrap_or(-1);
            let server_creds = kids.iter().find(|k| k.tag == 0x87).map(|k| k.data.to_vec()).unwrap_or_default();
            match code {
                0 => return Ok(()),
                14 => {
                    if !gss.complete {
                        token = gss.step(&server_creds)?;
                        continue;
                    }
                    // Güvenlik katmanı teklifi: 1 = yok, 2 = bütünlük, 4 = gizlilik
                    let offer = gss.unwrap(&server_creds)?;
                    let layers = offer.first().copied().unwrap_or(1);
                    let choice = if layers & 2 != 0 { 2u8 } else { 1 };
                    let resp = gss.wrap(&[choice, 0x00, 0xff, 0xff])?;
                    let op = tlv(0x60, &[int(0x02, 3), octets(b""), tlv(0xa3, &[octets(b"GSSAPI"), octets(&resp)].concat())].concat());
                    self.send_op(op)?;
                    let (_, _, data) = self.read_msg()?;
                    let code = ber_children(&data).first().map(|k| ber_int(k.data)).unwrap_or(-1);
                    if code != 0 {
                        return Err(MailError(format!("LDAP bağlanma hatası ({code})")));
                    }
                    if choice == 2 {
                        self.layer = Some(gss);
                    }
                    return Ok(());
                }
                c => {
                    let diag = kids.get(2).map(|k| String::from_utf8_lossy(k.data).to_string()).unwrap_or_default();
                    return Err(MailError(format!("LDAP bağlanma hatası ({c}) {diag}")));
                }
            }
        }
        Err(MailError("LDAP: bağlanma tamamlanamadı".into()))
    }

    /// Arama: her sonuç için (dn, [(öznitelik, değerler)])
    fn search(&mut self, base: &str, scope: i64, filter: Vec<u8>, attrs: &[&str]) -> MailResult<Vec<(String, Vec<(String, Vec<String>)>)>> {
        let attr_seq = seq(&attrs.iter().map(|a| octets(a.as_bytes())).collect::<Vec<_>>());
        let op = tlv(
            0x63,
            &[octets(base.as_bytes()), int(0x0a, scope), int(0x0a, 0), int(0x02, 5), int(0x02, 10), tlv(0x01, &[0]), filter, attr_seq].concat(),
        );
        let id = self.send_op(op)?;
        let mut out = Vec::new();
        loop {
            let (mid, tag, data) = self.read_msg()?;
            if mid != id {
                continue;
            }
            match tag {
                0x64 => {
                    let kids = ber_children(&data);
                    let dn = kids.first().map(|k| String::from_utf8_lossy(k.data).to_string()).unwrap_or_default();
                    let mut attrs = Vec::new();
                    if let Some(list) = kids.get(1) {
                        for a in ber_children(list.data) {
                            let parts = ber_children(a.data);
                            let name = parts.first().map(|k| String::from_utf8_lossy(k.data).to_string()).unwrap_or_default();
                            let vals = parts.get(1).map(|s| ber_children(s.data).iter().map(|v| String::from_utf8_lossy(v.data).to_string()).collect()).unwrap_or_default();
                            attrs.push((name, vals));
                        }
                    }
                    out.push((dn, attrs));
                }
                0x65 => break,
                _ => {}
            }
        }
        Ok(out)
    }
}

fn get<'a>(attrs: &'a [(String, Vec<String>)], name: &str) -> Vec<&'a str> {
    attrs.iter().filter(|(n, _)| n.eq_ignore_ascii_case(name)).flat_map(|(_, v)| v.iter().map(|s| s.as_str())).collect()
}

fn base_from_realm(realm: &str) -> String {
    realm.to_lowercase().split('.').filter(|p| !p.is_empty()).map(|p| format!("DC={p}")).collect::<Vec<_>>().join(",")
}

/// Denenecek LDAP sunucuları: (bağlantı adresi, Kerberos için sunucu adı)
fn servers(domain: &str) -> Vec<(String, String)> {
    if let Ok(s) = std::env::var("LYNORA_LDAP_SERVER") {
        let host = s.rsplit_once(':').map(|(h, _)| h.to_string()).unwrap_or_else(|| s.clone());
        return vec![(s, host)];
    }
    let mut v = Vec::new();
    for name in [format!("_ldap._tcp.dc._msdcs.{domain}"), format!("_ldap._tcp.{domain}")] {
        for r in dns::srv(&name) {
            let t = r.target.trim_end_matches('.').to_string();
            if !t.is_empty() && !v.iter().any(|(a, _): &(String, String)| a.starts_with(&t)) {
                v.push((format!("{t}:{}", r.port), t));
            }
        }
        if !v.is_empty() {
            break;
        }
    }
    v.truncate(3);
    v
}

fn connect(addr: &str) -> MailResult<TcpStream> {
    let sa = addr.to_socket_addrs().map_err(|e| MailError(format!("{addr}: {e}")))?.next().ok_or_else(|| MailError(addr.to_string()))?;
    let s = TcpStream::connect_timeout(&sa, Duration::from_secs(3)).map_err(|e| MailError(format!("{addr}: {e}")))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(6)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(6)));
    Ok(s)
}

fn lookup_at(addr: &str, host: &str, realm: &str, user: &str) -> MailResult<Option<DirectoryUser>> {
    let mut c = Conn { stream: connect(addr)?, next_id: 1, layer: None, inbuf: Vec::new() };
    c.bind_gssapi(host)?;
    // Temel DN: kök DSE'den (AD: defaultNamingContext), yoksa alan adından
    let root = c.search("", 0, f_present("objectClass"), &["defaultNamingContext", "namingContexts"]).unwrap_or_default();
    let mut base = String::new();
    if let Some((_, attrs)) = root.first() {
        base = get(attrs, "defaultNamingContext").first().map(|s| s.to_string()).unwrap_or_default();
        if base.is_empty() {
            let wanted = base_from_realm(realm).to_lowercase();
            let ctxs = get(attrs, "namingContexts");
            base = ctxs.iter().find(|c| c.to_lowercase() == wanted).or(ctxs.first()).map(|s| s.to_string()).unwrap_or_default();
        }
    }
    if base.is_empty() {
        base = base_from_realm(realm);
    }
    let upn = format!("{user}@{realm}");
    let filter = f_or(&[f_eq("sAMAccountName", user), f_eq("userPrincipalName", &upn), f_eq("uid", user)]);
    let res = c.search(&base, 2, filter, &["mail", "displayName", "proxyAddresses", "cn"])?;
    let Some((_, attrs)) = res.first() else { return Ok(None) };
    let proxies = get(attrs, "proxyAddresses");
    let primary = proxies.iter().find(|p| p.starts_with("SMTP:")).map(|p| p[5..].to_string());
    let email = get(attrs, "mail").first().map(|s| s.to_string()).or(primary).unwrap_or_default();
    if email.is_empty() {
        return Ok(None);
    }
    let aliases = proxies
        .iter()
        .filter_map(|p| p.strip_prefix("smtp:").or_else(|| p.strip_prefix("SMTP:")))
        .filter(|a| !a.eq_ignore_ascii_case(&email))
        .map(String::from)
        .collect();
    let display_name = get(attrs, "displayName").first().or(get(attrs, "cn").first()).map(|s| s.to_string()).unwrap_or_default();
    Ok(Some(DirectoryUser { email, display_name, aliases, server: host.to_string() }))
}

/// Kerberos biletinin sahibi için dizindeki e-posta adresini bulur.
/// `principal`: "ali@KURUM.LOCAL"
pub fn lookup_principal(principal: &str, log: &mut Vec<String>) -> Option<DirectoryUser> {
    let (user, realm) = principal.split_once('@')?;
    let user = user.split('/').next().unwrap_or(user);
    let domain = realm.to_lowercase();
    for (addr, host) in servers(&domain) {
        match lookup_at(&addr, &host, realm, user) {
            Ok(Some(u)) => return Some(u),
            Ok(None) => {
                super::discover::step(format!("LDAP {host}: kullanıcı/e-posta bulunamadı"));
                log.push(format!("LDAP {host}: kullanıcı/e-posta bulunamadı"))
            }
            Err(e) => {
                super::discover::step(format!("LDAP {host}: {e}"));
                log.push(format!("LDAP {host}: {e}"))
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ber_roundtrip() {
        let m = seq(&[int(0x02, 300), octets(b"abc")]);
        let (b, n) = ber_read(&m).unwrap();
        assert_eq!(n, m.len());
        let k = ber_children(b.data);
        assert_eq!(ber_int(k[0].data), 300);
        assert_eq!(k[1].data, b"abc");
        // uzun uzunluk biçimi
        let big = octets(&[7u8; 300]);
        assert_eq!(&big[..4], &[0x04, 0x82, 0x01, 0x2c]);
        assert_eq!(ber_read(&big).unwrap().0.data.len(), 300);
        assert_eq!(base_from_realm("KURUM.GOV.TR"), "DC=kurum,DC=gov,DC=tr");
    }
}
