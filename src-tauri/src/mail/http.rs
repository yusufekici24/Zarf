//! Küçük HTTP/1.1 istemcisi (rustls). Exchange EWS / Autodiscover / OAuth için.
//!
//! NTLM el sıkışması aynı TCP bağlantısında yapılmak zorunda olduğundan
//! bağlantı yönetimi elle yapılır (keep-alive, chunked yanıtlar, yönlendirme).

use super::tls::{self, MailStream};
use super::{gssapi, ntlm, MailError, MailResult};
use base64::Engine;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Auth {
    None,
    Basic { user: String, password: String },
    Ntlm { user: String, password: String },
    /// Kerberos (SPNEGO) — oturum biletini kullanır
    Negotiate,
    Bearer(String),
    /// Sunucunun önerdiği yönteme göre: Negotiate > NTLM > Basic
    Auto { user: String, password: String, kerberos: bool },
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn headers_all(&self, name: &str) -> Vec<&str> {
        self.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).collect()
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

#[derive(Clone, Debug)]
pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Url {
    pub fn parse(s: &str) -> MailResult<Url> {
        let (https, rest) = if let Some(r) = s.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = s.strip_prefix("http://") {
            (false, r)
        } else {
            return Err(MailError(format!("Geçersiz adres: {s}")));
        };
        let (hostport, path) = match rest.find('/') {
            Some(i) => (&rest[..i], rest[i..].to_string()),
            None => (rest, "/".to_string()),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h.to_string(), p.parse().unwrap_or(443)),
            _ => (hostport.to_string(), if https { 443 } else { 80 }),
        };
        Ok(Url { https, host, port, path })
    }

    pub fn to_string(&self) -> String {
        let default = if self.https { 443 } else { 80 };
        let port = if self.port == default { String::new() } else { format!(":{}", self.port) };
        format!("{}://{}{}{}", if self.https { "https" } else { "http" }, self.host, port, self.path)
    }
}

struct Conn {
    host: String,
    port: u16,
    https: bool,
    reader: BufReader<MailStream>,
    /// Son kullanım: uzun süre boşta kalan bağlantıyı sunucu/güvenlik duvarı sessizce
    /// kapatmış olabilir; böyle bir bağlantıya yazmak 60 sn bekleyip hata veriyordu.
    last_used: std::time::Instant,
}

/// Bu süreden uzun boşta kalan bağlantı yeniden kullanılmaz (IIS varsayılanı 120 sn)
const IDLE_REUSE: Duration = Duration::from_secs(90);

pub struct Client {
    pub auth: Auth,
    conn: Option<Conn>,
    pub timeout: Duration,
    pub user_agent: String,
    /// Kullanılan yöntem (Auto için seçildikten sonra sabitlenir)
    resolved: Option<Auth>,
    /// Açık bağlantının sıra numarası (her yeni TCP/TLS bağlantısında artar)
    conn_gen: u64,
    /// NTLM/Kerberos kimliği doğrulanmış bağlantının sıra numarası. Bu yöntemler
    /// bağlantı başına çalışır: aynı bağlantıda sonraki istekler el sıkışmasız gider
    /// (Outlook da böyle yapar; her istekte 2-3 gidiş-dönüş yerine 1).
    authed_gen: Option<u64>,
}

impl Client {
    pub fn new(auth: Auth) -> Client {
        Client {
            auth,
            conn: None,
            timeout: Duration::from_secs(60),
            user_agent: format!("Lynora/{}", env!("CARGO_PKG_VERSION")),
            resolved: None,
            conn_gen: 0,
            authed_gen: None,
        }
    }

    fn connect(&mut self, url: &Url) -> MailResult<()> {
        let reuse = matches!(&self.conn, Some(c) if c.host == url.host && c.port == url.port && c.https == url.https && c.last_used.elapsed() < IDLE_REUSE);
        if !reuse {
            self.conn = None;
            let stream = tls::connect(&url.host, url.port, url.https, self.timeout)?;
            self.conn_gen += 1;
            self.conn = Some(Conn { host: url.host.clone(), port: url.port, https: url.https, reader: BufReader::new(stream), last_used: std::time::Instant::now() });
        }
        Ok(())
    }

    fn send_once(&mut self, method: &str, url: &Url, headers: &[(String, String)], body: &[u8]) -> MailResult<Response> {
        for attempt in 0..2 {
            self.connect(url)?;
            let conn = self.conn.as_mut().unwrap();
            let mut req = format!("{method} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nConnection: keep-alive\r\nContent-Length: {}\r\n", url.path, url.host, self.user_agent, body.len());
            for (k, v) in headers {
                req.push_str(&format!("{k}: {v}\r\n"));
            }
            req.push_str("\r\n");
            let w = conn.reader.get_mut();
            let wrote = w.write_all(req.as_bytes()).and_then(|_| w.write_all(body)).and_then(|_| w.flush());
            if wrote.is_err() {
                self.conn = None;
                if attempt == 0 {
                    continue;
                }
                return Err(MailError(format!("HTTP: {}", wrote.unwrap_err())));
            }
            match read_response(&mut conn.reader, method == "HEAD") {
                Ok((resp, keep)) => {
                    if !keep {
                        self.conn = None;
                    } else {
                        conn.last_used = std::time::Instant::now();
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    // Sunucu boşta kalan bağlantıyı kapatmış olabilir: bir kez yeniden dene
                    self.conn = None;
                    if attempt == 0 {
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        unreachable!()
    }

    /// Kimlik doğrulama ve yönlendirmeleri yöneterek istek gönderir.
    pub fn request(&mut self, method: &str, url: &str, content_type: &str, body: &[u8]) -> MailResult<Response> {
        let mut url = Url::parse(url)?;
        for _ in 0..5 {
            let resp = self.authed(method, &url, content_type, body)?;
            if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                if let Some(loc) = resp.header("Location") {
                    let next = if loc.starts_with("http") { Url::parse(loc)? } else { Url { path: loc.to_string(), ..url.clone() } };
                    // Güvenli bağlantıdan güvensize düşürülmesine izin verme
                    if url.https && !next.https {
                        return Err(MailError("HTTP: güvensiz yönlendirme reddedildi".into()));
                    }
                    url = next;
                    continue;
                }
            }
            return Ok(resp);
        }
        Err(MailError("HTTP: çok fazla yönlendirme".into()))
    }

    fn authed(&mut self, method: &str, url: &Url, content_type: &str, body: &[u8]) -> MailResult<Response> {
        let mut base = Vec::new();
        if !content_type.is_empty() {
            base.push(("Content-Type".to_string(), content_type.to_string()));
        }
        let auth = self.resolved.clone().unwrap_or_else(|| self.auth.clone());
        match auth {
            Auth::None => self.send_once(method, url, &base, body),
            Auth::Bearer(t) => {
                let mut h = base.clone();
                h.push(("Authorization".into(), format!("Bearer {t}")));
                self.send_once(method, url, &h, body)
            }
            Auth::Basic { user, password } => {
                let mut h = base.clone();
                let tok = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
                h.push(("Authorization".into(), format!("Basic {tok}")));
                self.send_once(method, url, &h, body)
            }
            Auth::Ntlm { .. } | Auth::Negotiate if self.conn.is_some() && self.authed_gen == Some(self.conn_gen) => {
                let r = self.send_once(method, url, &base, body)?;
                if r.status != 401 {
                    return Ok(r);
                }
                self.authed_gen = None;
                self.authed(method, url, content_type, body)
            }
            Auth::Ntlm { user, password } => {
                let r = self.ntlm(method, url, &base, body, &user, &password)?;
                if r.status != 401 && self.conn.is_some() {
                    self.authed_gen = Some(self.conn_gen);
                }
                Ok(r)
            }
            Auth::Negotiate => {
                let r = self.negotiate(method, url, &base, body)?;
                if r.status != 401 && self.conn.is_some() {
                    self.authed_gen = Some(self.conn_gen);
                }
                Ok(r)
            }
            Auth::Auto { user, password, kerberos } => {
                let probe = self.send_once(method, url, &base, body)?;
                if probe.status != 401 {
                    return Ok(probe);
                }
                let offered: Vec<String> = probe.headers_all("WWW-Authenticate").iter().map(|s| s.to_lowercase()).collect();
                let has = |s: &str| offered.iter().any(|o| o.starts_with(s));
                let chosen = if kerberos && has("negotiate") && gssapi::available() {
                    Auth::Negotiate
                } else if has("ntlm") || (has("negotiate") && !password.is_empty()) {
                    Auth::Ntlm { user: user.clone(), password: password.clone() }
                } else if has("basic") {
                    Auth::Basic { user: user.clone(), password: password.clone() }
                } else if has("bearer") {
                    return Ok(probe);
                } else {
                    Auth::Ntlm { user: user.clone(), password: password.clone() }
                };
                self.resolved = Some(chosen);
                self.authed(method, url, content_type, body)
            }
        }
    }

    fn ntlm(&mut self, method: &str, url: &Url, base: &[(String, String)], body: &[u8], user: &str, password: &str) -> MailResult<Response> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut h = base.to_vec();
        h.push(("Authorization".into(), format!("NTLM {}", b64.encode(ntlm::negotiate()))));
        let r1 = self.send_once(method, url, &h, body)?;
        if r1.status != 401 {
            return Ok(r1);
        }
        let challenge = r1
            .headers_all("WWW-Authenticate")
            .iter()
            .find_map(|v| v.strip_prefix("NTLM ").or_else(|| v.strip_prefix("Negotiate ")))
            .and_then(|t| b64.decode(t.trim()).ok())
            .and_then(|d| ntlm::parse_challenge(&d));
        let Some(ch) = challenge else {
            return Ok(r1);
        };
        let ws = std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_uppercase()).unwrap_or_else(|_| "LYNORA".into());
        let t3 = ntlm::authenticate(&ch, user, password, &ws);
        let mut h = base.to_vec();
        h.push(("Authorization".into(), format!("NTLM {}", b64.encode(t3))));
        self.send_once(method, url, &h, body)
    }

    fn negotiate(&mut self, method: &str, url: &Url, base: &[(String, String)], body: &[u8]) -> MailResult<Response> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut ctx = gssapi::Client::new("HTTP", &url.host, gssapi::Mech::Spnego)?;
        let mut token = ctx.step(&[])?;
        for _ in 0..4 {
            let mut h = base.to_vec();
            h.push(("Authorization".into(), format!("Negotiate {}", b64.encode(&token))));
            let r = self.send_once(method, url, &h, body)?;
            if r.status != 401 {
                return Ok(r);
            }
            let next = r.headers_all("WWW-Authenticate").iter().find_map(|v| v.strip_prefix("Negotiate ")).and_then(|t| b64.decode(t.trim()).ok());
            match next {
                Some(t) if !ctx.complete => token = ctx.step(&t)?,
                _ => return Ok(r),
            }
        }
        Err(MailError("HTTP Negotiate: el sıkışma tamamlanamadı".into()))
    }
}

impl Client {
    /// Uzun süreli akış isteği (EWS GetStreamingEvents): yanıt gövdesi geldikçe
    /// `on_data`'ya verilir; `false` dönerse bağlantı kapatılır. NTLM/Kerberos
    /// için bağlantı önceden (aynı adrese normal bir istekle) doğrulanmış olmalıdır.
    pub fn post_streaming(&mut self, url: &str, content_type: &str, body: &[u8], idle: Duration, mut on_data: impl FnMut(&[u8]) -> bool) -> MailResult<u16> {
        let url = Url::parse(url)?;
        let mut headers = vec![("Content-Type".to_string(), content_type.to_string())];
        match self.resolved.clone().unwrap_or_else(|| self.auth.clone()) {
            Auth::Bearer(t) => headers.push(("Authorization".into(), format!("Bearer {t}"))),
            Auth::Basic { user, password } => {
                let tok = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
                headers.push(("Authorization".into(), format!("Basic {tok}")));
            }
            _ => {}
        }
        self.connect(&url)?;
        let conn = self.conn.as_mut().unwrap();
        let mut req = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nConnection: keep-alive\r\nContent-Length: {}\r\n",
            url.path,
            url.host,
            self.user_agent,
            body.len()
        );
        for (k, v) in &headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        let w = conn.reader.get_mut();
        w.write_all(req.as_bytes()).and_then(|_| w.write_all(body)).and_then(|_| w.flush()).map_err(|e| MailError(format!("HTTP: {e}")))?;
        conn.reader.get_ref().set_read_timeout(Some(idle));
        let result = (|| -> MailResult<u16> {
            let reader = &mut self.conn.as_mut().unwrap().reader;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| MailError("HTTP: geçersiz yanıt".into()))?;
            let mut chunked = false;
            let mut len: Option<usize> = None;
            loop {
                line.clear();
                if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("transfer-encoding") && v.to_lowercase().contains("chunked") {
                        chunked = true;
                    }
                    if k.trim().eq_ignore_ascii_case("content-length") {
                        len = v.trim().parse().ok();
                    }
                }
            }
            if status != 200 {
                return Ok(status);
            }
            if chunked {
                loop {
                    line.clear();
                    if reader.read_line(&mut line)? == 0 {
                        break;
                    }
                    let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
                    if size == 0 {
                        break;
                    }
                    let mut chunk = vec![0u8; size];
                    reader.read_exact(&mut chunk)?;
                    line.clear();
                    reader.read_line(&mut line)?;
                    if !on_data(&chunk) {
                        break;
                    }
                }
            } else if let Some(n) = len {
                let mut b = vec![0u8; n];
                reader.read_exact(&mut b)?;
                on_data(&b);
            }
            Ok(status)
        })();
        // Akış bağlantısı yeniden kullanılmaz
        self.conn = None;
        result
    }
}

fn read_response(reader: &mut BufReader<MailStream>, head: bool) -> MailResult<(Response, bool)> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(MailError("HTTP: bağlantı kapandı".into()));
    }
    let mut parts = line.split_whitespace();
    let version = parts.next().unwrap_or("").to_string();
    let status: u16 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| MailError(format!("HTTP: geçersiz yanıt: {}", line.trim())))?;
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let get = |n: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v): &(String, String)| v.clone());
    let mut keep = version != "HTTP/1.0" && !get("Connection").map(|c| c.eq_ignore_ascii_case("close")).unwrap_or(false);
    let mut body = Vec::new();
    if head || status == 204 || status == 304 || (100..200).contains(&status) {
        // gövde yok
    } else if get("Transfer-Encoding").map(|t| t.to_lowercase().contains("chunked")).unwrap_or(false) {
        loop {
            line.clear();
            reader.read_line(&mut line)?;
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
            if size == 0 {
                // son satırlar (trailer)
                loop {
                    line.clear();
                    if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
                        break;
                    }
                }
                break;
            }
            let mut chunk = vec![0u8; size];
            reader.read_exact(&mut chunk)?;
            body.extend_from_slice(&chunk);
            line.clear();
            reader.read_line(&mut line)?;
        }
    } else if let Some(len) = get("Content-Length").and_then(|l| l.parse::<usize>().ok()) {
        body.resize(len, 0);
        reader.read_exact(&mut body)?;
    } else {
        reader.read_to_end(&mut body)?;
        keep = false;
    }
    Ok((Response { status, headers, body }, keep))
}

/// application/x-www-form-urlencoded kodlama
pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v))).collect::<Vec<_>>().join("&")
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = Url::parse("https://mail.kurum.gov.tr/EWS/Exchange.asmx").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("mail.kurum.gov.tr", 443, "/EWS/Exchange.asmx"));
        let u = Url::parse("http://127.0.0.1:8080").unwrap();
        assert_eq!((u.port, u.path.as_str(), u.https), (8080, "/", false));
        assert_eq!(url_decode("a%20b+c%C4%9F"), "a b cğ");
        assert_eq!(url_encode("a b/ğ"), "a%20b%2F%C4%9F");
    }
}
