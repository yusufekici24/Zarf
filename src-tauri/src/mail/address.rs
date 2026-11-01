//! Adres temizleme: Outlook'un tanıdığı bozuk/özel biçimdeki alıcıları düzgün
//! "Ad <adres>" biçimine getirir.
//!
//! - Akıllı/tipografik tırnaklar (‘ ’ “ ” „ « » ‹ › ＂ ` ´) ve iç içe tırnaklar
//! - Görünmez karakterler (sıfır genişlikli boşluk, yön işaretleri, BOM...) ve
//!   bölünemez boşluklar
//! - Çift ya da kapanmamış köşeli parantez (`<<a@b>>`, `Ad <a@b`)
//! - MIME kodlu sözcükler (`=?UTF-8?B?...?=`)
//! - `mailto:` / `SMTP:` önekleri
//! - Exchange iç adresleri: `IMCEAEX-...@alan`, `/O=.../CN=...` (EX), `X500:` →
//!   gönderirken dizinden SMTP adresine çözülür (`is_exchange_dn`)

use base64::Engine;

/// Tırnak yerine geçen karakterler
const QUOTES: &[char] = &['"', '\'', '‘', '’', '‚', '‛', '“', '”', '„', '‟', '«', '»', '‹', '›', '`', '´', '＂', '＇', '′', '″'];

fn is_invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}' | '\u{180E}')
}

fn is_space_like(c: char) -> bool {
    matches!(c, '\u{00A0}' | '\u{202F}' | '\u{2000}'..='\u{200A}' | '\u{205F}' | '\u{3000}' | '\t' | '\r' | '\n')
}

/// Görünmez karakterleri siler, özel boşlukları ve tam genişlikli işaretleri sadeleştirir
pub fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !is_invisible(*c))
        .map(|c| match c {
            c if is_space_like(c) => ' ',
            '＠' => '@',
            '＜' => '<',
            '＞' => '>',
            '，' => ',',
            '；' => ';',
            c => c,
        })
        .collect()
}

/// Adres listesinde tırnak açıp kapatan karakter mi (içindeki virgül bölmez)
pub fn is_list_quote(c: char) -> bool {
    matches!(c, '"' | '“' | '”' | '„' | '«' | '»' | '＂')
}

/// MIME kodlu sözcükleri (RFC 2047: `=?charset?B|Q?...?=`) çözer
pub fn decode_mime_words(s: &str) -> String {
    if !s.contains("=?") {
        return s.to_string();
    }
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"=\?([^?\s]+)\?([bBqQ])\?([^?\s]*)\?=").unwrap());
    let mut out = String::new();
    let mut last = 0;
    let mut prev_encoded = false;
    for m in RE.captures_iter(s) {
        let all = m.get(0).unwrap();
        let between = &s[last..all.start()];
        // Kodlu sözcükler arasındaki boşluk atılır (RFC 2047 §6.2)
        if !(prev_encoded && between.trim().is_empty()) {
            out.push_str(between);
        }
        let charset = m[1].split('*').next().unwrap_or("utf-8");
        let bytes: Option<Vec<u8>> = if m[2].eq_ignore_ascii_case("b") {
            base64::engine::general_purpose::STANDARD.decode(m[3].trim_end_matches('=').to_string() + &"=".repeat((4 - m[3].trim_end_matches('=').len() % 4) % 4)).ok()
        } else {
            let mut v = Vec::new();
            let b = m[3].as_bytes();
            let mut i = 0;
            while i < b.len() {
                match b[i] {
                    b'_' => v.push(b' '),
                    b'=' if i + 2 < b.len() => {
                        match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                            Ok(x) => {
                                v.push(x);
                                i += 2;
                            }
                            Err(_) => v.push(b'='),
                        }
                    }
                    x => v.push(x),
                }
                i += 1;
            }
            Some(v)
        };
        match bytes {
            Some(b) => {
                let enc = encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
                out.push_str(&enc.decode(&b).0);
            }
            None => out.push_str(all.as_str()),
        }
        last = all.end();
        prev_encoded = true;
    }
    out.push_str(&s[last..]);
    out
}

/// IMCEAEX kapsüllemesini (`IMCEAEX-_O=ORG_OU=..._CN=ALI@alan`) Exchange DN'ye açar
fn imceaex_to_dn(local: &str) -> String {
    let b = local.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b'/'),
            b'+' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(x) => {
                    out.push(x);
                    i += 2;
                }
                None => out.push(b'+'),
            },
            x => out.push(x),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Exchange iç adresi mi (`/O=...` EX ya da X500 DN): SMTP adresine çözülmesi gerekir
pub fn is_exchange_dn(addr: &str) -> bool {
    let a = addr.trim().to_ascii_lowercase();
    a.starts_with("/o=") || a.starts_with("/cn=")
}

/// Adres kısmını temizler: tırnaklar, boşluklar, önekler; Exchange biçimleri DN'ye açılır
pub fn clean_email(raw: &str) -> String {
    let mut a: String = raw.trim().trim_matches(|c: char| QUOTES.contains(&c) || c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';')).to_string();
    // Önekler (Outlook adres türleri)
    loop {
        let low = a.to_ascii_lowercase();
        let cut = ["mailto:", "smtp:", "x500:", "ex:", "sip:"].iter().find(|p| low.starts_with(**p)).map(|p| p.len());
        match cut {
            Some(n) => a = a[n..].trim().trim_matches(|c: char| QUOTES.contains(&c) || c.is_whitespace()).to_string(),
            None => break,
        }
    }
    if is_exchange_dn(&a) {
        return a.trim().to_string();
    }
    // Adreste boşluk ya da tırnak bulunamaz
    a.retain(|c| !c.is_whitespace() && !QUOTES.contains(&c));
    let a = a.trim_end_matches(['.', ':']).to_string();
    // (Bayt dilimi harf ortasına düşmesin diye get(): "Şükrü" gibi adlar da buradan geçer)
    if a.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("imceaex-")) {
        if let Some(local) = a.rfind('@').and_then(|at| a.get(8..at)) {
            return imceaex_to_dn(local);
        }
    }
    a
}

/// Görünen adı temizler: dış tırnaklar, fazla boşluklar
fn clean_name(raw: &str) -> String {
    let mut n = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let t = n.trim_matches(|c: char| QUOTES.contains(&c) || c.is_whitespace()).to_string();
        if t == n {
            break;
        }
        n = t;
    }
    n
}

/// Tek alıcıyı düzgün "Ad <adres>" (ya da yalnızca adres) biçimine getirir.
/// Adres bulunamazsa (yalnızca ad yazılmış) temizlenmiş metin döner.
pub fn clean_address(entry: &str) -> String {
    let s = decode_mime_words(&sanitize(entry));
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let (name, email) = if let Some(lt) = s.find('<') {
        // "Ad <adres>", "<<adres>>", kapanmamış "Ad <adres", "“Ad <adres>”"
        let inner = s[lt..].trim_start_matches('<');
        let inner = inner.split('>').next().unwrap_or(inner);
        (s[..lt].to_string(), clean_email(inner))
    } else {
        let bare = clean_email(s);
        let words = s.trim_matches(|c: char| QUOTES.contains(&c) || c.is_whitespace()).split_whitespace().count();
        if (bare.contains('@') && words <= 1) || is_exchange_dn(&bare) {
            (String::new(), bare)
        } else if let Some(tok) = s.split_whitespace().rev().find(|w| w.contains('@')) {
            // "Ad Soyad adres@alan" (köşeli parantezsiz)
            let at = s.rfind(tok).unwrap_or(0);
            (s[..at].to_string(), clean_email(tok))
        } else {
            (s.to_string(), String::new())
        }
    };
    let mut name = clean_name(&decode_mime_words(&name));
    if email.is_empty() {
        return name;
    }
    if name.eq_ignore_ascii_case(&email) || clean_email(&name).eq_ignore_ascii_case(&email) {
        name.clear();
    }
    if name.is_empty() {
        email
    } else if name.contains([',', ';', '"', '<', '>', '@', '(', ')']) {
        format!("\"{}\" <{}>", name.replace('"', "'"), email)
    } else {
        format!("{name} <{email}>")
    }
}

/// Exchange DN'den çözümlemede denenecek takma ad (son "CN=" değeri; yeni sunucuların
/// başına koyduğu 32 haneli kimlik ve "-" atılır)
pub fn dn_alias(dn: &str) -> String {
    let low = dn.to_ascii_lowercase();
    let Some(i) = low.rfind("/cn=") else { return String::new() };
    let v = &dn[i + 4..];
    match v.split_once('-') {
        Some((pre, rest)) if pre.len() >= 8 && pre.chars().all(|c| c.is_ascii_hexdigit()) && !rest.is_empty() => rest.to_string(),
        _ => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlook_address_forms() {
        let e = "kullanici@firma.com";
        let n = format!("Ahmet Yılmaz <{e}>");
        let cases: &[(&str, &str)] = &[
            ("‘kullanici@firma.com’", e),
            ("“kullanici@firma.com”", e),
            ("‘“kullanici@firma.com”’", e),
            ("“‘kullanici@firma.com’”", e),
            ("'“kullanici@firma.com”'", e),
            ("\"‘kullanici@firma.com’\"", e),
            ("„kullanici@firma.com“", e),
            ("«kullanici@firma.com»", e),
            ("‹kullanici@firma.com›", e),
            ("＂kullanici@firma.com＂", e),
            ("`kullanici@firma.com`", e),
            ("´kullanici@firma.com´", e),
            ("’kullanici@firma.com‘", e),
            ("\"kullanici@firma.com’", e),
            ("‘kullanici@firma.com\"", e),
            ("“Ahmet Yılmaz” <kullanici@firma.com>", &n),
            ("‘Ahmet Yılmaz’ <kullanici@firma.com>", &n),
            ("\"Ahmet Yılmaz\" <kullanici@firma.com>", &n),
            ("Ahmet Yılmaz <‘kullanici@firma.com’>", &n),
            ("Ahmet Yılmaz <“kullanici@firma.com”>", &n),
            ("“Ahmet Yılmaz <kullanici@firma.com>”", &n),
            ("Ahmet Yılmaz <<kullanici@firma.com>>", &n),
            ("Ahmet Yılmaz <kullanici@firma.com", &n),
            ("\"Ahmet Yılmaz <kullanici@firma.com>\"", &n),
            ("kullanici@firma.com\u{200B}", e),
            ("kullanici@firma.com\u{200C}", e),
            ("kullanici@firma.com\u{200D}", e),
            ("kullanici@firma.com\u{2060}", e),
            ("kullanici@firma.com\u{00A0}", e),
            ("kullanici@firma.com\u{202F}", e),
            ("\u{FEFF}kullanici@firma.com", e),
            ("kullanici@firma.com\u{200E}", e),
            ("\u{200F}kullanici@firma.com", e),
            ("\u{202A}kullanici@firma.com\u{202C}", e),
            ("\u{202B}kullanici@firma.com", e),
            ("kulla\u{200B}nici@firma.com", e),
            ("mailto:kullanici@firma.com", e),
            ("SMTP:kullanici@firma.com", e),
            ("=?UTF-8?B?QWhtZXQgWcSxbG1heg==?= <kullanici@firma.com>", &n),
            ("=?utf-8?Q?Ahmet_Y=C4=B1lmaz?= <kullanici@firma.com>", &n),
            ("=?UTF-8?B?a3VsbGFuaWNpQGZpcm1hLmNvbQ==?=", e),
            ("Ahmet Yılmaz kullanici@firma.com", &n),
            ("kullanici@firma.com <kullanici@firma.com>", e),
        ];
        // Türkçe harfli adlar (bayt sınırı) çökmemeli
        for inp in ["Ahmet Şık <a@x.com>", "Ahmet Şık", "Şükrü Öztürk <s@x.com>", "Çiğdem Işıklı (Bilgi İşlem) <c@x.com>", "\"Ğ, Ü\" <g@x.com>", "Öööööööö", "İİİİİİİİİ@x.com", "IMCEAEX-Şş@x.com", "IMCEAEX-+Ş_@x.com", "Ş+20Ş"] {
            let _ = clean_address(inp);
            let _ = clean_email(inp);
        }
        assert_eq!(clean_address("Şükrü Öztürk <s@x.com>"), "Şükrü Öztürk <s@x.com>");
        for (inp, want) in cases {
            assert_eq!(&clean_address(inp), want, "girdi: {inp:?}");
        }
    }

    #[test]
    fn exchange_forms() {
        let dn = "/O=ORG/OU=EXCHANGE ADMINISTRATIVE GROUP (FYDIBOHF23SPDLT)/CN=RECIPIENTS/CN=ahmet.yilmaz";
        assert_eq!(clean_address(&format!("IMCEAEX-_O=ORG_OU=EXCHANGE+20ADMINISTRATIVE+20GROUP+20+28FYDIBOHF23SPDLT+29_CN=RECIPIENTS_CN=ahmet+2Eyilmaz@firma.com")), dn);
        assert_eq!(clean_address(&format!("Ahmet Yılmaz <{dn}>")), format!("Ahmet Yılmaz <{dn}>"));
        assert_eq!(clean_address(&format!("X500:{dn}")), dn);
        assert_eq!(clean_address(&format!("EX:{dn}")), dn);
        assert!(is_exchange_dn(dn));
        assert_eq!(dn_alias(dn), "ahmet.yilmaz");
        assert_eq!(dn_alias("/o=ExchangeLabs/ou=x/cn=Recipients/cn=6a1b2c3d4e5f60718293a4b5c6d7e8f9-ahmet.yil"), "ahmet.yil");
    }
}
