//! Çok dilli arayüz desteği.
//!
//! Tüm çeviriler `locales/<dil-kodu>.json` dosyalarındadır ve hem Rust arka ucu
//! hem de Svelte arayüzü tarafından kullanılır. Biçim düz bir anahtar → metin
//! eşlemesidir:
//!
//! ```json
//! { "@name": "Türkçe", "mail.reply": "Yanıtla", "mail.items": "{0} öğe" }
//! ```
//!
//! Yerleşik diller (tr, en) derleme sırasında uygulamaya gömülür. Yeni bir dil
//! eklemek için `locales/` klasörüne yeni bir JSON dosyası eklemek ve `BUILTIN`
//! listesine bir satır yazmak yeterlidir. Yeniden derlemeden dil eklemek için
//! dosya şu klasörlerden birine bırakılabilir:
//! `~/.local/share/mailnix/locales/`, `/usr/share/mailnix/locales/`.

use std::collections::HashMap;
use std::sync::RwLock;

pub type LangMap = HashMap<String, String>;

const BUILTIN: &[(&str, &str)] = &[
    ("tr", include_str!("../../locales/tr.json")),
    ("en", include_str!("../../locales/en.json")),
];

pub const FALLBACK: &str = "en";

struct State {
    current: String,
    langs: HashMap<String, LangMap>,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);

fn parse(text: &str) -> LangMap {
    serde_json::from_str::<HashMap<String, serde_json::Value>>(text)
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn search_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = dirs::data_dir() {
        v.push(d.join("mailnix").join("locales"));
    }
    v.push("/usr/share/mailnix/locales".into());
    v.push("/usr/local/share/mailnix/locales".into());
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            v.push(p.join("locales"));
            v.push(p.join("../share/mailnix/locales"));
        }
    }
    v
}

pub fn init(preferred: Option<&str>) {
    let mut langs: HashMap<String, LangMap> = HashMap::new();
    for (code, text) in BUILTIN {
        langs.insert(code.to_string(), parse(text));
    }
    for dir in search_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let code = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if let Ok(text) = std::fs::read_to_string(&p) {
                langs.entry(code).or_default().extend(parse(&text));
            }
        }
    }
    let current = match preferred {
        Some(p) if !p.is_empty() && p != "auto" && langs.contains_key(p) => p.to_string(),
        _ => detect_system(&langs),
    };
    *STATE.write().unwrap() = Some(State { current, langs });
}

pub fn detect_system(langs: &HashMap<String, LangMap>) -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            for part in v.split(':') {
                let code = part.split(['_', '.', '@']).next().unwrap_or("").to_lowercase();
                if langs.contains_key(&code) {
                    return code;
                }
            }
        }
    }
    FALLBACK.to_string()
}

pub fn set_language(code: &str) -> String {
    let mut g = STATE.write().unwrap();
    if let Some(s) = g.as_mut() {
        if code == "auto" || code.is_empty() {
            s.current = detect_system(&s.langs);
        } else if s.langs.contains_key(code) {
            s.current = code.to_string();
        }
        return s.current.clone();
    }
    FALLBACK.to_string()
}

pub fn current() -> String {
    STATE.read().unwrap().as_ref().map(|s| s.current.clone()).unwrap_or_else(|| FALLBACK.into())
}

/// Tüm diller: kod → çeviri tablosu (arayüze gönderilir).
pub fn all() -> HashMap<String, LangMap> {
    STATE.read().unwrap().as_ref().map(|s| s.langs.clone()).unwrap_or_default()
}

pub fn tr(key: &str) -> String {
    let g = STATE.read().unwrap();
    if let Some(s) = g.as_ref() {
        if let Some(v) = s.langs.get(&s.current).and_then(|m| m.get(key)) {
            return v.clone();
        }
        if let Some(v) = s.langs.get(FALLBACK).and_then(|m| m.get(key)) {
            return v.clone();
        }
    }
    key.to_string()
}

pub fn trf(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut s = tr(key);
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

#[macro_export]
macro_rules! t {
    ($key:expr) => { $crate::i18n::tr($key) };
    ($key:expr, $($arg:expr),+ $(,)?) => {
        $crate::i18n::trf($key, &[$(&$arg as &dyn std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tüm yerleşik diller İngilizce ile aynı anahtarlara sahip olmalı.
    #[test]
    fn locales_are_complete() {
        let en = parse(BUILTIN.iter().find(|(c, _)| *c == "en").unwrap().1);
        assert!(en.len() > 400);
        for (code, text) in BUILTIN {
            let m = parse(text);
            let missing: Vec<_> = en.keys().filter(|k| !m.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{code} dilinde eksik anahtarlar: {missing:?}");
            for (k, v) in &m {
                let want = (0..5).filter(|i| en[k].contains(&format!("{{{i}}}"))).count();
                let got = (0..5).filter(|i| v.contains(&format!("{{{i}}}"))).count();
                assert_eq!(want, got, "{code}: '{k}' yer tutucuları uyuşmuyor");
            }
        }
    }
}
