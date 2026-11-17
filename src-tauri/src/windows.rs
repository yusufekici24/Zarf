//! Ayrı pencereler (Outlook gibi): yeni ileti / yanıt ve açılan ileti kendi
//! işletim sistemi penceresinde, çerçeve ve gölgesiyle açılır.
//!
//! Açılış hızı için arka planda gizli, yüklenmiş bir "yedek" pencere hazır tutulur;
//! istek gelince içerik ona verilir ve hemen gösterilir.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct Registry {
    /// Pencereye verilecek içerik (pencere hazır olunca alır)
    payloads: HashMap<String, Value>,
    /// Yüklenmiş ve atanmayı bekleyen gizli pencere
    spare: Option<String>,
    /// Şu an içerik gösteren ikincil pencereler
    active: Vec<String>,
    /// Pencere türü ("compose" | "read")
    kinds: HashMap<String, String>,
}

static REG: std::sync::LazyLock<Mutex<Registry>> = std::sync::LazyLock::new(Default::default);

fn reg() -> std::sync::MutexGuard<'static, Registry> {
    REG.lock().unwrap_or_else(|e| e.into_inner())
}

/// Arayüz ölçeği (yüzde): yeni pencereler içerikle birlikte büyür
static UI_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

pub fn set_ui_scale(scale: f64) {
    UI_SCALE.store((scale * 100.0).round().clamp(50.0, 300.0) as u32, std::sync::atomic::Ordering::SeqCst);
}

fn size_for(kind: &str) -> (f64, f64, f64, f64) {
    let (w, h, mw, mh) = match kind {
        "read" => (980.0, 760.0, 560.0, 400.0),
        "event" => (1180.0, 820.0, 720.0, 500.0),
        "settings" => (900.0, 680.0, 640.0, 460.0),
        "account" => (700.0, 720.0, 520.0, 460.0),
        _ => (1000.0, 760.0, 640.0, 420.0),
    };
    let k = UI_SCALE.load(std::sync::atomic::Ordering::SeqCst) as f64 / 100.0;
    (w * k, h * k, mw * k.min(1.2), mh * k.min(1.2))
}

/// Pencereyi bulunduğu ekrana göre "yeni açılmış gibi" boyutlandırır: açılış boyutu × arayüz
/// ölçeği, ekranın çalışma alanına sığacak şekilde; ortalanır. Ekranı kaplayan pencere yeni
/// ekranı kaplar. Çözünürlük değişince ve açılışta çağrılır.
pub fn fit_to_monitor<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, kind: &str) {
    if win.is_maximized().unwrap_or(false) {
        // Pencere yöneticisi yeni boyuta göre yeniden kaplasın
        let _ = win.unmaximize();
        let _ = win.maximize();
        return;
    }
    let Some(m) = win.current_monitor().ok().flatten().or_else(|| win.primary_monitor().ok().flatten()) else { return };
    let sf = m.scale_factor().max(0.5);
    let wa = m.work_area();
    let (ax, ay) = (wa.position.x as f64 / sf, wa.position.y as f64 / sf);
    let (aw, ah) = (wa.size.width as f64 / sf, wa.size.height as f64 / sf);
    if aw < 200.0 || ah < 200.0 {
        return;
    }
    let k = UI_SCALE.load(std::sync::atomic::Ordering::SeqCst) as f64 / 100.0;
    let (w, h, mw, mh) = if kind == "main" { (1360.0 * k, 860.0 * k, 900.0, 560.0) } else { size_for(kind) };
    let (mw, mh) = (mw.min(aw), mh.min(ah));
    let w = w.min(aw * 0.94).max(mw);
    let h = h.min(ah * 0.94).max(mh);
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(mw, mh)));
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(ax + (aw - w) / 2.0, ay + (ah - h) / 2.0));
}

// ------------------------------------------------------------------ pencere konumu/boyutu
// Outlook gibi: pencere kapandığı boyutta ve ekranın aynı yerinde açılır. Kayıtlı yer artık
// hiçbir ekranın içinde değilse (çözünürlük düştü, ikinci monitör çıkarıldı) ekrana sığdırılır.

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub struct WinState {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub max: bool,
}

fn state_key(label: &str) -> String {
    format!("win_state_{}", kind_of(label))
}

fn db_of<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) -> Option<crate::db::Db> {
    win.app_handle().try_state::<crate::commands::AppState>().map(|s| s.db.clone())
}

static LAST_SAVE: std::sync::LazyLock<Mutex<HashMap<String, std::time::Instant>>> = std::sync::LazyLock::new(Default::default);
static PENDING: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> = std::sync::LazyLock::new(Default::default);
/// Yerleştirilmiş pencereler: açılıştaki geçici boyut/konum olayları kaydı ezmesin
static PLACED: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> = std::sync::LazyLock::new(Default::default);

/// Pencerenin şu anki yeri ve boyutu kaydedilir (taşıma/boyutlandırma/kapanış)
pub fn remember<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, force: bool) {
    let label = win.label().to_string();
    if label == "alerts" || win.is_minimized().unwrap_or(false) || !win.is_visible().unwrap_or(false) {
        return;
    }
    if !PLACED.lock().unwrap_or_else(|e| e.into_inner()).contains(&label) {
        return;
    }
    {
        let mut m = LAST_SAVE.lock().unwrap_or_else(|e| e.into_inner());
        if !force && m.get(&label).map(|t| t.elapsed() < std::time::Duration::from_millis(250)).unwrap_or(false) {
            // Sık gelen olaylarda son durum kaybolmasın: kısa süre sonra bir kez daha kaydedilir
            let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if p.insert(label.clone()) {
                let w = win.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(w.label());
                    remember(&w, true);
                });
            }
            return;
        }
        m.insert(label.clone(), std::time::Instant::now());
    }
    let Some(db) = db_of(win) else { return };
    let key = state_key(&label);
    let max = win.is_maximized().unwrap_or(false);
    let prev: Option<WinState> = db.setting(&key).and_then(|v| serde_json::from_str(&v).ok());
    let st = if max {
        // Ekranı kaplıyorsa normal (küçültülmüş) boyutu korunur
        match prev {
            Some(p) => WinState { max: true, ..p },
            None => return,
        }
    } else {
        let sf = win.scale_factor().unwrap_or(1.0).max(0.5);
        let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else { return };
        WinState { x: pos.x as f64 / sf, y: pos.y as f64 / sf, w: size.width as f64 / sf, h: size.height as f64 / sf, max: false }
    };
    if st.w < 200.0 || st.h < 150.0 {
        return;
    }
    db.set_setting(&key, &serde_json::to_string(&st).unwrap_or_default());
}

/// Kayıtlı yer bir ekranın çalışma alanının içinde mi (birkaç piksel taşma hoş görülür)
fn fits_somewhere<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, st: &WinState) -> bool {
    let Ok(monitors) = win.available_monitors() else { return false };
    monitors.iter().any(|m| {
        let sf = m.scale_factor().max(0.5);
        let wa = m.work_area();
        let (ax, ay, aw, ah) = (wa.position.x as f64 / sf, wa.position.y as f64 / sf, wa.size.width as f64 / sf, wa.size.height as f64 / sf);
        let tol = 24.0;
        st.x >= ax - tol && st.y >= ay - tol && st.x + st.w <= ax + aw + tol && st.y + st.h <= ay + ah + tol
    })
}

/// Pencereyi yerleştirir: kayıtlı yer geçerliyse orada (ekranı kaplıyorduysa yine kaplar),
/// değilse bulunduğu ekrana göre açılış boyutunda ortalanır.
pub fn place_window<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, kind: &str) {
    // Yerleştirme bitince (pencere yöneticisinin olayları geldikten sonra) kayıt başlar
    {
        let label = win.label().to_string();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            PLACED.lock().unwrap_or_else(|e| e.into_inner()).insert(label);
        });
    }
    let saved: Option<WinState> = db_of(win).and_then(|db| db.setting(&state_key(win.label()))).and_then(|v| serde_json::from_str(&v).ok());
    match saved {
        Some(st) if fits_somewhere(win, &st) => {
            if win.is_maximized().unwrap_or(false) && st.max {
                return;
            }
            if win.is_maximized().unwrap_or(false) {
                let _ = win.unmaximize();
            }
            let _ = win.set_size(tauri::LogicalSize::new(st.w, st.h));
            let _ = win.set_position(tauri::LogicalPosition::new(st.x, st.y));
            if st.max {
                let _ = win.maximize();
            }
        }
        Some(st) if st.max => {
            // Yer artık geçerli değil ama ekranı kaplıyordu: yeni ekranı kaplasın
            fit_to_monitor(win, kind);
            let _ = win.maximize();
        }
        _ => fit_to_monitor(win, kind),
    }
}

// ------------------------------------------------------------------ gösterme
static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Açılışta tema belirlenir: pencerelerin kendi arka planı arayüzün zemin rengiyle aynı
/// olur (gösterildiği ilk karede farklı renkte boş yüzey görünmez)
pub fn init_theme(theme: &str) {
    let dark = match theme {
        "dark" => true,
        "light" => false,
        _ => std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("dark"))
            .unwrap_or(false)
            || std::env::var("GTK_THEME").map(|t| t.to_lowercase().contains("dark")).unwrap_or(false),
    };
    DARK.store(dark, Ordering::SeqCst);
}

pub fn chrome_color() -> tauri::window::Color {
    if DARK.load(Ordering::SeqCst) {
        tauri::window::Color(0x1b, 0x1b, 0x1b, 255)
    } else {
        tauri::window::Color(0xf0, 0xf0, 0xf0, 255)
    }
}
static START_MINIMIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_start_minimized(v: bool) {
    START_MINIMIZED.store(v, Ordering::SeqCst);
}

/// Yalnızca bir ileti dosyası (.msg/.eml) ya da mailto ile başlatıldı: ana pencere hiç
/// gösterilmez (uygulama bildirim alanında çalışır), ileti penceresi kapanınca uygulama da
/// kapanır. Kullanıcı bu sırada uygulamayı açarsa ana pencere gelir ve açık kalır.
static START_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_start_hidden(v: bool) {
    START_HIDDEN.store(v, Ordering::SeqCst);
}

/// Ana pencereyi gösterir ve öne getirir (bildirim alanından, uygulama yeniden açılınca)
pub fn show_main<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    START_HIDDEN.store(false, Ordering::SeqCst);
    if let Some(m) = app.get_webview_window("main") {
        bring_to_front(&m);
    }
    crate::splash::close(app);
}

/// Gizli oluşturulan pencereyi gösterir (ilk kez). Ana pencere açılış kipine göre simge
/// durumunda ya da önde; diğer pencereler öne getirilir.
fn reveal<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    if win.is_visible().unwrap_or(false) {
        return;
    }
    if win.label() == "main" {
        crate::perf::log(&format!("açılış ana_pencere_gösterildi_ms={} (süreç başlangıcından)", crate::perf::since_start()));
    }
    if win.label() == "main" && START_HIDDEN.load(Ordering::SeqCst) {
        // İleti penceresi açılamadıysa (bozuk dosya vb.) uygulama görünmez kalmasın
        let app = win.app_handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(15));
            if START_HIDDEN.load(Ordering::SeqCst) && reg().active.is_empty() {
                show_main(&app);
            }
        });
        return;
    }
    if win.label() == "main" && START_MINIMIZED.load(Ordering::SeqCst) {
        let _ = win.minimize();
        let _ = win.show();
        let _ = win.minimize();
    } else {
        // İkincil pencereler (yeni ileti, ileti...) önce saydam gösterilir: WebKit gizli
        // pencerede çizim yapmadığından gösterildiği an kısa süre boş kalıyordu. Ön yüz ilk
        // çizimi bitirince (window_painted) görünür olur.
        let fade = win.label() != "main" && win.label() != "alerts";
        if fade {
            set_opacity(win, 0.0);
            let w = win.clone();
            std::thread::spawn(move || {
                // Ön yüz çizimi bitince (window_painted) hemen görünür; bu yalnızca yedek: boş
                // gri pencere gösterilmesin diye çizim için yeterince beklenir
                std::thread::sleep(std::time::Duration::from_millis(2500));
                set_opacity(&w, 1.0);
            });
        }
        bring_to_front(win);
        // İlk pencere ekrana geldi: açılış ekranı kapanır
        crate::splash::close(win.app_handle());
    }
}

/// Pencerenin saydamlığı (birleştiricisi olmayan masaüstlerinde etkisizdir, pencere doğrudan görünür)
fn set_opacity<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, v: f64) {
    #[cfg(target_os = "linux")]
    {
        let apply = move |w: &tauri::WebviewWindow<R>| {
            if let Ok(gw) = w.gtk_window() {
                use gtk::prelude::WidgetExt;
                gw.set_opacity(v);
                // Gösterilmeden önce yerel pencere oluşturulur ki saydamlık ilk karede de geçerli olsun
                if gw.window().is_none() {
                    gw.realize();
                }
                if let Some(gdk) = gw.window() {
                    gdk.set_opacity(v);
                }
            }
        };
        // Ana iş parçacığındaysak hemen (pencere gösterilmeden önce) uygulanır
        if gtk::is_initialized_main_thread() {
            apply(win);
        } else {
            let w = win.clone();
            let _ = win.run_on_main_thread(move || apply(&w));
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (win, v);
}

/// Ön yüz, gösterilen pencerenin ilk karesini çizdi: saydamlık kalkar
#[tauri::command]
pub fn window_painted(window: tauri::WebviewWindow) {
    set_opacity(&window, 1.0);
}

/// Ön yüz ilk çizimini bitirince çağırır: pencere ancak o zaman görünür (beyaz ekran yok)
#[tauri::command]
pub fn reveal_window(window: tauri::WebviewWindow) {
    reveal(&window);
}

/// Ön yüz bildirmezse (ör. yükleme hatası) pencere yine de gösterilir
pub fn reveal_later<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>, ms: u64) {
    let w = win.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        reveal(&w);
    });
}

/// Pencereyi öne getirir. Linux pencere yöneticileri (odak çalma önleme) başka bir
/// uygulamadan (ör. dosya yöneticisinden .msg açılınca) tetiklenen pencereyi arkada
/// bırakabiliyor: kısa süre "her zaman üstte" yapılıp odaklanır, sonra normale döner.
pub fn bring_to_front<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_always_on_top(true);
    let _ = win.set_focus();
    let _ = win.request_user_attention(Some(tauri::UserAttentionType::Informational));
    let w = win.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        let _ = w.set_always_on_top(false);
        // Odak yeniden zorlanmaz: kullanıcı bu arada başka uygulamaya (ör. Chrome) geçtiyse
        // klavye arkada kalan Zarf penceresine gitmesin
        let _ = w.request_user_attention(None);
        // Masaüstü uyarıları yine en üstte kalsın (öne getirilen pencere örtmesin)
        if let Some(a) = w.app_handle().get_webview_window("alerts") {
            if a.is_visible().unwrap_or(false) {
                let _ = a.set_always_on_top(false);
                let _ = a.set_always_on_top(true);
            }
        }
    });
}

/// Pencerenin bulunduğu ekranın mantıksal boyutu (sayfa yakınlaştırmasından etkilenmez)
pub fn screen_size<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) -> Option<(f64, f64)> {
    let m = win.current_monitor().ok().flatten().or_else(|| win.primary_monitor().ok().flatten())?;
    let sf = m.scale_factor().max(0.5);
    Some((m.size().width as f64 / sf, m.size().height as f64 / sf))
}

/// Pencerenin türü değişti (ileti penceresi yanıt yazmaya dönüştü)
#[tauri::command]
pub fn set_window_kind(window: tauri::WebviewWindow, kind: String) {
    reg().kinds.insert(window.label().to_string(), kind);
}

pub fn kind_of(label: &str) -> String {
    if label == "main" {
        return "main".into();
    }
    reg().kinds.get(label).cloned().unwrap_or_else(|| "compose".into())
}

fn build(app: &AppHandle, label: &str, visible: bool) -> tauri::Result<tauri::WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Zarf")
        .inner_size(1000.0, 760.0)
        .min_inner_size(560.0, 400.0)
        // Ana pencere gibi Outlook tarzı kendi başlık çubuğu; gölge ve kenarlardan
        // boyutlandırma korunur (Windows 11'de yuvarlak köşeler)
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .visible(visible)
        .focused(visible)
        .center()
        .background_color(chrome_color())
        .build()
}

/// Yedek pencere hazırlar (zaten varsa bir şey yapmaz).
pub fn ensure_spare(app: &AppHandle) {
    if reg().spare.is_some() {
        return;
    }
    let label = format!("w-{}", NEXT.fetch_add(1, Ordering::SeqCst));
    reg().spare = Some(format!("pending:{label}"));
    // Arka plan iş parçacığından çağrılır; Tauri pencereyi ana döngüde oluşturur
    if build(app, &label, false).is_err() {
        reg().spare = None;
    }
}

/// Pencere ön yüzü yüklendiğinde çağırır: içeriği varsa alır; yoksa yedek olarak bekler.
#[tauri::command]
pub fn window_ready(window: tauri::WebviewWindow) -> Option<Value> {
    let label = window.label().to_string();
    let mut r = reg();
    if let Some(p) = r.payloads.remove(&label) {
        if !r.active.contains(&label) {
            r.active.push(label);
        }
        return Some(p);
    }
    if r.spare.as_deref() == Some(&format!("pending:{label}")) {
        r.spare = Some(label);
    }
    None
}

async fn tokio_sleep(ms: u64) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_millis(ms))).await;
}

/// Yeni pencere açar. `kind`: "compose" | "read"; `payload`: pencereye özel içerik.
/// Not: async (Windows'ta senkron komuttan pencere oluşturmak kilitlenir)
#[tauri::command]
pub async fn open_window(app: AppHandle, kind: String, payload: Value, title: Option<String>) -> Result<String, String> {
    // Ayarlar tek pencere (Outlook Seçenekleri gibi): açıksa öne gelir, istenen sekmeye geçer
    if kind == "settings" {
        let open = { reg().kinds.iter().find(|(_, k)| k.as_str() == "settings").map(|(l, _)| l.clone()) };
        if let Some(win) = open.and_then(|l| app.get_webview_window(&l)) {
            let _ = win.emit_to(win.label(), "settings://show", payload);
            bring_to_front(&win);
            return Ok(win.label().to_string());
        }
    }
    let data = json!({ "kind": kind, "payload": payload });
    let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Zarf".into());
    // Yedek pencere hâlâ yükleniyorsa kısa bir süre onu bekle: yarı yüklenmiş pencere,
    // sıfırdan yeni pencere kurmaktan (WebKit süreci + arayüz yükleme) her zaman daha hızlı
    for _ in 0..30 {
        if !reg().spare.as_deref().is_some_and(|l| l.starts_with("pending:")) {
            break;
        }
        tokio_sleep(100).await;
    }
    // Hazır yedek pencere varsa onu kullan (anında açılır)
    let spare = {
        let mut r = reg();
        match r.spare.clone() {
            Some(l) if !l.starts_with("pending:") => {
                r.spare = None;
                r.active.push(l.clone());
                r.kinds.insert(l.clone(), kind.clone());
                Some(l)
            }
            _ => None,
        }
    };
    if let Some(label) = spare {
        if let Some(win) = app.get_webview_window(&label) {
            let _ = win.set_title(&title);
            place_window(&win, &kind);
            let _ = win.emit_to(label.as_str(), "window://assign", data);
            // İçerik çizilince ön yüz gösterir (reveal_window); gelmezse kısa süre sonra
            reveal_later(&win, 1200);
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(800));
                ensure_spare(&app2);
            });
            return Ok(label);
        }
        reg().active.retain(|l| l != &label);
    }
    let label = format!("w-{}", NEXT.fetch_add(1, Ordering::SeqCst));
    reg().payloads.insert(label.clone(), data);
    reg().kinds.insert(label.clone(), kind.clone());
    // Gizli oluşturulur; ön yüz içeriği çizince gösterir (beyaz ekran görünmez)
    let win = build(&app, &label, false).map_err(|e| e.to_string())?;
    let _ = win.set_title(&title);
    place_window(&win, &kind);
    reveal_later(&win, 5000);
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        ensure_spare(&app2);
    });
    Ok(label)
}

/// İleti/yeni ileti penceresi kapatılırken yok edilmez, gizlenip hazır yedek pencere olur:
/// bir sonraki açılış (aynı ileti dahil) yeni WebKit süreci kurmadan anında olur. Zaten
/// hazır bir yedek varsa pencere kapatılır.
#[tauri::command]
pub fn recycle_window(app: AppHandle, window: tauri::WebviewWindow) {
    let label = window.label().to_string();
    if label == "main" || label == "alerts" {
        return;
    }
    remember(&window, true);
    let kind = kind_of(&label);
    let mut dropped: Option<String> = None;
    let reuse = matches!(kind.as_str(), "read" | "compose") && {
        let mut r = reg();
        let free = r.spare.is_none() || r.spare.as_deref().is_some_and(|s| s.starts_with("pending:"));
        if free {
            r.active.retain(|l| l != &label);
            r.payloads.remove(&label);
            r.kinds.remove(&label);
            // Kurulmakta olan yedeğe gerek kalmadı (bu pencere zaten yüklü)
            dropped = r.spare.take().and_then(|s| s.strip_prefix("pending:").map(String::from));
            r.spare = Some(label.clone());
        }
        free
    };
    if let Some(w) = dropped.and_then(|l| app.get_webview_window(&l)) {
        let _ = w.destroy();
    }
    if !reuse {
        let _ = window.destroy();
        return;
    }
    PLACED.lock().unwrap_or_else(|e| e.into_inner()).remove(&label);
    let _ = window.hide();
    let _ = window.emit_to(label.as_str(), "window://reset", ());
    // İçerik gösteren pencere kalmadıysa ve ana pencere gizliyse (yalnızca .msg açılmıştı) çık
    let none_left = reg().active.is_empty();
    let main_hidden = app.get_webview_window("main").map(|m| !m.is_visible().unwrap_or(true)).unwrap_or(true);
    if none_left && main_hidden {
        app.exit(0);
    }
}

/// Pencere kapanınca kayıttan düşer; içerik gösteren pencere kalmadıysa ve ana
/// pencere de gizliyse uygulama kapanır.
pub fn on_destroyed(app: &AppHandle, label: &str) {
    PLACED.lock().unwrap_or_else(|e| e.into_inner()).remove(label);
    let none_left = {
        let mut r = reg();
        r.active.retain(|l| l != label);
        r.payloads.remove(label);
        r.kinds.remove(label);
        if r.spare.as_deref() == Some(label) || r.spare.as_deref() == Some(&format!("pending:{label}")) {
            r.spare = None;
        }
        r.active.is_empty()
    };
    if label == "main" {
        return;
    }
    let main_hidden = app.get_webview_window("main").map(|m| !m.is_visible().unwrap_or(true)).unwrap_or(true);
    if none_left && main_hidden {
        app.exit(0);
    }
}

/// Ana pencere kapatılırken: açık ileti pencereleri varsa yalnızca ana pencere gizlenir
/// (Outlook'ta da açık ileti pencereleri kapanmaz), yoksa uygulama kapanır.
pub fn main_close_requested(app: &AppHandle) -> bool {
    let active = reg().active.clone();
    let any_open = active.iter().any(|l| app.get_webview_window(l).map(|w| w.is_visible().unwrap_or(false)).unwrap_or(false));
    if any_open {
        if let Some(m) = app.get_webview_window("main") {
            let _ = m.hide();
        }
        return true;
    }
    app.exit(0);
    false
}

/// Ana pencereyi öne getirir (ikincil pencerelerden "takvimde aç" vb. için).
#[tauri::command]
pub fn focus_main(app: AppHandle) {
    START_HIDDEN.store(false, Ordering::SeqCst);
    if let Some(m) = app.get_webview_window("main") {
        let _ = m.show();
        let _ = m.unminimize();
        let _ = m.set_focus();
    }
}

/// Ana pencere kullanıcıya hiç gösterilmeden mi çalışıyor (yalnızca ileti penceresi açık)
#[tauri::command]
pub fn main_hidden(app: AppHandle) -> bool {
    app.get_webview_window("main").map(|m| !m.is_visible().unwrap_or(false)).unwrap_or(true)
}

// ---------------------------------------------------------------------- görev çubuğu sayacı
/// Okunmamış ileti sayısını görev çubuğunda gösterir. Linux (GNOME/KDE) ve
/// macOS'ta simge rozeti, Windows'ta simge üzerine sayı resmi (overlay).
#[tauri::command]
pub fn set_unread_badge(app: AppHandle, count: i64) {
    let Some(w) = app.get_webview_window("main") else { return };
    #[cfg(target_os = "windows")]
    {
        let _ = w.set_overlay_icon(if count > 0 { Some(badge_image(count)) } else { None });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = w.set_badge_count(if count > 0 { Some(count) } else { None });
    }
    // Görev çubuğundaki pencere simgesine kendi rozetimiz (küçük, sayı ortalı, 99+):
    // rozet desteklemeyen görev çubuklarında da sayı görünür
    #[cfg(target_os = "linux")]
    if let Some(icon) = app.default_window_icon() {
        if let Some((rgba, size)) = badged_icon(icon.rgba(), icon.width() as i32, icon.height() as i32, count) {
            let _ = w.set_icon(tauri::image::Image::new_owned(rgba, size as u32, size as u32));
        }
    }
}

/// Rozet metni: 99'dan fazlası "99+"
pub fn badge_text(count: i64) -> String {
    if count > 99 {
        "99+".into()
    } else {
        count.to_string()
    }
}

/// Uygulama simgesi + sağ üst köşede kırmızı rozet (beyaz kenarlı, sayı ortalı)
#[cfg(target_os = "linux")]
fn badged_icon(rgba: &[u8], w: i32, h: i32, count: i64) -> Option<(Vec<u8>, i32)> {
    use gtk::cairo;
    use gtk::gdk::prelude::GdkContextExt;
    use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
    const S: i32 = 128;
    let base = Pixbuf::from_bytes(&gtk::glib::Bytes::from(rgba), Colorspace::Rgb, true, 8, w, h, w * 4).scale_simple(S, S, InterpType::Bilinear)?;
    if count <= 0 {
        let stride = base.rowstride() as usize;
        let px = base.read_pixel_bytes();
        let out = px.chunks(stride).take(S as usize).flat_map(|r| r[..(S as usize) * 4].to_vec()).collect();
        return Some((out, S));
    }
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, S, S).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    cr.set_source_pixbuf(&base, 0.0, 0.0);
    cr.paint().ok()?;
    let text = badge_text(count);
    let bh = S as f64 * 0.40;
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(bh * if text.len() > 2 { 0.56 } else { 0.66 });
    let ext = cr.text_extents(&text).ok()?;
    let bw = bh.max(ext.width() + bh * 0.5);
    let (x, y) = (S as f64 - bw, 0.0);
    let r = bh / 2.0;
    let pill = |cr: &cairo::Context, inset: f64| {
        let (x, y, bw, r) = (x + inset, y + inset, bw - 2.0 * inset, r - inset);
        cr.new_path();
        cr.arc(x + r, y + r, r, std::f64::consts::FRAC_PI_2, 3.0 * std::f64::consts::FRAC_PI_2);
        cr.arc(x + bw - r, y + r, r, -std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        cr.close_path();
    };
    // Beyaz kenar (simgeden ayrılsın), kırmızı dolgu
    pill(&cr, 0.0);
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.fill().ok()?;
    pill(&cr, S as f64 * 0.035);
    cr.set_source_rgb(0xd1 as f64 / 255.0, 0x34 as f64 / 255.0, 0x38 as f64 / 255.0);
    cr.fill().ok()?;
    // Sayı tam ortada (yazının kendi kutusuna göre)
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.move_to(x + (bw - ext.width()) / 2.0 - ext.x_bearing(), y + (bh - ext.height()) / 2.0 - ext.y_bearing());
    cr.show_text(&text).ok()?;
    drop(cr);
    surface.flush();
    // Kahire yüzeyi (önceden çarpılmış BGRA) → RGBA resim
    let stride = surface.stride() as usize;
    let data = surface.take_data().ok()?;
    let mut out = Vec::with_capacity((S * S * 4) as usize);
    for row in data.chunks(stride).take(S as usize) {
        for p in row[..(S as usize) * 4].chunks(4) {
            let a = p[3] as u32;
            let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
            out.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
        }
    }
    Some((out, S))
}

/// Kırmızı daire içinde beyaz sayı (32×32 RGBA). Rakamlar 3×5 piksellik yazı tipiyle çizilir.
#[allow(dead_code)]
pub fn badge_image(count: i64) -> tauri::image::Image<'static> {
    const S: usize = 32;
    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b010, 0b010, 0b010],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];
    let text: Vec<usize> = badge_text(count.min(99)).bytes().map(|b| (b - b'0') as usize).collect();
    let plus = count > 99;
    let mut px = vec![0u8; S * S * 4];
    let c = (S as f32 - 1.0) / 2.0;
    for y in 0..S {
        for x in 0..S {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = (c + 0.5 - d).clamp(0.0, 1.0);
            let i = (y * S + x) * 4;
            px[i..i + 4].copy_from_slice(&[0xd1, 0x34, 0x38, (a * 255.0) as u8]);
        }
    }
    // Ölçek: tek hane büyük, iki hane daha küçük
    let scale = if text.len() == 1 { 4 } else { 3 };
    let gap = scale;
    let glyph_w = 3 * scale;
    let total_w = text.len() * glyph_w + (text.len() - 1) * gap + if plus { gap + glyph_w } else { 0 };
    let x0 = (S - total_w.min(S)) / 2;
    let y0 = (S - 5 * scale) / 2;
    let mut put = |gx: usize, gy: usize| {
        for dy in 0..scale {
            for dx in 0..scale {
                let (x, y) = (gx + dx, gy + dy);
                if x < S && y < S {
                    let i = (y * S + x) * 4;
                    px[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    };
    let mut cx = x0;
    for d in &text {
        for (row, bits) in DIGITS[*d].iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    put(cx + col * scale, y0 + row * scale);
                }
            }
        }
        cx += glyph_w + gap;
    }
    if plus {
        // "+" işareti
        for k in 0..3 {
            put(cx + scale, y0 + (1 + k) * scale);
            put(cx + k * scale, y0 + 2 * scale);
        }
    }
    tauri::image::Image::new_owned(px, S as u32, S as u32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn badge_has_pixels() {
        let img = super::badge_image(7);
        assert_eq!(img.width(), 32);
        assert!(img.rgba().chunks(4).any(|p| p == [255, 255, 255, 255]));
        let _ = super::badge_image(150);
        assert_eq!(super::badge_text(7), "7");
        assert_eq!(super::badge_text(99), "99");
        assert_eq!(super::badge_text(150), "99+");
    }

    /// Rozetli simgeleri PNG olarak yazar (gözle denetim): LYNORA_BADGE_OUT=klasör
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn badge_icon_render() {
        let out = std::env::var("LYNORA_BADGE_OUT").unwrap_or_else(|_| "/tmp".into());
        let img = image_from_icon();
        for n in [0, 3, 42, 150] {
            let (px, sz) = super::badged_icon(&img.0, img.1, img.2, n).unwrap();
            let pb = gtk::gdk_pixbuf::Pixbuf::from_bytes(&gtk::glib::Bytes::from_owned(px), gtk::gdk_pixbuf::Colorspace::Rgb, true, 8, sz, sz, sz * 4);
            pb.savev(format!("{out}/badge-{n}.png"), "png", &[]).unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    fn image_from_icon() -> (Vec<u8>, i32, i32) {
        let pb = gtk::gdk_pixbuf::Pixbuf::from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/128x128.png")).unwrap();
        let pb = if pb.has_alpha() { pb } else { pb.add_alpha(false, 0, 0, 0).unwrap() };
        (pb.read_pixel_bytes().to_vec(), pb.width(), pb.height())
    }
}

// ---------------------------------------------------------------------- pencereler arası sürükle-bırak
static HOVER: std::sync::LazyLock<Mutex<Option<String>>> = std::sync::LazyLock::new(Default::default);

/// Ana penceredeki ileti listesinden sürüklenen iletiler (Outlook gibi .msg eki olur).
/// `phase`: "start" (yeni ileti pencereleri geçici olarak öne alınır), "move" (imlecin
/// altındaki pencere vurgulanır), "drop" (iletiler o pencereye gönderilir), "cancel".
/// Koordinatlar ekran koordinatıdır (mantıksal piksel). Hedef pencerenin etiketi döner.
#[tauri::command]
pub fn drag_messages(app: AppHandle, x: f64, y: f64, ids: Vec<i64>, phase: String) -> Option<String> {
    let composes: Vec<String> = {
        let r = reg();
        r.active.iter().filter(|l| r.kinds.get(*l).map(|k| k == "compose").unwrap_or(false)).cloned().collect()
    };
    let wins: Vec<tauri::WebviewWindow> = composes.iter().filter_map(|l| app.get_webview_window(l)).filter(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)).collect();
    match phase.as_str() {
        "start" => {
            for w in &wins {
                let _ = w.set_always_on_top(true);
            }
            return None;
        }
        "drop" | "cancel" => {
            for w in &wins {
                let _ = w.set_always_on_top(false);
            }
        }
        _ => {}
    }
    let target = if phase == "cancel" {
        None
    } else {
        wins.iter().rev().find(|w| {
            let (Ok(p), Ok(s)) = (w.outer_position(), w.outer_size()) else { return false };
            let k = w.scale_factor().unwrap_or(1.0);
            let (wx, wy, ww, wh) = (p.x as f64 / k, p.y as f64 / k, s.width as f64 / k, s.height as f64 / k);
            x >= wx && x < wx + ww && y >= wy && y < wy + wh
        }).map(|w| w.label().to_string())
    };
    let drop = phase == "drop";
    let mut hover = HOVER.lock().unwrap_or_else(|e| e.into_inner());
    if hover.as_ref() != target.as_ref() || drop || phase == "cancel" {
        if let Some(old) = hover.take() {
            let _ = app.emit_to(old.as_str(), "compose://drag-hover", false);
        }
        if let (Some(t), false) = (&target, drop) {
            let _ = app.emit_to(t.as_str(), "compose://drag-hover", true);
            *hover = Some(t.clone());
        }
    }
    if drop {
        if let Some(t) = &target {
            let _ = app.emit_to(t.as_str(), "compose://drop-messages", ids);
            if let Some(w) = app.get_webview_window(t) {
                let _ = w.set_focus();
            }
        }
    }
    target
}

// ------------------------------------------------------------------ masaüstü uyarıları
// Outlook/Skype gibi yeni posta geldiğinde ekranın sağ altında üst üste dizilen kartlar.
// Kartları ayrı, kenarsız, her zaman üstte, odak çalmayan bir pencere gösterir; pencere
// yalnızca kartların kapladığı alan kadardır (masaüstündeki tıklamaları engellemez).

static ALERT_QUEUE: std::sync::LazyLock<Mutex<Vec<Value>>> = std::sync::LazyLock::new(Default::default);
pub const ALERT_WIDTH: f64 = 460.0;

fn alerts_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(w) = app.get_webview_window("alerts") {
        return Some(w);
    }
    WebviewWindowBuilder::new(app, "alerts", WebviewUrl::App("index.html".into()))
        .title("Zarf")
        .inner_size(ALERT_WIDTH, 120.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .build()
        .ok()
}

/// Yeni postaları masaüstü uyarı penceresine iletir
#[tauri::command]
pub fn show_mail_alerts(app: AppHandle, items: Value) {
    let Some(arr) = items.as_array().cloned() else { return };
    let fresh = app.get_webview_window("alerts").is_none();
    if fresh {
        // Pencere hazır olunca alır (alerts_take)
        ALERT_QUEUE.lock().unwrap_or_else(|e| e.into_inner()).extend(arr);
        let _ = alerts_window(&app);
    } else {
        let _ = app.emit_to("alerts", "alerts://add", Value::Array(arr));
    }
}

/// Uyarı kartları için kullanılabilir yükseklik (çalışma alanı, mantıksal piksel)
#[tauri::command]
pub fn alerts_area(window: tauri::WebviewWindow) -> f64 {
    let m = window.primary_monitor().ok().flatten().or_else(|| window.current_monitor().ok().flatten());
    m.map(|m| m.work_area().size.height as f64 / m.scale_factor().max(0.5)).unwrap_or(1040.0)
}

/// Saydam uyarı penceresinde yalnızca kartların bulunduğu alan tıklama alır; üstteki boş
/// kısım alttaki pencerelere geçer (pencere her kartta yeniden boyutlanmasın diye)
#[tauri::command]
pub fn alerts_input(window: tauri::WebviewWindow, top: f64, height: f64) {
    #[cfg(target_os = "linux")]
    if let Ok(gw) = window.gtk_window() {
        use gtk::prelude::WidgetExt;
        let w = gw.allocated_width().max(ALERT_WIDTH as i32 + 50);
        let top = top.max(0.0).floor() as i32;
        let h = (height.ceil() as i32 - top).max(0);
        let region = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(0, top, w, h));
        gw.input_shape_combine_region(Some(&region));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (window, top, height);
}

/// Uyarı penceresi açılınca bekleyen kartlar
#[tauri::command]
pub fn alerts_take() -> Vec<Value> {
    std::mem::take(&mut *ALERT_QUEUE.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Uyarı penceresinin boyutu kartların yüksekliği kadar; sağ altta, görev çubuğunun üstünde.
/// Döner: ekranın kullanılabilir yüksekliği (kaç kart sığacağını hesaplamak için)
#[tauri::command]
pub fn alerts_layout(window: tauri::WebviewWindow, height: f64) -> f64 {
    let m = window.primary_monitor().ok().flatten().or_else(|| window.current_monitor().ok().flatten());
    let (ax, ay, aw, ah) = match &m {
        Some(m) => {
            let sf = m.scale_factor().max(0.5);
            let wa = m.work_area();
            (wa.position.x as f64 / sf, wa.position.y as f64 / sf, wa.size.width as f64 / sf, wa.size.height as f64 / sf)
        }
        None => (0.0, 0.0, 1920.0, 1040.0),
    };
    if height <= 0.0 {
        let _ = window.hide();
        return ah;
    }
    let h = height.min(ah);
    // Pencere ekranın sağ kenarına dayanır: kart kenarın dışından kayarak girer (kenar boşluğu
    // kartın kendi iç boşluğundadır)
    let (x, y) = (ax + aw - ALERT_WIDTH, ay + ah - h - 12.0);
    // Gizli pencerede verilen konumu bazı pencere yöneticileri (xfwm, mutter) gösterirken yok
    // sayıp eski yerini kullanıyor: önce gösterilir, sonra boyut/konum verilir ve kısa süre
    // sonra yeniden uygulanır (kart görev çubuğunun hemen üstünde kalsın)
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
    }
    let apply = move |w: &tauri::WebviewWindow| {
        let _ = w.set_size(tauri::LogicalSize::new(ALERT_WIDTH, h));
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
    };
    apply(&window);
    let _ = window.set_always_on_top(true);
    let w = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        apply(&w);
    });
    ah
}

// ------------------------------------------------------------- anımsatıcı penceresi
/// Outlook "Anımsatıcı" penceresi: ayrı, her zaman üstte; zamanı gelen toplantı ya da görev
/// varsa ana pencere gizli/simge durumunda olsa bile açılır.
pub fn show_reminders(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("reminders") {
        let _ = w.emit_to("reminders", "reminders://refresh", ());
        if w.is_visible().unwrap_or(false) {
            bring_to_front(&w);
        } else {
            reveal(&w);
        }
        return;
    }
    // İlk açılış: içerik çizilince ön yüz gösterir (reveal_window)
    if let Ok(w) = WebviewWindowBuilder::new(app, "reminders", WebviewUrl::App("index.html".into()))
        .title("Zarf")
        .inner_size(540.0, 420.0)
        .min_inner_size(440.0, 340.0)
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .always_on_top(true)
        .visible(false)
        .center()
        .background_color(chrome_color())
        .build()
    {
        reveal_later(&w, 3000);
    }
}

/// Anımsatıcı penceresinin X / Esc düğmesi (pencere izinlerinden bağımsız çalışır)
#[tauri::command]
pub fn close_reminders(app: AppHandle) {
    hide_reminders(&app);
}

/// Bekleyen anımsatıcı kalmadı: pencere gizlenir
pub fn hide_reminders(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("reminders") {
        let _ = w.hide();
    }
}
