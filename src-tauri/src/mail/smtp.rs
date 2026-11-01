//! Basit ve bağımsız SMTP istemcisi (SSL/TLS, STARTTLS, AUTH PLAIN/LOGIN).

use super::tls::{self, MailStream};
use super::{MailError, MailResult};
use crate::db::Account;
use base64::Engine;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

struct Conn {
    reader: BufReader<MailStream>,
}

impl Conn {
    fn read_reply(&mut self) -> MailResult<(u16, String)> {
        let mut full = String::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(MailError("SMTP: sunucu bağlantıyı kapattı".into()));
            }
            full.push_str(&line);
            if line.len() < 4 || line.as_bytes()[3] != b'-' {
                let code = line.get(..3).and_then(|c| c.parse().ok()).unwrap_or(0);
                return Ok((code, full.trim().to_string()));
            }
        }
    }

    fn cmd(&mut self, c: &str) -> MailResult<(u16, String)> {
        let s = self.reader.get_mut();
        s.write_all(c.as_bytes())?;
        s.write_all(b"\r\n")?;
        s.flush()?;
        self.read_reply()
    }

    /// Kimlik doğrulama adımı: 535/534/530 yanıtları kimlik hatasıdır.
    fn expect_auth(&mut self, c: &str, ok: &[u16]) -> MailResult<String> {
        let (code, text) = self.cmd(c)?;
        if ok.contains(&code) {
            Ok(text)
        } else if matches!(code, 535 | 534 | 530 | 500 | 501) {
            Err(MailError::auth(crate::t!("error.smtp_login", text)))
        } else {
            Err(MailError(crate::t!("error.smtp_login", text)))
        }
    }

    fn expect(&mut self, c: &str, ok: &[u16]) -> MailResult<String> {
        let (code, text) = self.cmd(c)?;
        if ok.contains(&code) {
            Ok(text)
        } else {
            let shown = if c.starts_with("AUTH") || !c.is_ascii() || c.len() > 60 { "AUTH" } else { c };
            Err(MailError(format!("SMTP ({shown}): {text}")))
        }
    }
}

fn ehlo_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.is_ascii() && !s.contains(' '))
        .unwrap_or_else(|| "localhost".into())
}

fn authenticate(c: &mut Conn, account: &Account, password: &str, caps: &str, host: &str) -> MailResult<()> {
    let b64 = |s: &[u8]| base64::engine::general_purpose::STANDARD.encode(s);
    let unb64 = |s: &str| base64::engine::general_purpose::STANDARD.decode(s.trim()).unwrap_or_default();
    let caps_up = caps.to_uppercase();
    if account.uses_kerberos() {
        // SASL GSSAPI (Kerberos tek oturum açma) — parola gönderilmez
        let mut sasl = super::gssapi::SaslGssapi::new("smtp", host, "")?;
        let first = sasl.respond(&[])?;
        let (mut code, mut text) = c.cmd(&format!("AUTH GSSAPI {}", b64(&first)))?;
        for _ in 0..10 {
            match code {
                235 => return Ok(()),
                334 => {
                    let challenge = unb64(text.get(4..).unwrap_or(""));
                    let resp = sasl.respond(&challenge)?;
                    (code, text) = c.cmd(&b64(&resp))?;
                }
                _ => return Err(MailError(crate::t!("error.smtp_login", text))),
            }
        }
        return Err(MailError(crate::t!("error.smtp_login", text)));
    }
    if account.auth_method == "oauth" && !account.oauth_token.is_empty() {
        let token = crate::oauth::access_token(account)?;
        let xo = format!("user={}\x01auth=Bearer {token}\x01\x01", account.login());
        c.expect_auth(&format!("AUTH XOAUTH2 {}", b64(xo.as_bytes())), &[235])?;
        return Ok(());
    }
    if password.is_empty() {
        return Ok(());
    }
    let user = account.login();
    if caps_up.contains("PLAIN") || !caps_up.contains("LOGIN") {
        let token = b64(format!("\0{user}\0{password}").as_bytes());
        c.expect_auth(&format!("AUTH PLAIN {token}"), &[235])?;
    } else {
        c.expect("AUTH LOGIN", &[334])?;
        c.expect(&b64(user.as_bytes()), &[334])?;
        c.expect_auth(&b64(password.as_bytes()), &[235])?;
    }
    Ok(())
}

/// Ham iletiyi gönderir. `recipients` yalın adreslerdir.
pub fn send(account: &Account, password: &str, recipients: &[String], raw: &[u8]) -> MailResult<()> {
    if recipients.is_empty() {
        return Err(MailError(crate::t!("error.no_recipients")));
    }
    let host = account.smtp_host.trim();
    if host.is_empty() {
        return Err(MailError(crate::t!("error.no_smtp_host")));
    }
    let port = account.smtp_port as u16;
    let timeout = Duration::from_secs(60);
    let sec = account.smtp_security.as_str();
    let stream = tls::connect(host, port, sec == "ssl", timeout)?;
    let mut c = Conn { reader: BufReader::new(stream) };
    let (code, text) = c.read_reply()?;
    if code != 220 {
        return Err(MailError(format!("SMTP: {text}")));
    }
    let helo = ehlo_name();
    let mut caps = c.expect(&format!("EHLO {helo}"), &[250])?;
    if sec == "starttls" {
        c.expect("STARTTLS", &[220])?;
        let MailStream::Plain(tcp) = c.reader.into_inner() else {
            return Err(MailError("STARTTLS".into()));
        };
        c = Conn { reader: BufReader::new(tls::wrap_tls(tcp, host)?) };
        caps = c.expect(&format!("EHLO {helo}"), &[250])?;
    }
    authenticate(&mut c, account, password, &caps, host)?;
    let size_ext = if caps.to_uppercase().contains("SIZE") { format!(" SIZE={}", raw.len()) } else { String::new() };
    c.expect(&format!("MAIL FROM:<{}>{size_ext}", account.email), &[250])?;
    let mut accepted = 0;
    let mut rejected = Vec::new();
    for r in recipients {
        let (code, text) = c.cmd(&format!("RCPT TO:<{r}>"))?;
        if code == 250 || code == 251 {
            accepted += 1;
        } else {
            rejected.push(format!("{r}: {text}"));
        }
    }
    if accepted == 0 {
        let _ = c.cmd("RSET");
        return Err(MailError(crate::t!("error.recipients_rejected", rejected.join("; "))));
    }
    c.expect("DATA", &[354])?;
    // Nokta doldurma ve CRLF normalleştirme
    let mut body = Vec::with_capacity(raw.len() + 1024);
    let mut at_line_start = true;
    let mut prev = 0u8;
    for &b in raw {
        if b == b'\n' && prev != b'\r' {
            body.push(b'\r');
        }
        if at_line_start && b == b'.' {
            body.push(b'.');
        }
        body.push(b);
        at_line_start = b == b'\n';
        prev = b;
    }
    if !body.ends_with(b"\r\n") {
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(b".\r\n");
    {
        let s = c.reader.get_mut();
        s.write_all(&body)?;
        s.flush()?;
    }
    let (code, text) = c.read_reply()?;
    if code != 250 {
        return Err(MailError(format!("SMTP: {text}")));
    }
    let _ = c.cmd("QUIT");
    let mut sink = [0u8; 64];
    let _ = c.reader.get_mut().read(&mut sink);
    if !rejected.is_empty() {
        return Err(MailError(crate::t!("error.some_rejected", rejected.join("; "))));
    }
    Ok(())
}

/// Bağlantı ve kimlik doğrulamayı sınar.
pub fn send_test(account: &Account, password: &str) -> MailResult<()> {
    let host = account.smtp_host.trim();
    let port = account.smtp_port as u16;
    let timeout = Duration::from_secs(20);
    let sec = account.smtp_security.as_str();
    let stream = tls::connect(host, port, sec == "ssl", timeout)?;
    let mut c = Conn { reader: BufReader::new(stream) };
    c.read_reply()?;
    let helo = ehlo_name();
    let mut caps = c.expect(&format!("EHLO {helo}"), &[250])?;
    if sec == "starttls" {
        c.expect("STARTTLS", &[220])?;
        let MailStream::Plain(tcp) = c.reader.into_inner() else {
            return Err(MailError("STARTTLS".into()));
        };
        c = Conn { reader: BufReader::new(tls::wrap_tls(tcp, host)?) };
        caps = c.expect(&format!("EHLO {helo}"), &[250])?;
    }
    authenticate(&mut c, account, password, &caps, host)?;
    let _ = c.cmd("QUIT");
    Ok(())
}
