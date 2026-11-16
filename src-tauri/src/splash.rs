//! Açılış ekranı: uygulamaya tıklanır tıklanmaz görünür (GTK; web görünümünün yüklenmesini
//! beklemez), arka planda arayüz ve veriler yüklenip ilk pencere gösterilince kapanır.

#[cfg(target_os = "linux")]
mod imp {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    thread_local! {
        static WIN: RefCell<Option<(gtk::Window, gtk::Label)>> = const { RefCell::new(None) };
    }
    static OPEN: AtomicBool = AtomicBool::new(false);

    const W: i32 = 520;
    const CSS: &str = "
        window.lynora-splash { background: #ffffff; border: 1px solid #c8cdd4; }
        .lynora-splash progressbar trough { min-height: 3px; background: #e4e4e7; border: none; border-radius: 0; }
        .lynora-splash progressbar progress { min-height: 3px; background: #b91c1c; border: none; border-radius: 0; }
        .lynora-splash label { color: #5f6670; font-size: 12px; }
    ";

    /// Açılış ekranını gösterir (ana iş parçacığında). Zaten açıksa (veritabanı yükseltmesi)
    /// yalnızca yazısı değişir. `auto_close`: arayüz hiç bildirmezse kendiliğinden kapanır.
    pub fn show(text: &str, auto_close: bool) {
        let open = WIN.with(|w| w.borrow().as_ref().map(|(_, label)| label.set_text(text)).is_some());
        if !open {
            build(text);
        }
        // Arayüz hiç bildirmezse (yükleme hatası vb.) açılış ekranı ekranda kalmasın
        if auto_close {
            gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(20), close);
        }
    }

    fn build(text: &str) {
        let win = gtk::Window::new(gtk::WindowType::Toplevel);
        win.set_title("Zarf");
        win.set_icon_name(Some("lynora"));
        win.set_decorated(false);
        win.set_resizable(false);
        win.set_position(gtk::WindowPosition::Center);
        win.set_type_hint(gtk::gdk::WindowTypeHint::Splashscreen);
        win.style_context().add_class("lynora-splash");
        let css = gtk::CssProvider::new();
        let _ = css.load_from_data(CSS.as_bytes());
        if let Some(screen) = gtk::gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(&screen, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let loader = gtk::gdk_pixbuf::PixbufLoader::new();
        let scale = win.scale_factor().max(1);
        loader.set_size(88 * scale, 88 * scale);
        let _ = loader.write(include_bytes!("../../static/icon.svg"));
        let _ = loader.close();
        if let Some(pb) = loader.pixbuf() {
            use gtk::gdk::prelude::GdkPixbufExt;
            let surface = pb.create_surface(scale, None::<&gtk::gdk::Window>);
            let logo = gtk::Image::from_surface(surface.as_ref());
            logo.set_margin_top(36);
            logo.set_margin_bottom(16);
            vbox.pack_start(&logo, false, false, 0);
        }
        let brand = gtk::Label::new(None);
        brand.set_markup("<span size='24000' weight='bold' foreground='#b91c1c'>Zarf</span>");
        vbox.pack_start(&brand, false, false, 0);
        let version = gtk::Label::new(Some(env!("CARGO_PKG_VERSION")));
        version.set_margin_top(4);
        version.set_margin_bottom(24);
        vbox.pack_start(&version, false, false, 0);
        let bar = gtk::ProgressBar::new();
        bar.set_pulse_step(0.025);
        bar.set_margin_start(40);
        bar.set_margin_end(40);
        vbox.pack_start(&bar, false, false, 0);
        let label = gtk::Label::new(Some(text));
        label.set_margin_top(10);
        label.set_margin_bottom(22);
        vbox.pack_start(&label, false, false, 0);
        win.add(&vbox);
        win.set_default_size(W, -1);
        win.show_all();
        OPEN.store(true, Ordering::SeqCst);
        let animated = gtk::Settings::default().map(|s| s.is_gtk_enable_animations()).unwrap_or(true);
        if !animated { bar.set_fraction(0.5); }
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            if OPEN.load(Ordering::SeqCst) {
                if animated { bar.pulse(); }
                gtk::glib::ControlFlow::Continue
            } else {
                gtk::glib::ControlFlow::Break
            }
        });
        WIN.with(|w| *w.borrow_mut() = Some((win, label)));
    }

    pub fn is_open() -> bool {
        OPEN.load(Ordering::SeqCst)
    }

    /// Kapatır (ana iş parçacığında çağrılmalı)
    pub fn close() {
        OPEN.store(false, Ordering::SeqCst);
        WIN.with(|w| {
            if let Some((win, _)) = w.borrow_mut().take() {
                win.close();
            }
        });
    }

    /// Bekleyen GTK olaylarını işler: Tauri olay döngüsü başlamadan önce açılış ekranı canlı kalır
    pub fn pump() {
        while gtk::events_pending() {
            gtk::main_iteration();
        }
    }
}

/// Açılış ekranını göster
pub fn show(text: &str) {
    #[cfg(target_os = "linux")]
    imp::show(text, true);
    #[cfg(not(target_os = "linux"))]
    let _ = text;
}

/// Tauri başlamadan önce süren uzun açılış işi (veritabanı yükseltmesi) beklenirken açılış
/// ekranı: iş bitene kadar `pump` ile canlı tutulur, kendiliğinden kapanmaz
pub fn wait_with_splash<T: Send + 'static>(text: &str, job: impl FnOnce() -> T + Send + 'static) -> T {
    #[cfg(target_os = "linux")]
    if gtk::init().is_ok() {
        imp::show(text, false);
        let job = std::thread::spawn(job);
        while !job.is_finished() {
            imp::pump();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        // Ön yüz açılmayacaksa (oturum açılışında simge durumunda) açılış ekranı burada kapanır
        if std::env::args().any(|a| a == "--minimized") {
            imp::close();
        }
        return job.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
    }
    let _ = text;
    job()
}

/// İlk pencere gösterildiğinde açılış ekranını kapatır (her iş parçacığından çağrılabilir)
pub fn close<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    #[cfg(target_os = "linux")]
    if imp::is_open() {
        let _ = app.run_on_main_thread(imp::close);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}
