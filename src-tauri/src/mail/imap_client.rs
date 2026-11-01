//! IMAP istemcisi.

use super::tls::{self, MailStream};
use super::{MailError, MailResult};
use crate::db::Account;
use imap::types::Flag;
use imap_proto::NameAttribute;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

/// IMAP AUTHENTICATE GSSAPI
struct GssAuth {
    sasl: std::cell::RefCell<super::gssapi::SaslGssapi>,
    error: std::cell::RefCell<Option<String>>,
}

impl imap::Authenticator for GssAuth {
    type Response = Vec<u8>;
    fn process(&self, challenge: &[u8]) -> Self::Response {
        match self.sasl.borrow_mut().respond(challenge) {
            Ok(r) => r,
            Err(e) => {
                *self.error.borrow_mut() = Some(e.to_string());
                Vec::new()
            }
        }
    }
}

/// IMAP AUTHENTICATE XOAUTH2 (Microsoft 365, Gmail)
struct XOAuth2 {
    user: String,
    token: String,
}

impl imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

pub struct RemoteFolder {
    pub remote: String,
    pub path: String,
    pub delim: String,
    pub role: Option<String>,
}

pub struct FetchedMessage {
    pub uid: u32,
    pub flags: Vec<String>,
    pub raw: Vec<u8>,
}

pub struct Imap {
    session: imap::Session<MailStream>,
    caps: Vec<String>,
    pub selected: Option<String>,
}

fn ierr(e: imap::Error) -> MailError {
    MailError(format!("IMAP: {e}"))
}

/// IMAP değiştirilmiş UTF-7 → UTF-8
pub fn utf7_decode(s: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('-') {
            Some(0) => {
                out.push('&');
                rest = &after[1..];
            }
            Some(j) => {
                let mut b64 = after[..j].replace(',', "/");
                while b64.len() % 4 != 0 {
                    b64.push('=');
                }
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&b64) {
                    let u16s: Vec<u16> = bytes.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    out.push_str(&String::from_utf16_lossy(&u16s));
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// UTF-8 → IMAP değiştirilmiş UTF-7
pub fn utf7_encode(s: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    let mut buf: Vec<u16> = Vec::new();
    let flush = |buf: &mut Vec<u16>, out: &mut String| {
        if !buf.is_empty() {
            let bytes: Vec<u8> = buf.iter().flat_map(|u| u.to_be_bytes()).collect();
            let enc = base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes).replace('/', ",");
            out.push('&');
            out.push_str(&enc);
            out.push('-');
            buf.clear();
        }
    };
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut buf, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut tmp = [0u16; 2];
            buf.extend_from_slice(c.encode_utf16(&mut tmp));
        }
    }
    flush(&mut buf, &mut out);
    out
}

fn role_by_name(leaf: &str) -> Option<&'static str> {
    let l = leaf.to_lowercase();
    Some(match l.as_str() {
        "inbox" | "gelen kutusu" => "inbox",
        "sent" | "sent items" | "sent mail" | "sent messages" | "gönderilmiş öğeler" | "gönderilenler"
        | "gönderilmiş" => "sent",
        "drafts" | "draft" | "taslaklar" => "drafts",
        "trash" | "deleted" | "deleted items" | "deleted messages" | "bin" | "çöp kutusu"
        | "silinmiş öğeler" | "silinenler" | "çöp" => "trash",
        "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" | "gereksiz e-posta"
        | "istenmeyen" | "istenmeyen e-posta" => "junk",
        "archive" | "archives" | "arşiv" | "all mail" => "archive",
        _ => return None,
    })
}

impl Imap {
    pub fn connect(account: &Account, password: &str) -> MailResult<Self> {
        let host = account.imap_host.trim();
        if host.is_empty() {
            return Err(MailError(crate::t!("error.no_imap_host")));
        }
        let port = account.imap_port as u16;
        let timeout = Duration::from_secs(40);
        let stream = match account.imap_security.as_str() {
            "ssl" => tls::connect(host, port, true, timeout)?,
            "starttls" => {
                let tcp = tls::tcp_connect(host, port, timeout)?;
                let mut reader = BufReader::new(tcp.try_clone()?);
                let mut line = String::new();
                reader.read_line(&mut line)?; // karşılama
                let mut w = tcp.try_clone()?;
                w.write_all(b"a0 STARTTLS\r\n")?;
                loop {
                    line.clear();
                    if reader.read_line(&mut line)? == 0 {
                        return Err(MailError("STARTTLS: bağlantı kapandı".into()));
                    }
                    if line.starts_with("a0 ") {
                        if !line[3..].to_uppercase().starts_with("OK") {
                            return Err(MailError(format!("STARTTLS reddedildi: {}", line.trim())));
                        }
                        break;
                    }
                }
                drop(reader);
                tls::wrap_tls(tcp, host)?
            }
            _ => tls::connect(host, port, false, timeout)?,
        };
        let needs_greeting = account.imap_security != "starttls";
        let mut client = imap::Client::new(stream);
        if needs_greeting {
            client.read_greeting().map_err(ierr)?;
        }
        let mut session = if account.uses_kerberos() {
            // Kerberos tek oturum açma: parola yerine oturum biletini kullan
            let auth = GssAuth { sasl: std::cell::RefCell::new(super::gssapi::SaslGssapi::new("imap", host, "")?), error: Default::default() };
            match client.authenticate("GSSAPI", &auth) {
                Ok(s) => s,
                Err((e, _)) => {
                    let detail = auth.error.borrow().clone().unwrap_or_else(|| e.to_string());
                    return Err(MailError(crate::t!("error.kerberos", detail)));
                }
            }
        } else if account.auth_method == "oauth" {
            let token = crate::oauth::access_token(account)?;
            let auth = XOAuth2 { user: account.login().to_string(), token };
            client.authenticate("XOAUTH2", &auth).map_err(|(e, _)| MailError::auth(crate::t!("error.imap_login", e)))?
        } else {
            client.login(account.login(), password).map_err(|(e, _)| match e {
                imap::Error::No(_) | imap::Error::Bad(_) => MailError::auth(crate::t!("error.imap_login", e)),
                other => MailError(crate::t!("error.imap_login", other)),
            })?
        };
        let caps = session
            .capabilities()
            .map(|c| {
                c.iter()
                    .map(|x| format!("{x:?}").to_uppercase())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(Imap { session, caps, selected: None })
    }

    pub fn has_cap(&self, cap: &str) -> bool {
        let c = cap.to_uppercase();
        self.caps.iter().any(|x| x.contains(&format!("\"{c}\"")) || x.ends_with(&format!("({c})")) || x == &c)
    }

    pub fn noop(&mut self) -> bool {
        self.session.noop().is_ok()
    }

    pub fn logout(mut self) {
        let _ = self.session.logout();
    }

    pub fn list_folders(&mut self) -> MailResult<Vec<RemoteFolder>> {
        let names = self.session.list(Some(""), Some("*")).map_err(ierr)?;
        let mut out = Vec::new();
        for n in names.iter() {
            let attrs = n.attributes();
            if attrs.iter().any(|a| matches!(a, NameAttribute::NoSelect)) {
                continue;
            }
            let remote = n.name().to_string();
            let delim = n.delimiter().unwrap_or("/").to_string();
            let path = utf7_decode(&remote);
            let mut role: Option<String> = None;
            let mut is_all = false;
            for a in attrs {
                // Özel kullanım öznitelikleri (RFC 6154)
                match a {
                    NameAttribute::Sent => role = Some("sent".into()),
                    NameAttribute::Drafts => role = Some("drafts".into()),
                    NameAttribute::Trash => role = Some("trash".into()),
                    NameAttribute::Junk => role = Some("junk".into()),
                    NameAttribute::Archive => role = Some("archive".into()),
                    NameAttribute::All => is_all = true,
                    NameAttribute::Extension(c) => match c.to_lowercase().as_str() {
                        "\\spam" => role = Some("junk".into()),
                        "\\nonexistent" | "\\noselect" => role = Some("__skip".into()),
                        _ => {}
                    },
                    _ => {}
                }
            }
            if role.as_deref() == Some("__skip") {
                continue;
            }
            if role.is_none() {
                if remote.eq_ignore_ascii_case("INBOX") {
                    role = Some("inbox".into());
                } else if !is_all {
                    let leaf = path.rsplit(delim.as_str()).next().unwrap_or(&path);
                    role = role_by_name(leaf).map(String::from);
                }
            }
            if is_all {
                role = None;
            }
            out.push(RemoteFolder { remote, path, delim, role });
        }
        Ok(out)
    }

    /// Klasörü seçer; (ileti sayısı, UIDVALIDITY) döndürür.
    pub fn select(&mut self, remote: &str) -> MailResult<(u32, u32)> {
        let mb = self.session.select(remote).map_err(|e| MailError(format!("{remote}: {e}")))?;
        self.selected = Some(remote.to_string());
        Ok((mb.exists, mb.uid_validity.unwrap_or(0)))
    }

    pub fn search_all(&mut self) -> MailResult<Vec<u32>> {
        let mut v: Vec<u32> = self.session.uid_search("ALL").map_err(ierr)?.into_iter().collect();
        v.sort_unstable();
        Ok(v)
    }

    fn uid_set(uids: &[u32]) -> String {
        uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",")
    }

    fn flag_strings(flags: &[Flag]) -> Vec<String> {
        flags
            .iter()
            .map(|f| match f {
                Flag::Seen => "\\Seen".to_string(),
                Flag::Answered => "\\Answered".to_string(),
                Flag::Flagged => "\\Flagged".to_string(),
                Flag::Deleted => "\\Deleted".to_string(),
                Flag::Draft => "\\Draft".to_string(),
                Flag::Recent => "\\Recent".to_string(),
                Flag::MayCreate => "\\*".to_string(),
                Flag::Custom(c) => c.to_string(),
                _ => String::new(),
            })
            .collect()
    }

    pub fn fetch_messages(&mut self, uids: &[u32]) -> MailResult<Vec<FetchedMessage>> {
        let mut out = Vec::new();
        for chunk in uids.chunks(20) {
            let fetches = self
                .session
                .uid_fetch(Self::uid_set(chunk), "(UID FLAGS BODY.PEEK[])")
                .map_err(ierr)?;
            for f in fetches.iter() {
                if let (Some(uid), Some(body)) = (f.uid, f.body()) {
                    out.push(FetchedMessage { uid, flags: Self::flag_strings(f.flags()), raw: body.to_vec() });
                }
            }
        }
        Ok(out)
    }

    pub fn fetch_flags(&mut self, uids: &[u32]) -> MailResult<Vec<(u32, Vec<String>)>> {
        let mut out = Vec::new();
        for chunk in uids.chunks(500) {
            let fetches = self.session.uid_fetch(Self::uid_set(chunk), "(UID FLAGS)").map_err(ierr)?;
            for f in fetches.iter() {
                if let Some(uid) = f.uid {
                    out.push((uid, Self::flag_strings(f.flags())));
                }
            }
        }
        Ok(out)
    }

    pub fn store_flag(&mut self, uids: &[u32], flag: &str, add: bool) -> MailResult<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let q = format!("{}FLAGS.SILENT ({flag})", if add { "+" } else { "-" });
        self.session.uid_store(Self::uid_set(uids), q).map_err(ierr)?;
        Ok(())
    }

    pub fn move_to(&mut self, uids: &[u32], dest: &str) -> MailResult<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let set = Self::uid_set(uids);
        if self.has_cap("MOVE") && self.session.uid_mv(&set, dest).is_ok() {
            return Ok(());
        }
        self.session.uid_copy(&set, dest).map_err(ierr)?;
        self.delete(uids)
    }

    pub fn delete(&mut self, uids: &[u32]) -> MailResult<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let set = Self::uid_set(uids);
        self.session.uid_store(&set, "+FLAGS.SILENT (\\Deleted)").map_err(ierr)?;
        if self.has_cap("UIDPLUS") {
            self.session.uid_expunge(&set).map_err(ierr)?;
        } else {
            self.session.expunge().map_err(ierr)?;
        }
        Ok(())
    }

    pub fn append(&mut self, remote: &str, raw: &[u8], seen: bool, draft: bool) -> MailResult<()> {
        let mut flags = Vec::new();
        if seen {
            flags.push(Flag::Seen);
        }
        if draft {
            flags.push(Flag::Draft);
        }
        self.session.append(remote, raw).flags(flags).finish().map(|_| ()).map_err(ierr)
    }

    pub fn create(&mut self, remote: &str) -> MailResult<()> {
        self.session.create(remote).map_err(ierr)?;
        let _ = self.session.subscribe(remote);
        Ok(())
    }

    pub fn delete_folder(&mut self, remote: &str) -> MailResult<()> {
        self.session.delete(remote).map_err(ierr)
    }

    pub fn rename(&mut self, from: &str, to: &str) -> MailResult<()> {
        self.session.rename(from, to).map_err(ierr)
    }
}

pub fn flags_contain(flags: &[String], f: &str) -> bool {
    flags.iter().any(|x| x.eq_ignore_ascii_case(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf7() {
        for s in ["Gönderilmiş Öğeler", "INBOX", "A&B", "Çöp/Ünlü", "日本語"] {
            assert_eq!(utf7_decode(&utf7_encode(s)), s);
        }
        assert_eq!(utf7_decode("&AMcA9g-p"), "Çöp");
        assert_eq!(utf7_encode("A&B"), "A&-B");
    }

    /// Gerçek sunucuya karşı uçtan uca (ör. yerel Dovecot):
    /// MAILNIX_IMAP_TEST="sunucu:port:kullanıcı:parola" cargo test -- --ignored imap_live
    #[test]
    #[ignore]
    fn imap_live() {
        let Ok(spec) = std::env::var("MAILNIX_IMAP_TEST") else { return };
        let p: Vec<&str> = spec.splitn(4, ':').collect();
        let acc = Account { imap_host: p[0].into(), imap_port: p[1].parse().unwrap(), imap_security: "none".into(), username: p[2].into(), email: p[2].into(), ..Default::default() };
        let mut c = Imap::connect(&acc, p[3]).expect("bağlan");
        assert!(c.has_cap("MOVE"), "yetenekler: {:?}", c.caps);
        // Klasörler ve özel kullanım rolleri
        let folders = c.list_folders().unwrap();
        let role = |r: &str| folders.iter().find(|f| f.role.as_deref() == Some(r)).map(|f| f.remote.clone());
        assert_eq!(role("inbox").as_deref(), Some("INBOX"));
        assert_eq!(role("sent").as_deref(), Some("Sent"));
        assert_eq!(role("trash").as_deref(), Some("Trash"));
        assert_eq!(role("junk").as_deref(), Some("Junk"));
        // Türkçe adlı klasör (UTF-7) oluştur, ekle, getir, bayrak, taşı, sil
        let tr = utf7_encode("Arşiv Ğüş");
        let _ = c.delete_folder(&tr);
        c.create(&tr).unwrap();
        let raw = b"From: a@b.c\r\nTo: d@e.f\r\nSubject: Deneme\r\n\r\nMerhaba\r\n";
        c.select("INBOX").unwrap();
        let before = c.search_all().unwrap().len();
        c.append("INBOX", raw, false, false).unwrap();
        c.select("INBOX").unwrap();
        let uids = c.search_all().unwrap();
        assert_eq!(uids.len(), before + 1);
        let last = *uids.iter().max().unwrap();
        let got = c.fetch_messages(&[last]).unwrap();
        assert_eq!(got.len(), 1);
        assert!(String::from_utf8_lossy(&got[0].raw).contains("Subject: Deneme"));
        c.store_flag(&[last], "\\Seen", true).unwrap();
        let fl = c.fetch_flags(&[last]).unwrap();
        assert!(flags_contain(&fl[0].1, "\\Seen"), "{:?}", fl);
        c.move_to(&[last], &tr).unwrap();
        c.select(&tr).unwrap();
        let moved = c.search_all().unwrap();
        assert_eq!(moved.len(), 1);
        c.delete(&moved).unwrap();
        c.select(&tr).unwrap();
        assert!(c.search_all().unwrap().is_empty());
        c.select("INBOX").unwrap();
        c.delete_folder(&tr).unwrap();
        assert!(!c.list_folders().unwrap().iter().any(|f| f.remote == tr));
        c.logout();
    }
}
