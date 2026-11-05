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

/// Görüntülemeden önce HTML'i izin listesiyle temizler (ammonia, tarayıcıyla aynı HTML5
/// ayrıştırıcısı): yalnızca bilinen biçim etiketleri ve öznitelikleri kalır. Betik, olay
/// işleyicisi, form, iframe, gömülü nesne, <base>, <meta>, <link> ve javascript: bağlantıları
/// her yazımıyla (kodlanmış, bölünmüş, büyük/küçük harf) gider. E-postanın görünüşü için stiller
/// (öznitelik ve <style> blokları), sınıflar, eski tablo/yazı tipi öznitelikleri ve gömülü
/// görseller (cid:, data:) korunur. Arayüz ayrıca HTML'i betiksiz, CSP'li bir iframe'de gösterir.
pub fn sanitize(html: &str) -> String {
    static CLEANER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    static BODY: OnceLock<Regex> = OnceLock::new();
    let cleaner = CLEANER.get_or_init(|| {
        let mut b = ammonia::Builder::default();
        b.add_tags(["font", "center", "big", "nobr", "style"])
            .rm_clean_content_tags(["style"])
            // Başlık etiketi gider ama metni iletinin başında görünmesin
            .add_clean_content_tags(["title"])
            .add_generic_attributes([
                "style", "class", "id", "dir", "align", "valign", "width", "height", "bgcolor", "color", "border",
                "cellpadding", "cellspacing", "face", "size", "colspan", "rowspan", "nowrap", "background",
            ])
            .add_tag_attributes("a", ["name", "target"])
            .add_tag_attributes("img", ["border", "hspace", "vspace"])
            .add_url_schemes(["cid", "data"])
            // data: yalnızca görsellerde (bağlantı data:text/html sayfası açamasın)
            .attribute_filter(|_, attr, value| {
                let data = value.trim_start().get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:"));
                if data && attr != "src" {
                    None
                } else {
                    Some(value.into())
                }
            });
        b
    });
    // Bültenlerin arka planı çoğu zaman <body bgcolor/style>'dadır: temizlikte gövde etiketi
    // gideceği için öznitelikleri içeriği saran bir kapsayıcıya taşınır (onlar da süzülür)
    let wrapped;
    let src = match re(&BODY, r"(?is)<body\b([^>]*)>").captures(html).map(|c| c[1].to_string()) {
        Some(attrs) if !attrs.trim().is_empty() => {
            wrapped = format!("<div{attrs}>{html}</div>");
            wrapped.as_str()
        }
        _ => html,
    };
    cleaner.clean(src).to_string()
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

    /// Eski düzenli ifade temizleyicisinin kaçırdığı yazımlar da gider
    #[test]
    fn sanitize_strips_evasions() {
        for evil in [
            r#"<img/onerror=alert(1) src=x>"#,
            r#"<img src=x OnError = "alert(1)">"#,
            r#"<a href="jav&#x61;script:alert(1)">x</a>"#,
            r#"<a href="  JaVaScRiPt:alert(1)">x</a>"#,
            r#"<a href="java&#09;script:alert(1)">x</a>"#,
            r#"<scr<script>ipt>alert(1)</script>"#,
            r#"<svg onload=alert(1)><circle/></svg>"#,
            r#"<math><mtext><img src=x onerror=alert(1)></mtext></math>"#,
            r#"<iframe src="https://kotu.example"></iframe>"#,
            r#"<object data="x.swf"></object><embed src="x.swf">"#,
            r#"<form action="https://kotu.example"><input name=p type=password><button>Gir</button></form>"#,
            r#"<meta http-equiv="refresh" content="0;url=https://kotu.example">"#,
            r#"<base href="https://kotu.example/">"#,
            r#"<link rel=stylesheet href="https://kotu.example/a.css">"#,
            r#"<a href="data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==">x</a>"#,
            r##"<body onload="alert(1)" bgcolor="#fff">x</body>"##,
        ] {
            let s = sanitize(evil).to_lowercase();
            for bad in ["onerror", "onload", "javascript", "<script", "alert(1)</", "<iframe", "<object", "<embed", "<form", "<input", "<meta", "<base", "<link", "data:text", "<svg"] {
                assert!(!s.contains(bad), "{evil} → {s}");
            }
        }
    }

    /// E-postanın görünüşü korunur: stiller, Word sınıfları, eski öznitelikler, gömülü görseller
    #[test]
    fn sanitize_keeps_mail_formatting() {
        let src = r##"<html><head><title>Bülten</title><style><!-- p.MsoNormal {margin:0cm; font-family:"Calibri"} --></style></head>
<body bgcolor="#eeeeee" style="margin:0"><table width="600" cellpadding="4" cellspacing="0" border="0" bgcolor="#ffffff" align="center"><tr><td valign="top" style="color:#333">
<p class="MsoNormal"><font face="Arial" color="red" size="2">Merhaba</font></p>
<img src="cid:logo@x" width="120" alt="logo"><img src="data:image/png;base64,iVBORw0KGgo=" alt="g">
<a href="https://ornek.com.tr/kampanya" target="_blank"><table><tr><td>Düğme</td></tr></table></a>
<a href="mailto:destek@ornek.com.tr">destek</a><center><b>son</b></center></td></tr></table></body></html>"##;
        let s = sanitize(src);
        for keep in [
            "p.MsoNormal {margin:0cm",
            r##"bgcolor="#eeeeee""##,
            r#"style="margin:0""#,
            r#"width="600""#,
            r#"cellpadding="4""#,
            r#"class="MsoNormal""#,
            r#"<font face="Arial" color="red" size="2">"#,
            r#"src="cid:logo@x""#,
            r#"src="data:image/png;base64,iVBORw0KGgo=""#,
            r#"href="mailto:destek@ornek.com.tr""#,
            "<center>",
        ] {
            assert!(s.contains(keep), "{keep} kaybolmuş:\n{s}");
        }
        assert!(!s.contains("Bülten"), "başlık metni gövdede görünmemeli");
        // Ön yüzdeki düğme düzeltmesi (bağlantının içindeki tablo) ham kalıbı görmeye devam eder
        assert!(regex::Regex::new(r"(?s)<a\b[^>]*>(?:(?:[^<]|<[^/]|</[^a])*?)<table").unwrap().is_match(&s), "{s}");
    }
}
