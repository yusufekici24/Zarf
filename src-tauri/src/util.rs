//! Genel yardımcılar.

use base64::Engine;

const OBF_PREFIX: &str = "obf:";

/// Parolayı veritabanında düz metin olarak tutmamak için basit gizleme.
/// (Veritabanı dosyası yalnızca kullanıcının okuyabileceği izinlerle saklanır.)
pub fn obfuscate(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut bytes: Vec<u8> = s.as_bytes().to_vec();
    bytes.reverse();
    for (i, b) in bytes.iter_mut().enumerate() {
        *b ^= 0x5A ^ (i as u8).wrapping_mul(31);
    }
    format!("{OBF_PREFIX}{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub fn deobfuscate(s: &str) -> String {
    let Some(rest) = s.strip_prefix(OBF_PREFIX) else { return s.to_string() };
    let Ok(mut bytes) = base64::engine::general_purpose::STANDARD.decode(rest) else {
        return String::new();
    };
    for (i, b) in bytes.iter_mut().enumerate() {
        *b ^= 0x5A ^ (i as u8).wrapping_mul(31);
    }
    bytes.reverse();
    String::from_utf8(bytes).unwrap_or_default()
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn safe_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|\r\n\t".contains(c) { '_' } else { c })
        .collect();
    out = out.trim_matches(|c: char| c == ' ' || c == '.').to_string();
    if out.chars().count() > 120 {
        out = out.chars().take(120).collect();
    }
    if out.is_empty() {
        "adsiz".into()
    } else {
        out
    }
}

/// Yerel saatle `YYYY-MM-DD HH:MM`.
pub fn fmt_local(ts: f64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|d| d.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_default()
}

pub fn temp_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("lynora-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscation_roundtrip() {
        for p in ["", "parola", "çok gizli şifre ığüşöç", "a"] {
            assert_eq!(deobfuscate(&obfuscate(p)), p);
        }
        assert_eq!(deobfuscate("duz"), "duz");
    }
}
