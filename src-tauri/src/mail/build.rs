//! Giden ileti oluşturma (mail-builder üzerine).

use crate::db::Account;
use mail_builder::headers::address::Address;
use mail_builder::headers::raw::Raw;
use mail_builder::MessageBuilder;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct OutAttachment {
    pub name: String,
    pub mime: String,
    /// Diskteki dosya yolu
    pub path: Option<String>,
    /// İletilen bir iletideki ek: (ileti kimliği, ek sırası)
    pub source_msg: Option<i64>,
    pub source_index: Option<usize>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct ComposedMail {
    pub account_id: Option<i64>,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub attachments: Vec<OutAttachment>,
    pub in_reply_to: String,
    pub references: String,
    pub importance: i64,
    pub read_receipt: bool,
    pub send_at: Option<f64>,
    pub ics: Option<String>,
    pub draft_id: Option<i64>,
}

fn to_address(list: &str) -> Option<Address<'static>> {
    let items: Vec<Address<'static>> = super::split_addresses(list)
        .into_iter()
        .map(|a| {
            let (n, e) = super::split_name_addr(&a);
            Address::new_address(if n.is_empty() { None } else { Some(n) }, e)
        })
        .collect();
    match items.len() {
        0 => None,
        1 => items.into_iter().next(),
        _ => Some(Address::new_list(items)),
    }
}

fn strip_id(s: &str) -> String {
    s.trim().trim_matches(|c| c == '<' || c == '>').to_string()
}

pub fn new_message_id(account: Option<&Account>) -> String {
    let domain = account
        .and_then(|a| a.email.rsplit_once('@').map(|(_, d)| d.to_string()))
        .unwrap_or_else(|| "mailnix.local".into());
    format!("{}@{}", uuid::Uuid::new_v4().simple(), domain)
}

/// İletiyi ham RFC 5322 baytlarına dönüştürür.
pub fn build_raw(
    account: Option<&Account>,
    m: &ComposedMail,
    attachments: &[(String, String, Vec<u8>)],
    include_bcc: bool,
) -> Vec<u8> {
    let from = account
        .map(|a| {
            Address::new_address(
                if a.display_name.is_empty() { None } else { Some(a.display_name.clone()) },
                a.email.clone(),
            )
        })
        .unwrap_or_else(|| Address::new_address(None::<String>, "mailnix@localhost".to_string()));
    let mut b = MessageBuilder::new()
        .from(from)
        .subject(m.subject.clone())
        .date(chrono::Utc::now().timestamp())
        .message_id(new_message_id(account))
        .header("X-Mailer", Raw::new("MailNix"));
    if let Some(a) = to_address(&m.to) {
        b = b.to(a);
    }
    if let Some(a) = to_address(&m.cc) {
        b = b.cc(a);
    }
    if include_bcc {
        if let Some(a) = to_address(&m.bcc) {
            b = b.bcc(a);
        }
    }
    if !m.in_reply_to.trim().is_empty() {
        b = b.in_reply_to(strip_id(&m.in_reply_to));
    }
    if !m.references.trim().is_empty() {
        let refs: Vec<String> = m.references.split_whitespace().map(strip_id).collect();
        b = b.references(refs);
    }
    match m.importance {
        2 => {
            b = b.header("X-Priority", Raw::new("1 (Highest)")).header("Importance", Raw::new("High"));
        }
        0 => {
            b = b.header("X-Priority", Raw::new("5 (Lowest)")).header("Importance", Raw::new("Low"));
        }
        _ => {}
    }
    if m.read_receipt {
        if let Some(a) = account {
            b = b.header("Disposition-Notification-To", Raw::new(a.email.clone()));
        }
    }
    b = b.text_body(if m.text.is_empty() { super::html::html_to_text(&m.html) } else { m.text.clone() });
    if !m.html.trim().is_empty() {
        // Yapıştırılan/eklenen resimler (data: adresleri) satır içi ek olur (cid): Outlook ve
        // çoğu istemci HTML içindeki data: resimlerini göstermez
        let (html, images) = extract_data_images(&m.html);
        b = b.html_body(html);
        for (cid, mime, data) in images {
            b = b.inline(mime, cid, data);
        }
    }
    if let Some(ics) = &m.ics {
        b = b.attachment("text/calendar; method=REQUEST; charset=utf-8", "davet.ics", ics.clone().into_bytes());
    }
    for (name, mime, data) in attachments {
        let mime = if mime.is_empty() { "application/octet-stream".to_string() } else { mime.clone() };
        b = b.attachment(mime, name.clone(), data.clone());
    }
    b.write_to_vec().unwrap_or_default()
}

/// HTML'deki `<img src="data:image/...;base64,...">` resimlerini `cid:` başvurusuna çevirir.
/// Döner: (yeni HTML, [(cid, mime, veri)])
pub fn extract_data_images(html: &str) -> (String, Vec<(String, String, Vec<u8>)>) {
    use base64::Engine;
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)(src\s*=\s*["'])data:(image/[a-z0-9.+-]+);base64,([a-z0-9+/=\s]+)(["'])"#).unwrap());
    let mut images = Vec::new();
    let out = RE.replace_all(html, |c: &regex::Captures| {
        let clean: String = c[3].chars().filter(|ch| !ch.is_whitespace()).collect();
        match base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()) {
            Ok(data) => {
                let cid = format!("image{:03}.{}@mailnix", images.len() + 1, uuid::Uuid::new_v4().simple());
                images.push((cid.clone(), c[2].to_lowercase(), data));
                format!("{}cid:{cid}{}", &c[1], &c[4])
            }
            Err(_) => c[0].to_string(),
        }
    });
    (out.into_owned(), images)
}

/// Yalnızca düz metinli basit ileti (hoş geldiniz iletisi, otomatik yanıtlar).
pub fn simple_raw(from: &str, to: &str, subject: &str, body: &str) -> Vec<u8> {
    let (fname, femail) = super::split_name_addr(from);
    let mut b = MessageBuilder::new()
        .from(Address::new_address(if fname.is_empty() { None } else { Some(fname) }, femail))
        .subject(subject.to_string())
        .date(chrono::Utc::now().timestamp())
        .message_id(new_message_id(None))
        .text_body(body.to_string());
    if let Some(a) = to_address(to) {
        b = b.to(a);
    } else {
        b = b.header("To", Raw::new(to.to_string()));
    }
    b.write_to_vec().unwrap_or_default()
}

/// Tahmini MIME türü (dosya uzantısından).
pub fn guess_mime(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "ics" => "text/calendar",
        "vcf" => "text/vcard",
        "eml" => "message/rfc822",
        "msg" => "application/vnd.ms-outlook",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "json" => "application/json",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_images_become_inline_parts() {
        let (h, imgs) = extract_data_images(r#"<p>a</p><img src="data:image/png;base64,iVBORw0K" alt=""><img src='cid:x'>"#);
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].1, "image/png");
        assert!(h.contains(&format!("src=\"cid:{}\"", imgs[0].0)));
        assert!(h.contains("src='cid:x'"));
    }

    #[test]
    fn build_and_reparse() {
        let acc = Account { email: "ben@ornek.com".into(), display_name: "Ben Biri".into(), ..Default::default() };
        let m = ComposedMail {
            to: "Ali <ali@x.com>, veli@y.com".into(),
            cc: "cc@z.com".into(),
            bcc: "gizli@z.com".into(),
            subject: "Toplantı notları".into(),
            text: "Merhaba,\nekte rapor var.".into(),
            html: "<p>Merhaba,<br><b>ekte</b> rapor var.</p>".into(),
            in_reply_to: "<abc@x.com>".into(),
            importance: 2,
            ..Default::default()
        };
        let raw = build_raw(Some(&acc), &m, &[("rapor.txt".into(), "text/plain".into(), b"veri".to_vec())], false);
        let p = super::super::parse::parse(&raw);
        assert_eq!(p.subject, "Toplantı notları");
        assert_eq!(p.sender, "Ben Biri <ben@ornek.com>");
        assert!(p.to.contains("ali@x.com") && p.to.contains("veli@y.com"));
        assert!(p.bcc.is_empty());
        assert_eq!(p.in_reply_to, "<abc@x.com>");
        assert_eq!(p.importance, 2);
        assert!(p.html.contains("<b>ekte</b>"));
        assert_eq!(p.attachments.len(), 1);
        assert_eq!(p.attachments[0].data, b"veri");
    }
}
