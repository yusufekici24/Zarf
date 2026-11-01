//! E-posta çekirdeği: adres yardımcıları, MIME ayrıştırma/oluşturma,
//! IMAP ve SMTP istemcileri.

pub mod address;
pub mod build;
pub mod dav;
pub mod discover;
pub mod dns;
pub mod ews;
pub mod gssapi;
pub mod html;
pub mod http;
pub mod imap_client;
pub mod ldap;
pub mod ntlm;
pub mod parse;
pub mod rule_spec;
pub mod smtp;
pub mod tls;
pub mod tnef;
pub mod xml;
pub mod net;

use serde::Serialize;

#[derive(Debug)]
pub struct MailError(pub String);

/// Kimlik doğrulama hatalarını ayırt etmek için iç işaret.
const AUTH_MARK: &str = "\u{1}auth\u{1}";
/// Ağ hatası (sunucuya ulaşılamıyor) işareti: çevrimdışı durumunu belirler.
const NET_MARK: &str = "\u{1}net\u{1}";

impl MailError {
    /// Sunucu kullanıcı adı/parolayı reddetti. Bu hata alındığında hesap,
    /// AD/LDAP hesap kilitlenmesini önlemek için otomatik denemelerden çıkarılır.
    pub fn auth(msg: impl Into<String>) -> Self {
        MailError(format!("{AUTH_MARK}{}", msg.into()))
    }

    pub fn is_auth(&self) -> bool {
        self.0.starts_with(AUTH_MARK)
    }

    /// Sunucuya ulaşılamadı (kurum ağı dışında, bağlantı yok, çevrimdışı çalışma)
    pub fn net(msg: impl Into<String>) -> Self {
        MailError(format!("{NET_MARK}{}", msg.into()))
    }

    pub fn is_net(&self) -> bool {
        self.0.starts_with(NET_MARK)
    }

    pub fn message(&self) -> &str {
        self.0.trim_start_matches(AUTH_MARK).trim_start_matches(NET_MARK)
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for MailError {}

pub type MailResult<T> = Result<T, MailError>;

impl From<std::io::Error> for MailError {
    fn from(e: std::io::Error) -> Self {
        MailError(e.to_string())
    }
}

/// Bilinen sağlayıcıların sunucu ayarları.
#[derive(Serialize, Clone, Debug)]
pub struct ServerGuess {
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_security: String,
    pub note: String,
}

pub fn guess_servers(email: &str) -> ServerGuess {
    let domain = email.rsplit('@').next().unwrap_or("").trim().to_lowercase();
    let g = |ih: &str, ip: i64, is: &str, sh: &str, sp: i64, ss: &str, note: &str| ServerGuess {
        imap_host: ih.into(),
        imap_port: ip,
        imap_security: is.into(),
        smtp_host: sh.into(),
        smtp_port: sp,
        smtp_security: ss.into(),
        note: note.into(),
    };
    match domain.as_str() {
        "gmail.com" | "googlemail.com" => g("imap.gmail.com", 993, "ssl", "smtp.gmail.com", 465, "ssl", "provider.gmail"),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "outlook.com.tr" | "hotmail.com.tr" => {
            g("outlook.office365.com", 993, "ssl", "smtp-mail.outlook.com", 587, "starttls", "provider.outlook")
        }
        "yandex.com" | "yandex.ru" | "ya.ru" => g("imap.yandex.com", 993, "ssl", "smtp.yandex.com", 465, "ssl", "provider.yandex"),
        "yandex.com.tr" => g("imap.yandex.com.tr", 993, "ssl", "smtp.yandex.com.tr", 465, "ssl", "provider.yandex"),
        "yahoo.com" | "ymail.com" | "rocketmail.com" => g("imap.mail.yahoo.com", 993, "ssl", "smtp.mail.yahoo.com", 465, "ssl", "provider.yahoo"),
        "icloud.com" | "me.com" | "mac.com" => g("imap.mail.me.com", 993, "ssl", "smtp.mail.me.com", 587, "starttls", "provider.icloud"),
        "gmx.com" | "gmx.net" | "gmx.de" => g("imap.gmx.com", 993, "ssl", "mail.gmx.com", 465, "ssl", ""),
        "mail.ru" | "bk.ru" | "inbox.ru" | "list.ru" => g("imap.mail.ru", 993, "ssl", "smtp.mail.ru", 465, "ssl", ""),
        "zoho.com" | "zohomail.com" => g("imap.zoho.com", 993, "ssl", "smtp.zoho.com", 465, "ssl", ""),
        "aol.com" => g("imap.aol.com", 993, "ssl", "smtp.aol.com", 465, "ssl", ""),
        "proton.me" | "protonmail.com" => g("127.0.0.1", 1143, "starttls", "127.0.0.1", 1025, "starttls", "provider.proton"),
        _ => g(&format!("imap.{domain}"), 993, "ssl", &format!("smtp.{domain}"), 465, "ssl", ""),
    }
}

/// "Ad Soyad <a@b.c>" → ("Ad Soyad", "a@b.c")
pub fn split_name_addr(s: &str) -> (String, String) {
    let s = s.trim();
    if let (Some(lt), Some(gt)) = (s.rfind('<'), s.rfind('>')) {
        if lt < gt {
            let name = s[..lt].trim().trim_matches('"').trim().to_string();
            let addr = s[lt + 1..gt].trim().to_string();
            return (name, addr);
        }
    }
    if s.contains('@') {
        (String::new(), s.trim_matches(|c| c == '<' || c == '>').to_string())
    } else {
        (s.to_string(), String::new())
    }
}

pub fn display_name(s: &str) -> String {
    let (n, a) = split_name_addr(s);
    if !n.is_empty() {
        n
    } else if !a.is_empty() {
        a
    } else {
        s.to_string()
    }
}

/// Virgül veya noktalı virgülle ayrılmış adres listesini böler (tırnak içini korur) ve her
/// alıcıyı Outlook gibi temizler (akıllı tırnaklar, görünmez karakterler, MIME kodlu adlar,
/// bozuk köşeli parantezler; Exchange iç adresleri DN olarak bırakılır, gönderirken çözülür).
pub fn split_addresses(text: &str) -> Vec<String> {
    let text = address::sanitize(text);
    let (parts, balanced) = split_raw(&text, true);
    // Kapanmamış tırnak ("a@b.com’, c@d.com") virgülleri yutmasın: tırnaklar yok sayılır
    let parts = if balanced { parts } else { split_raw(&text, false).0 };
    parts
        .into_iter()
        .map(|a| address::clean_address(&a))
        .filter(|a| a.contains('@') || address::is_exchange_dn(&split_name_addr(a).1))
        .collect()
}

fn split_raw(text: &str, quotes: bool) -> (Vec<String>, bool) {
    let mut unbalanced = false;
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut in_angle = false;
    for c in text.chars() {
        match c {
            c if quotes && address::is_list_quote(c) && !in_angle => {
                in_q = !in_q;
                cur.push(c);
            }
            '<' => {
                in_angle = true;
                cur.push(c);
            }
            '>' => {
                in_angle = false;
                cur.push(c);
            }
            // Kapanmamış "<" ya da tırnak satır sonunda biter
            '\n' => {
                if in_q {
                    unbalanced = true;
                }
                in_q = false;
                in_angle = false;
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            ',' | ';' if !in_q && !in_angle => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    (out, !in_q && !unbalanced)
}

pub fn bare_addresses(text: &str) -> Vec<String> {
    split_addresses(text).iter().map(|a| split_name_addr(a).1).filter(|a| !a.is_empty()).collect()
}

/// Konu başındaki RE:/FW:/YNT:/İLT: öneklerini temizler.
pub fn normalize_subject(s: &str) -> String {
    let mut s = s.trim().to_string();
    'outer: loop {
        for p in ["re:", "fw:", "fwd:", "ynt:", "yanıt:", "ilt:", "i̇lt:", "ilet:", "aw:", "wg:", "tr:", "rv:"] {
            // Önekin karakter sayısı küçük harfe çevirince değişebilir ("İLT:" → "i̇lt:"),
            // bu yüzden baştaki 1..=n karakter ayrı ayrı karşılaştırılır.
            let n = p.chars().count();
            for k in 1..=n {
                let Some((end, _)) = s.char_indices().nth(k).or(if s.chars().count() == k { Some((s.len(), ' ')) } else { None }) else { break };
                if s[..end].to_lowercase() == p {
                    s = s[end..].trim().to_string();
                    continue 'outer;
                }
            }
        }
        break;
    }
    s
}

pub fn reply_subject(subject: &str, prefix: &str) -> String {
    let base = normalize_subject(subject);
    format!("{prefix}: {base}")
}

pub fn make_snippet(text: &str, n: usize) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        let v = split_addresses("\"Soyad, Ad\" <a@b.com>; c@d.org, geçersiz");
        assert_eq!(v, vec!["\"Soyad, Ad\" <a@b.com>", "c@d.org"]);
        assert_eq!(split_name_addr("Ali Veli <ali@x.com>"), ("Ali Veli".into(), "ali@x.com".into()));
        assert_eq!(bare_addresses("A <a@x.com>, b@y.com"), vec!["a@x.com", "b@y.com"]);
        assert_eq!(split_addresses("“Yılmaz, Ahmet” <a@x.com>; ‘b@y.com’"), vec!["\"Yılmaz, Ahmet\" <a@x.com>", "b@y.com"]);
        assert_eq!(split_addresses("\"a@x.com’, c@d.org"), vec!["a@x.com", "c@d.org"]);
        assert_eq!(split_addresses("a@x.com\u{200B}; Ali <<b@y.com>>"), vec!["a@x.com", "Ali <b@y.com>"]);
    }

    #[test]
    fn subjects() {
        assert_eq!(normalize_subject("RE: Fw: YNT: Toplantı"), "Toplantı");
        assert_eq!(normalize_subject("İLT: YNT: Bütçe"), "Bütçe");
        assert_eq!(normalize_subject("Re:"), "");
        assert_eq!(normalize_subject("İ"), "İ");
        assert_eq!(reply_subject("Re: Merhaba", "YNT"), "YNT: Merhaba");
    }
}
