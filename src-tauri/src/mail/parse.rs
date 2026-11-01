//! MIME ayrıştırma (mail-parser üzerine).

use crate::db::Message;
use mail_parser::{Address, MessageParser, MimeHeaders, PartType};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Attachment {
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
    pub cid: String,
    pub inline: bool,
    #[serde(skip)]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedMail {
    pub subject: String,
    pub sender: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub reply_to: String,
    pub message_id: String,
    pub in_reply_to: String,
    pub references: String,
    pub date: f64,
    pub importance: i64,
    pub text: String,
    pub html: String,
    pub attachments: Vec<Attachment>,
    pub read_receipt_to: String,
    pub auto_submitted: bool,
    pub list_id: bool,
    pub raw: Vec<u8>,
}

fn addr_to_string(a: Option<&Address>) -> String {
    let Some(a) = a else { return String::new() };
    a.iter()
        .filter_map(|x| {
            let email = x.address().unwrap_or("");
            match x.name() {
                Some(n) if !n.is_empty() && !email.is_empty() => {
                    if n.contains(',') || n.contains('"') {
                        Some(format!("\"{}\" <{}>", n.replace('"', "'"), email))
                    } else {
                        Some(format!("{n} <{email}>"))
                    }
                }
                Some(n) if !n.is_empty() => Some(n.to_string()),
                _ if !email.is_empty() => Some(email.to_string()),
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn id_list(v: &mail_parser::HeaderValue) -> String {
    match v {
        mail_parser::HeaderValue::Text(t) => format!("<{t}>"),
        mail_parser::HeaderValue::TextList(l) => {
            l.iter().map(|t| format!("<{t}>")).collect::<Vec<_>>().join(" ")
        }
        _ => String::new(),
    }
}

/// Kodlanmamış 8 bit başlıklar (Türkiye'de sık: "Subject: Yeni E\xFEifre" windows-1254)
/// UTF-8 değilse iletinin karakter kümesine (yoksa windows-1254) göre UTF-8'e çevrilir;
/// aksi halde Türkçe harfler "�" olarak görünür.
fn fix_8bit_headers(raw: &[u8]) -> Option<Vec<u8>> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
        .unwrap_or(raw.len());
    let head = &raw[..end];
    if std::str::from_utf8(head).is_ok() {
        return None;
    }
    let lower = String::from_utf8_lossy(head).to_lowercase();
    let label = lower
        .find("charset=")
        .map(|i| lower[i + 8..].trim_start_matches(['"', '\'']).chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect::<String>())
        .filter(|c| !c.is_empty() && c != "utf-8" && c != "us-ascii")
        .unwrap_or_else(|| "windows-1254".into());
    let enc = encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1254);
    let (text, _, _) = enc.decode(head);
    let mut out = text.into_owned().into_bytes();
    out.extend_from_slice(&raw[end..]);
    Some(out)
}

pub fn parse(raw: &[u8]) -> ParsedMail {
    let mut out = ParsedMail { raw: raw.to_vec(), importance: 1, ..Default::default() };
    let fixed = fix_8bit_headers(raw);
    let src: &[u8] = fixed.as_deref().unwrap_or(raw);
    let Some(msg) = MessageParser::default().parse(src) else {
        out.text = String::from_utf8_lossy(raw).to_string();
        out.date = crate::db::now();
        return out;
    };
    out.subject = msg.subject().unwrap_or("").to_string();
    out.sender = addr_to_string(msg.from());
    out.to = addr_to_string(msg.to());
    out.cc = addr_to_string(msg.cc());
    out.bcc = addr_to_string(msg.bcc());
    out.reply_to = addr_to_string(msg.reply_to());
    out.message_id = msg.message_id().map(|m| format!("<{m}>")).unwrap_or_default();
    out.in_reply_to = id_list(msg.in_reply_to());
    out.references = id_list(msg.references());
    out.date = msg.date().map(|d| d.to_timestamp() as f64).unwrap_or_else(crate::db::now);

    let hdr = |name: &str| -> String {
        msg.header_raw(name).map(|s| s.trim().to_string()).unwrap_or_default()
    };
    let prio = hdr("X-Priority");
    let imp = hdr("Importance").to_lowercase();
    out.importance = if prio.starts_with('1') || prio.starts_with('2') || imp == "high" {
        2
    } else if prio.starts_with('4') || prio.starts_with('5') || imp == "low" {
        0
    } else {
        1
    };
    out.read_receipt_to = hdr("Disposition-Notification-To");
    out.auto_submitted = {
        let a = hdr("Auto-Submitted").to_lowercase();
        !a.is_empty() && a != "no"
    };
    out.list_id = !hdr("List-Id").is_empty() || !hdr("List-Unsubscribe").is_empty();

    let has_html = msg.html_part(0).map(|p| p.is_text_html()).unwrap_or(false);
    let has_text = msg.text_part(0).map(|p| !p.is_text_html()).unwrap_or(false);
    if has_text {
        out.text = msg.body_text(0).map(|c| c.to_string()).unwrap_or_default();
    }
    if has_html {
        out.html = msg.body_html(0).map(|c| c.to_string()).unwrap_or_default();
        if !has_text {
            out.text = super::html::html_to_text(&out.html);
        }
    }

    for (i, part) in msg.attachments().enumerate() {
        let ct = part.content_type();
        let mime = ct
            .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
            .unwrap_or_else(|| "application/octet-stream".into())
            .to_lowercase();
        let is_msg = matches!(part.body, PartType::Message(_));
        let mut name = part.attachment_name().map(|s| s.to_string()).unwrap_or_default();
        if name.is_empty() {
            name = if is_msg {
                let sub = part.message().and_then(|m| m.subject()).unwrap_or("ileti");
                format!("{}.eml", crate::util::safe_filename(sub))
            } else if mime == "text/calendar" {
                "davet.ics".into()
            } else {
                format!("ek-{}", i + 1)
            };
        }
        let inline = part
            .content_disposition()
            .map(|d| d.is_inline())
            .unwrap_or(false)
            || matches!(part.body, PartType::InlineBinary(_));
        let data = part.contents().to_vec();
        out.attachments.push(Attachment {
            index: i,
            size: data.len(),
            name,
            mime: if is_msg { "message/rfc822".into() } else { mime },
            cid: part.content_id().unwrap_or("").trim_matches(|c| c == '<' || c == '>').to_string(),
            inline,
            data,
        });
    }
    expand_tnef(&mut out);
    out
}

/// winmail.dat (TNEF): Outlook gibi içindeki ekler ayrı dosya olarak, gövde ve alıcılar
/// MIME'de yoksa TNEF'teki MAPI bilgisinden alınır. Ek sırası kararlı kalsın diye
/// çıkarılan eklerin kimliği `1000 × (winmail sırası + 1) + n` olur.
fn expand_tnef(out: &mut ParsedMail) {
    let Some(pos) = out
        .attachments
        .iter()
        .position(|a| (a.mime == "application/ms-tnef" || a.name.eq_ignore_ascii_case("winmail.dat")) && super::tnef::is_tnef(&a.data))
    else {
        return;
    };
    let Some(t) = super::tnef::parse(&out.attachments[pos].data) else { return };
    let base = (out.attachments[pos].index + 1) * 1000;
    out.attachments.remove(pos);
    for (j, a) in t.attachments.into_iter().enumerate() {
        let mime = if a.mime == "application/octet-stream" { super::build::guess_mime(&a.name) } else { a.mime };
        out.attachments.push(Attachment { index: base + j, size: a.data.len(), name: a.name, mime, inline: !a.cid.is_empty(), cid: a.cid, data: a.data });
    }
    if out.html.trim().is_empty() && !t.html.trim().is_empty() {
        out.html = t.html;
        if out.text.trim().is_empty() {
            out.text = super::html::html_to_text(&out.html);
        }
    } else if out.text.trim().is_empty() && !t.text.trim().is_empty() {
        out.text = t.text;
    }
    let list = |k: i64| t.recipients.iter().filter(|r| r.2 == k).map(|(n, e, _)| super::address::clean_address(&if n.is_empty() { e.clone() } else { format!("\"{}\" <{e}>", n.replace('"', "'")) })).collect::<Vec<_>>().join(", ");
    if out.to.trim().is_empty() {
        out.to = list(1);
    }
    if out.cc.trim().is_empty() {
        out.cc = list(2);
    }
}

impl ParsedMail {
    /// Görünür ekler (HTML içinde cid ile kullanılan satır içi resimler hariç).
    pub fn real_attachments(&self) -> Vec<&Attachment> {
        self.attachments
            .iter()
            .filter(|a| !(a.inline && !a.cid.is_empty() && self.html.contains(&format!("cid:{}", a.cid))))
            .collect()
    }

    /// `cid:` bağlantılarını data: URI'ye çevirir.
    pub fn html_with_inline_images(&self) -> String {
        use base64::Engine;
        let mut html = self.html.clone();
        for a in &self.attachments {
            if !a.cid.is_empty() && html.contains(&format!("cid:{}", a.cid)) {
                let uri = format!(
                    "data:{};base64,{}",
                    a.mime,
                    base64::engine::general_purpose::STANDARD.encode(&a.data)
                );
                html = html.replace(&format!("cid:{}", a.cid), &uri);
            }
        }
        html
    }

    pub fn into_message(self) -> Message {
        let real = self.real_attachments();
        let has_att = !real.is_empty();
        let att_names = real.iter().map(|a| a.name.replace('\n', " ")).filter(|n| !n.trim().is_empty()).collect::<Vec<_>>().join("\n");
        Message {
            snippet: super::make_snippet(&self.text, 200),
            message_id: self.message_id,
            in_reply_to: self.in_reply_to,
            refs: self.references,
            subject: self.subject,
            sender: self.sender,
            to_addr: self.to,
            cc: self.cc,
            bcc: self.bcc,
            date: self.date,
            body_text: self.text,
            body_html: self.html,
            importance: self.importance,
            has_attachments: has_att,
            att_names,
            raw: self.raw,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_bit_turkish_headers() {
        let raw = b"From: Ali <a@x.com>\r\nSubject: Yeni E\xFEifre Talebi \xDE\xDD\xF0\r\nContent-Type: text/plain; charset=windows-1254\r\n\r\ngovde\r\n";
        let p = parse(raw);
        assert_eq!(p.subject, "Yeni Eşifre Talebi Şİğ");
    }

    #[test]
    fn parse_multipart() {
        let raw = b"From: =?utf-8?q?Ay=C5=9Fe?= <ayse@ornek.com>\r\nTo: b@c.com\r\nSubject: =?utf-8?b?xZ51YmF0IHJhcG9ydQ==?=\r\nDate: Tue, 1 Oct 2024 10:00:00 +0300\r\nMessage-ID: <x1@ornek.com>\r\nX-Priority: 1\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nMerhaba d\xc3\xbcnya\r\n--B\r\nContent-Type: application/pdf; name=\"rapor.pdf\"\r\nContent-Disposition: attachment; filename=\"rapor.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8=\r\n--B--\r\n";
        let p = parse(raw);
        assert_eq!(p.subject, "Şubat raporu");
        assert_eq!(p.sender, "Ayşe <ayse@ornek.com>");
        assert_eq!(p.message_id, "<x1@ornek.com>");
        assert_eq!(p.importance, 2);
        assert!(p.text.contains("Merhaba dünya"));
        assert_eq!(p.attachments.len(), 1);
        assert_eq!(p.attachments[0].name, "rapor.pdf");
        assert_eq!(p.attachments[0].data, b"Hello");
    }
}
