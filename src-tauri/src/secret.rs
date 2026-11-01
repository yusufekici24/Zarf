//! Parola ve oturum belirteçlerinin güvenli saklanması.
//!
//! 1. Öncelikle sistem anahtarlığı (Secret Service: GNOME Anahtarlığı / KWallet). Anahtarlık
//!    kullanıcının oturum parolasıyla şifrelenir; veritabanında yalnızca kaydın adı durur
//!    ("kr:v1:<ad>"). Outlook'un Windows'ta kullandığı DPAPI'nin karşılığıdır.
//! 2. Anahtarlık yoksa AES-256-GCM ("enc:v1:<nonce+şifreli>"). Anahtar, yalnızca kullanıcının
//!    okuyabildiği (0600) ayrı bir dosyadaki rastgele 32 bayt ile bu bilgisayarın kimliğinden
//!    (machine-id) türetilir: veritabanı başka bir makineye kopyalansa çözülemez.
//! 3. Eski sürümlerin karıştırılmış ("obf:") değerleri okunabilir ve açılışta taşınır.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::OnceLock;

const SERVICE: &str = "Lynora";
/// Uygulamanın eski adı: o adla anahtarlığa yazılmış parolalar okunur ve yeni ada taşınır
const LEGACY_SERVICE: &str = "MailNix";
const KR_PREFIX: &str = "kr:v1:";
const ENC_PREFIX: &str = "enc:v1:";

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Sistem anahtarlığı kullanılabilir mi (bir kez denenir)
fn keyring_ok() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        if std::env::var("LYNORA_NO_KEYRING").is_ok() {
            return false;
        }
        match keyring::Entry::new(SERVICE, "__probe__") {
            Ok(e) => matches!(e.get_password(), Ok(_) | Err(keyring::Error::NoEntry)),
            Err(_) => false,
        }
    })
}

fn key_path() -> PathBuf {
    crate::db::default_data_dir().join(".key")
}

/// Yerel şifreleme anahtarı: dosyadaki rastgele bayt + makine kimliği
fn local_key() -> [u8; 32] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    *KEY.get_or_init(|| {
        let path = key_path();
        let mut raw = std::fs::read(&path).unwrap_or_default();
        if raw.len() != 32 {
            raw = vec![0u8; 32];
            let _ = getrandom::getrandom(&mut raw);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &raw);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        let machine = std::fs::read_to_string("/etc/machine-id").or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id")).unwrap_or_default();
        let mut h = Sha256::new();
        // Anahtar türetme sabiti: DEĞİŞTİRİLMEMELİ
        // (değişirse daha önce şifrelenmiş parolalar çözülemez)
        h.update(b"lynora-secret-v1");
        h.update(&raw);
        h.update(machine.trim().as_bytes());
        h.finalize().into()
    })
}

/// Yerel anahtarla şifreler (anahtarlıktan bağımsız; ör. oturum belirteçleri)
pub fn encrypt_local(secret: &str) -> String {
    if secret.is_empty() {
        return String::new();
    }
    let cipher = Aes256Gcm::new_from_slice(&local_key()).expect("anahtar");
    let mut nonce = [0u8; 12];
    let _ = getrandom::getrandom(&mut nonce);
    let Ok(ct) = cipher.encrypt(Nonce::from_slice(&nonce), secret.as_bytes()) else { return String::new() };
    let mut out = nonce.to_vec();
    out.extend(ct);
    format!("{ENC_PREFIX}{}", b64().encode(out))
}

fn decrypt_local(s: &str) -> Option<String> {
    let data = b64().decode(s).ok()?;
    if data.len() < 13 {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(&local_key()).ok()?;
    let pt = cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()?;
    String::from_utf8(pt).ok()
}

/// Gizli bilgiyi saklar; veritabanına yazılacak değeri döndürür.
/// `name`: anahtarlıktaki kaydın adı (ör. "ali@kurum.com.tr|password").
pub fn seal(name: &str, secret: &str) -> String {
    if secret.is_empty() {
        return String::new();
    }
    if keyring_ok() {
        if let Ok(e) = keyring::Entry::new(SERVICE, name) {
            if e.set_password(secret).is_ok() {
                return format!("{KR_PREFIX}{name}");
            }
        }
    }
    encrypt_local(secret)
}

/// Saklanan değerden gizli bilgiyi çözer (eski biçimler dahil)
pub fn open(stored: &str) -> String {
    if stored.is_empty() {
        return String::new();
    }
    if let Some(name) = stored.strip_prefix(KR_PREFIX) {
        if let Ok(p) = keyring::Entry::new(SERVICE, name).and_then(|e| e.get_password()) {
            return p;
        }
        // Eski adla saklanmış: yeni ada taşı (bir sonraki açılışta doğrudan bulunur)
        if let Ok(old) = keyring::Entry::new(LEGACY_SERVICE, name) {
            if let Ok(p) = old.get_password() {
                if keyring::Entry::new(SERVICE, name).and_then(|e| e.set_password(&p)).is_ok() {
                    let _ = old.delete_credential();
                }
                return p;
            }
        }
        return String::new();
    }
    if let Some(rest) = stored.strip_prefix(ENC_PREFIX) {
        return decrypt_local(rest).unwrap_or_default();
    }
    crate::util::deobfuscate(stored)
}

/// Anahtarlıktaki kaydı siler (hesap silinince ya da parola değişince)
pub fn remove(stored: &str) {
    if let Some(name) = stored.strip_prefix(KR_PREFIX) {
        for svc in [SERVICE, LEGACY_SERVICE] {
            if let Ok(e) = keyring::Entry::new(svc, name) {
                let _ = e.delete_credential();
            }
        }
    }
}

/// Eski (karıştırılmış ya da düz) biçim mi: açılışta güvenli biçime taşınır
pub fn is_legacy(stored: &str) -> bool {
    !stored.is_empty() && !stored.starts_with(KR_PREFIX) && !stored.starts_with(ENC_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_roundtrip_and_legacy() {
        std::env::set_var("LYNORA_NO_KEYRING", "1");
        let dir = std::env::temp_dir().join(format!("lynora-secret-{}", std::process::id()));
        std::env::set_var("LYNORA_DATA_DIR", &dir);
        let s = seal("x@y|password", "Gizli Parola ğüş!");
        assert!(s.starts_with(ENC_PREFIX));
        assert!(!s.contains("Gizli"));
        assert_eq!(open(&s), "Gizli Parola ğüş!");
        // Her şifrelemede farklı (rastgele nonce)
        assert_ne!(seal("x@y|password", "a"), seal("x@y|password", "a"));
        // Eski biçim okunur ve taşınması gerektiği bilinir
        let old = crate::util::obfuscate("eski");
        assert!(is_legacy(&old));
        assert_eq!(open(&old), "eski");
        // Bozuk veri boş döner (hata değil)
        assert_eq!(open("enc:v1:AAAA"), "");
    }
}
