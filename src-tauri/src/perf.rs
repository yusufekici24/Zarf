//! Kalıcı performans günlüğü: `~/.local/share/lynora/perf.log` (1 MB'ı geçince `perf.log.1`'e taşınır).
//!
//! Her zaman açıktır ve yalnızca yavaş olayları yazar (kişisel içerik yazmaz: adres, konu, gövde, parola yok):
//! açılışta sistem bilgisi ve pencerenin ne zaman gösterildiği, arayüzün kilitlendiği süreler ve geç işlenen
//! tıklamalar (ön yüzden `perf_log`), yavaş ileti açılışı, yavaş Exchange çağrıları. Kullanıcıdan "yavaş"
//! bildirimi gelince bu dosya istenir. `ZARF_PERF=1` ile ayrıca her ölçüm stderr'e yazılır.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());
const MAX_BYTES: u64 = 1_000_000;

/// Süreç başlangıcı (main'in ilk satırında çağrılır)
pub fn mark_start() {
    let _ = START.set(Instant::now());
}

/// Süreç başlangıcından beri geçen süre (ms)
pub fn since_start() -> u128 {
    START.get().map(|s| s.elapsed().as_millis()).unwrap_or(0)
}

pub fn path() -> PathBuf {
    crate::db::default_data_dir().join("perf.log")
}

/// Bir satır ekler (zaman damgalı). Hata olursa sessizce geçer: günlük uygulamayı bozmamalı.
pub fn log(line: &str) {
    if std::env::var_os("ZARF_PERF").is_some() {
        eprintln!("zarf perf: {line}");
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), line.replace('\n', " "));
    }
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Açılışta bir kez: sürüm, işlemci, bellek, sanal makine, ekran kartı sürücüsü ve WebKit çizim ayarları
pub fn log_system() {
    let cpu = read("/proc/cpuinfo").lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).unwrap_or("?").trim().to_string();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let mem_mb = read("/proc/meminfo").lines().find(|l| l.starts_with("MemTotal")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) / 1024;
    let vendor = read("/sys/class/dmi/id/sys_vendor").trim().to_string();
    let product = read("/sys/class/dmi/id/product_name").trim().to_string();
    let os = read("/etc/os-release").lines().find(|l| l.starts_with("PRETTY_NAME=")).map(|l| l[12..].trim_matches('"').to_string()).unwrap_or_default();
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let no_gpu = crate::db::default_data_dir().join("no-gpu").exists();
    log(&format!(
        "sistem sürüm={} os=\"{os}\" cpu=\"{cpu}\" çekirdek={cores} bellek_mb={mem_mb} makine=\"{vendor} {product}\" nvidia={} \
         oturum={} gdk={} dmabuf_kapalı={} compositing_kapalı={} donanım_hızlandırma_kapalı={no_gpu}",
        env!("CARGO_PKG_VERSION"),
        std::path::Path::new("/proc/driver/nvidia").exists(),
        env("XDG_SESSION_TYPE"),
        env("GDK_BACKEND"),
        !env("WEBKIT_DISABLE_DMABUF_RENDERER").is_empty(),
        !env("WEBKIT_DISABLE_COMPOSITING_MODE").is_empty(),
    ));
}

#[cfg(test)]
mod tests {
    /// Satır zaman damgasıyla eklenir, satır sonları tek satıra iner; sistem satırı yazılır
    #[test]
    fn writes_lines() {
        let p = super::path();
        let _ = std::fs::remove_file(&p);
        super::log("deneme a=1\nikinci");
        super::log_system();
        let s = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2, "{s}");
        assert!(lines[0].ends_with(" deneme a=1 ikinci"), "{s}");
        assert!(lines[1].contains(" sistem sürüm="), "{s}");
        let _ = std::fs::remove_file(&p);
    }
}
