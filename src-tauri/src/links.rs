//! İleti gövdesindeki bağlantılar (Outlook gibi): tıklanınca varsayılan tarayıcıda açılır,
//! üzerine gelince tam adres pencerenin sol altında gösterilir.
//!
//! İleti gövdesi betiksiz korumalı bir iframe'de durur; WebKitGTK böyle bir çerçevede ana
//! pencerenin eklediği tıklama/üzerine gelme dinleyicilerini çalıştırmaz. Bu yüzden iş yerel
//! tarafta yapılır: çerçevedeki gezinme isteği yakalanıp engellenir ve dışarıda açılır;
//! fare altındaki bağlantıyı WebKit'in kendi isabet testi bildirir.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Emitter, Manager, Runtime, Url};

/// Uygulamanın kendi sayfaları (gezinmesine izin verilir)
fn is_internal(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "tauri.localhost" | "ipc.localhost")),
        _ => true,
    }
}

fn is_external(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "ftp") && !is_internal(url)
}

/// Aynı bağlantı hem tıklamadan hem gezinme isteğinden gelebilir: kısa sürede ikinci kez açılmaz
fn open_external<R: Runtime>(app: &tauri::AppHandle<R>, url: String) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<(String, Instant)>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((u, t)) = last.as_ref() {
            if *u == url && t.elapsed() < Duration::from_millis(1500) {
                return;
            }
        }
        *last = Some((url.clone(), Instant::now()));
    }
    let _ = app;
    let _ = crate::desktop::open_url(&url);
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("links")
        .on_navigation(|webview, url| {
            if url.scheme() == "mailto" {
                let _ = webview.emit_to(webview.label(), "app://mailto", url.as_str());
                return false;
            }
            if is_external(url) {
                open_external(webview.app_handle(), url.to_string());
                return false;
            }
            true
        })
        .on_webview_ready(|webview| {
            #[cfg(target_os = "linux")]
            {
                let w = webview.clone();
                let _ = webview.with_webview(move |pv| {
                    use gtk::prelude::WidgetExt;
                    use std::cell::RefCell;
                    use std::rc::Rc;
                    use webkit2gtk::{HitTestResultExt, SettingsExt, WebViewExt};
                    let view = pv.inner();
                    // Akıcı kaydırma (Outlook gibi): tekerlek adımları animasyonla, kesik kesik
                    // atlamadan kayar; kaydırma ayrı katmanda çizildiği için tepki gecikmez
                    if let Some(st) = WebViewExt::settings(&view) {
                        st.set_enable_smooth_scrolling(true);
                    }
                    // Fare altındaki bağlantı (WebKit isabet testi)
                    let hover = Rc::new(RefCell::new(String::new()));
                    let h = hover.clone();
                    let w2 = w.clone();
                    view.connect_mouse_target_changed(move |_, hit, _| {
                        let uri = if hit.context_is_link() { hit.link_uri().map(|s| s.to_string()).unwrap_or_default() } else { String::new() };
                        if *h.borrow() != uri {
                            let _ = w2.emit_to(w2.label(), "app://hover-link", &uri);
                            *h.borrow_mut() = uri;
                        }
                    });
                    // Bağlantı üzerinde basılıp aynı bağlantıda bırakılınca da açılır: düğme
                    // biçimli bağlantılarda (bağlantı içinde düğme vb.) tıklama gezinme
                    // isteğine dönüşmeyebiliyor. Gezinme de gelirse ikinci kez açılmaz.
                    let pressed = Rc::new(RefCell::new(String::new()));
                    let (p1, h1) = (pressed.clone(), hover.clone());
                    view.connect_button_press_event(move |_, ev| {
                        *p1.borrow_mut() = if ev.button() == 1 { h1.borrow().clone() } else { String::new() };
                        gtk::glib::Propagation::Proceed
                    });
                    let app = w.app_handle().clone();
                    view.connect_button_release_event(move |_, ev| {
                        let p = std::mem::take(&mut *pressed.borrow_mut());
                        if ev.button() == 1 && !p.is_empty() && *hover.borrow() == p {
                            if let Ok(url) = Url::parse(&p) {
                                if is_external(&url) {
                                    open_external(&app, p);
                                }
                            }
                        }
                        gtk::glib::Propagation::Proceed
                    });
                });
            }
            #[cfg(not(target_os = "linux"))]
            let _ = webview;
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_links() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(is_external(&u("https://www.example.com/a?b=1")));
        assert!(is_external(&u("http://intranet.kurum.local/")));
        assert!(!is_external(&u("http://localhost:1420/")));
        assert!(!is_external(&u("tauri://localhost/index.html")));
        assert!(!is_external(&u("about:srcdoc")));
        assert!(!is_external(&u("mailto:a@b.com")));
    }
}
