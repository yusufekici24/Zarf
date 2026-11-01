//! Dosya aç / kaydet / klasör seç diyaloğu.
//!
//! Tauri'nin diyalog eklentisi GTK dosya seçicisini uygulamanın içinde açar. Dosya seçici
//! açılırken GVFS (Nautilus'un ağ konumları, takılmış SMB paylaşımı vb.) yanıt vermezse
//! uygulamanın ana iş parçacığı kilitlenip her şey donuyordu. Burada:
//! - Masaüstünün dosya seçici servisi (xdg-desktop-portal) varsa diyalog ayrı bir süreçte
//!   açılır (GtkFileChooserNative + GTK_USE_PORTAL): orada takılma olsa da MailNix donmaz.
//! - Diyalog iç içe ana döngü kurmadan (run() değil, show() + yanıt geri çağrısı) açılır;
//!   arayüz çizilmeye devam eder.

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Filter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// Uygulama açılırken (GTK başlamadan, tek iş parçacığında) çağrılır: dosya seçici servisi
/// varsa GTK'nın yerel diyaloğu onu kullanır.
pub fn prefer_portal() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("GTK_USE_PORTAL").is_some() || !portal_available() {
            return;
        }
        std::env::set_var("GTK_USE_PORTAL", "1");
    }
}

#[cfg(target_os = "linux")]
fn portal_available() -> bool {
    use gtk::gio;
    use gtk::glib;
    // Oturum veri yolu yoksa denenmez (aksi halde GIO dbus-launch ile başlatmaya çalışıp
    // açılışı yavaşlatabilir)
    let runtime_bus = std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::Path::new(&d).join("bus").exists()).unwrap_or(false);
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() && !runtime_bus {
        return false;
    }
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return false };
    let has = |method: &str| -> bool {
        bus.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            method,
            None,
            Some(glib::VariantTy::new("(as)").unwrap()),
            gio::DBusCallFlags::NONE,
            500,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|v| v.child_value(0).get::<Vec<String>>())
        .is_some_and(|names| names.iter().any(|n| n == "org.freedesktop.portal.Desktop"))
    };
    has("ListNames") || has("ListActivatableNames")
}

#[cfg(target_os = "linux")]
thread_local! {
    /// Açık diyaloglar: yanıt gelene dek nesne yaşamalı
    static OPEN: std::cell::RefCell<Vec<gtk::FileChooserNative>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `mode`: "save" | "open" | "folder". Döner: seçilen yollar (vazgeçilirse None).
#[tauri::command]
pub async fn file_dialog(
    window: tauri::WebviewWindow,
    mode: String,
    title: Option<String>,
    default_path: Option<String>,
    filters: Option<Vec<Filter>>,
    multiple: Option<bool>,
) -> Result<Option<Vec<String>>, String> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::channel::<Option<Vec<String>>>();
        let filters = filters.unwrap_or_default();
        let w = window.clone();
        window
            .run_on_main_thread(move || {
                use gtk::prelude::*;
                let parent = w.gtk_window().ok();
                let action = match mode.as_str() {
                    "save" => gtk::FileChooserAction::Save,
                    "folder" => gtk::FileChooserAction::SelectFolder,
                    _ => gtk::FileChooserAction::Open,
                };
                let accept = match mode.as_str() {
                    "save" => crate::i18n::tr("common.save"),
                    _ => crate::i18n::tr("common.open"),
                };
                let dlg = gtk::FileChooserNative::new(title.as_deref(), parent.as_ref(), action, Some(&accept), Some(&crate::i18n::tr("common.cancel")));
                dlg.set_modal(true);
                if mode == "save" {
                    dlg.set_do_overwrite_confirmation(true);
                } else {
                    dlg.set_select_multiple(multiple.unwrap_or(false));
                }
                if let Some(p) = default_path.filter(|p| !p.trim().is_empty()) {
                    let path = std::path::Path::new(&p);
                    if let Some(dir) = path.parent().filter(|d| d.is_absolute() && d.is_dir()) {
                        dlg.set_current_folder(dir);
                    }
                    if mode == "save" {
                        if let Some(name) = path.file_name() {
                            dlg.set_current_name(&name.to_string_lossy());
                        }
                    }
                }
                let mut gtk_filters = Vec::new();
                for f in &filters {
                    let ff = gtk::FileFilter::new();
                    ff.set_name(Some(&f.name));
                    for e in &f.extensions {
                        ff.add_pattern(&format!("*.{}", e.to_lowercase()));
                        ff.add_pattern(&format!("*.{}", e.to_uppercase()));
                    }
                    dlg.add_filter(ff.clone());
                    gtk_filters.push((ff, f.extensions.clone()));
                }
                let is_save = mode == "save";
                dlg.connect_response(move |d, resp| {
                    let res = if resp == gtk::ResponseType::Accept {
                        let mut paths: Vec<std::path::PathBuf> = d.filenames();
                        // Kaydetmede seçilen türün uzantısı yoksa eklenir (Outlook gibi)
                        if is_save {
                            if let (Some(p), Some(sel)) = (paths.first_mut(), d.filter()) {
                                if let Some((_, exts)) = gtk_filters.iter().find(|(f, _)| f == &sel) {
                                    let cur = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                                    if !exts.is_empty() && !exts.iter().any(|e| e.eq_ignore_ascii_case(&cur)) {
                                        p.set_extension(&exts[0]);
                                    }
                                }
                            }
                        }
                        Some(paths.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
                    } else {
                        None
                    };
                    let _ = tx.send(res);
                    d.destroy();
                    OPEN.with(|o| o.borrow_mut().retain(|x| x != d));
                });
                dlg.show();
                OPEN.with(|o| o.borrow_mut().push(dlg));
            })
            .map_err(|e| e.to_string())?;
        // Yanıt beklenirken ana iş parçacığı serbesttir (arayüz donmaz)
        tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten()).await.map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, mode, title, default_path, filters, multiple);
        Err("unsupported".into())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn portal_check_is_fast() {
        let t = std::time::Instant::now();
        let _ = super::portal_available();
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
    }
}
