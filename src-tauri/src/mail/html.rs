//! HTML ↔ düz metin dönüşümleri.

use regex::Regex;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

pub fn decode_entities(s: &str) -> String {
    static ENT: OnceLock<Regex> = OnceLock::new();
    re(&ENT, r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);")
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
                return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default();
            }
            if let Some(dec) = e.strip_prefix('#') {
                return dec.parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default();
            }
            match e {
                "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" => "'",
                "nbsp" => " ", "copy" => "©", "reg" => "®", "euro" => "€", "hellip" => "…",
                "mdash" => "—", "ndash" => "–", "laquo" => "«", "raquo" => "»", "bull" => "•",
                "rsquo" => "’", "lsquo" => "‘", "rdquo" => "”", "ldquo" => "“", "trade" => "™",
                _ => return c[0].to_string(),
            }
            .to_string()
        })
        .to_string()
}

pub fn html_to_text(src: &str) -> String {
    static R1: OnceLock<Regex> = OnceLock::new();
    static R2: OnceLock<Regex> = OnceLock::new();
    static R3: OnceLock<Regex> = OnceLock::new();
    static R4: OnceLock<Regex> = OnceLock::new();
    static R5: OnceLock<Regex> = OnceLock::new();
    static R6: OnceLock<Regex> = OnceLock::new();
    static R7: OnceLock<Regex> = OnceLock::new();
    static R8: OnceLock<Regex> = OnceLock::new();
    if src.is_empty() {
        return String::new();
    }
    let s = re(&R1, r"(?is)<(script|style|head|title)[^>]*>.*?</(script|style|head|title)>").replace_all(src, "");
    let s = re(&R8, r"(?s)<!--.*?-->").replace_all(&s, "");
    let s = re(&R2, r"(?i)<br\s*/?>").replace_all(&s, "\n");
    let s = re(&R3, r"(?i)</(p|div|tr|li|h[1-6]|table|blockquote|ul|ol)>").replace_all(&s, "\n");
    let s = re(&R4, r"(?i)<li[^>]*>").replace_all(&s, "• ");
    let s = re(&R7, r"(?i)</t[dh]>").replace_all(&s, "\t");
    let s = re(&R5, r"<[^>]+>").replace_all(&s, "");
    let s = decode_entities(&s);
    let lines: Vec<String> = s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let joined = lines.join("\n");
    re(&R6, r"\n{3,}").replace_all(&joined, "\n\n").trim().to_string()
}

pub fn text_to_html(text: &str) -> String {
    static URL: OnceLock<Regex> = OnceLock::new();
    let esc = crate::util::html_escape(text);
    let linked = re(&URL, r#"(https?://[^\s<"]+)"#).replace_all(&esc, r#"<a href="$1">$1</a>"#);
    linked.replace('\n', "<br>\n")
}

/// Görüntülemeden önce betik ve olay işleyicilerini temizler
/// (arayüz ayrıca HTML'i korumalı bir iframe içinde gösterir).
pub fn sanitize(html: &str) -> String {
    static S1: OnceLock<Regex> = OnceLock::new();
    static S2: OnceLock<Regex> = OnceLock::new();
    static S3: OnceLock<Regex> = OnceLock::new();
    let s = re(&S1, r"(?is)<(script|iframe|object|embed|applet|form|meta[^>]*refresh)[^>]*>(.*?</(script|iframe|object|embed|applet|form)>)?").replace_all(html, "");
    let s = re(&S2, r#"(?i)\son[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#).replace_all(&s, "");
    re(&S3, r#"(?i)(href|src)\s*=\s*(["']?)\s*javascript:"#).replace_all(&s, "$1=$2#").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        let t = html_to_text("<html><style>x{}</style><p>Merhaba&nbsp;<b>dünya</b></p><ul><li>bir</li><li>iki</li></ul>");
        assert_eq!(t, "Merhaba dünya\n• bir\n• iki");
        assert_eq!(text_to_html("a<b\nhttp://x.com"), "a&lt;b<br>\n<a href=\"http://x.com\">http://x.com</a>");
        let s = sanitize(r#"<p onclick="x()">a</p><script>alert(1)</script><a href="javascript:x">l</a>"#);
        assert!(!s.contains("onclick") && !s.contains("script") && !s.contains("javascript"));
    }
}
