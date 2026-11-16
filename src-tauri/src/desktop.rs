//! Masaüstü ile bütünleşme: bağlantı/dosya açma, masaüstü bildirimi ve tek örnek.
//!
//! Tauri'nin opener / notification / single-instance eklentileri yerine: o eklentiler
//! bakımı bırakılmış zvariant / zbus_names (RUSTSEC-2026-0370..0372) getiriyordu. Burada
//! masaüstünün kendi araçları (xdg-open / gio), sistemin libdbus'ı (notify-rust "d") ve
//! kullanıcıya özel bir yerel soket kullanılır.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ------------------------------------------------------------------ açma

/// AppImage içinden başlatılan programlara AppImage'ın kütüphane yolları geçmesin
fn clean_env(cmd: &mut Command) {
    if std::env::var_os("APPIMAGE").is_some() {
        for k in ["LD_LIBRARY_PATH", "LD_PRELOAD", "GDK_PIXBUF_MODULE_FILE", "GIO_EXTRA_MODULES", "GSETTINGS_SCHEMA_DIR", "GTK_PATH", "GTK_EXE_PREFIX", "GTK_DATA_PREFIX", "PYTHONHOME", "PYTHONPATH", "PERLLIB", "QT_PLUGIN_PATH", "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0"] {
            cmd.env_remove(k);
        }
    }
}

/// Masaüstünün varsayılan uygulamasıyla açar (xdg-open, yoksa gio / KDE)
fn launch(target: &str) -> Result<(), String> {
    let tries: [(&str, &[&str]); 4] = [("xdg-open", &[]), ("gio", &["open"]), ("kioclient5", &["exec"]), ("kde-open5", &[])];
    let mut last = String::from("xdg-open bulunamadı");
    for (prog, pre) in tries {
        let mut cmd = Command::new(prog);
        cmd.args(pre).arg(target).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        clean_env(&mut cmd);
        match cmd.spawn() {
            Ok(mut child) => {
                // Zombi süreç kalmasın
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(e) => last = format!("{prog}: {e}"),
        }
    }
    Err(last)
}

/// Yalnızca güvenli şemalar (web, e-posta, telefon, ftp) dışarıda açılır
pub fn is_openable_url(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:", "tel:", "ftp://"].iter().any(|p| l.starts_with(p))
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !is_openable_url(url) {
        return Err(format!("izin verilmeyen bağlantı: {url}"));
    }
    launch(url.trim())
}

/// Doğrudan çalıştırılabilen türler: Outlook'un tehlikeli ek listesi gibi açılmaz,
/// kullanıcı "Farklı kaydet" ile kaydedebilir
const BLOCKED_EXT: &[&str] = &[
    "desktop", "sh", "bash", "zsh", "csh", "ksh", "run", "bin", "appimage", "deb", "rpm", "jar", "py", "pyc", "pl", "rb", "php", "exe", "com", "bat", "cmd", "msi",
    "msp", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "scr", "pif", "cpl", "hta", "lnk", "reg", "inf", "dll", "so", "ko", "elf", "command", "app",
];

pub fn is_blocked_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if BLOCKED_EXT.contains(&ext.as_str()) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// Yalnızca uygulamanın ek klasöründeki dosyalar açılır (rastgele yol açılamaz)
pub fn open_attachment_file(path: &str) -> Result<(), String> {
    let base = crate::util::temp_dir().canonicalize().map_err(|e| e.to_string())?;
    let p = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    if !p.starts_with(&base) || !p.is_file() {
        return Err(crate::t!("error.open_not_allowed"));
    }
    if is_blocked_file(&p) {
        return Err(crate::t!("error.attachment_blocked"));
    }
    launch(&p.to_string_lossy())
}

// ------------------------------------------------------------------ bildirim

pub fn notify(title: &str, body: &str, sound: bool) {
    let mut n = notify_rust::Notification::new();
    n.appname("Zarf").summary(title).body(body).icon("lynora");
    if sound {
        n.sound_name("message-new-email");
    }
    let _ = n.show();
}

// ------------------------------------------------------------------ tek örnek

/// Kullanıcıya ve veri klasörüne özel soket ($XDG_RUNTIME_DIR, yalnızca kullanıcı erişir)
fn socket_path() -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    crate::db::default_data_dir().hash(&mut h);
    let name = format!("lynora-{:08x}.sock", h.finish() as u32);
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()) {
        Some(d) => d.join(name),
        None => {
            #[cfg(unix)]
            let uid = {
                use std::os::unix::fs::MetadataExt;
                std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0)
            };
            #[cfg(not(unix))]
            let uid = 0;
            std::env::temp_dir().join(format!("lynora-{uid}")).join(name)
        }
    }
}

/// Göreli dosya yolları çalışan örnekte de doğru açılsın diye mutlak yapılır
fn absolute_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|a| {
            let p = Path::new(a);
            if a.contains("://") || a.to_ascii_lowercase().starts_with("mailto:") || p.is_absolute() {
                a.clone()
            } else {
                std::env::current_dir().map(|d| d.join(p).to_string_lossy().to_string()).unwrap_or_else(|_| a.clone())
            }
        })
        .collect()
}

/// Uygulama zaten çalışıyorsa argümanları ona iletir ve `true` döner (bu süreç kapanmalı).
/// Çalışmıyorsa dinlemeye başlar; gelen argümanlar `on_args` ile işlenir.
#[cfg(unix)]
pub fn single_instance(args: &[String], on_args: impl Fn(Vec<String>) + Send + 'static) -> bool {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = socket_path();
    if let Ok(mut s) = UnixStream::connect(&path) {
        let msg = serde_json::to_vec(&absolute_args(args)).unwrap_or_default();
        let _ = s.write_all(&msg);
        let _ = s.shutdown(std::net::Shutdown::Write);
        // Çalışan örnek aldığını bildirsin (en çok 3 sn)
        let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(3)));
        let mut ack = [0u8; 2];
        let _ = s.read(&mut ack);
        return true;
    }
    // Eski (çökmüş örnekten kalan) soket
    let _ = std::fs::remove_file(&path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let Ok(listener) = UnixListener::bind(&path) else { return false };
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let _ = std::thread::Builder::new().name("lynora-instance".into()).spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = Vec::new();
            let _ = (&mut s).take(64 * 1024).read_to_end(&mut buf);
            let args: Vec<String> = serde_json::from_slice(&buf).unwrap_or_default();
            let _ = s.write_all(b"ok");
            on_args(args);
        }
    });
    false
}

#[cfg(not(unix))]
pub fn single_instance(_args: &[String], _on_args: impl Fn(Vec<String>) + Send + 'static) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_and_file_rules() {
        assert!(is_openable_url("https://ornek.com/a?b=1"));
        assert!(is_openable_url("MAILTO:a@b.c"));
        assert!(!is_openable_url("file:///etc/passwd"));
        assert!(!is_openable_url("javascript:alert(1)"));
        assert!(!is_openable_url("/usr/bin/xterm"));
        let dir = std::env::temp_dir().join(format!("lynora-desktop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, blocked) in [("rapor.pdf", false), ("kur.desktop", true), ("x.SH", true), ("a.AppImage", true), ("fatura.xlsx", false)] {
            let p = dir.join(name);
            std::fs::write(&p, b"x").unwrap();
            assert_eq!(is_blocked_file(&p), blocked, "{name}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.join("calistir.txt");
            std::fs::write(&p, b"x").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(is_blocked_file(&p));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Gerçek oturum veriyolunda bildirim gönderir (dbus-run-session + dbus-monitor ile izlenir)
    #[test]
    #[ignore]
    fn notify_live() {
        notify("Zarf deneme", "Yeni ileti: Bütçe & Planlama", true);
    }

    #[cfg(unix)]
    #[test]
    fn single_instance_forwards_args() {
        let dir = std::env::temp_dir().join(format!("lynora-si-{}", std::process::id()));
        std::env::set_var("LYNORA_DATA_DIR", &dir);
        std::env::set_var("XDG_RUNTIME_DIR", std::env::temp_dir());
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(!single_instance(&[], move |a| tx.send(a).unwrap()));
        assert!(single_instance(&["/tmp/a.msg".into(), "mailto:x@y.z".into()], |_| {}));
        let got = rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
        assert_eq!(got, vec!["/tmp/a.msg".to_string(), "mailto:x@y.z".to_string()]);
        let _ = std::fs::remove_file(socket_path());
    }
}
