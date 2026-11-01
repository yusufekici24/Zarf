//! Lynora — Debian tabanlı dağıtımlar için Outlook benzeri kişisel bilgi yöneticisi.

pub mod backup;
pub mod commands;
pub mod db;
pub mod desktop;
pub mod ews_sync;
pub mod filedialog;
pub mod i18n;
pub mod links;
pub mod ical;
pub mod mail;
pub mod msgfile;
pub mod oauth;
pub mod pim_sync;
pub mod recurrence;
pub mod rules;
pub mod sync;
pub mod util;
pub mod visibility;
pub mod windows;
pub mod secret;
pub mod splash;

use commands::AppState;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager};

/// Komut satırında verilen dosya yolları ve mailto: bağlantıları.
#[derive(Default)]
pub struct StartupArgs(pub Mutex<Vec<String>>);

#[tauri::command]
fn startup_args(args: tauri::State<StartupArgs>) -> Vec<String> {
    std::mem::take(&mut *args.0.lock().unwrap())
}

fn useful_args(argv: &[String]) -> Vec<String> {
    argv.iter().skip(1).filter(|a| !a.starts_with("--")).cloned().collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// WebKitGTK'nin DMA-BUF çizicisi NVIDIA sürücüsünde ve sanal makinelerde (VMware,
/// VirtualBox, Hyper-V, QEMU) boş/donuk pencere ve takılmaya yol açabiliyor: bu
/// ortamlarda paylaşımlı bellek çizicisi kullanılır. Kullanıcı değişkeni kendisi
/// ayarladıysa dokunulmaz.
/// Wayland oturumunda XWayland üzerinden (X11) çalışılır: Wayland pencerelerin kendi
/// konumunu belirlemesine, her zaman üstte kalmasına ve öne gelmesine izin vermiyor; bu
/// yüzden masaüstü uyarı kartları sağ altta duramaz, pencereler kapandığı yerde açılamaz ve
/// "arkada mı" denetimi yapılamaz. XWayland yoksa ya da LYNORA_WAYLAND=1 verilmişse
/// doğrudan Wayland kullanılır (bu özellikler kısıtlı çalışır).
#[cfg(target_os = "linux")]
fn prefer_x11() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland");
    if wayland && std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() && std::env::var_os("LYNORA_WAYLAND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

#[cfg(target_os = "linux")]
fn tune_webkit() {
    // Ayarlar > Gelişmiş > "Donanım hızlandırma" kapatıldıysa (bazı ekran kartı sürücülerinde
    // ekranın kararıp geri gelmesi gibi görüntü sorunlarında) WebKit yazılımla çizer
    if db::default_data_dir().join("no-gpu").exists() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        return;
    }
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }
    let vendor = std::fs::read_to_string("/sys/class/dmi/id/sys_vendor").unwrap_or_default().to_lowercase();
    let product = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default().to_lowercase();
    let vm = ["vmware", "innotek", "virtualbox", "qemu", "kvm", "xen", "parallels", "bochs"].iter().any(|v| vendor.contains(v) || product.contains(v))
        || (vendor.contains("microsoft") && product.contains("virtual"));
    if vm || std::path::Path::new("/proc/driver/nvidia").exists() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

pub(crate) static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
static PENDING_ARGS: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

/// İkinci kez başlatıldı (ör. bir .msg dosyasına çift tıklandı): argümanlar çalışan örneğe
fn on_second_instance(args: Vec<String>) {
    match APP.get() {
        Some(app) => handle_second_instance(app, args),
        // Uygulama henüz kuruluyorsa kurulunca işlenir
        None => PENDING_ARGS.lock().unwrap_or_else(|e| e.into_inner()).push(args),
    }
}

fn handle_second_instance(app: &tauri::AppHandle, args: Vec<String>) {
    // İleti/yeni ileti kendi penceresinde açılır; diğer dosyalar (ör. .ics) ana pencerede
    let own_window = !args.is_empty()
        && args.iter().all(|a| {
            let l = a.to_lowercase();
            l.ends_with(".msg") || l.ends_with(".eml") || l.starts_with("mailto:")
        });
    if !args.is_empty() {
        let _ = app.emit("app://open-args", args);
    }
    // Dosya (ör. .msg) açılıyorsa onun penceresi öne gelir; yalnızca uygulama
    // yeniden başlatıldıysa ana pencere öne getirilir
    if !own_window {
        windows::show_main(app);
    }
}

/// Bildirim alanı simgesi: ana pencere gizliyken (yalnızca ileti penceresi açıkken) de
/// uygulamanın çalıştığı görünür; menüden ana pencere açılır ya da uygulama kapatılır.
fn tray(app: &tauri::AppHandle) {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let build = || -> tauri::Result<()> {
        let open = MenuItem::with_id(app, "open", t!("tray.open"), true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", t!("tray.exit"), true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])?;
        let mut b = TrayIconBuilder::with_id("lynora").tooltip("Lynora").menu(&menu).show_menu_on_left_click(false);
        if let Some(icon) = app.default_window_icon() {
            b = b.icon(icon.clone());
        }
        b.on_menu_event(|app, e| match e.id().as_ref() {
            "open" => windows::show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                windows::show_main(tray.app_handle());
            }
        })
        .build(app)?;
        Ok(())
    };
    // Masaüstünde bildirim alanı yoksa (ya da kitaplık kurulu değilse) sessizce geçilir
    let _ = build();
}

pub fn run() {
    #[cfg(target_os = "linux")]
    tune_webkit();
    // Dosya diyaloğu: masaüstünün dosya seçici servisi varsa ayrı süreçte (uygulama donmaz)
    filedialog::prefer_portal();
    #[cfg(target_os = "linux")]
    prefer_x11();
    // Dil sistemi veritabanından önce yüklenmeli: ilk çalıştırmada varsayılan
    // klasör/kategori adları ve hoş geldiniz iletisi seçili dilde oluşturulur.
    // Beklenmeyen hatalar ayrıntısıyla günlüğe yazılır (hata bildirimi için)
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let line = format!("[{}] {info}\n{bt}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        eprintln!("{line}");
        if let Some(dir) = dirs::data_dir().map(|d| d.join("lynora")) {
            let _ = std::fs::create_dir_all(&dir);
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }));
    // Tek örnek: uygulama zaten açıksa dosya/bağlantı ona iletilir ve bu süreç kapanır
    let argv0: Vec<String> = std::env::args().collect();
    if desktop::single_instance(&useful_args(&argv0), on_second_instance) {
        return;
    }
    i18n::init(None);
    let db = db::Db::open_default().expect("veritabanı açılamadı");
    oauth::init(db.clone());
    // Eski sürümlerin karıştırılmış parolaları/belirteçleri güvenli biçime taşınır
    // (anahtarlık kilit açma sorabileceği için açılışı bekletmez)
    {
        let db = db.clone();
        std::thread::spawn(move || {
            for mut a in db.accounts() {
                let mut changed = false;
                if secret::is_legacy(&a.password) {
                    let plain = util::deobfuscate(&a.password);
                    a.password = secret::seal(&format!("{}|password", a.email.to_lowercase()), &plain);
                    changed = true;
                }
                if secret::is_legacy(&a.oauth_token) {
                    a.oauth_token = secret::encrypt_local(&util::deobfuscate(&a.oauth_token));
                    changed = true;
                }
                if changed {
                    let _ = db.save_account(&a);
                }
            }
        });
    }
    if let Some(lang) = db.setting("language") {
        i18n::set_language(&lang);
    }

    let initial: Vec<String> = useful_args(&std::env::args().collect::<Vec<_>>());

    tauri::Builder::default()
        .plugin(links::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(StartupArgs(Mutex::new(initial)))
        .on_window_event(|window, event| match event {
            // Pencere yeri ve boyutu hatırlanır (Outlook gibi kapandığı yerde açılır)
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                if let Some(w) = window.app_handle().get_webview_window(window.label()) {
                    windows::remember(&w, false);
                }
            }
            tauri::WindowEvent::Focused(false) => {
                if let Some(w) = window.app_handle().get_webview_window(window.label()) {
                    windows::remember(&w, true);
                }
            }
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                if let Some(w) = window.app_handle().get_webview_window("main") {
                    windows::remember(&w, true);
                }
                if windows::main_close_requested(window.app_handle()) {
                    api.prevent_close();
                }
            }
            // Anımsatıcı penceresi kapatılınca yok edilmez, gizlenir (sonraki anımsatıcıda yine açılır)
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "reminders" => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Destroyed => windows::on_destroyed(window.app_handle(), window.label()),
            _ => {}
        })
        .setup(move |app| {
            // Açılış ekranı hemen görünür; ilk pencere hazır olunca kapanır
            if !std::env::args().any(|a| a == "--minimized") {
                splash::show(&t!("splash.starting"));
            }
            let _ = APP.set(app.handle().clone());
            for args in std::mem::take(&mut *PENDING_ARGS.lock().unwrap_or_else(|e| e.into_inner())) {
                handle_second_instance(app.handle(), args);
            }
 
            // Ana pencere gizli oluşturulur, ön yüz ilk çizimini bitirince gösterilir
            // (windows::reveal_window): açılışta beyaz ekran ve yerleşim kayması görünmez.
            // Oturum açılışında (autostart) ya da yalnızca bir ileti dosyası / mailto ile
            // başlatıldıysa ana pencere simge durumunda açılır; ileti kendi penceresinde öne gelir.
            let argv: Vec<String> = std::env::args().collect();
            let files = useful_args(&argv);
            let only_items = !files.is_empty()
                && files.iter().all(|a| {
                    let l = a.to_lowercase();
                    l.ends_with(".msg") || l.ends_with(".eml") || l.starts_with("mailto:")
                });
            windows::set_start_minimized(argv.iter().any(|a| a == "--minimized"));
            windows::set_start_hidden(only_items);
            tray(app.handle());
            let handle = app.handle().clone();
            let emitter: sync::Emitter = Arc::new(move |event: &str, payload: serde_json::Value| {
                let _ = handle.emit(event, payload);
            });
            let mail = sync::MailService::new(db.clone(), emitter);
            app.manage(AppState {
                db: db.clone(),
                mail: mail.clone(),
                external: Mutex::new(Default::default()),
                next_external: AtomicI64::new(1),
                rule_cancel: AtomicBool::new(false),
            });

            // Ana pencere son kapandığı yerde ve boyutta açılır (gizliyken yerleştirilir)
            windows::init_theme(&db.setting("theme").unwrap_or_else(|| "system".into()));
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_background_color(Some(windows::chrome_color()));
                windows::place_window(&w, "main");
                windows::reveal_later(&w, 5000);
            }

            // Eski adla (MailNix) yapılmış masaüstü ayarlarını yeni ada aktar
            std::thread::spawn(commands::migrate_legacy_integration);

            // Yeni ileti / okuma pencereleri anında açılsın diye yedek pencere hazırla
            let h2 = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                windows::ensure_spare(&h2);
            });

            // Görev çubuğu okunmamış sayısı arka uçta da tutulur: ana pencere gizli/küçültülmüş,
            // ekran kilitli ya da uygulama yeni açılmışken de (ön yüz beklemeden) her zaman doğru
            // sayı görünür. Değişince hemen, değişmese de dakikada bir yeniden uygulanır.
            let hb = app.handle().clone();
            let dbb = db.clone();
            std::thread::Builder::new()
                .name("lynora-badge".into())
                .spawn(move || {
                    let mut last = -1i64;
                    let mut tick = 0u32;
                    loop {
                        std::thread::sleep(Duration::from_secs(if tick < 6 { 1 } else { 4 }));
                        tick = tick.wrapping_add(1);
                        let n = dbb.inbox_unread();
                        if n != last || tick % 15 == 0 {
                            last = n;
                            windows::set_unread_badge(hb.clone(), n);
                        }
                    }
                })
                .ok();

            // Eski iletilerin ek adları arka planda doldurulur (aramada ek önerileri için)
            let dba = db.clone();
            std::thread::Builder::new()
                .name("lynora-attnames".into())
                .spawn(move || {
                    std::thread::sleep(Duration::from_secs(40));
                    while dba.backfill_att_names(100) > 0 {
                        std::thread::sleep(Duration::from_millis(300));
                    }
                })
                .ok();

            // Zamanlayıcılar: giden kutusu, otomatik eşitleme, hatırlatıcılar
            let handle = app.handle().clone();
            let db2 = db.clone();
            std::thread::Builder::new()
                .name("lynora-timer".into())
                .spawn(move || {
                    if db2.setting_bool("sync_on_start", true) {
                        // Önce arayüz yüklensin (açılışta işlemci ve disk ona kalsın)
                        std::thread::sleep(Duration::from_millis(1500));
                        mail.sync_all();
                    }
                    let mut tick: u64 = 0;
                    let mut last_reminders = String::new();
                    // Sağdan kayan toplantı/görev kartları: "yaklaşıyor" ve "başladı" birer kez
                    let mut toasted: std::collections::HashSet<String> = Default::default();
                    // Adres defteri: açılıştan bir süre sonra ve saatte bir, süresi dolmuşsa
                    // arka planda yenilenir (Outlook OAB gibi günde bir)
                    let mut dir_check: u64 = 8;
                    loop {
                        // 15 sn'lik adımlar: yeni posta Outlook gibi hemen gelsin
                        std::thread::sleep(Duration::from_secs(15));
                        tick += 1;
                        // Exchange: anlık bildirim açık değilse 15 sn'de bir, açıksa dakikada bir
                        // güvenlik denetimi (değişiklik yoksa tek küçük istek)
                        mail.ensure_push();
                        mail.quick_check_ex(tick % 4 == 0);
                        if tick % 2 == 0 {
                            mail.process_outbox();
                        }
                        let interval = db2.setting_i64("sync_interval", 10).max(0) as u64;
                        if interval > 0 && tick % (interval * 4) == 0 && !mail.busy() {
                            mail.sync_all();
                        }
                        if tick >= dir_check {
                            dir_check = tick + 240;
                            commands::refresh_directories_if_due(&db2);
                        }
                        let due = commands::due_reminders(&db2);
                        let keys: String = due.iter().map(|r| r_key(r)).collect::<Vec<_>>().join("|");
                        if db2.setting("reminder_alerts").map_or(true, |v| v == "1") {
                            let n = db::now();
                            let mut cards = Vec::new();
                            for r in &due {
                                let Ok(serde_json::Value::Object(mut v)) = serde_json::to_value(r) else { continue };
                                let key = v.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string();
                                let start = v.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                let is_event = v.get("kind").and_then(|x| x.as_str()) == Some("event");
                                // Başlangıç anı (son 3 dakika içinde) ayrı "Toplantı başladı" kartı
                                let started = is_event && start <= n && n - start < 180.0;
                                let phase = if started && toasted.insert(format!("{key}|now")) {
                                    toasted.insert(format!("{key}|soon"));
                                    "now"
                                } else if toasted.insert(format!("{key}|soon")) {
                                    "soon"
                                } else {
                                    continue;
                                };
                                v.insert("type".into(), "reminder".into());
                                v.insert("phase".into(), phase.into());
                                cards.push(serde_json::Value::Object(v));
                            }
                            // Kapatılan/ertelenen öğe yeniden gelirse kartı yine gösterilir
                            let live: std::collections::HashSet<String> = due.iter().map(r_key).collect();
                            toasted.retain(|k| live.contains(k.split('|').next().unwrap_or("")));
                            if !cards.is_empty() {
                                windows::show_mail_alerts(handle.clone(), serde_json::Value::Array(cards));
                            }
                        }
                        // Outlook gibi ayrı "Anımsatıcı" penceresi (zamanı gelen öğe değişince)
                        if keys != last_reminders {
                            if due.is_empty() {
                                windows::hide_reminders(&handle);
                            } else {
                                windows::show_reminders(&handle);
                            }
                        }
                        last_reminders = keys;
                    }
                })
                .expect("zamanlayıcı");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            startup_args,
            commands::get_locales,
            commands::set_language,
            commands::get_settings,
            commands::set_setting,
            commands::list_accounts,
            commands::guess_servers,
            commands::save_account,
            commands::delete_account,
            commands::retry_account,
            commands::test_account,
            commands::folder_tree,
            commands::create_folder,
            commands::rename_folder,
            commands::delete_folder,
            commands::empty_folder,
            commands::mark_folder_read,
            commands::sync_folder,
            commands::sync_all,
            commands::sync_account,
            commands::mail_busy,
            commands::list_messages,
            commands::get_message,
            commands::conversation,
            commands::set_read,
            commands::set_flagged,
            commands::set_categories,
            commands::toggle_category,
            commands::set_importance,
            commands::move_messages,
            commands::copy_messages,
            commands::move_to_role,
            commands::delete_messages,
            commands::block_sender,
            commands::save_attachment,
            commands::save_all_attachments,
            commands::attachment_temp_path,
            commands::open_attachment_as_message,
            commands::import_attachment_calendar,
            commands::import_attachment_vcard,
            commands::save_message_as,
            commands::import_external_message,
            commands::send_read_receipt,
            commands::compose_prefill,
            commands::add_shared_mailbox,
            commands::create_backup,
            commands::backup_is_encrypted,
            commands::prepare_restore,
            commands::cancel_restore,
            commands::finish_restore,
            commands::save_recovery,
            commands::list_recovery,
            commands::remove_recovery,
            commands::send_mail,
            commands::save_draft,
            commands::discard_draft,
            commands::process_outbox,
            commands::retry_outbox,
            commands::address_book,
            commands::open_file,
            commands::import_mail_files,
            commands::export_mbox,
            commands::import_pst,
            commands::list_contacts,
            commands::get_contact,
            commands::contact_by_email,
            commands::save_contact,
            commands::delete_contacts,
            commands::list_groups,
            commands::save_group,
            commands::delete_group,
            commands::import_contacts,
            commands::export_contacts,
            commands::list_calendars,
            commands::save_calendar,
            commands::delete_calendar,
            commands::list_occurrences,
            commands::search_events,
            commands::get_event,
            commands::save_event,
            commands::delete_event,
            commands::detach_occurrence,
            commands::import_ics,
            commands::export_ics,
            commands::export_event,
            commands::send_invitation,
            commands::list_tasks,
            commands::get_task,
            commands::save_task,
            commands::delete_tasks,
            commands::export_tasks,
            commands::list_notes,
            commands::save_note,
            commands::delete_note,
            commands::list_rules,
            commands::save_rule,
            commands::delete_rule,
            commands::preview_rules,
            commands::run_rules,
            commands::cancel_rule_run,
            commands::list_categories,
            commands::save_category,
            commands::delete_category,
            commands::get_reminders,
            filedialog::file_dialog,
            windows::close_reminders,
            windows::set_window_kind,
            commands::dismiss_reminder,
            commands::app_info,
            commands::system_identity,
            commands::discover_account,
            commands::suggest_recipients,
            commands::refresh_address_book,
            commands::address_book_status,
            commands::forget_recipient,
            commands::oauth_authorize,
            commands::set_auto_reply,
            commands::respond_meeting,
            commands::meeting_info,
            commands::frontend_log,
            commands::set_folder_favorite,
            commands::message_temp_file,
            commands::set_autostart,
            commands::cancel_outbox,
            commands::set_active_folder,
            commands::load_older,
            commands::search_server,
            commands::refresh_server_rules,
            commands::server_rule_spec,
            commands::save_server_rule,
            commands::sender_photo,
            commands::set_pinned,
            commands::read_text_file,
            commands::write_text_file,
            commands::file_info,
            commands::write_temp_file,
            commands::search_suggest,
            commands::image_data_url,
            commands::list_rooms,
            commands::free_busy,
            commands::free_busy_details,
            commands::mailbox_usage,
            windows::open_window,
            windows::window_ready,
            windows::focus_main,
            windows::set_unread_badge,
            windows::drag_messages,
            commands::system_fonts,
            commands::group_addresses,
            commands::expand_group,
            commands::contact_card,
            commands::calendar_ics_temp,
            commands::remote_occurrences,
            commands::refresh_team_calendars,
            commands::directory_page,
            commands::net_state,
            commands::set_ui_scale,
            commands::fit_window,
            windows::show_mail_alerts,
            windows::reveal_window,
            windows::window_painted,
            windows::main_hidden,
            windows::recycle_window,
            visibility::app_visible,
            commands::open_url,
            commands::open_attachment,
            commands::desktop_notify,
            windows::alerts_area,
            windows::alerts_input,
            windows::alerts_take,
            windows::alerts_layout,
            commands::screen_info,
            commands::sibling_ids,
            commands::default_app_status,
            commands::set_default_app,
            commands::add_internet_calendar,
            commands::refresh_internet_calendars,
            commands::display_names,
        ])
        .run(tauri::generate_context!())
        .expect("Lynora başlatılamadı");
}

fn r_key(r: &commands::Reminder) -> String {
    serde_json::to_value(r).ok().and_then(|v| v.get("key").and_then(|k| k.as_str()).map(String::from)).unwrap_or_default()
}
