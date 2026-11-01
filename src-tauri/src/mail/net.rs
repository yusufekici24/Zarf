//! Bağlantı durumu (Outlook "Bağlandı / Bağlantı kesildi / Çevrimdışı çalışılıyor").
//!
//! Kurum ağı dışındayken sunucuya her istekte yeniden bağlanmaya çalışılıp beklenmez:
//! kısa süre önce ulaşılamayan sunucu için istek hemen başarısız olur, arka planda
//! düzenli yoklama yapılır ve bağlantı gelince eşitleme kendiliğinden sürer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static OFFLINE: AtomicBool = AtomicBool::new(false);
static WORK_OFFLINE: AtomicBool = AtomicBool::new(false);
static DOWN: std::sync::LazyLock<Mutex<HashMap<String, Instant>>> = std::sync::LazyLock::new(Default::default);

/// Bu süre içinde ulaşılamayan sunucuya yeni bağlantı denenmez (yoklama dışında)
pub const FAST_FAIL: Duration = Duration::from_secs(25);

pub fn is_offline() -> bool {
    OFFLINE.load(Ordering::SeqCst) || WORK_OFFLINE.load(Ordering::SeqCst)
}

/// Bağlantı kesildi mi (çevrimdışı çalışma hariç)
pub fn is_disconnected() -> bool {
    OFFLINE.load(Ordering::SeqCst)
}

pub fn work_offline() -> bool {
    WORK_OFFLINE.load(Ordering::SeqCst)
}

pub fn set_work_offline(v: bool) {
    WORK_OFFLINE.store(v, Ordering::SeqCst);
}

/// Sunucu kısa süre önce ulaşılamadıysa true (istek beklemeden başarısız olur)
pub fn recently_down(host: &str) -> bool {
    DOWN.lock().unwrap_or_else(|e| e.into_inner()).get(&host.to_lowercase()).map(|t| t.elapsed() < FAST_FAIL).unwrap_or(false)
}

pub fn mark_down(host: &str) {
    DOWN.lock().unwrap_or_else(|e| e.into_inner()).insert(host.to_lowercase(), Instant::now());
}

pub fn mark_up(host: &str) {
    DOWN.lock().unwrap_or_else(|e| e.into_inner()).remove(&host.to_lowercase());
}

/// Hesap sunucusuna ulaşılamadı / yeniden ulaşıldı (posta hizmeti belirler;
/// Autodiscover denemeleri gibi yan sunucular durumu değiştirmez)
pub fn set_disconnected(v: bool) {
    OFFLINE.store(v, Ordering::SeqCst);
}

/// Yoklama: hızlı-başarısızlık kaydını yok sayıp bağlantıyı dener
pub fn forget(host: &str) {
    DOWN.lock().unwrap_or_else(|e| e.into_inner()).remove(&host.to_lowercase());
}
