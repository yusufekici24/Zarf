//! Açılış ekranı: uygulamaya tıklanır tıklanmaz görünür (GTK; web görünümünün yüklenmesini
//! beklemez), arka planda arayüz ve veriler yüklenip ilk pencere gösterilince kapanır.

#[cfg(target_os = "linux")]
mod imp {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    thread_local! {
        static WIN: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
    }
    static OPEN: AtomicBool = AtomicBool::new(false);

    const W: i32 = 520;
    const CSS: &str = "
        window.mailnix-splash { background: #ffffff; border: 1px solid #c8cdd4; }
        .mailnix-splash progressbar trough { min-height: 3px; background: #e6eef8; border: none; border-radius: 0; }
        .mailnix-splash progressbar progress { min-height: 3px; background: #2b8af0; border: none; border-radius: 0; }
        .mailnix-splash label { color: #5f6670; font-size: 12px; }
    ";

    /// Açılış ekranını gösterir (ana iş parçacığında)
    pub fn show(text: &str) {
        let win = gtk::Window::new(gtk::WindowType::Toplevel);
        win.set_title("MailNix");
        win.set_icon_name(Some("mailnix"));
        win.set_decorated(false);
        win.set_resizable(false);
        win.set_position(gtk::WindowPosition::Center);
        win.set_type_hint(gtk::gdk::WindowTypeHint::Splashscreen);
        win.style_context().add_class("mailnix-splash");
        let css = gtk::CssProvider::new();
        let _ = css.load_from_data(CSS.as_bytes());
        if let Some(screen) = gtk::gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(&screen, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // Logo ve ad (2x görsel: yüksek çözünürlüklü ekranlarda da keskin)
        let loader = gtk::gdk_pixbuf::PixbufLoader::new();
        let _ = loader.write(include_bytes!("../assets/splash@2x.png"));
        let _ = loader.close();
        if let Some(pb) = loader.pixbuf() {
            let scale = win.scale_factor().max(1);
            let pb = if scale >= 2 { Some(pb) } else { pb.scale_simple(W, pb.height() * W / pb.width(), gtk::gdk_pixbuf::InterpType::Bilinear) };
            if let Some(pb) = pb {
                use gtk::gdk::prelude::GdkPixbufExt;
                let surface = pb.create_surface(scale, None::<&gtk::gdk::Window>);
                vbox.pack_start(&gtk::Image::from_surface(surface.as_ref()), false, false, 0);
            }
        }
        let bar = gtk::ProgressBar::new();
        bar.set_pulse_step(0.08);
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
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(60), move || {
            if OPEN.load(Ordering::SeqCst) {
                bar.pulse();
                gtk::glib::ControlFlow::Continue
            } else {
                gtk::glib::ControlFlow::Break
            }
        });
        WIN.with(|w| *w.borrow_mut() = Some(win));
        // Arayüz hiç bildirmezse (yükleme hatası vb.) açılış ekranı ekranda kalmasın
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(20), close);
    }

    pub fn is_open() -> bool {
        OPEN.load(Ordering::SeqCst)
    }

    /// Kapatır (ana iş parçacığında çağrılmalı)
    pub fn close() {
        OPEN.store(false, Ordering::SeqCst);
        WIN.with(|w| {
            if let Some(win) = w.borrow_mut().take() {
                win.close();
            }
        });
    }
}

/// Açılış ekranını göster
pub fn show(text: &str) {
    #[cfg(target_os = "linux")]
    imp::show(text);
    #[cfg(not(target_os = "linux"))]
    let _ = text;
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
