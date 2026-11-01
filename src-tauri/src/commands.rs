//! Arayüzün çağırdığı Tauri komutları.

use crate::db::*;
use crate::mail::build::{build_raw, guess_mime, ComposedMail};
use crate::mail::{self, parse};
use crate::sync::MailService;
use crate::{ical, msgfile, recurrence, t, util};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub db: Db,
    pub mail: MailService,
    /// Dosyadan açılan (veritabanında olmayan) iletiler: negatif kimlik → ham veri
    pub external: Mutex<HashMap<i64, Vec<u8>>>,
    pub next_external: AtomicI64,
}

impl AppState {
    fn raw_of(&self, id: i64) -> Option<Vec<u8>> {
        if id < 0 {
            self.external.lock().unwrap().get(&id).cloned()
        } else {
            crate::ews_sync::ensure_content(&self.db, id)
        }
    }

    /// raw_of; sunucudan indirme gerekirse ortak iş parçacıklarını bekletmeden (spawn_blocking)
    async fn raw_of_async(&self, id: i64) -> Option<Vec<u8>> {
        if id < 0 {
            return self.raw_of(id);
        }
        let db = self.db.clone();
        tauri::async_runtime::spawn_blocking(move || crate::ews_sync::ensure_content(&db, id)).await.ok().flatten()
    }

    fn add_external(&self, raw: Vec<u8>) -> i64 {
        let id = -self.next_external.fetch_add(1, Ordering::SeqCst);
        self.external.lock().unwrap().insert(id, raw);
        id
    }
}

type R<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ======================================================================= dil / ayarlar
#[tauri::command(async)]
pub fn get_locales() -> Value {
    json!({ "current": crate::i18n::current(), "langs": crate::i18n::all() })
}

#[tauri::command(async)]
pub fn set_language(state: State<AppState>, code: String) -> String {
    state.db.set_setting("language", &code);
    crate::i18n::set_language(&code)
}

#[tauri::command(async)]
pub fn get_settings(state: State<AppState>) -> HashMap<String, String> {
    let c = state.db.conn();
    let mut st = c.prepare("SELECT key, value FROM settings WHERE key NOT LIKE 'recent:%' AND key NOT LIKE 'autoreply:%'").unwrap();
    st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
        .unwrap()
        .flatten()
        .collect()
}

#[tauri::command(async)]
pub fn set_setting(state: State<AppState>, key: String, value: String) {
    state.db.set_setting(&key, &value);
    // Donanım hızlandırma: veritabanı açılmadan (WebKit başlamadan) okunabilsin diye dosyayla
    if key == "hw_accel" {
        let marker = crate::db::default_data_dir().join("no-gpu");
        if value == "0" {
            let _ = std::fs::write(&marker, b"");
        } else {
            let _ = std::fs::remove_file(&marker);
        }
    }
    // Outlook "Çevrimdışı Çalış": kapatılınca hemen eşitlenir
    if key == "work_offline" {
        let off = value == "1";
        mail::net::set_work_offline(off);
        if !off {
            mail::net::set_disconnected(false);
            state.mail.sync_all();
        }
    }
}

/// Bağlantı durumu: "online" | "disconnected" | "work_offline"
#[tauri::command(async)]
pub fn net_state() -> &'static str {
    if mail::net::work_offline() {
        "work_offline"
    } else if mail::net::is_disconnected() {
        "disconnected"
    } else {
        "online"
    }
}

// ======================================================================= hesaplar
#[tauri::command(async)]
pub fn list_accounts(state: State<AppState>) -> Vec<Account> {
    state.db.accounts()
}

#[tauri::command(async)]
pub fn guess_servers(email: String) -> mail::ServerGuess {
    mail::guess_servers(&email)
}

#[tauri::command(async)]
pub fn save_account(state: State<AppState>, account: Account, password: Option<String>, oauth_token: Option<String>) -> R<i64> {
    let mut a = account;
    let old = state.db.account(a.id);
    a.password = match &password {
        // Sistem anahtarlığına (yoksa AES-256-GCM ile) şifreli; eski kayıt silinir
        Some(p) if !p.is_empty() => {
            if let Some(o) = &old {
                if !o.email.eq_ignore_ascii_case(&a.email) {
                    crate::secret::remove(&o.password);
                }
            }
            crate::secret::seal(&format!("{}|password", a.email.to_lowercase()), p)
        }
        _ => old.as_ref().map(|x| x.password.clone()).unwrap_or_default(),
    };
    let new_token = oauth_token.filter(|t| !t.is_empty());
    a.oauth_token = new_token.clone().unwrap_or_else(|| old.as_ref().map(|x| x.oauth_token.clone()).unwrap_or_default());
    // Yeni parola girildiyse ya da kimlik doğrulama yöntemi değiştiyse hesap yeniden etkinleşir
    let auth_changed = old.as_ref().map(|o| o.auth_method != a.auth_method || o.username != a.username).unwrap_or(true);
    if password.as_deref().map(|p| !p.is_empty()).unwrap_or(false) || new_token.is_some() || auth_changed {
        a.auth_state = "ok".into();
        a.auth_error = String::new();
    } else if let Some(o) = &old {
        a.auth_state = o.auth_state.clone();
        a.auth_error = o.auth_error.clone();
    }
    if a.name.trim().is_empty() {
        a.name = a.email.clone();
    }
    if a.sync_limit <= 0 {
        a.sync_limit = 500;
    }
    let is_new = a.id <= 0;
    let id = state.db.save_account(&a).map_err(err)?;
    mail::ews::pool_clear(id);
    if !is_new && a.auth_state == "ok" {
        state.db.update_shared_credentials(id, &a.password, &a.oauth_token);
    }
    if is_new {
        if state.db.accounts().len() == 1 {
            state.db.remove_welcome();
        }
        state.mail.sync_account(id);
    }
    Ok(id)
}

/// Duraklatılmış hesabı kullanıcının isteğiyle bir kez yeniden dener.
#[tauri::command(async)]
pub fn retry_account(state: State<AppState>, id: i64) {
    state.db.set_account_auth(id, "ok", "");
    state.mail.sync_account(id);
}

#[tauri::command(async)]
pub fn delete_account(state: State<AppState>, id: i64) -> R<()> {
    if let Some(a) = state.db.account(id) {
        // Paylaşılan kutunun parolası ana hesabındır, silinmez
        if a.shared_of.is_none() {
            crate::secret::remove(&a.password);
        }
    }
    // Ana hesapla birlikte onun kimliğiyle açılan paylaşılan kutular da kalkar
    for shared in state.db.accounts().into_iter().filter(|a| a.shared_of == Some(id)) {
        state.db.delete_account(shared.id).map_err(err)?;
    }
    state.db.delete_account(id).map_err(err)
}

/// Paylaşılan posta kutusu: erişim yetkimiz olan başka bir kutu, kendi kimliğimizle ayrı hesap
/// gibi eklenir; parola yeniden girilmez, ana hesapla ortaktır.
#[tauri::command(async)]
pub fn add_shared_mailbox(state: State<AppState>, base_id: i64, email: String, name: String) -> R<i64> {
    let base = state.db.account(base_id).filter(|a| a.is_exchange()).ok_or_else(|| t!("error.no_account"))?;
    let email = email.trim().to_lowercase();
    if mail::bare_addresses(&email).len() != 1 {
        return Err(t!("shared.bad_email"));
    }
    if state.db.accounts().iter().any(|a| a.email.eq_ignore_ascii_case(&email)) {
        return Err(t!("shared.exists", email));
    }
    let mut a = base.clone();
    a.id = 0;
    a.username = base.login().to_string();
    a.name = if name.trim().is_empty() { email.clone() } else { name.trim().to_string() };
    a.display_name = a.name.clone();
    a.email = email;
    a.signature = String::new();
    a.shared_of = Some(base.id);
    let id = state.db.save_account(&a).map_err(err)?;
    state.mail.sync_account(id);
    Ok(id)
}

#[derive(Serialize)]
pub struct TestResult {
    imap_ok: bool,
    smtp_ok: bool,
    log: String,
}

#[tauri::command]
pub async fn test_account(state: State<'_, AppState>, account: Account, password: Option<String>) -> R<TestResult> {
    let pw = match password {
        Some(p) if !p.is_empty() => p,
        _ => state.db.account(account.id).map(|a| a.password_plain()).unwrap_or_default(),
    };
    let res = tauri::async_runtime::spawn_blocking(move || MailService::test_account(&account, &pw)).await.map_err(err)?;
    Ok(TestResult { imap_ok: res.0, smtp_ok: res.1, log: res.2 })
}

// ======================================================================= klasörler
#[derive(Serialize, Clone)]
pub struct FolderNode {
    #[serde(flatten)]
    folder: Folder,
    display: String,
    unread: i64,
    total: i64,
}

#[derive(Serialize)]
pub struct AccountTree {
    account: Option<Account>,
    folders: Vec<FolderNode>,
}

#[tauri::command(async)]
pub fn folder_tree(state: State<AppState>) -> Value {
    let db = &state.db;
    // Sayılar klasör başına iki sorgu yerine tek gruplu sorguyla alınır
    let counts = db.folder_counts();
    let node = |f: Folder| {
        let (total, unread) = counts.get(&f.id).copied().unwrap_or((0, 0));
        FolderNode { display: f.display_name(), unread, total, folder: f }
    };
    let accounts = db.accounts();
    // Outlook gibi: her hesabın Gelen/Gönderilmiş/Taslaklar/Silinmiş klasörleri ilk kez Sık Kullanılanlar'a eklenir
    for a in &accounts {
        let key = format!("fav_init:{}", a.id);
        if db.setting(&key).is_none() {
            let folders = db.folders(Some(a.id));
            if folders.iter().any(|f| f.role.as_deref() == Some("inbox")) {
                for mut f in folders.into_iter().filter(|f| matches!(f.role.as_deref(), Some("inbox" | "sent" | "drafts" | "trash"))) {
                    f.favorite = true;
                    let _ = db.update_folder(&f);
                }
                db.set_setting(&key, "1");
            }
        }
    }
    let mut favorites: Vec<FolderNode> = Vec::new();
    let mut groups: Vec<AccountTree> = Vec::new();
    for a in accounts.iter() {
        let folders: Vec<FolderNode> = db.folders(Some(a.id)).into_iter().map(node).collect();
        favorites.extend(folders.iter().filter(|n| n.folder.favorite).cloned());
        groups.push(AccountTree { folders, account: Some(a.clone()) });
    }
    // Outlook'ta "Bu Bilgisayarda" diye bir klasör grubu yoktur: hesap varken yerel
    // klasörler gösterilmez (Giden Kutusu doluysa ilk hesabın altında görünür).
    let local: Vec<FolderNode> = db.folders(None).into_iter().map(node).collect();
    let outbox = local.iter().find(|n| n.folder.role.as_deref() == Some("outbox")).cloned();
    if accounts.is_empty() {
        favorites.extend(local.iter().filter(|n| n.folder.favorite).cloned());
        groups.push(AccountTree { account: None, folders: local });
    }
    // Sık kullanılanlar sırası: önce role göre Outlook düzeni, sonra ada göre
    let rank = |r: Option<&str>| match r {
        Some("inbox") => 0,
        Some("sent") => 1,
        Some("drafts") => 2,
        Some("trash") => 3,
        Some("archive") => 4,
        Some("junk") => 5,
        _ => 6,
    };
    favorites.sort_by_key(|n| (rank(n.folder.role.as_deref()), n.folder.account_id.unwrap_or(0)));
    json!({
        "groups": groups,
        "favorites": favorites,
        "outbox": if accounts.is_empty() { None } else { outbox },
        "multi": accounts.len() > 1,
        "unread_total": db.unread_total()
    })
}

#[tauri::command(async)]
pub fn set_folder_favorite(state: State<AppState>, id: i64, value: bool) -> R<()> {
    let mut f = state.db.folder(id).ok_or_else(|| t!("error.not_found"))?;
    f.favorite = value;
    state.db.update_folder(&f).map_err(err)
}

/// İletiyi geçici bir .msg/.eml dosyasına yazar (yeni iletiye ek olarak sürükleme / ek olarak iletme).
#[tauri::command]
pub async fn message_temp_file(state: State<'_, AppState>, id: i64, format: Option<String>) -> R<Value> {
    guarded(move || {
    let raw = state.raw_of(id).ok_or_else(|| t!("error.not_found"))?;
    let p = parse::parse(&raw);
    let ext = if format.as_deref() == Some("eml") { "eml" } else { "msg" };
    let base = util::safe_filename(if p.subject.trim().is_empty() { "ileti" } else { p.subject.trim() });
    let base: String = base.chars().take(80).collect();
    let name = format!("{base}.{ext}");
    let dir = util::temp_dir().join(format!("drag-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(&name);
    let (data, mime) = if ext == "msg" {
        (msgfile::eml_to_msg(&raw).map_err(err)?, "application/vnd.ms-outlook")
    } else {
        (raw.clone(), "message/rfc822")
    };
    std::fs::write(&path, &data).map_err(err)?;
    Ok(json!({ "path": path.to_string_lossy(), "name": name, "mime": mime, "size": data.len() }))
    })
}

#[tauri::command(async)]
pub fn create_folder(state: State<AppState>, account_id: Option<i64>, name: String, parent_id: Option<i64>) {
    state.mail.create_folder(account_id, name, parent_id);
}

#[tauri::command(async)]
pub fn rename_folder(state: State<AppState>, id: i64, name: String) {
    state.mail.rename_folder(id, name);
}

#[tauri::command(async)]
pub fn delete_folder(state: State<AppState>, id: i64) {
    state.mail.delete_folder(id);
}

#[tauri::command(async)]
pub fn empty_folder(state: State<AppState>, id: i64) {
    state.mail.empty_folder(id);
}

#[tauri::command(async)]
pub fn mark_folder_read(state: State<AppState>, id: i64) {
    state.mail.mark_folder_read(id);
}

#[tauri::command(async)]
pub fn sync_folder(state: State<AppState>, id: i64) {
    state.mail.sync_folder(id);
}

#[tauri::command(async)]
pub fn sync_all(state: State<AppState>) {
    state.mail.sync_all();
}

#[tauri::command(async)]
pub fn sync_account(state: State<AppState>, id: i64) {
    state.mail.sync_account(id);
}

#[tauri::command(async)]
pub fn mail_busy(state: State<AppState>) -> bool {
    state.mail.busy()
}

// ======================================================================= iletiler
#[tauri::command(async)]
pub fn list_messages(
    state: State<AppState>,
    folder_id: Option<i64>,
    search: String,
    filter: String,
    order: Option<crate::db::ListOrder>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Value {
    let (rows, total) = state.db.messages_page(
        folder_id,
        search.trim(),
        &MsgFilter::parse(&filter),
        &order.unwrap_or_default(),
        offset.unwrap_or(0).max(0),
        limit.unwrap_or(500).clamp(1, 5000),
    );
    json!({ "rows": rows, "total": total })
}

#[derive(Serialize)]
pub struct MessageView {
    #[serde(flatten)]
    message: Message,
    html: String,
    text: String,
    attachments: Vec<parse::Attachment>,
    reply_to: String,
    read_receipt_to: String,
    has_remote: bool,
    has_calendar: bool,
    has_vcard: bool,
    folder_role: String,
    external: bool,
}

fn view_from_raw(id: i64, raw: &[u8], base: Option<Message>) -> MessageView {
    let p = parse::parse(raw);
    let html = if p.html.is_empty() { String::new() } else { mail::html::sanitize(&p.html_with_inline_images()) };
    static REMOTE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)(src|background)\s*=\s*["']?\s*(https?:)?//"#).unwrap());
    let has_remote = REMOTE.is_match(&html)
        || html.to_lowercase().contains("url(http");
    let atts: Vec<parse::Attachment> = p.real_attachments().into_iter().cloned().collect();
    let has_calendar = atts.iter().any(|a| a.mime == "text/calendar" || a.name.to_lowercase().ends_with(".ics"));
    let has_vcard = atts.iter().any(|a| a.mime.contains("vcard") || a.name.to_lowercase().ends_with(".vcf"));
    let message = base.unwrap_or_else(|| {
        let mut m = p.clone().into_message();
        m.id = id;
        m.is_read = true;
        m
    });
    MessageView {
        text: if p.text.is_empty() { message.body_text.clone() } else { p.text.clone() },
        html,
        attachments: atts,
        reply_to: p.reply_to.clone(),
        read_receipt_to: p.read_receipt_to.clone(),
        has_remote,
        has_calendar,
        has_vcard,
        folder_role: String::new(),
        external: id < 0,
        message,
    }
}

#[tauri::command]
pub async fn get_message(state: State<'_, AppState>, id: i64) -> R<MessageView> {
    if id < 0 {
        let raw = state.raw_of(id).ok_or("?")?;
        return tauri::async_runtime::spawn_blocking(move || guarded(move || Ok(view_from_raw(id, &raw, None)))).await.map_err(err)?;
    }
    // Yalnızca başlığı inmiş Exchange iletisinin gövdesi ortak iş parçacıklarını bekletmeden
    // indirilir (başarısız olursa — çevrimdışı — yeniden denenmez, başlıklar gösterilir)
    if state.db.needs_content(id) {
        let _ = state.raw_of_async(id).await;
    }
    // Ayrıştırma ve HTML temizleme ayrı iş parçacığında (büyük iletilerde arayüz beklemez)
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        guarded(move || {
            let m = db.message(id).ok_or_else(|| t!("error.not_found"))?;
            let raw = m.raw.clone();
            let role = db.folder(m.folder_id).and_then(|f| f.role).unwrap_or_default();
            let mut v = if raw.is_empty() {
                MessageView {
                    html: String::new(),
                    text: m.body_text.clone(),
                    attachments: vec![],
                    reply_to: String::new(),
                    read_receipt_to: String::new(),
                    has_remote: false,
                    has_calendar: false,
                    has_vcard: false,
                    folder_role: String::new(),
                    external: false,
                    message: m,
                }
            } else {
                view_from_raw(id, &raw, Some(m))
            };
            v.folder_role = role;
            Ok(v)
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command(async)]
pub fn conversation(state: State<AppState>, id: i64) -> Vec<MessageRow> {
    state.db.message_row(id).map(|m| state.db.conversation(&m)).unwrap_or_default()
}

#[tauri::command(async)]
pub fn set_read(state: State<AppState>, ids: Vec<i64>, value: bool) {
    state.mail.set_read(&ids, value);
}

#[tauri::command(async)]
pub fn set_flagged(state: State<AppState>, ids: Vec<i64>, value: bool, due: Option<f64>) {
    state.mail.set_flagged(&ids, value);
    // Bayraklanan ileti için takip görevi (Outlook davranışı)
    for id in ids {
        let existing = state.db.task_for_message(id);
        if value {
            if let Some(m) = state.db.message(id) {
                let mut t = existing.unwrap_or_else(Task::new_default);
                t.title = if m.subject.is_empty() { t!("mail.no_subject") } else { m.subject.clone() };
                t.notes = format!("{}: {}\n\n{}", t!("mail.from"), m.sender, m.body_text.chars().take(2000).collect::<String>());
                t.message_id = Some(id);
                t.due_date = due.or(t.due_date);
                t.status = "not_started".into();
                let _ = state.db.save_task(&t);
            }
        } else if let Some(mut t) = existing {
            t.status = "completed".into();
            let _ = state.db.save_task(&t);
        }
    }
}

#[tauri::command(async)]
pub fn set_categories(state: State<AppState>, ids: Vec<i64>, categories: Vec<String>) {
    state.mail.set_categories(&ids, &categories);
}

#[tauri::command(async)]
pub fn toggle_category(state: State<AppState>, ids: Vec<i64>, category: String) {
    for id in ids {
        if let Some(m) = state.db.message_row(id) {
            let mut c = m.category_list();
            if let Some(p) = c.iter().position(|x| x == &category) {
                c.remove(p);
            } else {
                c.push(category.clone());
            }
            state.mail.set_categories(&[id], &c);
        }
    }
}

#[tauri::command(async)]
pub fn set_importance(state: State<AppState>, ids: Vec<i64>, importance: i64) {
    state.mail.set_importance(&ids, importance);
}

#[tauri::command(async)]
pub fn move_messages(state: State<AppState>, ids: Vec<i64>, folder_id: i64) {
    state.mail.move_messages(&ids, folder_id);
}

#[tauri::command(async)]
pub fn copy_messages(state: State<AppState>, ids: Vec<i64>, folder_id: i64) {
    for id in ids {
        if let Some(raw) = state.raw_of(id) {
            if !raw.is_empty() {
                state.mail.import_raw(folder_id, raw, true);
            }
        }
    }
}

#[tauri::command(async)]
pub fn move_to_role(state: State<AppState>, ids: Vec<i64>, role: String) {
    let Some(first) = ids.first().and_then(|i| state.db.message_row(*i)) else { return };
    if let Some(f) = state.db.folder_by_role_or_local(first.account_id, &role) {
        state.mail.move_messages(&ids, f.id);
    }
}

#[tauri::command(async)]
pub fn delete_messages(state: State<AppState>, ids: Vec<i64>, permanent: bool) {
    state.mail.delete(&ids, permanent);
}

#[tauri::command(async)]
pub fn block_sender(state: State<AppState>, ids: Vec<i64>) -> R<String> {
    let m = ids.first().and_then(|i| state.db.message_row(*i)).ok_or("?")?;
    let addr = mail::split_name_addr(&m.sender).1;
    if addr.is_empty() {
        return Err("?".into());
    }
    state
        .db
        .save_rule(&Rule {
            name: t!("rules.blocked", addr),
            enabled: true,
            field: "from".into(),
            op: "contains".into(),
            value: addr.clone(),
            action: "junk".into(),
            stop: true,
            ..Default::default()
        })
        .map_err(err)?;
    if let Some(f) = state.db.folder_by_role_or_local(m.account_id, "junk") {
        state.mail.move_messages(&ids, f.id);
    }
    Ok(addr)
}

fn attachment(state: &AppState, msg_id: i64, index: usize) -> R<parse::Attachment> {
    let raw = state.raw_of(msg_id).ok_or_else(|| t!("error.not_found"))?;
    parse::parse(&raw).attachments.into_iter().find(|a| a.index == index).ok_or_else(|| t!("error.not_found"))
}

#[tauri::command]
pub async fn save_attachment(state: State<'_, AppState>, msg_id: i64, index: usize, path: String) -> R<()> {
    guarded(move || {
    let a = attachment(&state, msg_id, index)?;
    write_file_timeout(path, a.data, 30)
    })
}

#[tauri::command(async)]
pub fn save_all_attachments(state: State<AppState>, msg_id: i64, dir: String) -> R<usize> {
    let raw = state.raw_of(msg_id).ok_or_else(|| t!("error.not_found"))?;
    let p = parse::parse(&raw);
    let atts = p.real_attachments();
    for a in &atts {
        let mut target = Path::new(&dir).join(util::safe_filename(&a.name));
        let mut n = 1;
        while target.exists() {
            let stem = Path::new(&a.name).file_stem().and_then(|s| s.to_str()).unwrap_or("ek").to_string();
            let ext = Path::new(&a.name).extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            target = Path::new(&dir).join(util::safe_filename(&format!("{stem} ({n}){ext}")));
            n += 1;
        }
        write_file_timeout(target, &a.data, 30)?;
    }
    Ok(atts.len())
}

/// Eki geçici klasöre yazar ve yolunu döndürür (arayüz varsayılan uygulamayla açar).
#[tauri::command]
pub async fn attachment_temp_path(state: State<'_, AppState>, msg_id: i64, index: usize) -> R<String> {
    guarded(move || {
    let a = attachment(&state, msg_id, index)?;
    let dir = util::temp_dir().join(format!("{}-{index}", msg_id.abs()));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(util::safe_filename(&a.name));
    std::fs::write(&path, a.data).map_err(err)?;
    Ok(path.to_string_lossy().to_string())
    })
}

/// .eml / .msg ekini ayrı bir ileti olarak açar.
#[tauri::command]
pub async fn open_attachment_as_message(state: State<'_, AppState>, msg_id: i64, index: usize) -> R<i64> {
    guarded(move || {
    let a = attachment(&state, msg_id, index)?;
    let raw = if msgfile::is_msg(&a.data) { msgfile::MsgFile::parse(&a.data)?.to_eml() } else { a.data };
    Ok(state.add_external(raw))
    })
}

#[tauri::command]
pub async fn import_attachment_calendar(state: State<'_, AppState>, msg_id: i64) -> R<usize> {
    guarded(move || {
    let raw = state.raw_of(msg_id).ok_or("?")?;
    let p = parse::parse(&raw);
    let mut n = 0;
    for a in p.attachments.iter().filter(|a| a.mime == "text/calendar" || a.name.to_lowercase().ends_with(".ics")) {
        n += import_ics_text(&state.db, &String::from_utf8_lossy(&a.data));
    }
    Ok(n)
    })
}

#[tauri::command]
pub async fn import_attachment_vcard(state: State<'_, AppState>, msg_id: i64) -> R<usize> {
    guarded(move || {
    let raw = state.raw_of(msg_id).ok_or("?")?;
    let p = parse::parse(&raw);
    let mut n = 0;
    for a in p.attachments.iter().filter(|a| a.mime.contains("vcard") || a.name.to_lowercase().ends_with(".vcf")) {
        for c in ical::parse_vcf(&String::from_utf8_lossy(&a.data)) {
            if state.db.save_contact(&c).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
    })
}

/// Dosyaya yazma: ağ diski/GVFS bağlantısı takılırsa uygulama beklemede kalmasın, süre
/// dolunca hata verilir (yazma arka planda biter ya da düşer)
pub fn write_file_timeout(path: impl AsRef<std::path::Path>, data: impl AsRef<[u8]>, secs: u64) -> R<()> {
    let (path, data) = (path.as_ref().to_path_buf(), data.as_ref().to_vec());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(std::fs::write(&path, data).map_err(|e| e.to_string()));
    });
    match rx.recv_timeout(std::time::Duration::from_secs(secs)) {
        Ok(r) => r,
        Err(_) => Err(t!("error.write_timeout")),
    }
}

/// (ayrı iş parçacığında çalışır: ana iş parçacığını hiç bekletmez)
#[tauri::command(async)]
pub fn save_message_as(state: State<AppState>, id: i64, path: String) -> R<()> {
    guarded(move || {
    let raw = state.raw_of(id).ok_or_else(|| t!("error.not_found"))?;
    let lower = path.to_lowercase();
    let p = parse::parse(&raw);
    let data: Vec<u8> = if lower.ends_with(".msg") {
        msgfile::eml_to_msg(&raw).map_err(err)?
    } else if lower.ends_with(".txt") {
        format!(
            "{}: {}\n{}: {}\n{}: {}\n{}: {}\n\n{}",
            t!("mail.from"), p.sender, t!("mail.to"), p.to, t!("mail.date"), util::fmt_local(p.date), t!("mail.subject"), p.subject, p.text
        )
        .into_bytes()
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        let body = if p.html.is_empty() { mail::html::text_to_html(&p.text) } else { p.html_with_inline_images() };
        format!("<!doctype html><meta charset=\"utf-8\"><title>{}</title>{body}", util::html_escape(&p.subject)).into_bytes()
    } else {
        raw
    };
    write_file_timeout(path, data, 30)
    })
}

#[tauri::command(async)]
pub fn import_external_message(state: State<AppState>, id: i64, folder_id: Option<i64>) -> R<i64> {
    let raw = state.raw_of(id).ok_or("?")?;
    let fid = match folder_id {
        Some(f) => f,
        None => state.db.folder_by_role(None, "inbox").ok_or("?")?.id,
    };
    state.mail.import_raw(fid, raw, true).ok_or_else(|| "?".into())
}

#[tauri::command(async)]
pub fn send_read_receipt(state: State<AppState>, id: i64) -> R<()> {
    let raw = state.raw_of(id).ok_or("?")?;
    let p = parse::parse(&raw);
    if p.read_receipt_to.is_empty() {
        return Ok(());
    }
    let acc_id = state.db.message_row(id).and_then(|m| m.account_id).or_else(|| state.db.accounts().first().map(|a| a.id)).ok_or_else(|| t!("error.no_account"))?;
    let acc = state.db.account(acc_id).ok_or_else(|| t!("error.no_account"))?;
    let body = t!("mail.receipt_body", p.subject, util::fmt_local(now()));
    let raw = mail::build::simple_raw(&acc.from_header(), &p.read_receipt_to, &t!("mail.receipt_subject", p.subject), &body);
    state.mail.send(acc.id, raw, None);
    state.mail.set_answered(&[id]);
    Ok(())
}

// ======================================================================= yazma
#[derive(Serialize, Default)]
pub struct ComposeData {
    account_id: Option<i64>,
    to: String,
    cc: String,
    bcc: String,
    subject: String,
    html: String,
    in_reply_to: String,
    references: String,
    attachments: Vec<Value>,
    draft_id: Option<i64>,
    importance: i64,
    /// Taslakta seçili "Kimden" adresi (takma ad)
    from_addr: String,
    /// Yanıtlanan iletinin alıcıları: hangi takma ada geldiği buradan anlaşılır
    recipients: String,
}

fn quote_header(p: &parse::ParsedMail) -> String {
    let mut h = format!(
        "<b>{}:</b> {}<br><b>{}:</b> {}<br><b>{}:</b> {}<br>",
        util::html_escape(&t!("mail.from")),
        util::html_escape(&p.sender),
        util::html_escape(&t!("mail.sent")),
        util::html_escape(&util::fmt_local(p.date)),
        util::html_escape(&t!("mail.to")),
        util::html_escape(&p.to)
    );
    if !p.cc.is_empty() {
        h.push_str(&format!("<b>{}:</b> {}<br>", util::html_escape(&t!("mail.cc")), util::html_escape(&p.cc)));
    }
    h.push_str(&format!("<b>{}:</b> {}", util::html_escape(&t!("mail.subject")), util::html_escape(&p.subject)));
    h
}

fn body_html_of(p: &parse::ParsedMail) -> String {
    if p.html.is_empty() {
        mail::html::text_to_html(&p.text)
    } else {
        // Alıntıdaki uzak resimler (izleme pikselleri) yazma penceresinde yüklenmesin
        let remote_img = regex::Regex::new(r#"(?is)<img\b[^>]*\bsrc\s*=\s*["']?\s*(https?:)?//[^>]*>"#).unwrap();
        let h = mail::html::sanitize(&remote_img.replace_all(&p.html_with_inline_images(), ""));
        // Yalnızca <body> içeriğini al. ASCII küçültme bayt konumlarını korur
        // (to_lowercase "İ" gibi harflerde uzunluğu değiştirir ve dilimleme bozulur).
        let lower = h.to_ascii_lowercase();
        match (lower.find("<body"), lower.rfind("</body>")) {
            (Some(s), Some(e)) => {
                let start = h[s..].find('>').map(|x| s + x + 1).unwrap_or(s);
                h[start..e].to_string()
            }
            _ => h,
        }
    }
}

#[tauri::command]
pub async fn compose_prefill(state: State<'_, AppState>, mode: String, source_id: i64) -> R<ComposeData> {
    let raw = state.raw_of_async(source_id).await;
    guarded(move || {
    let raw = raw.ok_or_else(|| t!("error.not_found"))?;
    let p = parse::parse(&raw);
    let row = if source_id > 0 { state.db.message(source_id) } else { None };
    let mut d = ComposeData { importance: 1, account_id: row.as_ref().and_then(|m| m.account_id), ..Default::default() };
    match mode.as_str() {
        "draft" | "edit" => {
            d.to = p.to.clone();
            d.cc = p.cc.clone();
            d.bcc = p.bcc.clone();
            d.subject = p.subject.clone();
            d.html = body_html_of(&p);
            d.in_reply_to = p.in_reply_to.clone();
            d.references = p.references.clone();
            d.importance = p.importance;
            d.from_addr = mail::split_name_addr(&p.sender).1;
            d.attachments = p.real_attachments().iter().map(|a| json!({"name": a.name, "mime": a.mime, "size": a.size, "source_msg": source_id, "source_index": a.index})).collect();
            if mode == "draft" {
                d.draft_id = Some(source_id);
                if let Some(m) = &row {
                    if let Some(a) = m.pending.strip_prefix("draft:").and_then(|s| s.parse().ok()) {
                        d.account_id = Some(a);
                    }
                }
            }
        }
        "forward" => {
            d.subject = mail::reply_subject(&p.subject, &t!("compose.fw_prefix"));
            d.html = format!(
                "<div><br></div><div><br></div><div class=\"lynora-quote\"><div style=\"border:none;border-top:solid #E1E1E1 1.0pt;padding:3.0pt 0 0 0\">{}</div><div><br></div>{}</div>",
                quote_header(&p),
                body_html_of(&p)
            );
            d.attachments = p.real_attachments().iter().map(|a| json!({"name": a.name, "mime": a.mime, "size": a.size, "source_msg": source_id, "source_index": a.index})).collect();
        }
        _ => {
            d.subject = mail::reply_subject(&p.subject, &t!("compose.re_prefix"));
            d.recipients = format!("{}, {}", p.to, p.cc);
            // Bozuk Reply-To (geçerli adres içermiyor) yok sayılır, gönderene yanıt verilir
            let reply_to = mail::split_addresses(&p.reply_to);
            d.to = if reply_to.is_empty() { mail::split_addresses(&p.sender).join(", ") } else { reply_to.join(", ") };
            if d.to.is_empty() {
                d.to = p.sender.clone();
            }
            if mode == "reply_all" {
                // Outlook gibi: Kime = gönderen + özgün Kime alıcıları, Bilgi = özgün Bilgi alıcıları
                // (kendi adreslerimiz ve yinelenenler çıkarılır)
                let me = state.db.my_addresses();
                let mut seen: std::collections::HashSet<String> = mail::bare_addresses(&d.to).into_iter().map(|a| a.to_lowercase()).collect();
                let mut keep = |list: &str| -> Vec<String> {
                    mail::split_addresses(list)
                        .into_iter()
                        .filter(|a| {
                            let e = mail::split_name_addr(a).1.to_lowercase();
                            let key = if e.is_empty() { a.trim().to_lowercase() } else { e.clone() };
                            !key.is_empty() && !me.contains(&e) && seen.insert(key)
                        })
                        .collect()
                };
                let to_rest = keep(&p.to);
                let cc_rest = keep(&p.cc);
                // Kendi gönderdiğimiz iletiye "tümünü yanıtla": gönderen biziz, Kime özgün alıcılar
                let sender_is_me = mail::bare_addresses(&d.to).iter().all(|a| me.contains(&a.to_lowercase()));
                if sender_is_me && !to_rest.is_empty() {
                    d.to = to_rest.join(", ");
                } else if !to_rest.is_empty() {
                    d.to = format!("{}, {}", d.to, to_rest.join(", "));
                }
                d.cc = cc_rest.join(", ");
            }
            d.in_reply_to = p.message_id.clone();
            d.references = [p.references.as_str(), p.message_id.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
            d.html = format!(
                "<div><br></div><div><br></div><div class=\"lynora-quote\"><div style=\"border:none;border-top:solid #E1E1E1 1.0pt;padding:3.0pt 0 0 0\">{}</div><div><br></div>{}</div>",
                quote_header(&p),
                body_html_of(&p)
            );
        }
    }
    Ok(d)
    })
}

fn resolve_attachments(state: &AppState, m: &ComposedMail) -> R<Vec<(String, String, Vec<u8>)>> {
    let mut out = Vec::new();
    for a in &m.attachments {
        if let Some(path) = &a.path {
            let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            let name = if a.name.is_empty() { Path::new(path).file_name().and_then(|s| s.to_str()).unwrap_or("ek").to_string() } else { a.name.clone() };
            let mime = if a.mime.is_empty() { guess_mime(&name) } else { a.mime.clone() };
            out.push((name, mime, data));
        } else if let (Some(src), Some(idx)) = (a.source_msg, a.source_index) {
            let at = attachment(state, src, idx)?;
            out.push((at.name, at.mime, at.data));
        }
    }
    Ok(out)
}

fn remember_recipients(db: &Db, m: &ComposedMail) {
    for a in mail::split_addresses(&format!("{}, {}, {}", m.to, m.cc, m.bcc)) {
        let (name, e) = mail::split_name_addr(&a);
        if !e.is_empty() {
            db.remember_recipient(&name, &e.to_lowercase());
        }
    }
}

/// Exchange iç adresleri (EX / X500 DN, IMCEAEX) Outlook gibi kurum dizininden SMTP
/// adresine çözülür; çözülemeyen alıcı için gönderim durdurulur.
fn resolve_exchange_recipients(db: &Db, list: &str) -> R<String> {
    let entries = mail::split_addresses(list);
    if !entries.iter().any(|a| mail::address::is_exchange_dn(&mail::split_name_addr(a).1)) {
        return Ok(list.to_string());
    }
    let accounts: Vec<Account> = db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed").collect();
    let mut out = Vec::new();
    for entry in entries {
        let (name, dn) = mail::split_name_addr(&entry);
        if !mail::address::is_exchange_dn(&dn) {
            out.push(entry);
            continue;
        }
        let alias = mail::address::dn_alias(&dn);
        let mut found: Option<(String, String)> = None;
        for acc in &accounts {
            // Önce DN'nin kendisi, olmazsa takma ad (tek ve kesin eşleşme)
            if let Ok((rows, _)) = mail::ews::pooled(acc, |e| e.resolve_names_dir(&dn)) {
                if let Some(r) = rows.into_iter().find(|r| r.email.contains('@')) {
                    found = Some((r.name, r.email));
                    break;
                }
            }
            if !alias.is_empty() {
                if let Ok((rows, _)) = mail::ews::pooled(acc, |e| e.resolve_names_dir(&alias)) {
                    let exact = rows.iter().find(|r| r.email.split('@').next().is_some_and(|l| l.eq_ignore_ascii_case(&alias)));
                    if let Some(r) = exact.or(if rows.len() == 1 { rows.first() } else { None }) {
                        found = Some((r.name.clone(), r.email.clone()));
                        break;
                    }
                }
            }
        }
        // Sunucuya ulaşılamazsa çevrimdışı adres defteri (takma ad = adresin yerel kısmı)
        if found.is_none() && !alias.is_empty() {
            found = db
                .directory_search(None, &alias, 0, 20)
                .into_iter()
                .find(|r| r.email.split('@').next().is_some_and(|l| l.eq_ignore_ascii_case(&alias)))
                .map(|r| (r.name, r.email));
        }
        match found {
            Some((n, e)) => {
                let n = if name.is_empty() { n } else { name };
                out.push(mail::address::clean_address(&if n.is_empty() { e } else { format!("\"{}\" <{e}>", n.replace('"', "'")) }));
            }
            None => return Err(t!("error.unresolved_recipient", if name.is_empty() { alias } else { name })),
        }
    }
    Ok(out.join(", "))
}

#[tauri::command(async)]
pub fn send_mail(state: State<AppState>, mut mail: ComposedMail) -> R<i64> {
    let account_id = mail.account_id.ok_or_else(|| t!("error.no_account"))?;
    let acc = state.db.account(account_id).ok_or_else(|| t!("error.no_account"))?;
    mail.to = resolve_exchange_recipients(&state.db, &mail.to)?;
    mail.cc = resolve_exchange_recipients(&state.db, &mail.cc)?;
    mail.bcc = resolve_exchange_recipients(&state.db, &mail.bcc)?;
    if mail::bare_addresses(&format!("{}, {}, {}", mail.to, mail.cc, mail.bcc)).is_empty() {
        return Err(t!("error.no_recipients"));
    }
    let atts = resolve_attachments(&state, &mail)?;
    let raw = build_raw(Some(&acc), &mail, &atts, true);
    let id = state.mail.send(account_id, raw, mail.send_at).ok_or("?")?;
    remember_recipients(&state.db, &mail);
    if let Some(d) = mail.draft_id {
        state.mail.discard_draft(d);
    }
    Ok(id)
}

#[tauri::command(async)]
pub fn save_draft(state: State<AppState>, mail: ComposedMail) -> R<i64> {
    let acc = mail.account_id.and_then(|a| state.db.account(a));
    let atts = resolve_attachments(&state, &mail)?;
    let raw = build_raw(acc.as_ref(), &mail, &atts, true);
    state.mail.save_draft(mail.account_id, raw, mail.draft_id).ok_or_else(|| "?".into())
}

#[tauri::command(async)]
pub fn discard_draft(state: State<AppState>, id: i64) {
    state.mail.discard_draft(id);
}

#[tauri::command(async)]
pub fn process_outbox(state: State<AppState>) {
    state.mail.process_outbox();
}

/// "Gönderimi geri al": henüz gönderilmemiş iletiyi giden kutusundan alıp taslağa çevirir.
#[tauri::command(async)]
pub fn cancel_outbox(state: State<AppState>, id: i64) -> R<i64> {
    let m = state.db.message(id).ok_or_else(|| t!("compose.undo_too_late"))?;
    let p: Vec<&str> = m.pending.split(':').collect();
    if p.len() != 3 || p[0] != "send" {
        return Err(t!("compose.undo_too_late"));
    }
    // Önce beklemeye al ki zamanlayıcı göndermesin
    state.db.set_message_field(id, "pending", "hold");
    let account_id = p[2].parse::<i64>().ok();
    let draft = state.mail.save_draft(account_id, m.raw.clone(), None).ok_or("?")?;
    state.db.delete_message(id);
    Ok(draft)
}

#[tauri::command(async)]
pub fn retry_outbox(state: State<AppState>, id: i64) {
    state.mail.retry_outbox_item(id);
}

#[tauri::command(async)]
pub fn address_book(state: State<AppState>) -> Vec<(String, String)> {
    state.db.address_book()
}

// ======================================================================= yedekleme
#[tauri::command(async)]
pub fn create_backup(state: State<AppState>, path: String, password: String) -> R<Value> {
    crate::backup::create(&state.db, Path::new(&path), &password, env!("CARGO_PKG_VERSION"))
}

#[tauri::command(async)]
pub fn backup_is_encrypted(path: String) -> R<bool> {
    crate::backup::is_encrypted(Path::new(&path))
}

#[tauri::command(async)]
pub fn prepare_restore(path: String, password: String) -> R<Value> {
    crate::backup::prepare(Path::new(&path), &password)
}

#[tauri::command(async)]
pub fn cancel_restore() {
    crate::backup::cancel();
}

/// Geri yüklemeyi onaylar ve uygulamayı yeniden başlatır: veritabanı bağlantısı yokken yerine
/// konur. Yeni süreç kısa bekler ki çalışan örnek kapanmış olsun (ikinci örnek olarak açılmasın).
#[tauri::command(async)]
pub fn finish_restore(app: tauri::AppHandle) -> R<()> {
    crate::backup::confirm()?;
    let exe = std::env::current_exe().map_err(err)?;
    std::process::Command::new("sh").arg("-c").arg("sleep 1; exec \"$0\"").arg(exe).spawn().map_err(err)?;
    app.exit(0);
    Ok(())
}

// ======================================================================= çökme kurtarma
fn recovery_dir() -> std::path::PathBuf {
    crate::db::default_data_dir().join("recovery")
}

/// Kimlik pencereden gelir; dosya adı dışına çıkılmasın diye yalnızca güvenli karakterler
fn recovery_file(id: &str) -> Option<std::path::PathBuf> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| recovery_dir().join(format!("compose-{id}.json")))
}

/// Yazılmakta olan iletinin yerel kopyası: yarım yazım kalmasın diye önce geçici dosyaya
#[tauri::command(async)]
pub fn save_recovery(id: String, payload: String) -> R<()> {
    let path = recovery_file(&id).ok_or("?")?;
    std::fs::create_dir_all(recovery_dir()).map_err(err)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, payload).map_err(err)?;
    std::fs::rename(&tmp, &path).map_err(err)
}

#[tauri::command(async)]
pub fn list_recovery() -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(recovery_dir()) else { return Vec::new() };
    dir.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .collect()
}

#[tauri::command(async)]
pub fn remove_recovery(id: String) {
    if let Some(path) = recovery_file(&id) {
        let _ = std::fs::remove_file(path);
    }
}

// ======================================================================= dosya açma / içe-dışa aktarma
fn import_ics_text(db: &Db, text: &str) -> usize {
    let imp = ical::parse_ics(text);
    let mut n = 0;
    for e in imp.events {
        let existing = db.event_by_uid(&e.uid);
        let mut e = e;
        if let Some(x) = existing {
            e.id = x.id;
            e.calendar_id = x.calendar_id;
        }
        if db.save_event(&e).is_ok() {
            n += 1;
        }
    }
    for t in imp.tasks {
        if db.save_task(&t).is_ok() {
            n += 1;
        }
    }
    n
}

/// Bir dosyayı türüne göre açar: ileti görüntüleme, takvime/kişilere/görevlere ekleme.
#[tauri::command(async)]
pub fn open_file(state: State<AppState>, path: String) -> R<Value> {
    guarded(move || {
    let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let lower = path.to_lowercase();
    if msgfile::is_msg(&data) {
        let m = msgfile::MsgFile::parse(&data)?;
        match m.kind() {
            "contact" => {
                if let Some(c) = m.to_contact() {
                    let id = state.db.save_contact(&c).map_err(err)?;
                    return Ok(json!({"kind": "contact", "id": id}));
                }
            }
            "task" => {
                if let Some(t) = m.to_task() {
                    let id = state.db.save_task(&t).map_err(err)?;
                    return Ok(json!({"kind": "task", "id": id}));
                }
            }
            _ => {}
        }
        let id = state.add_external(m.to_eml());
        return Ok(json!({"kind": "message", "id": id, "appointment": m.kind() == "appointment"}));
    }
    if lower.ends_with(".ics") || lower.ends_with(".ical") || lower.ends_with(".vcs") {
        let n = import_ics_text(&state.db, &String::from_utf8_lossy(&data));
        return Ok(json!({"kind": "events", "count": n}));
    }
    if lower.ends_with(".vcf") || lower.ends_with(".vcard") {
        let mut n = 0;
        for c in ical::parse_vcf(&String::from_utf8_lossy(&data)) {
            if state.db.save_contact(&c).is_ok() {
                n += 1;
            }
        }
        return Ok(json!({"kind": "contacts", "count": n}));
    }
    if lower.ends_with(".mbox") || data.starts_with(b"From ") {
        let inbox = state.db.folder_by_role(None, "inbox").ok_or("?")?;
        let n = import_mbox_data(&state, inbox.id, &data);
        return Ok(json!({"kind": "imported", "count": n}));
    }
    // Varsayılan: .eml
    let id = state.add_external(data);
    Ok(json!({"kind": "message", "id": id}))
    })
}

fn import_mbox_data(state: &AppState, folder_id: i64, data: &[u8]) -> usize {
    let mut n = 0;
    let mut cur: Vec<u8> = Vec::new();
    let mut flush = |cur: &mut Vec<u8>| {
        if !cur.is_empty() {
            // ">From " kaçışlarını geri al
            let text = cur.split(|&b| b == b'\n').map(|l| if l.starts_with(b">From ") { &l[1..] } else { l }).collect::<Vec<_>>().join(&b'\n');
            if state.mail.import_raw(folder_id, text, true).is_some() {
                n += 1;
            }
            cur.clear();
        }
    };
    for line in data.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(b"From ") {
            flush(&mut cur);
            continue;
        }
        cur.extend_from_slice(line);
    }
    flush(&mut cur);
    n
}

#[tauri::command(async)]
pub fn import_mail_files(state: State<AppState>, folder_id: i64, paths: Vec<String>) -> R<usize> {
    let mut n = 0;
    for p in paths {
        let data = std::fs::read(&p).map_err(|e| format!("{p}: {e}"))?;
        if msgfile::is_msg(&data) {
            let raw = msgfile::MsgFile::parse(&data)?.to_eml();
            if state.mail.import_raw(folder_id, raw, true).is_some() {
                n += 1;
            }
        } else if p.to_lowercase().ends_with(".mbox") || data.starts_with(b"From ") {
            n += import_mbox_data(&state, folder_id, &data);
        } else if state.mail.import_raw(folder_id, data, true).is_some() {
            n += 1;
        }
    }
    Ok(n)
}

#[tauri::command(async)]
pub fn export_mbox(state: State<AppState>, folder_id: i64, path: String) -> R<usize> {
    let mut out: Vec<u8> = Vec::new();
    let ids = state.db.message_ids_in_folder(folder_id, false);
    for id in &ids {
        if let Some(m) = state.db.message(*id) {
            let sender = mail::split_name_addr(&m.sender).1;
            let date = chrono::DateTime::from_timestamp(m.date as i64, 0).map(|d| d.format("%a %b %e %H:%M:%S %Y").to_string()).unwrap_or_default();
            out.extend_from_slice(format!("From {} {}\n", if sender.is_empty() { "MAILER-DAEMON" } else { &sender }, date).as_bytes());
            let raw = if m.raw.is_empty() { crate::ews_sync::ensure_content(&state.db, *id).unwrap_or_default() } else { m.raw.clone() };
            for line in raw.split_inclusive(|&b| b == b'\n') {
                let l = line.strip_suffix(b"\r\n").map(|x| [x, b"\n"].concat()).unwrap_or_else(|| line.to_vec());
                if l.starts_with(b"From ") {
                    out.push(b'>');
                }
                out.extend_from_slice(&l);
            }
            out.extend_from_slice(b"\n");
        }
    }
    write_file_timeout(path, out, 30)?;
    Ok(ids.len())
}

/// Outlook .pst dosyasını `readpst` (pst-utils) yardımıyla içe aktarır.
#[tauri::command]
pub async fn import_pst(state: State<'_, AppState>, path: String) -> R<Value> {
    let tmp = util::temp_dir().join(format!("pst-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).map_err(err)?;
    let tmp2 = tmp.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new("readpst").args(["-e", "-q", "-o"]).arg(&tmp2).arg(&path).output()
    })
    .await
    .map_err(err)?;
    match out {
        Err(_) => return Err(t!("import.pst_missing")),
        Ok(o) if !o.status.success() => return Err(String::from_utf8_lossy(&o.stderr).to_string()),
        _ => {}
    }
    // Her PST klasörü için yerel bir klasör oluştur
    let (mut messages, mut contacts, mut events) = (0usize, 0usize, 0usize);
    let root_parent = state
        .db
        .insert_folder(&Folder { name: t!("import.pst_root"), position: 200, ..Default::default() })
        .map_err(err)?;
    let mut folder_ids: HashMap<std::path::PathBuf, i64> = HashMap::new();
    let mut stack = vec![tmp.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.path());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                let parent = folder_ids.get(&dir).copied().unwrap_or(root_parent);
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
                let fid = state.db.insert_folder(&Folder { name, parent_id: Some(parent), position: 200, ..Default::default() }).map_err(err)?;
                folder_ids.insert(p.clone(), fid);
                stack.push(p);
            } else {
                let data = std::fs::read(&p).unwrap_or_default();
                let lower = p.to_string_lossy().to_lowercase();
                if lower.ends_with(".vcf") {
                    for c in ical::parse_vcf(&String::from_utf8_lossy(&data)) {
                        if state.db.save_contact(&c).is_ok() {
                            contacts += 1;
                        }
                    }
                } else if lower.ends_with(".ics") {
                    events += import_ics_text(&state.db, &String::from_utf8_lossy(&data));
                } else {
                    let fid = folder_ids.get(&dir).copied().unwrap_or(root_parent);
                    let mut m = parse::parse(&data).into_message();
                    m.folder_id = fid;
                    m.is_read = true;
                    if state.db.insert_message(&m).is_ok() {
                        messages += 1;
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(json!({"messages": messages, "contacts": contacts, "events": events}))
}

// ======================================================================= kişiler
#[tauri::command(async)]
pub fn list_contacts(state: State<AppState>, search: String) -> Vec<Contact> {
    state.db.contacts(search.trim())
}

#[tauri::command(async)]
pub fn get_contact(state: State<AppState>, id: i64) -> Option<Contact> {
    state.db.contact(id)
}

#[tauri::command(async)]
pub fn contact_by_email(state: State<AppState>, email: String) -> Option<Contact> {
    state.db.contact_by_email(&email)
}

#[tauri::command(async)]
pub fn save_contact(state: State<AppState>, contact: Contact) -> R<i64> {
    state.db.save_contact(&contact).map_err(err)
}

#[tauri::command(async)]
pub fn delete_contacts(state: State<AppState>, ids: Vec<i64>) {
    for id in ids {
        state.db.delete_contact(id);
    }
}

/// Yeni görev, not ve grupların eşitleneceği Exchange hesabı (kullanıcının kendi kutusu)
fn pim_account(db: &Db) -> Option<i64> {
    db.accounts().into_iter().find(|a| a.is_exchange() && a.shared_of.is_none() && a.auth_state != "failed").map(|a| a.id)
}

#[tauri::command(async)]
pub fn list_groups(state: State<AppState>) -> Vec<ContactGroup> {
    state.db.groups()
}

#[tauri::command(async)]
pub fn save_group(state: State<AppState>, group: ContactGroup) -> R<i64> {
    let mut group = group;
    match state.db.group(group.id) {
        Some(old) => {
            group.account_id = old.account_id;
            group.remote_id = old.remote_id;
        }
        None => group.account_id = pim_account(&state.db),
    }
    group.dirty = group.account_id.is_some();
    let id = state.db.save_group(&group).map_err(err)?;
    if group.dirty {
        state.mail.push_group(id);
    }
    Ok(id)
}

#[tauri::command(async)]
pub fn delete_group(state: State<AppState>, id: i64) {
    if let Some(g) = state.db.group(id) {
        if let (Some(account), Some(rid)) = (g.account_id, g.remote_id) {
            state.db.add_tombstone("groups", account, &rid);
            state.mail.flush_deleted("groups", account);
        }
    }
    state.db.delete_group(id);
}

#[tauri::command(async)]
pub fn import_contacts(state: State<AppState>, path: String) -> R<usize> {
    let data = std::fs::read(&path).map_err(err)?;
    let list = if msgfile::is_msg(&data) {
        msgfile::MsgFile::parse(&data)?.to_contact().into_iter().collect()
    } else if path.to_lowercase().ends_with(".csv") {
        ical::parse_csv_contacts(&String::from_utf8_lossy(&data))
    } else {
        ical::parse_vcf(&String::from_utf8_lossy(&data))
    };
    let mut n = 0;
    for c in list {
        if state.db.save_contact(&c).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

#[tauri::command(async)]
pub fn export_contacts(state: State<AppState>, path: String, ids: Option<Vec<i64>>) -> R<usize> {
    let mut list = state.db.contacts("");
    if let Some(ids) = ids.filter(|v| !v.is_empty()) {
        list.retain(|c| ids.contains(&c.id));
    }
    let text = if path.to_lowercase().ends_with(".csv") { ical::export_csv_contacts(&list) } else { ical::export_vcf(&list) };
    write_file_timeout(path, text, 30)?;
    Ok(list.len())
}

// ======================================================================= takvim
#[tauri::command(async)]
pub fn list_calendars(state: State<AppState>) -> Vec<Calendar> {
    let list = state.db.calendars();
    // Outlook "Doğum günleri" takvimi (kişilerin doğum günlerinden; varsayılan gizli)
    if !list.iter().any(|c| c.kind == "birthdays") {
        let _ = state.db.save_calendar(&Calendar { name: t!("calendar.birthdays"), color: "#8e44ad".into(), visible: false, kind: "birthdays".into(), ..Default::default() });
        return state.db.calendars();
    }
    list
}

/// Kişilerin doğum günleri (tüm gün, her yıl) — "Doğum günleri" takvimi
fn birthday_occurrences(db: &Db, cal: &Calendar, start: f64, end: f64) -> Vec<recurrence::Occurrence> {
    use chrono::{Datelike, NaiveDate, TimeZone};
    let mut out = Vec::new();
    let (Some(s), Some(e)) = (chrono::DateTime::from_timestamp(start as i64, 0), chrono::DateTime::from_timestamp(end as i64, 0)) else { return out };
    for c in db.contacts("") {
        let b = c.birthday.trim();
        if b.is_empty() {
            continue;
        }
        let parts: Vec<u32> = b.split(|ch: char| !ch.is_ascii_digit()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
        // "YYYY-MM-DD" ya da "GG.AA.YYYY"
        let (m, d) = match parts.as_slice() {
            [y, m, d, ..] if *y > 31 => (*m, *d),
            [d, m, ..] => (*m, *d),
            _ => continue,
        };
        for y in s.year() - 1..=e.year() + 1 {
            let Some(day) = NaiveDate::from_ymd_opt(y, m, d) else { continue };
            let Some(t0) = chrono::Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap()).single() else { continue };
            let t0 = t0.timestamp() as f64;
            if t0 + 86400.0 <= start || t0 >= end {
                continue;
            }
            let name = if c.display_name.is_empty() { format!("{} {}", c.first_name, c.last_name).trim().to_string() } else { c.display_name.clone() };
            let ev = Event {
                id: -(1_000_000 + c.id),
                calendar_id: cal.id,
                title: t!("calendar.birthday_of", name),
                start: t0,
                end: t0 + 86400.0,
                all_day: true,
                show_as: "free".into(),
                reminder_minutes: -1,
                recurrence: "yearly".into(),
                ..Event::new_default()
            };
            out.push(recurrence::Occurrence { event: ev, start: t0, end: t0 + 86400.0 });
        }
    }
    out
}

/// Başkasının (ekip arkadaşı, oda, paylaşılan) takvimindeki öğeler — salt okunur, kaydedilmez.
/// Klasör okuma izni varsa ayrıntılı (konu, konum, düzenleyen); yoksa Outlook gibi
/// meşguliyet bilgisi (izin veriliyorsa konu/konum ile).
#[tauri::command]
pub async fn remote_occurrences(state: State<'_, AppState>, calendar_id: i64, start: f64, end: f64) -> R<Vec<recurrence::Occurrence>> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cal = db.calendars().into_iter().find(|c| c.id == calendar_id).ok_or_else(|| t!("error.not_found"))?;
        if cal.owner.is_empty() {
            return Ok(vec![]);
        }
        let acc = db.accounts().into_iter().find(|a| a.is_exchange() && a.auth_state != "failed").ok_or_else(|| t!("error.no_account"))?;
        let owner = cal.owner.clone();
        let events = match mail::ews::pooled(&acc, |e| e.calendar_view_of(Some(&owner), start, end)) {
            Ok(list) => list,
            Err(_) => mail::ews::pooled(&acc, |e| e.availability_events(&owner, start, end)).map_err(|e| e.to_string())?,
        };
        Ok(events
            .into_iter()
            .enumerate()
            .map(|(i, mut ev)| {
                ev.id = -(calendar_id * 100_000 + i as i64 + 1);
                ev.calendar_id = calendar_id;
                if ev.title.is_empty() {
                    ev.title = t!(&format!("showas.{}", ev.show_as));
                }
                let (s, e) = (ev.start, ev.end);
                recurrence::Occurrence { event: ev, start: s, end: e }
            })
            .collect())
    })
    .await
    .map_err(err)?
}

/// Outlook "Ekip" takvim grubu: yöneticiniz ve ona bağlı ekip arkadaşlarınız (siz yöneticiyseniz
/// size bağlı kişiler) dizinden otomatik bulunur; takvimleri bu grupta (varsayılan gizli) listelenir.
#[tauri::command]
pub async fn refresh_team_calendars(state: State<'_, AppState>) -> R<String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let acc = db.accounts().into_iter().find(|a| a.is_exchange() && a.auth_state != "failed").ok_or_else(|| t!("error.no_account"))?;
        let me = acc.email.to_lowercase();
        let (_, manager, my_reports) = mail::ews::pooled(&acc, |e| e.org_info(&acc.email)).map_err(|e| e.to_string())?;
        let (label, members): (String, Vec<(String, String)>) = if !my_reports.is_empty() {
            (t!("calendar.my_team"), my_reports)
        } else if let Some((mname, memail)) = manager {
            let (_, _, peers) = mail::ews::pooled(&acc, |e| e.org_info(&memail)).map_err(|e| e.to_string())?;
            let mut list = vec![(mname.clone(), memail.clone())];
            list.extend(peers.into_iter().filter(|(_, e)| e.to_lowercase() != me));
            (t!("calendar.team_of", mname), list)
        } else {
            (String::new(), vec![])
        };
        let existing: Vec<Calendar> = db.calendars().into_iter().filter(|c| c.grp == "team").collect();
        const COLORS: [&str; 8] = ["#d13438", "#498205", "#ca5010", "#8764b8", "#038387", "#c239b3", "#986f0b", "#0078d4"];
        for (i, (name, email)) in members.iter().enumerate() {
            if existing.iter().any(|c| c.owner.eq_ignore_ascii_case(email)) {
                continue;
            }
            let _ = db.save_calendar(&Calendar {
                name: if name.is_empty() { email.clone() } else { name.clone() },
                color: COLORS[i % COLORS.len()].into(),
                visible: false,
                owner: email.clone(),
                grp: "team".into(),
                kind: "remote".into(),
                ..Default::default()
            });
        }
        for c in existing {
            if !members.iter().any(|(_, e)| e.eq_ignore_ascii_case(&c.owner)) {
                db.delete_calendar(c.id);
            }
        }
        db.set_setting("team_group_name", &label);
        Ok(label)
    })
    .await
    .map_err(err)?
}

#[tauri::command(async)]
pub fn save_calendar(state: State<AppState>, calendar: Calendar) -> R<i64> {
    state.db.save_calendar(&calendar).map_err(err)
}

#[tauri::command(async)]
pub fn delete_calendar(state: State<AppState>, id: i64) -> R<()> {
    let cals = state.db.calendars();
    let target = cals.iter().find(|c| c.id == id);
    if target.map(|c| c.kind.is_empty()).unwrap_or(false) && cals.iter().filter(|c| c.kind.is_empty()).count() <= 1 {
        return Err(t!("calendar.last_calendar"));
    }
    state.db.delete_calendar(id);
    Ok(())
}

#[tauri::command(async)]
pub fn list_occurrences(state: State<AppState>, start: f64, end: f64) -> Vec<recurrence::Occurrence> {
    let cals = state.db.calendars();
    let hidden: Vec<i64> = cals.iter().filter(|c| !c.visible).map(|c| c.id).collect();
    let mut out = recurrence::expand(state.db.events_between(start, end), &hidden, start, end);
    for c in cals.iter().filter(|c| c.kind == "birthdays" && c.visible) {
        out.extend(birthday_occurrences(&state.db, c, start, end));
    }
    out
}

#[tauri::command(async)]
pub fn search_events(state: State<AppState>, query: String) -> Vec<Event> {
    state.db.search_events(query.trim())
}

#[tauri::command(async)]
pub fn get_event(state: State<AppState>, id: i64) -> Option<Event> {
    state.db.event(id)
}

fn calendar_account(db: &Db, calendar_id: i64) -> Option<i64> {
    db.calendars().into_iter().find(|c| c.id == calendar_id).and_then(|c| c.account_id)
}

#[tauri::command(async)]
pub fn save_event(state: State<AppState>, event: Event) -> R<i64> {
    let mut e = event;
    if e.end <= e.start {
        e.end = e.start + if e.all_day { 86400.0 } else { 1800.0 };
    }
    // Takvim değiştirilirse sunucu kimliği geçersizdir
    if let Some(old) = (e.id > 0).then(|| state.db.event(e.id)).flatten() {
        if old.calendar_id != e.calendar_id {
            e.remote_id = None;
            e.master_id = None;
        }
    }
    let id = state.db.save_event(&e).map_err(err)?;
    if let Some(acc) = calendar_account(&state.db, e.calendar_id) {
        state.mail.push_event(acc, id);
    }
    Ok(id)
}

/// mode: "series" (tümü) veya "one" (yalnızca bu yineleme)
#[tauri::command(async)]
pub fn delete_event(state: State<AppState>, id: i64, mode: String, occurrence_start: Option<f64>) -> R<()> {
    if let Some(e) = state.db.event(id) {
        if let (Some(acc), Some(rid)) = (calendar_account(&state.db, e.calendar_id), e.remote_id.clone()) {
            let series = mode == "series" && e.master_id.is_some();
            state.db.delete_event(id);
            state.mail.delete_remote_event(acc, rid, series, !(e.attendees.is_empty() && e.optional_attendees.is_empty() && e.resources.is_empty()));
            return Ok(());
        }
    }
    if mode == "one" {
        if let (Some(mut e), Some(os)) = (state.db.event(id), occurrence_start) {
            if e.recurrence != "none" {
                let d = chrono::DateTime::from_timestamp(os as i64, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
                    .unwrap_or_default();
                let mut ex = split_list(&e.exdates);
                ex.push(d);
                e.exdates = ex.join(",");
                state.db.save_event(&e).map_err(err)?;
                return Ok(());
            }
        }
    }
    state.db.delete_event(id);
    Ok(())
}

/// Tek bir yinelemeyi seriden ayırıp ayrı bir etkinlik yapar.
#[tauri::command(async)]
pub fn detach_occurrence(state: State<AppState>, id: i64, occurrence_start: f64) -> R<i64> {
    let mut e = state.db.event(id).ok_or("?")?;
    let dur = e.end - e.start;
    delete_event(state.clone(), id, "one".into(), Some(occurrence_start))?;
    e.id = 0;
    e.uid = String::new();
    e.recurrence = "none".into();
    e.exdates = String::new();
    e.start = occurrence_start;
    e.end = occurrence_start + dur;
    state.db.save_event(&e).map_err(err)
}

#[tauri::command(async)]
pub fn import_ics(state: State<AppState>, path: String) -> R<usize> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    Ok(import_ics_text(&state.db, &text))
}

#[tauri::command(async)]
pub fn export_ics(state: State<AppState>, path: String) -> R<usize> {
    let events = state.db.all_events();
    write_file_timeout(path, ical::export_ics(&events, None, None), 30)?;
    Ok(events.len())
}

/// "Takvimi E-postayla Gönder": aralıktaki etkinlikler geçici .ics dosyasına yazılır (ek olarak)
#[tauri::command(async)]
pub fn calendar_ics_temp(state: State<AppState>, start: f64, end: f64, name: String) -> R<Value> {
    let events: Vec<Event> = state
        .db
        .all_events()
        .into_iter()
        .filter(|e| (e.start < end && e.end > start) || (e.recurrence != "none" && e.start < end && e.recur_until.map(|u| u >= start).unwrap_or(true)))
        .collect();
    let dir = util::temp_dir().join(format!("cal-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let file = format!("{}.ics", util::safe_filename(if name.trim().is_empty() { "takvim" } else { name.trim() }));
    let path = dir.join(&file);
    let data = ical::export_ics(&events, None, None);
    std::fs::write(&path, &data).map_err(err)?;
    Ok(json!({ "name": file, "mime": "text/calendar", "size": data.len(), "path": path.to_string_lossy() }))
}

#[tauri::command(async)]
pub fn export_event(state: State<AppState>, id: i64, path: String) -> R<()> {
    let e = state.db.event(id).ok_or("?")?;
    write_file_timeout(path, ical::export_ics(&[e], None, None), 30)
}

/// Katılımcılara toplantı daveti (iCalendar REQUEST) gönderir.
#[tauri::command(async)]
pub fn send_invitation(state: State<AppState>, event_id: i64, account_id: i64, cancel: bool) -> R<()> {
    let e = state.db.event(event_id).ok_or("?")?;
    let acc = state.db.account(account_id).ok_or_else(|| t!("error.no_account"))?;
    let attendees = split_list(&e.attendees);
    let optional = split_list(&e.optional_attendees);
    let rooms = split_list(&e.resources);
    if attendees.is_empty() && optional.is_empty() && rooms.is_empty() {
        return Err(t!("calendar.no_attendees"));
    }
    // Exchange hesabı: toplantı Exchange'in kendisiyle gönderilir (OWA/Outlook gibi gerçek
    // toplantı isteği; oda rezerve edilir, alıcıda "Kabul Et/Reddet" ile görünür). Ayrıca
    // .ics ekli e-posta gönderilmez — alıcıda "davet.ics" ekli düz ileti olarak görünüyordu.
    if acc.is_exchange() {
        let cal_acc = calendar_account(&state.db, e.calendar_id);
        if cal_acc.is_some() {
            // Exchange takviminde: kaydederken oluşturuldu/güncellendi, iptal de silinirken gider
            return Ok(());
        }
        if cancel {
            return Ok(());
        }
        // Yerel takvimdeki toplantı: hesabın Exchange takvimine taşınıp oradan gönderilir
        let cal = state
            .db
            .calendars()
            .into_iter()
            .find(|c| c.account_id == Some(acc.id) && c.kind.is_empty() && c.owner.is_empty())
            .ok_or_else(|| t!("error.no_account"))?;
        let mut moved = e.clone();
        moved.calendar_id = cal.id;
        moved.remote_id = None;
        moved.master_id = None;
        state.db.save_event(&moved).map_err(err)?;
        state.mail.push_event(acc.id, moved.id);
        return Ok(());
    }
    let method = if cancel { "CANCEL" } else { "REQUEST" };
    let ics = ical::export_ics(&[e.clone()], Some(method), Some(&acc.email));
    let when = util::fmt_local(e.start);
    let text = format!(
        "{}\n\n{}: {}\n{}: {}\n\n{}",
        e.title,
        t!("calendar.when"),
        when,
        t!("calendar.location"),
        e.location,
        e.description
    );
    let m = ComposedMail {
        account_id: Some(account_id),
        to: attendees.iter().chain(rooms.iter()).cloned().collect::<Vec<_>>().join(", "),
        cc: optional.join(", "),
        subject: if cancel { t!("calendar.cancelled_subject", e.title) } else { t!("calendar.invite_subject", e.title) },
        text: text.clone(),
        html: mail::html::text_to_html(&text),
        ics: Some(ics),
        importance: 1,
        ..Default::default()
    };
    let raw = build_raw(Some(&acc), &m, &[], true);
    state.mail.send(account_id, raw, None);
    Ok(())
}

// ======================================================================= görevler
#[tauri::command(async)]
pub fn list_tasks(state: State<AppState>) -> Vec<Task> {
    state.db.tasks()
}

#[tauri::command(async)]
pub fn get_task(state: State<AppState>, id: i64) -> Option<Task> {
    state.db.task(id)
}

#[tauri::command(async)]
pub fn save_task(state: State<AppState>, task: Task) -> R<i64> {
    let mut task = task;
    // Takip bayraklı iletilerin görevleri iletiye bağlıdır, sunucuya yazılmaz
    if task.message_id.is_none() {
        match state.db.task(task.id) {
            Some(old) => {
                task.account_id = old.account_id;
                task.remote_id = old.remote_id;
            }
            None => task.account_id = pim_account(&state.db),
        }
        task.dirty = task.account_id.is_some();
    }
    let id = state.db.save_task(&task).map_err(err)?;
    if task.dirty {
        state.mail.push_task(id);
    }
    if let Some(mid) = task.message_id {
        if task.status == "completed" {
            state.mail.set_flagged(&[mid], false);
        }
    }
    Ok(id)
}

#[tauri::command(async)]
pub fn delete_tasks(state: State<AppState>, ids: Vec<i64>) {
    for id in ids {
        if let Some(t) = state.db.task(id) {
            if let Some(mid) = t.message_id {
                state.mail.set_flagged(&[mid], false);
            }
            if let (Some(account), Some(rid)) = (t.account_id, t.remote_id) {
                state.db.add_tombstone("tasks", account, &rid);
                state.mail.flush_deleted("tasks", account);
            }
        }
        state.db.delete_task(id);
    }
}

#[tauri::command(async)]
pub fn export_tasks(state: State<AppState>, path: String) -> R<usize> {
    let tasks = state.db.tasks();
    write_file_timeout(path, ical::export_tasks_ics(&tasks), 30)?;
    Ok(tasks.len())
}

// ======================================================================= notlar
#[tauri::command(async)]
pub fn list_notes(state: State<AppState>) -> Vec<Note> {
    state.db.notes()
}

#[tauri::command(async)]
pub fn save_note(state: State<AppState>, note: Note) -> R<i64> {
    let mut note = note;
    match state.db.note(note.id) {
        Some(old) => {
            note.account_id = old.account_id;
            note.remote_id = old.remote_id;
        }
        None => note.account_id = pim_account(&state.db),
    }
    note.dirty = note.account_id.is_some();
    let id = state.db.save_note(&note).map_err(err)?;
    if note.dirty {
        state.mail.push_note(id);
    }
    Ok(id)
}

#[tauri::command(async)]
pub fn delete_note(state: State<AppState>, id: i64) {
    if let Some(n) = state.db.note(id) {
        if let (Some(account), Some(rid)) = (n.account_id, n.remote_id) {
            state.db.add_tombstone("notes", account, &rid);
            state.mail.flush_deleted("notes", account);
        }
    }
    state.db.delete_note(id);
}

// ======================================================================= kurallar / kategoriler
#[tauri::command(async)]
pub fn list_rules(state: State<AppState>) -> Vec<Rule> {
    state.db.rules(false)
}

fn is_exchange_rule(db: &Db, r: &Rule) -> bool {
    r.remote_id.is_some() || r.account_id.and_then(|a| db.account(a)).map(|a| a.is_exchange()).unwrap_or(false)
}

/// Kural kaydı. Exchange hesabına bağlı kurallar sunucuya yazılır (Outlook gibi sunucuda çalışır).
/// `force`: sunucu "Outlook masaüstü istemci kuralları var" derse, onların silinmesine izin ver.
#[tauri::command]
pub async fn save_rule(state: State<'_, AppState>, rule: Rule, force: Option<bool>) -> R<i64> {
    let mut rule = rule;
    if let Some(old) = state.db.rules(false).into_iter().find(|r| r.id == rule.id && rule.id > 0) {
        rule.remote_id = old.remote_id.clone();
        rule.remote_xml = old.remote_xml.clone();
        rule.complex = old.complex;
        // Hesap değiştiyse eski sunucu kuralı silinir, yenisi oluşturulur
        if old.remote_id.is_some() && old.account_id != rule.account_id {
            let db = state.db.clone();
            let f = force.unwrap_or(false);
            tauri::async_runtime::spawn_blocking(move || crate::ews_sync::delete_server_rule(&db, &old, f)).await.map_err(err)??;
            rule.remote_id = None;
            rule.complex = false;
        }
    }
    if is_exchange_rule(&state.db, &rule) {
        let db = state.db.clone();
        let f = force.unwrap_or(false);
        let r2 = rule.clone();
        tauri::async_runtime::spawn_blocking(move || crate::ews_sync::push_server_rule(&db, &r2, f)).await.map_err(err)??;
        // Yerel kopya sunucudan yenilendi; yeni kuralın yerel kimliği adından bulunur
        if rule.id > 0 && rule.remote_id.is_none() {
            state.db.delete_rule(rule.id);
        }
        return Ok(state.db.rules(false).into_iter().find(|r| r.name == rule.name && r.account_id == rule.account_id).map(|r| r.id).unwrap_or(0));
    }
    rule.remote_id = None;
    state.db.save_rule(&rule).map_err(err)
}

#[tauri::command]
pub async fn delete_rule(state: State<'_, AppState>, id: i64, force: Option<bool>) -> R<()> {
    let Some(r) = state.db.rules(false).into_iter().find(|r| r.id == id) else { return Ok(()) };
    if r.remote_id.is_some() {
        let db = state.db.clone();
        let f = force.unwrap_or(false);
        tauri::async_runtime::spawn_blocking(move || crate::ews_sync::delete_server_rule(&db, &r, f)).await.map_err(err)??;
    }
    state.db.delete_rule(id);
    Ok(())
}

/// Exchange kuralının tam modeli (Kural Sihirbazı için). Taşıma/kopyalama hedefi olan
/// bilinen klasör adları ("inbox" vb.) hesabın klasör kimliğine çevrilir.
#[tauri::command]
pub async fn server_rule_spec(state: State<'_, AppState>, id: i64) -> R<mail::rule_spec::RuleSpec> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || server_rule_spec_blocking(&db, id)).await.map_err(err)?
}

fn server_rule_spec_blocking(db: &Db, id: i64) -> R<mail::rule_spec::RuleSpec> {
    let mut r = db.rules(false).into_iter().find(|r| r.id == id).ok_or_else(|| t!("error.not_found"))?;
    // Sunucu kuralının gövdesi henüz alınmamışsa (eski sürümde eşitlenmiş kural) önce
    // sunucudan okunur; yoksa sihirbaz koşulsuz/eylemsiz (boş) açılıyordu
    if r.remote_xml.is_empty() && r.remote_id.is_some() {
        if let Some(acc) = r.account_id.and_then(|a| db.account(a)).filter(|a| a.is_exchange()) {
            match mail::ews::pooled(&acc, |e| e.get_rules(&acc.email)) {
                Ok(list) => {
                    let rid = r.remote_id.clone();
                    if let Some(sr) = list.iter().find(|x| Some(&x.id) == rid.as_ref()) {
                        r.remote_xml = sr.body_xml.clone();
                    }
                    crate::ews_sync::store_server_rules(db, acc.id, list);
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    let state_db = db;
    let mut spec = if !r.remote_xml.is_empty() {
        mail::rule_spec::from_xml(&r.remote_xml)
    } else {
        // Yerel (basit) kuraldan dönüştür
        let body = mail::ews::ServerRule::build_body(&r.field, &r.op, &r.value, &r.action, &r.action_arg, r.stop).unwrap_or_default();
        mail::rule_spec::from_xml(&body)
    };
    let folders = r.account_id.map(|a| state_db.folders(Some(a))).unwrap_or_default();
    for a in spec.actions.iter_mut().filter(|a| a.kind == "move" || a.kind == "copy") {
        let role = match a.values.first().map(|s| s.as_str()) {
            Some("inbox") => Some("inbox"),
            Some("deleteditems") => Some("trash"),
            Some("junkemail") => Some("junk"),
            Some("sentitems") => Some("sent"),
            Some("drafts") => Some("drafts"),
            Some("archive") => Some("archive"),
            _ => None,
        };
        if let Some(f) = role.and_then(|ro| folders.iter().find(|f| f.role.as_deref() == Some(ro))) {
            if let Some(rn) = &f.remote_name {
                a.values = vec![rn.clone()];
            }
        }
    }
    Ok(spec)
}

/// Kural Sihirbazı ile düzenlenen Exchange kuralını kaydeder (oluştur/değiştir).
#[tauri::command]
pub async fn save_server_rule(
    state: State<'_, AppState>,
    id: i64,
    account_id: i64,
    name: String,
    enabled: bool,
    spec: mail::rule_spec::RuleSpec,
    force: Option<bool>,
) -> R<()> {
    let body = mail::rule_spec::to_xml(&spec)?;
    let old = state.db.rules(false).into_iter().find(|r| r.id == id && id > 0);
    let db = state.db.clone();
    let f = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        // Hesap değiştiyse eski kural silinir
        let mut remote = old.as_ref().and_then(|o| o.remote_id.clone());
        if let Some(o) = &old {
            if o.remote_id.is_some() && o.account_id != Some(account_id) {
                crate::ews_sync::delete_server_rule(&db, o, f)?;
                remote = None;
            }
            if o.remote_id.is_none() {
                db.delete_rule(o.id);
            }
        }
        crate::ews_sync::push_server_rule_body(&db, account_id, remote.as_deref(), &name, enabled, &body, f)
    })
    .await
    .map_err(err)?
}

/// Exchange kurallarını sunucudan yeniden okur.
#[tauri::command]
pub async fn refresh_server_rules(state: State<'_, AppState>) -> R<()> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        for acc in db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed") {
            match mail::ews::pooled(&acc, |e| e.get_rules(&acc.email)) {
                Ok(list) => crate::ews_sync::store_server_rules(&db, acc.id, list),
                Err(e) if e.is_auth() => db.set_account_auth(acc.id, "failed", e.message()),
                Err(_) => {}
            }
        }
    })
    .await
    .map_err(err)
}

/// Kuralları seçili klasördeki mevcut iletilere uygular (yalnızca yerel eylemler).
#[tauri::command(async)]
pub fn run_rules(state: State<AppState>, folder_id: i64) -> usize {
    let rules = state.db.rules(true);
    let me = state.db.my_addresses();
    let mut n = 0;
    for row in state.db.messages(Some(folder_id), "", &MsgFilter::All) {
        let Some(m) = state.db.message(row.id) else { continue };
        for (action, arg) in crate::rules::evaluate(&rules, &m, m.account_id, &me) {
            n += 1;
            match action.as_str() {
                "mark_read" => state.mail.set_read(&[m.id], true),
                "mark_unread" => state.mail.set_read(&[m.id], false),
                "flag" => state.mail.set_flagged(&[m.id], true),
                "unflag" => state.mail.set_flagged(&[m.id], false),
                "important" => state.mail.set_importance(&[m.id], 2),
                "importance" => state.mail.set_importance(&[m.id], crate::rules::importance_level(&arg)),
                "permanent_delete" => state.mail.delete(&[m.id], true),
                "copy" => {
                    if let (Ok(fid), Some(raw)) = (arg.parse::<i64>(), state.raw_of(m.id)) {
                        if !raw.is_empty() {
                            state.mail.import_raw(fid, raw, true);
                        }
                    }
                }
                "category" => {
                    let mut c = split_list(&m.categories);
                    if !c.contains(&arg) {
                        c.push(arg.clone());
                    }
                    state.mail.set_categories(&[m.id], &c);
                }
                "move" => {
                    if let Ok(fid) = arg.parse() {
                        state.mail.move_messages(&[m.id], fid);
                    }
                }
                "delete" => state.mail.delete(&[m.id], false),
                "junk" => {
                    if let Some(f) = state.db.folder_by_role_or_local(m.account_id, "junk") {
                        state.mail.move_messages(&[m.id], f.id);
                    }
                }
                _ => n -= 1,
            }
        }
    }
    n
}

#[tauri::command(async)]
pub fn list_categories(state: State<AppState>) -> Vec<Category> {
    state.db.categories()
}

#[tauri::command(async)]
pub fn save_category(state: State<AppState>, old_name: Option<String>, name: String, color: String) {
    state.db.save_category(old_name.as_deref(), &name, &color);
}

#[tauri::command(async)]
pub fn delete_category(state: State<AppState>, name: String) {
    state.db.delete_category(&name);
}

// ======================================================================= hatırlatıcılar
#[derive(Serialize, Clone)]
pub struct Reminder {
    key: String,
    kind: String,
    item_id: i64,
    title: String,
    location: String,
    start: f64,
    end: f64,
    all_day: bool,
    is_meeting: bool,
    organizer: String,
    /// Çevrimiçi toplantı bağlantısı (Teams, Zoom, Meet, Webex): "Çevrimiçi Katıl"
    join_url: String,
}

/// Konum ya da açıklamadaki çevrimiçi toplantı bağlantısı
fn join_url_of(location: &str, description: &str) -> String {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(?i)https://[^\s"'<>]*(?:teams\.microsoft\.com/(?:l/meetup-join|meet)|teams\.live\.com/meet|zoom\.us/(?:j|my|w)/|meet\.google\.com/|webex\.com/(?:meet|join|[a-z0-9]+/j\.php))[^\s"'<>]*"#).unwrap()
    });
    [location, description].iter().find_map(|t| RE.find(t).map(|m| m.as_str().replace("&amp;", "&"))).unwrap_or_default()
}

pub fn due_reminders(db: &Db) -> Vec<Reminder> {
    let n = now();
    let mut out = Vec::new();
    for o in recurrence::expand(db.events_between(n - 86400.0, n + 15.0 * 86400.0), &[], n - 86400.0, n + 15.0 * 86400.0) {
        let e = &o.event;
        if e.reminder_minutes < 0 || e.is_cancelled {
            continue;
        }
        let fire = o.start - (e.reminder_minutes * 60) as f64;
        if fire <= n && n < o.end {
            let key = format!("event:{}:{}", e.id, o.start as i64);
            if db.reminder_dismissed_until(&key).map(|u| u > n).unwrap_or(false) {
                continue;
            }
            out.push(Reminder {
                key,
                kind: "event".into(),
                item_id: e.id,
                title: e.title.clone(),
                location: e.location.clone(),
                start: o.start,
                end: o.end,
                all_day: e.all_day,
                is_meeting: e.is_meeting,
                organizer: e.organizer.clone(),
                join_url: join_url_of(&e.location, &e.description),
            });
        }
    }
    for t in db.tasks() {
        if t.is_completed() || t.reminder_done {
            continue;
        }
        if let Some(r) = t.reminder {
            if r <= n {
                let key = format!("task:{}", t.id);
                if db.reminder_dismissed_until(&key).map(|u| u > n).unwrap_or(false) {
                    continue;
                }
                let start = t.due_date.unwrap_or(r);
                out.push(Reminder { key, kind: "task".into(), item_id: t.id, title: t.title.clone(), location: String::new(), start, end: start, all_day: t.due_date.is_some(), is_meeting: false, organizer: String::new(), join_url: String::new() });
            }
        }
    }
    out
}

#[tauri::command(async)]
pub fn get_reminders(state: State<AppState>) -> Vec<Reminder> {
    due_reminders(&state.db)
}

/// `snooze_minutes` ya da `until` (ör. "başlangıçtan 5 dakika önce") verilmezse hatırlatıcı kapatılır.
#[tauri::command(async)]
pub fn dismiss_reminder(state: State<AppState>, key: String, snooze_minutes: Option<f64>, until: Option<f64>) {
    match until.filter(|u| *u > now()).or(snooze_minutes.map(|m| now() + m * 60.0)) {
        Some(u) => state.db.dismiss_reminder(&key, u),
        None => {
            if let Some(id) = key.strip_prefix("task:").and_then(|s| s.parse::<i64>().ok()) {
                if let Some(mut t) = state.db.task(id) {
                    t.reminder_done = true;
                    let _ = state.db.save_task(&t);
                }
            }
            state.db.dismiss_reminder(&key, f64::MAX);
        }
    }
}

// ======================================================================= otomatik kurulum / Exchange
#[tauri::command]
pub async fn system_identity() -> R<mail::gssapi::SystemIdentity> {
    tauri::async_runtime::spawn_blocking(mail::gssapi::system_identity).await.map_err(err)
}

#[tauri::command]
pub async fn discover_account(app: tauri::AppHandle, email: String, username: String, password: String) -> R<mail::discover::Discovery> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        // Her adım arayüze bildirilir ("Hesabınız ayarlanıyor" altında tek satır)
        let h = app.clone();
        mail::discover::set_progress(Some(Box::new(move |m: &str| {
            let _ = h.emit("account://progress", m.to_string());
        })));
        let d = mail::discover::discover(email.trim(), username.trim(), &password);
        mail::discover::set_progress(None);
        d
    })
    .await
    .map_err(err)
}

/// Genel adres listesi (Exchange) + yerel öneriler
#[tauri::command]
pub async fn suggest_recipients(state: State<'_, AppState>, query: String, include_gal: bool) -> R<Vec<Value>> {
    let mut list: Vec<Value> = state
        .db
        .suggest_recipients(&query, 12)
        .into_iter()
        .map(|(n, e, src, _)| json!({"name": n, "email": e, "source": src}))
        .collect();
    if include_gal && query.trim().chars().count() >= 3 {
        // Çevrimdışı adres defterinden anında öneri; adres defteri olan hesaplar sunucuya sorulmaz
        for r in state.db.directory_search(None, &query, 0, 15) {
            if !list.iter().any(|x| x["email"].as_str().map(|s| s.eq_ignore_ascii_case(&r.email)).unwrap_or(false)) {
                list.push(json!({"name": r.name, "email": r.email, "source": if r.is_group() { "dl" } else { "directory" }}));
            }
            if r.is_group() {
                GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(r.email.to_lowercase(), true);
            }
        }
        let accounts: Vec<Account> = state.db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed" && state.db.directory_count(a.id) == 0).collect();
        let q = query.clone();
        let db = state.db.clone();
        let gal = tauri::async_runtime::spawn_blocking(move || {
            let mut out = Vec::new();
            for a in accounts {
                match mail::ews::pooled(&a, |e| e.resolve_names_typed(&q)) {
                    Ok(r) => out.extend(r),
                    // Parola reddedildiyse hesabı duraklat: her tuş vuruşu AD'de hatalı deneme sayılmasın
                    Err(e) if e.is_auth() => db.set_account_auth(a.id, "failed", e.message()),
                    Err(_) => {}
                }
            }
            out
        })
        .await
        .unwrap_or_default();
        for (n, e, kind) in gal {
            let dl = kind.ends_with("DL");
            if dl {
                GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(e.to_lowercase(), true);
            }
            if !list.iter().any(|x| x["email"].as_str().map(|s| s.eq_ignore_ascii_case(&e)).unwrap_or(false)) {
                list.push(json!({"name": n, "email": e, "source": if dl { "dl" } else { "directory" }}));
            }
        }
    }
    Ok(list)
}

#[tauri::command(async)]
pub fn forget_recipient(state: State<AppState>, email: String) {
    state.db.forget_recipient(&email);
}

/// OAuth ile oturum açar; şifrelenmiş belirteç kümesini döndürür (save_account'a verilir).
#[tauri::command]
pub async fn oauth_authorize(app: tauri::AppHandle, provider: String, exchange: bool, email: String) -> R<String> {
    let _ = app;
    let ts = tauri::async_runtime::spawn_blocking(move || {
        crate::oauth::authorize(&provider, exchange, &email, |url| {
            let _ = crate::desktop::open_url(url);
        })
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    Ok(crate::oauth::encode(&ts))
}

/// Ofis dışı (otomatik yanıt): Exchange'de sunucuda, IMAP'te istemcide çalışır.
#[tauri::command]
pub async fn set_auto_reply(state: State<'_, AppState>, account_id: i64, enabled: bool, text: String) -> R<()> {
    let mut a = state.db.account(account_id).ok_or_else(|| t!("error.no_account"))?;
    a.auto_reply_enabled = enabled;
    a.auto_reply_text = text.clone();
    state.db.save_account(&a).map_err(err)?;
    if a.is_exchange() {
        if a.auth_state == "failed" {
            return Err(t!("error.auth_paused", a.email));
        }
        let acc = a.clone();
        let r = tauri::async_runtime::spawn_blocking(move || mail::ews::pooled(&acc, |e| e.set_oof(&acc.email, enabled, &text)))
            .await
            .map_err(err)?;
        if let Err(e) = r {
            if e.is_auth() {
                state.db.set_account_auth(a.id, "failed", e.message());
            }
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// Toplantı davetine yanıt: "accept" | "tentative" | "decline"
#[tauri::command(async)]
pub fn respond_meeting(state: State<AppState>, msg_id: i64, kind: String, comment: Option<String>, send: Option<bool>) -> R<()> {
    let comment = comment.unwrap_or_default();
    let send = send.unwrap_or(true);
    let row = state.db.message(msg_id);
    let acc = row.as_ref().and_then(|m| m.account_id).and_then(|a| state.db.account(a));
    if let (Some(acc), Some(rid)) = (acc.as_ref().filter(|a| a.is_exchange()), row.as_ref().and_then(|m| m.remote_id.clone())) {
        state.mail.respond_ews(acc.id, rid, kind, comment, send);
        return Ok(());
    }
    // iMIP (RFC 6047): düzenleyiciye METHOD:REPLY gönder
    let raw = state.raw_of(msg_id).ok_or("?")?;
    let p = parse::parse(&raw);
    let ics = p
        .attachments
        .iter()
        .find(|a| a.mime == "text/calendar" || a.name.to_lowercase().ends_with(".ics"))
        .map(|a| String::from_utf8_lossy(&a.data).to_string())
        .ok_or_else(|| t!("calendar.no_invite"))?;
    let acc = acc.or_else(|| state.db.accounts().into_iter().next()).ok_or_else(|| t!("error.no_account"))?;
    let partstat = match kind.as_str() {
        "accept" => "ACCEPTED",
        "tentative" => "TENTATIVE",
        _ => "DECLINED",
    };
    let (organizer, reply, summary) = ical::make_reply(&ics, &acc.email, partstat).ok_or_else(|| t!("calendar.no_invite"))?;
    if kind != "decline" {
        let imp = ical::parse_ics(&ics);
        for mut e in imp.events {
            if let Some(x) = state.db.event_by_uid(&e.uid) {
                e.id = x.id;
                e.calendar_id = x.calendar_id;
            }
            if kind == "tentative" {
                e.show_as = "tentative".into();
            }
            let _ = state.db.save_event(&e);
        }
    }
    if !send {
        return Ok(());
    }
    let subject = format!("{}: {summary}", t!(&format!("meeting.{kind}_prefix")));
    let body = t!(&format!("meeting.{kind}_body"), summary);
    let m = ComposedMail {
        account_id: Some(acc.id),
        to: organizer,
        subject,
        text: if comment.is_empty() { body } else { format!("{comment}\n\n{body}") },
        ics: Some(reply),
        importance: 1,
        ..Default::default()
    };
    let raw = build_raw(Some(&acc), &m, &[], true);
    state.mail.send(acc.id, raw, None);
    Ok(())
}

/// Toplantı isteği (Outlook okuma bölmesindeki davet): saat, konum, katılımcılar, yanıtım,
/// takvimimdeki çakışma/bitişik öğeler ve o günün kısa görünümü.
#[tauri::command(async)]
pub fn meeting_info(state: State<AppState>, msg_id: i64) -> R<Value> {
    let raw = state.raw_of(msg_id).ok_or_else(|| t!("error.not_found"))?;
    let p = parse::parse(&raw);
    let ics = p
        .attachments
        .iter()
        .find(|a| a.mime.starts_with("text/calendar") || a.name.to_lowercase().ends_with(".ics"))
        .map(|a| String::from_utf8_lossy(&a.data).to_string())
        .ok_or_else(|| t!("calendar.no_invite"))?;
    let imp = ical::parse_ics(&ics);
    let ev = imp.events.into_iter().next().ok_or_else(|| t!("calendar.no_invite"))?;
    let mine = (!ev.uid.is_empty()).then(|| state.db.event_by_uid(&ev.uid)).flatten();
    // Kendi takvimlerim (başkalarının/internet takvimleri sayılmaz); toplantının kendisi hariç
    let others: Vec<i64> = state.db.calendars().into_iter().filter(|c| !c.kind.is_empty() || !c.owner.is_empty()).map(|c| c.id).collect();
    let day0 = {
        let d = chrono::DateTime::from_timestamp(ev.start as i64, 0).map(|d| d.with_timezone(&chrono::Local).date_naive()).unwrap_or_default();
        d.and_hms_opt(0, 0, 0).and_then(|n| n.and_local_timezone(chrono::Local).single()).map(|d| d.timestamp() as f64).unwrap_or(ev.start)
    };
    let day1 = day0 + 86400.0;
    let same = |o: &recurrence::Occurrence| (!ev.uid.is_empty() && o.event.uid == ev.uid) || (o.event.title == ev.title && (o.start - ev.start).abs() < 1.0);
    let day: Vec<recurrence::Occurrence> = recurrence::expand(state.db.events_between(day0, day1), &others, day0, day1)
        .into_iter()
        .filter(|o| !same(o) && !o.event.is_cancelled && o.event.show_as != "free")
        .collect();
    let conflicts: Vec<&recurrence::Occurrence> = day.iter().filter(|o| !ev.all_day && o.start < ev.end && o.end > ev.start).collect();
    let adjacent: Vec<&recurrence::Occurrence> = day.iter().filter(|o| !ev.all_day && ((o.end - ev.start).abs() < 1.0 || (o.start - ev.end).abs() < 1.0)).collect();
    let occ = |o: &recurrence::Occurrence| json!({"title": o.event.title, "start": o.start, "end": o.end, "show_as": o.event.show_as, "all_day": o.event.all_day, "location": o.event.location});
    Ok(json!({
        "title": ev.title,
        "start": ev.start,
        "end": ev.end,
        "all_day": ev.all_day,
        "location": ev.location,
        "organizer": ev.organizer,
        "attendees": ev.attendees,
        "optional": ev.optional_attendees,
        "resources": ev.resources,
        "recurrence": ev.recurrence,
        "method": imp.method,
        "cancelled": imp.method.eq_ignore_ascii_case("CANCEL") || ev.is_cancelled,
        "response": mine.as_ref().map(|m| m.response.clone()).unwrap_or_default(),
        "event_id": mine.as_ref().map(|m| m.id),
        "conflicts": conflicts.iter().map(|o| occ(o)).collect::<Vec<_>>(),
        "adjacent": adjacent.iter().map(|o| occ(o)).collect::<Vec<_>>(),
        "day": day.iter().map(occ).collect::<Vec<_>>(),
    }))
}

// ======================================================================= genel
#[tauri::command(async)]
pub fn app_info() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "data_dir": default_data_dir().to_string_lossy(),
        "readpst": std::process::Command::new("readpst").arg("-V").output().is_ok(),
    })
}

/// Ön yüz hatalarını standart hata çıktısına yazar (sorun giderme için).
#[tauri::command(async)]
pub fn frontend_log(level: String, message: String) {
    eprintln!("[ui:{level}] {message}");
}

#[tauri::command(async)]
pub fn read_text_file(path: String) -> R<String> {
    std::fs::read_to_string(path).map_err(err)
}

#[tauri::command(async)]
pub fn write_text_file(path: String, content: String) -> R<()> {
    write_file_timeout(path, content, 30)
}

#[tauri::command(async)]
pub fn file_info(path: String) -> R<Value> {
    let md = std::fs::metadata(&path).map_err(err)?;
    let name = Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
    Ok(json!({"name": name, "size": md.len(), "mime": guess_mime(&name)}))
}

/// Arama kutusu önerileri: { people: [{name, email, count}], attachments: [{id, name, subject, sender, date}] }
#[tauri::command]
pub async fn search_suggest(state: State<'_, AppState>, query: String) -> R<Value> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (people, attachments) = db.search_suggest(&query);
        Ok(json!({"people": people, "attachments": attachments}))
    })
    .await
    .map_err(err)?
}

/// Panodan yapıştırılan dosya içeriği (ad + base64) geçici dosyaya yazılır (ek olarak eklenir)
#[tauri::command(async)]
pub fn write_temp_file(name: String, data: String) -> R<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(err)?;
    let dir = util::temp_dir().join(format!("paste-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let base = util::safe_filename(if name.trim().is_empty() { "dosya" } else { name.trim() });
    let path = dir.join(base);
    std::fs::write(&path, bytes).map_err(err)?;
    Ok(path.to_string_lossy().to_string())
}

/// Resim dosyası data: adresi olarak (dosya yöneticisinden kopyalanan resim gövdeye yapıştırılır)
#[tauri::command(async)]
pub fn image_data_url(path: String) -> R<String> {
    use base64::Engine;
    let md = std::fs::metadata(&path).map_err(err)?;
    if md.len() > 25 * 1024 * 1024 {
        return Err(t!("error.too_large"));
    }
    let name = Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let mime = guess_mime(&name);
    if !mime.starts_with("image/") {
        return Err("not an image".into());
    }
    let data = std::fs::read(&path).map_err(err)?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(data)))
}

/// Oturum açılışında başlatma (XDG autostart).
#[tauri::command(async)]
pub fn set_autostart(enabled: bool) -> R<()> {
    let dir = dirs::config_dir().ok_or("config")?.join("autostart");
    let path = dir.join("lynora.desktop");
    if enabled {
        std::fs::create_dir_all(&dir).map_err(err)?;
        let exe = std::env::var("APPIMAGE").ok().or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().to_string())).unwrap_or_else(|| "lynora".into());
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=Lynora\nExec=\"{exe}\" --minimized\nIcon=lynora\nX-GNOME-Autostart-enabled=true\nX-GNOME-Autostart-Delay=5\nNoDisplay=false\n"
        );
        std::fs::write(&path, body).map_err(err)?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(err)?;
    }
    Ok(())
}

// ------------------------------------------------------------ masaüstü
/// Bağlantıyı varsayılan uygulamayla açar (yalnızca web, e-posta, telefon, ftp)
#[tauri::command(async)]
pub fn open_url(url: String) -> R<()> {
    crate::desktop::open_url(&url)
}

/// Ek dosyasını varsayılan uygulamayla açar (yalnızca ek klasöründen; çalıştırılabilir değil)
#[tauri::command(async)]
pub fn open_attachment(path: String) -> R<()> {
    crate::desktop::open_attachment_file(&path)
}

/// Masaüstü bildirimi
#[tauri::command(async)]
pub fn desktop_notify(title: String, body: String, sound: bool) {
    crate::desktop::notify(&title, &body, sound);
}

// ------------------------------------------------------------ varsayılan e-posta uygulaması
/// Varsayılan yapılacak türler: e-posta bağlantıları ve ileti dosyaları (.msg, .eml)
const DEFAULT_MIMES: &[&str] = &["x-scheme-handler/mailto", "application/vnd.ms-outlook", "message/rfc822"];
/// .msg'nin tür tanımı sisteme işlenmemişse algılandığı genel tür
#[cfg(target_os = "linux")]
const OLE_MIME: &str = "application/x-ole-storage";

/// Uygulamanın masaüstü dosyası (paketle kurulduysa onunki; AppImage ise kullanıcı
/// klasöründe oluşturulur)
#[cfg(target_os = "linux")]
fn desktop_id() -> Option<String> {
    let mut dirs: Vec<std::path::PathBuf> = dirs::data_dir().into_iter().map(|d| d.join("applications")).collect();
    let sys = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(|s| std::path::PathBuf::from(s).join("applications")));
    if std::env::var_os("APPIMAGE").is_none() {
        for name in ["Lynora.desktop", "lynora.desktop"] {
            if dirs.iter().any(|d| d.join(name).exists()) {
                return Some(name.into());
            }
        }
    }
    // AppImage ya da kurulu masaüstü dosyası yok: kullanıcı için oluştur
    let exe = std::env::var("APPIMAGE").ok().or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().to_string()))?;
    let dir = dirs::data_dir()?.join("applications");
    std::fs::create_dir_all(&dir).ok()?;
    let body = format!(
        "[Desktop Entry]\nType=Application\nName=Lynora\nExec=\"{exe}\" %U\nIcon=lynora\nTerminal=false\nCategories=Office;Email;\nMimeType={};\n",
        DEFAULT_MIMES.join(";")
    );
    std::fs::write(dir.join("lynora-user.desktop"), body).ok()?;
    let _ = std::process::Command::new("update-desktop-database").arg(&dir).status();
    Some("lynora-user.desktop".into())
}

/// Masaüstüne özel varsayılan uygulama listeleri ($XDG_CURRENT_DESKTOP; varsa)
#[cfg(target_os = "linux")]
fn desktop_mimeapps_paths() -> Vec<std::path::PathBuf> {
    let Some(cfg) = dirs::config_dir() else { return vec![] };
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| cfg.join(format!("{}-mimeapps.list", d.to_lowercase())))
        .filter(|p| p.exists())
        .collect()
}

/// .msg dosya türü tanımı: shared-mime-info .msg uzantısını tanımlamıyor (dosya genel
/// "OLE belgesi" sanılıyor ve varsayılan e-posta uygulamasıyla açılmıyor). Paket tanımı
/// sisteme kurar; AppImage ya da eksikse kullanıcı klasörüne yazılır.
#[cfg(target_os = "linux")]
pub fn ensure_mime_definition() {
    const XML: &str = include_str!("../../packaging/lynora-mime.xml");
    // Paketle kurulan tanım sistem önbelleğine işlenmişse yeter (önbellek güncellenmemişse
    // .msg yine "OLE belgesi" sanılır: o durumda kullanıcı klasörüne de yazılır)
    let in_cache = |dir: &str| std::fs::read_to_string(format!("{dir}/globs2")).is_ok_and(|g| g.contains("application/vnd.ms-outlook:*.msg"));
    if std::path::Path::new("/usr/share/mime/packages/lynora.xml").exists() && in_cache("/usr/share/mime") {
        return;
    }
    let Some(base) = dirs::data_dir().map(|d| d.join("mime")) else { return };
    let file = base.join("packages").join("lynora.xml");
    if std::fs::read_to_string(&file).is_ok_and(|s| s == XML) && in_cache(&base.to_string_lossy()) {
        return;
    }
    let _ = std::fs::create_dir_all(base.join("packages"));
    if std::fs::write(&file, XML).is_ok() {
        let _ = std::process::Command::new("update-mime-database").arg(&base).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// Kullanıcının varsayılan uygulama listesi (freedesktop mimeapps.list)
#[cfg(target_os = "linux")]
fn mimeapps_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("mimeapps.list"))
}

/// mimeapps.list içinde bir bölümdeki anahtarları ayarlar (diğer satırlar korunur)
fn set_ini_keys(src: &str, section: &str, keys: &[(&str, String)]) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut in_sec = false;
    let mut found = false;
    let mut done: Vec<String> = Vec::new();
    let flush = |out: &mut Vec<String>, done: &mut Vec<String>| {
        for (k, v) in keys {
            if !done.iter().any(|d| d == k) {
                out.push(format!("{k}={v}"));
                done.push(k.to_string());
            }
        }
    };
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_sec {
                flush(&mut out, &mut done);
            }
            in_sec = t == header;
            found |= in_sec;
            out.push(line.to_string());
            continue;
        }
        if in_sec {
            if let Some((k, _)) = t.split_once('=') {
                if let Some((key, v)) = keys.iter().find(|(key, _)| *key == k.trim()) {
                    out.push(format!("{key}={v}"));
                    done.push(key.to_string());
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if in_sec {
        flush(&mut out, &mut done);
    }
    if !found {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(header);
        flush(&mut out, &mut done);
    }
    out.join("\n") + "\n"
}

/// Kullanıcının kendi listelerinde bir tür için açıkça seçilmiş uygulama (masaüstüne özel
/// liste önce okunur). xdg-mime seçim yoksa türü destekleyen herhangi bir uygulamayı
/// döndürdüğünden "varsayılan mı" sorusu için bu kullanılır.
#[cfg(target_os = "linux")]
fn explicit_default(mime: &str) -> Option<String> {
    let mut lists = desktop_mimeapps_paths();
    lists.extend(mimeapps_path());
    for p in lists {
        let src = std::fs::read_to_string(p).unwrap_or_default();
        let mut in_def = false;
        for line in src.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_def = t == "[Default Applications]";
            } else if in_def {
                if let Some(v) = t.strip_prefix(&format!("{mime}=")) {
                    return Some(v.split(';').next().unwrap_or("").trim().to_lowercase());
                }
            }
        }
    }
    None
}

/// Bir tür için varsayılan uygulamanın masaüstü dosyası (kullanıcı listesi, yoksa xdg-mime)
#[cfg(target_os = "linux")]
fn default_for(mime: &str) -> String {
    if let Some(v) = explicit_default(mime) {
        return v;
    }
    if let Ok(out) = std::process::Command::new("xdg-mime").args(["query", "default", mime]).output() {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
        }
    }
    String::new()
}

/// Lynora e-posta bağlantıları, .msg ve .eml için varsayılan mı
fn default_app_status_all() -> bool {
    #[cfg(target_os = "linux")]
    return default_for(DEFAULT_MIMES[0]).starts_with("lynora") && DEFAULT_MIMES[1..].iter().all(|m| explicit_default(m).is_some_and(|v| v.starts_with("lynora")));
    #[allow(unreachable_code)]
    false
}

/// Lynora varsayılan e-posta uygulaması mı (bağlantılar ve ileti dosyaları)
#[tauri::command(async)]
pub fn default_app_status() -> bool {
    default_app_status_all()
}

/// Lynora'yı e-posta bağlantıları ve .msg/.eml dosyaları için varsayılan yapar
#[tauri::command(async)]
pub fn set_default_app() -> R<()> {
    #[cfg(target_os = "linux")]
    {
        let id = desktop_id().ok_or_else(|| t!("error.internal"))?;
        // Standart kullanıcı listesine doğrudan yazılır (xdg-utils kurulu olmasa da çalışır)
        let path = mimeapps_path().ok_or_else(|| t!("error.internal"))?;
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let mut defaults: Vec<(&str, String)> = DEFAULT_MIMES.iter().map(|m| (*m, format!("{id};"))).collect();
        // Tür önbelleği eski kalmış sistemlerde .msg genel "OLE belgesi" sanılır: o tür için
        // kullanıcı başka bir uygulama seçmemişse Lynora açar
        if explicit_default(OLE_MIME).map_or(true, |v| v.is_empty() || v.starts_with("lynora") || v.starts_with("mailnix")) {
            defaults.push((OLE_MIME, format!("{id};")));
        }
        let out = set_ini_keys(&set_ini_keys(&src, "Default Applications", &defaults), "Added Associations", &defaults);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&path, out).map_err(err)?;
        // Masaüstüne özel listeler (ör. xfce-mimeapps.list) genel listeden önce okunur
        for p in desktop_mimeapps_paths() {
            if let Ok(src) = std::fs::read_to_string(&p) {
                let _ = std::fs::write(&p, set_ini_keys(&set_ini_keys(&src, "Default Applications", &defaults), "Added Associations", &defaults));
            }
        }
        ensure_mime_definition();
        let _ = crate::db::Db::open_default().map(|db| db.set_setting("default_app_chosen", "1"));
        // Varsa masaüstünün kendi aracıyla da (GNOME/Xfce/KDE önbellekleri)
        let _ = std::process::Command::new("xdg-mime").arg("default").arg(&id).args(DEFAULT_MIMES).status();
        let _ = std::process::Command::new("xdg-settings").args(["set", "default-url-scheme-handler", "mailto", &id]).status();
        // KDE seçimleri kendi önbelleğinden okur
        if std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_uppercase().contains("KDE") {
            for prog in ["kbuildsycoca6", "kbuildsycoca5"] {
                if std::process::Command::new(prog).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok() {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Uygulamanın eski adı (MailNix) ile yapılmış masaüstü ayarları yeni ada aktarılır:
/// oturumda başlatma kaydı ve varsayılan e-posta uygulaması seçimi. Bir kez çalışır.
pub fn migrate_legacy_integration() {
    #[cfg(target_os = "linux")]
    {
        // Oturum açılınca başlat
        if let Some(old) = dirs::config_dir().map(|d| d.join("autostart").join("mailnix.desktop")) {
            if old.exists() {
                let _ = std::fs::remove_file(&old);
                let _ = set_autostart(true);
            }
        }
        // Varsayılan e-posta uygulaması (kullanıcı daha önce seçmişse)
        let list = mimeapps_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let was_default = list.lines().any(|l| {
            let l = l.to_lowercase();
            DEFAULT_MIMES.iter().any(|m| l.starts_with(&format!("{m}="))) && (l.contains("=mailnix.desktop") || l.contains("=mailnix-user.desktop"))
        });
        if was_default {
            let _ = set_default_app();
        }
        ensure_mime_definition();
        // Kullanıcı varsayılan yapmayı seçtiyse ve başka bir uygulama .msg/.eml/mailto
        // atamasını değiştirdiyse sessizce geri alınır
        // E-posta uygulaması Lynora ise (ayarlardan ya da sistemin "Varsayılan uygulamalar"
        // bölümünden seçilmiş olsun) .msg/.eml dosyaları da Lynora ile açılır
        let chosen = crate::db::Db::open_default().ok().and_then(|db| db.setting("default_app_chosen")).as_deref() == Some("1");
        let mail_app = default_for(DEFAULT_MIMES[0]).starts_with("lynora");
        if (chosen || mail_app) && !was_default && !default_app_status_all() {
            let _ = set_default_app();
        }
        if let Some(f) = dirs::data_dir().map(|d| d.join("applications").join("mailnix-user.desktop")) {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// Posta içeriğinden kaynaklı beklenmedik bir hata (panic) uygulamayı kapatmasın;
/// hata olarak döndürülür, ayrıntı crash.log'a yazılır (bkz. lib.rs).
fn guarded<T>(f: impl FnOnce() -> R<T>) -> R<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(t!("error.internal")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mimeapps_keys() {
        let src = "[Default Applications]\ntext/html=firefox.desktop;\nx-scheme-handler/mailto=thunderbird.desktop;\n\n[Added Associations]\nimage/png=eog.desktop;\n";
        let keys = vec![("x-scheme-handler/mailto", "Lynora.desktop;".to_string()), ("message/rfc822", "Lynora.desktop;".to_string())];
        let out = set_ini_keys(&set_ini_keys(src, "Default Applications", &keys), "Added Associations", &keys);
        assert!(out.contains("text/html=firefox.desktop;"));
        assert!(out.contains("image/png=eog.desktop;"));
        assert!(!out.contains("thunderbird"));
        assert_eq!(out.matches("x-scheme-handler/mailto=Lynora.desktop;").count(), 2);
        assert_eq!(out.matches("message/rfc822=Lynora.desktop;").count(), 2);
        let fresh = set_ini_keys("", "Default Applications", &keys);
        assert!(fresh.starts_with("[Default Applications]\nx-scheme-handler/mailto=Lynora.desktop;"));
    }

    /// "İ" küçük harfe çevrilince uzunluğu değişir; gövde kesimi çökmemeli.
    #[test]
    fn body_html_with_turkish_dotted_i() {
        let raw = "From: a@b\r\nSubject: x\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<html><head><title>İİİİ İLETİ</title></head><body><p>İyi günler, İLT</p></body></html>\r\n";
        let p = parse::parse(raw.as_bytes());
        let h = body_html_of(&p);
        assert!(h.contains("İyi günler"), "{h}");
        assert!(!h.to_lowercase().contains("<body"));
    }
}

/// Arayüzde açılan klasör: hemen eşitlenir (Outlook gibi).
#[tauri::command(async)]
pub fn set_active_folder(state: State<AppState>, id: i64) {
    state.mail.set_active_folder(id);
}

/// Liste sonuna gelindiğinde daha eski iletileri sunucudan getirir.
#[tauri::command(async)]
pub fn load_older(state: State<AppState>, folder_id: i64) {
    state.mail.load_older(folder_id);
}

/// Exchange sunucusunda arama: yerelde olmayan eski iletiler de bulunur ve listeye eklenir.
/// `folder_id` boşsa tüm Exchange klasörlerinde aranır. Bulunan (yeni eklenen) ileti sayısını döndürür.
#[tauri::command]
pub async fn search_server(state: State<'_, AppState>, folder_id: Option<i64>, query: String) -> R<usize> {
    let aqs = crate::db::search_to_aqs(&query);
    if aqs.trim().is_empty() {
        return Ok(0);
    }
    // Outlook gibi: birden çok düz sözcük önce ifade olarak ("ahmet emre aydın"), sonuç
    // yoksa tüm sözcükler (VE) aranır
    let phrase_aqs = crate::db::phrase_variant(&query).map(|p| crate::db::search_to_aqs(&p)).filter(|p| !p.trim().is_empty());
    let db = state.db.clone();
    let folders: Vec<Folder> = match folder_id {
        Some(id) => db.folder(id).into_iter().collect(),
        None => db.folders(None).into_iter().chain(db.accounts().iter().flat_map(|a| db.folders(Some(a.id)))).collect(),
    };
    let targets: Vec<(Account, Folder)> = folders
        .into_iter()
        .filter(|f| f.remote_name.is_some() && f.role.as_deref() != Some("junk"))
        .filter_map(|f| {
            let a = db.account(f.account_id?)?;
            (a.is_exchange() && a.auth_state != "failed").then_some((a, f))
        })
        .take(40)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut clients: HashMap<i64, mail::ews::Ews> = HashMap::new();
        let mut added = 0usize;
        for (acc, f) in targets {
            if !clients.contains_key(&acc.id) {
                match mail::ews::Ews::for_account(&acc) {
                    Ok(c) => {
                        clients.insert(acc.id, c);
                    }
                    Err(_) => continue,
                }
            }
            let Some(c) = clients.get_mut(&acc.id) else { continue };
            let remote = f.remote_name.as_deref().unwrap_or("");
            let found = match &phrase_aqs {
                Some(p) => match c.find_items(remote, None, Some(p), 200) {
                    Ok((h, m)) if !h.is_empty() => Ok((h, m)),
                    Ok(_) => c.find_items(remote, None, Some(&aqs), 200),
                    Err(e) => Err(e),
                },
                None => c.find_items(remote, None, Some(&aqs), 200),
            };
            match found {
                Ok((headers, _)) => {
                    for h in headers {
                        if db.message_by_remote(f.id, &h.id).is_some() {
                            continue;
                        }
                        if db.insert_message(&crate::ews_sync::header_to_message(&h, &f, acc.id)).is_ok() {
                            added += 1;
                        }
                    }
                }
                Err(e) if e.is_auth() => {
                    db.set_account_auth(acc.id, "failed", e.message());
                    clients.remove(&acc.id);
                }
                Err(_) => {}
            }
        }
        added
    })
    .await
    .map_err(err)
}

/// Gönderenin fotoğrafı (Exchange/AD). Disk önbelleği: fotoğraf bir hafta, "fotoğraf yok" bir gün saklanır.
#[tauri::command]
pub async fn sender_photo(state: State<'_, AppState>, email: String) -> R<Option<String>> {
    let email = email.trim().to_lowercase();
    if !email.contains('@') {
        return Ok(None);
    }
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || photo_for(&db, &email)).await.map_err(err)
}


fn photo_for(db: &Db, email: &str) -> Option<String> {
    use base64::Engine;
    let dir = dirs::data_dir()?.join("lynora").join("photos");
    let _ = std::fs::create_dir_all(&dir);
    let key = util::safe_filename(email);
    let img = dir.join(format!("{key}.jpg"));
    let none = dir.join(format!("{key}.nophoto"));
    let age = |p: &Path| std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    let to_url = |b: &[u8]| format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b));
    if let Some(a) = age(&img) {
        if a < 7 * 86400 {
            return std::fs::read(&img).ok().map(|b| to_url(&b));
        }
    }
    if age(&none).map(|a| a < 86400).unwrap_or(false) {
        return None;
    }
    // Kişi kartında fotoğraf yoksa Exchange'e sor (aynı alan adındaki hesap öncelikli)
    let domain = email.rsplit('@').next().unwrap_or("");
    let mut accounts: Vec<Account> = db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed").collect();
    accounts.sort_by_key(|a| !a.email.to_lowercase().ends_with(domain));
    let acc = accounts.into_iter().next()?;
    match mail::ews::pooled(&acc, |c| c.get_user_photo(email)) {
        Ok(Some(bytes)) => {
            let _ = std::fs::write(&img, &bytes);
            let _ = std::fs::remove_file(&none);
            Some(to_url(&bytes))
        }
        Ok(None) => {
            let _ = std::fs::write(&none, b"");
            None
        }
        Err(e) => {
            if e.is_auth() {
                db.set_account_auth(acc.id, "failed", e.message());
            }
            None
        }
    }
}

#[tauri::command(async)]
pub fn set_pinned(state: State<AppState>, ids: Vec<i64>, value: bool) {
    state.mail.set_pinned(&ids, value);
}

// ======================================================================= odalar / uygunluk
/// Adı toplantı odası/kaynak gibi görünen kayıt (oda listesine eklenmemiş odalar için)
fn looks_like_room(name: &str, kind: &str) -> bool {
    let k = kind.to_ascii_lowercase();
    if k.contains("room") || k.contains("equipment") || k.contains("resource") {
        return true;
    }
    // Kelime bazında: "Hoda" gibi kişi adları oda sayılmaz
    let n = crate::db::fold(name);
    n.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).any(|w| {
        matches!(w, "oda" | "odası" | "odasi" | "odaları" | "odalari")
            || ["toplanti", "salon", "room", "meeting", "konferans", "conference"].iter().any(|p| w.starts_with(p))
    })
}

#[cfg(test)]
mod room_tests {
    #[test]
    fn room_names() {
        assert!(super::looks_like_room("KALAMIŞ TOPLANTI ODASI", ""));
        assert!(super::looks_like_room("Oda 3 - B Blok", ""));
        assert!(super::looks_like_room("Konferans Salonu", ""));
        assert!(super::looks_like_room("x", "Room"));
        assert!(!super::looks_like_room("Hoda Yılmaz", "Person"));
        assert!(!super::looks_like_room("Odabaşı Mehmet", ""));
    }
}

/// Kuruluştaki toplantı odaları: Exchange oda listeleri (Outlook "Oda Bulucu") ve listeye
/// eklenmemiş odalar için kurum dizini ("Tüm Odalar"). Oda listeleri bir gün önbelleğe alınır.
/// `query` verilirse (Oda Bulucu araması) dizinde sunucuda da canlı aranır: çevrimdışı adres
/// defterinde ya da oda listelerinde bulunmayan odalar da gelir ve sonraki açılışlar için saklanır.
fn all_rooms(db: &Db, query: &str) -> Vec<Value> {
    let cache = db.setting("rooms_cache").and_then(|c| serde_json::from_str::<Value>(&c).ok());
    let fresh = cache.as_ref().and_then(|v| v["at"].as_f64()).map(|a| now() - a < 86400.0).unwrap_or(false);
    let mut out: Vec<Value> = cache.as_ref().and_then(|v| v["rooms"].as_array().cloned()).unwrap_or_default();
    let accounts: Vec<Account> = db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed").collect();
    if !fresh {
        let mut list = Vec::new();
        let mut ok = false;
        for acc in &accounts {
            match mail::ews::pooled(acc, |e| e.rooms()) {
                Ok(l) => {
                    ok = true;
                    list.extend(l.into_iter().map(|(l, n, e)| json!({"list": l, "name": n, "email": e})));
                }
                Err(e) if e.is_auth() => db.set_account_auth(acc.id, "failed", e.message()),
                Err(_) => {}
            }
        }
        // Boş sonuç da saklanır (oda listesi olmayan kurumda her açılışta sunucuya gidilmez)
        if ok {
            db.set_setting("rooms_cache", &json!({"at": now(), "rooms": list}).to_string());
            out = list;
        }
    }
    let mut seen: std::collections::HashSet<String> = out.iter().filter_map(|r| r["email"].as_str().map(|e| e.to_lowercase())).collect();
    let other = t!("calendar.all_rooms");
    let mut push = |name: &str, email: &str, office: &str, out: &mut Vec<Value>| {
        if email.contains('@') && seen.insert(email.to_lowercase()) {
            out.push(json!({"list": other, "name": if name.is_empty() { email } else { name }, "email": email, "office": office}));
        }
    };
    // Çevrimdışı adres defterindeki odalar
    let local: Vec<crate::db::DirRow> = db.directory_search(None, "", 0, 1_000_000).into_iter().filter(|r| looks_like_room(&r.name, &r.kind)).collect();
    let had_local = accounts.iter().any(|a| db.directory_count(a.id) > 0);
    for r in &local {
        push(&r.name, &r.email, &r.office, &mut out);
    }
    // Sunucu aramalarında bulunmuş odalar
    let found_key = "rooms_found";
    let mut found: Vec<Value> = db.setting(found_key).and_then(|c| serde_json::from_str(&c).ok()).unwrap_or_default();
    let found_before = found.len();
    let search = |q: &str, found: &mut Vec<Value>| {
        for acc in &accounts {
            let mut rows = mail::ews::pooled(acc, |e| e.find_people(q, 0, 200)).map(|(r, _, _)| r).unwrap_or_default();
            rows.extend(mail::ews::pooled(acc, |e| e.resolve_names_dir(q)).map(|(r, _)| r).unwrap_or_default());
            for r in rows.into_iter().filter(|r| looks_like_room(&r.name, &r.kind)) {
                let em = r.email.to_lowercase();
                if !found.iter().any(|x| x["email"].as_str().unwrap_or("").eq_ignore_ascii_case(&em)) {
                    found.push(json!({"name": r.name, "email": r.email, "office": r.office}));
                }
            }
        }
    };
    let q = query.trim();
    if q.chars().count() >= 2 && !accounts.is_empty() {
        search(q, &mut found);
    }
    // Adres defteri henüz inmemişse oda adlarıyla bir kez (günde bir) aranır
    let kw_key = "rooms_kw_at";
    let kw_due = db.setting(kw_key).and_then(|v| v.parse::<f64>().ok()).map(|a| now() - a >= 86400.0).unwrap_or(true);
    if !had_local && kw_due && !accounts.is_empty() {
        for k in ["toplantı", "toplanti", "oda", "salon", "room", "meeting"] {
            search(k, &mut found);
        }
        db.set_setting(kw_key, &now().to_string());
    }
    if found.len() != found_before {
        found.truncate(5000);
        db.set_setting(found_key, &serde_json::to_string(&found).unwrap_or_default());
    }
    for r in &found {
        push(r["name"].as_str().unwrap_or(""), r["email"].as_str().unwrap_or(""), r["office"].as_str().unwrap_or(""), &mut out);
    }
    out
}

/// Kuruluştaki toplantı odaları (Oda Bulucu, Odalar alanı)
#[tauri::command]
pub async fn list_rooms(state: State<'_, AppState>, query: Option<String>) -> R<Vec<Value>> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || Ok(all_rooms(&db, query.as_deref().unwrap_or("")))).await.map_err(err)?
}

/// Zamanlama Yardımcısı: katılımcıların ve odaların meşguliyet bilgisi.
#[tauri::command]
pub async fn free_busy(state: State<'_, AppState>, emails: Vec<String>, start: f64, end: f64, interval: Option<u32>) -> R<Value> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(acc) = db.accounts().into_iter().find(|a| a.is_exchange() && a.auth_state != "failed") else { return Ok(json!({})) };
        let iv = interval.unwrap_or(30).clamp(5, 1440);
        // Exchange tek istekte en çok 100 posta kutusu kabul eder (ör. tüm odalar sorulurken)
        let mut map = serde_json::Map::new();
        for chunk in emails.chunks(100) {
            let r = mail::ews::pooled(&acc, |e| e.free_busy(chunk, start, end, iv)).map_err(|e| e.to_string())?;
            map.extend(r.into_iter().map(|(k, v)| (k, json!(v))));
        }
        Ok(Value::Object(map))
    })
    .await
    .map_err(err)?
}

/// Posta kutusu doluluğu (varsayılan Exchange hesabı): { used, quota } bayt (quota 0 = bilinmiyor)
#[tauri::command]
pub async fn mailbox_usage(state: State<'_, AppState>) -> R<Value> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let def: Option<i64> = db.setting("default_account").and_then(|v| v.parse().ok());
        let list: Vec<Account> = db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed").collect();
        let Some(acc) = list.iter().find(|a| Some(a.id) == def).or(list.first()).cloned() else { return Ok(json!(null)) };
        let (used, quota) = mail::ews::pooled(&acc, |e| e.mailbox_usage()).map_err(|e| e.to_string())?;
        Ok(json!({"used": used, "quota": quota, "email": acc.email}))
    })
    .await
    .map_err(err)?
}

/// Zamanlama Yardımcısı (ayrıntılı): kişi başına birleşik meşguliyet ve öğeler (konu/konum
/// paylaşılıyorsa). Döner: { adres: { merged, events: [{start, end, show_as, title, location}] } }
#[tauri::command]
pub async fn free_busy_details(state: State<'_, AppState>, emails: Vec<String>, start: f64, end: f64, interval: Option<u32>) -> R<Value> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(acc) = db.accounts().into_iter().find(|a| a.is_exchange() && a.auth_state != "failed") else { return Ok(json!({})) };
        let iv = interval.unwrap_or(30).clamp(5, 1440);
        let mut map = serde_json::Map::new();
        for chunk in emails.chunks(100) {
            let r = mail::ews::pooled(&acc, |e| e.free_busy_detailed(chunk, start, end, iv)).map_err(|e| e.to_string())?;
            for (k, merged, evs) in r {
                let evs: Vec<Value> = evs
                    .into_iter()
                    .map(|e| json!({"start": e.start, "end": e.end, "show_as": e.show_as, "title": if e.is_private { String::new() } else { e.title }, "location": if e.is_private { String::new() } else { e.location }}))
                    .collect();
                map.insert(k, json!({"merged": merged, "events": evs}));
            }
        }
        Ok(Value::Object(map))
    })
    .await
    .map_err(err)?
}

// ======================================================================= yazı tipleri
/// Sistemde kurulu yazı tipi aileleri (Linux: fontconfig). İleti yazarken listelenir;
/// gönderilen HTML'de yedek yazı tipleri eklendiğinden alıcıda yoksa da bozulmaz.
#[tauri::command]
pub async fn system_fonts() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut out: Vec<String> = Vec::new();
        if let Ok(o) = std::process::Command::new("fc-list").args([":", "family"]).output() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                // "DejaVu Sans,DejaVu Sans Light" → ilk ad
                let name = line.split(',').next().unwrap_or("").trim().replace('\\', "");
                if !name.is_empty() && !name.starts_with('.') && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        #[cfg(target_os = "windows")]
        if out.is_empty() {
            out = ["Aptos", "Arial", "Bahnschrift", "Calibri", "Cambria", "Candara", "Comic Sans MS", "Consolas", "Constantia", "Corbel", "Courier New", "Franklin Gothic Medium", "Gabriola", "Garamond", "Georgia", "Impact", "Lucida Console", "Palatino Linotype", "Segoe UI", "Tahoma", "Times New Roman", "Trebuchet MS", "Verdana"].iter().map(|s| s.to_string()).collect();
        }
        out.sort_by_key(|a| a.to_lowercase());
        out
    })
    .await
    .unwrap_or_default()
}

// ======================================================================= gruplar (dağıtım listeleri)
static GROUP_CACHE: std::sync::LazyLock<Mutex<HashMap<String, bool>>> = std::sync::LazyLock::new(Default::default);

/// Verilen adreslerden hangileri grup (dağıtım listesi / kişi grubu)? Sonuç önbelleğe alınır.
#[tauri::command]
pub async fn group_addresses(state: State<'_, AppState>, emails: Vec<String>) -> R<Vec<String>> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let groups = db.groups();
        let list: Vec<String> = emails.iter().map(|e| e.trim().to_lowercase()).filter(|e| e.contains('@')).take(200).collect();
        // Önbellekte olmayanlar dizinde tek seferde ve paralel çözülür (ad da önbelleğe girer)
        let unknown: Vec<String> = list
            .iter()
            .filter(|e| !groups.iter().any(|g| g.name.to_lowercase() == **e) && !GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).contains_key(*e))
            .cloned()
            .collect();
        resolve_directory(&db, &unknown);
        let cache = GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner());
        Ok(list.into_iter().filter(|e| groups.iter().any(|g| g.name.to_lowercase() == *e) || cache.get(e).copied().unwrap_or(false)).collect())
    })
    .await
    .map_err(err)?
}

/// Grubu üyelerine açar ("Ad <adres>" listesi).
#[tauri::command]
pub async fn expand_group(state: State<'_, AppState>, email: String) -> R<Vec<String>> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(g) = db.groups().into_iter().find(|g| g.name.eq_ignore_ascii_case(&email)) {
            return Ok(mail::split_addresses(&g.members));
        }
        let accounts: Vec<Account> = db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed").collect();
        let mut last = t!("error.not_found");
        for a in accounts {
            match mail::ews::pooled(&a, |c| c.expand_dl(&email)) {
                Ok(list) => {
                    return Ok(list
                        .into_iter()
                        .map(|(n, e, k)| {
                            if k.ends_with("DL") || k == "GroupMailbox" {
                                GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(e.to_lowercase(), true);
                            }
                            if n.is_empty() || n == e { e } else { format!("{} <{e}>", n.replace(['"', ',', ';'], " ").trim()) }
                        })
                        .collect())
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(last)
    })
    .await
    .map_err(err)?
}

/// Kişi kartı (Outlook'ta ada tıklayınca): dizindeki unvan, bölüm, telefonlar; grupta üyeler.
/// Exchange'e ulaşılamazsa yerel kişilerden doldurulur.
#[tauri::command]
pub async fn contact_card(state: State<'_, AppState>, email: String) -> R<mail::ews::ContactCard> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let email = email.trim().to_string();
        let mut card: Option<mail::ews::ContactCard> = None;
        for a in db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed") {
            if let Ok(c) = mail::ews::pooled(&a, |e| e.contact_card(&email)) {
                card = c;
                if let Some(c) = card.as_mut() {
                    if c.kind.ends_with("DL") || c.kind == "GroupMailbox" {
                        GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(email.to_lowercase(), true);
                        if let Ok(list) = mail::ews::pooled(&a, |e| e.expand_dl(&c.email)) {
                            c.members = list
                                .into_iter()
                                .map(|(n, e, _)| if n.is_empty() || n == e { e } else { format!("{} <{e}>", n.replace(['"', ',', ';'], " ").trim()) })
                                .collect();
                        }
                    }
                }
                break;
            }
        }
        let mut c = card.unwrap_or_else(|| mail::ews::ContactCard { email: email.clone(), ..Default::default() });
        // Yerel kişi bilgisiyle tamamla
        if let Some(lc) = db.contact_by_email(&email) {
            if c.name.is_empty() {
                c.name = lc.display_name.clone();
            }
            if c.job_title.is_empty() {
                c.job_title = lc.job_title.clone();
            }
            if c.department.is_empty() {
                c.department = lc.department.clone();
            }
            if c.company.is_empty() {
                c.company = lc.company.clone();
            }
            if c.phones.is_empty() {
                for (k, v) in [("BusinessPhone", &lc.phone_work), ("MobilePhone", &lc.phone_mobile), ("HomePhone", &lc.phone_home)] {
                    if !v.is_empty() {
                        c.phones.push((k.to_string(), v.clone()));
                    }
                }
            }
        }
        if let Some(g) = db.groups().into_iter().find(|g| g.name.eq_ignore_ascii_case(&email)) {
            c.kind = "PrivateDL".into();
            c.members = mail::split_addresses(&g.members);
        }
        Ok(c)
    })
    .await
    .map_err(err)?
}

static NAME_CACHE: std::sync::LazyLock<Mutex<HashMap<String, String>>> = std::sync::LazyLock::new(Default::default);
/// Dizinde çözülemeyen (sunucuya ulaşılamayan) adresler ve zamanı
static DIR_FAILED: std::sync::LazyLock<Mutex<HashMap<String, std::time::Instant>>> = std::sync::LazyLock::new(Default::default);

/// Adresleri dizinde çözer (ResolveNames): görünen ad NAME_CACHE'e, grup olup olmadığı
/// GROUP_CACHE'e yazılır. Çok alıcılı iletilerde (tümünü yanıtla) sıralı sorgu saniyeler
/// sürdüğünden adresler 8 iş parçacığına bölünüp aynı anda sorulur.
fn resolve_directory(db: &Db, emails: &[String]) {
    let todo: Vec<String> = {
        let names = NAME_CACHE.lock().unwrap_or_else(|x| x.into_inner());
        let groups = GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner());
        let mut seen = std::collections::HashSet::new();
        let failed = DIR_FAILED.lock().unwrap_or_else(|x| x.into_inner());
        // Kısa süre önce sunucuya sorulup yanıt alınamayanlar hemen yeniden sorulmaz
        let recent = |e: &String| failed.get(e).is_some_and(|t: &std::time::Instant| t.elapsed() < std::time::Duration::from_secs(300));
        emails.iter().filter(|e| !(names.contains_key(*e) && groups.contains_key(*e)) && !recent(e) && seen.insert((*e).clone())).cloned().collect()
    };
    if todo.is_empty() {
        return;
    }
    // Çevrimdışı adres defterinde bulunanlar sunucuya sorulmaz
    let local = db.directory_lookup(&todo);
    for (e, r) in &local {
        let name = if r.name.is_empty() || r.name.eq_ignore_ascii_case(e) { String::new() } else { r.name.clone() };
        NAME_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(e.clone(), name);
        GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(e.clone(), r.is_group());
    }
    let todo: Vec<String> = todo.into_iter().filter(|e| !local.contains_key(e)).collect();
    if todo.is_empty() {
        return;
    }
    let accounts: Vec<Account> = db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed").collect();
    if accounts.is_empty() {
        return;
    }
    let is_dl = |k: &str| k.ends_with("DL") || k == "GroupMailbox";
    let workers = todo.len().min(8);
    let chunk = todo.len().div_ceil(workers);
    std::thread::scope(|sc| {
        for part in todo.chunks(chunk) {
            let accounts = &accounts;
            sc.spawn(move || {
                for e in part {
                    let mut answered = false;
                    for a in accounts {
                        // "smtp:" ile tam eşleşme; grubun birincil adresi farklı (takma ad) olabilir,
                        // bu yüzden tek sonuç dönerse o kullanılır
                        let Ok(r) = mail::ews::pooled(a, |c| c.resolve_names_typed(&format!("smtp:{e}"))) else { continue };
                        let hit = r.iter().find(|(_, m, _)| m.eq_ignore_ascii_case(e)).or(if r.len() == 1 { r.first() } else { None });
                        let name = hit.map(|(n, _, _)| n.clone()).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(e)).unwrap_or_default();
                        let group = hit.map(|(_, _, k)| is_dl(k)).unwrap_or(false);
                        NAME_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(e.clone(), name);
                        GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).insert(e.clone(), group);
                        answered = true;
                        break;
                    }
                    if !answered {
                        DIR_FAILED.lock().unwrap_or_else(|x| x.into_inner()).insert(e.clone(), std::time::Instant::now());
                    }
                }
            });
        }
    });
}

/// Adı olmayan adreslerin görünen adları (Outlook gibi "BS Çevresel Sistemler"):
/// kişiler, son kullanılanlar, sonra kurum dizini. Bulunamayanlar sonuçta yer almaz.
#[tauri::command]
pub async fn display_names(state: State<'_, AppState>, emails: Vec<String>) -> R<HashMap<String, String>> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let list: Vec<String> = emails.iter().map(|e| e.trim().to_lowercase()).filter(|e| e.contains('@')).take(200).collect();
        let mut out = HashMap::new();
        let mut unknown = Vec::new();
        for e in &list {
            if let Some(n) = NAME_CACHE.lock().unwrap_or_else(|x| x.into_inner()).get(e).cloned() {
                if !n.is_empty() {
                    out.insert(e.clone(), n);
                }
                continue;
            }
            if let Some(n) = db.contact_by_email(e).map(|c| c.display_name).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(e)) {
                out.insert(e.clone(), n);
                continue;
            }
            unknown.push(e.clone());
        }
        resolve_directory(&db, &unknown);
        let cache = NAME_CACHE.lock().unwrap_or_else(|x| x.into_inner());
        for e in unknown {
            if let Some(n) = cache.get(&e).filter(|n| !n.is_empty()) {
                out.insert(e, n.clone());
            }
        }
        Ok(out)
    })
    .await
    .map_err(err)?
}

// ------------------------------------------------------------------ çevrimdışı adres defteri
// Outlook'taki Çevrimdışı Adres Defteri (OAB) gibi: kurum dizini hesap başına bir kez tümüyle
// indirilir ve yerelde aranır (Ad Seç, öneriler, görünen adlar, grup bilgisi sunucuya gitmez).
// Arka planda günde bir kez sessizce yenilenir; Dosya menüsünden elle de güncellenebilir.

static DIR_SYNCING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Adres defteri biçimi: değişince (ör. kişi türü/oda bilgisi eklendi) bir kez yeniden indirilir
const DIR_FORMAT: &str = "2";
/// Adres defteri yenileme aralığı (Outlook varsayılanı gibi 24 saat)
const DIR_MAX_AGE: f64 = 24.0 * 3600.0;

/// Hesabın kurum dizinini tümüyle indirip yerel kopyayı değiştirir. Döner: kayıt sayısı.
///
/// Önce FindPeople ile sayfa sayfa denenir. Sunucu boş aramada listenin tamamını vermiyorsa
/// (çoğu Exchange boş aramada en çok 100 kayıt döndürür) dizin ad önekleriyle (a, b, … ;
/// sonuç kesilirse aa, ab, …) taranır; böylece tüm kişi ve gruplar alınır.
fn download_directory(db: &Db, acc: &Account) -> R<usize> {
    use std::collections::HashMap;
    use tauri::Emitter;
    let pause = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
    let mut all: HashMap<String, mail::ews::DirEntry> = HashMap::new();
    let add = |rows: Vec<mail::ews::DirEntry>, all: &mut HashMap<String, mail::ews::DirEntry>| {
        for d in rows {
            all.entry(d.email.trim().to_lowercase()).or_insert(d);
        }
    };

    // 1) FindPeople: sunucu toplamı bildirip sayfalamaya izin veriyorsa tek başına yeter
    let mut complete = false;
    let mut offset = 0usize;
    loop {
        match mail::ews::pooled(acc, |e| e.find_people("", offset, 500)) {
            Ok((rows, total, n)) => {
                let before = all.len();
                add(rows, &mut all);
                offset += n;
                if total > 0 && offset >= total {
                    complete = true;
                    break;
                }
                // Sayfa boş ya da sunucu aynı kayıtları döndürüyor: sayfalama desteklenmiyor
                if n == 0 || all.len() == before || offset >= 300_000 {
                    break;
                }
                pause(100);
            }
            Err(_) => break,
        }
    }

    // 2) Harf harf FindPeople araması (sayfalı). Boş aramaya izin vermeyen sunucular
    //    sorgulu aramayı genelde destekler; bu yol kişi türünü de (Room/Equipment = Exchange
    //    "Kaynaklar": toplantı odaları) ve telefon/ofis bilgisini getirir
    if !complete {
        const LETTERS: &str = "abcçdefgğhıijklmnoöpqrsştuüvwxyz0123456789";
        let mut ok = true;
        'letters: for (li, ch) in LETTERS.chars().enumerate() {
            let q = ch.to_string();
            let mut off = 0usize;
            for _page in 0..60 {
                match mail::ews::pooled(acc, |e| e.find_people(&q, off, 500)) {
                    Ok((rows, total, n)) => {
                        let before = all.len();
                        add(rows, &mut all);
                        off += n;
                        if n == 0 || (total > 0 && off >= total) || (all.len() == before && n < 500) {
                            break;
                        }
                    }
                    Err(_) if li == 0 && off == 0 => {
                        // Sorgulu FindPeople da yok: ResolveNames taramasına geçilir
                        ok = false;
                        break 'letters;
                    }
                    Err(_) => break,
                }
                pause(60);
            }
            pause(60);
        }
        complete = ok;
    }

    // 3) Ad öneki taraması (ResolveNames sonuçları 100 ile sınırlı; kesilen önek genişletilir)
    if !complete {
        const FIRST: &str = "abcçdefgğhıijklmnoöpqrsştuüvwxyz0123456789";
        const NEXT: &str = "abcçdefgğhıijklmnoöpqrsştuüvwxyz0123456789._-";
        let mut stack: Vec<String> = FIRST.chars().rev().map(|c| c.to_string()).collect();
        let mut calls = 0usize;
        while let Some(prefix) = stack.pop() {
            // Geçici ağ hatasında bir kez daha denenir; yine olmazsa eldeki kopya korunur
            let res = mail::ews::pooled(acc, |e| e.resolve_names_dir(&prefix)).or_else(|_| {
                pause(1500);
                mail::ews::pooled(acc, |e| e.resolve_names_dir(&prefix))
            });
            let (rows, truncated) = res.map_err(|e| e.to_string())?;
            add(rows, &mut all);
            if truncated && prefix.chars().count() < 8 {
                for c in NEXT.chars().rev() {
                    stack.push(format!("{prefix}{c}"));
                }
            }
            calls += 1;
            if calls >= 20_000 {
                break;
            }
            pause(40);
        }
    }

    // Boş liste (sunucu dizine izin vermiyor) eldeki kopyayı silmez
    if all.is_empty() {
        return Err(t!("error.not_found"));
    }
    let rows: Vec<crate::db::DirRow> = all
        .into_values()
        .map(|d| crate::db::DirRow { email: d.email, name: d.name, title: d.title, department: d.department, office: d.office, phone: d.phone, kind: d.kind })
        .collect();
    db.replace_directory(acc.id, &rows);
    db.set_setting(&format!("dir_synced_{}", acc.id), &crate::db::now().to_string());
    db.set_setting(&format!("dir_fmt_{}", acc.id), DIR_FORMAT);
    // Ad Seç'in eski geçici listesi artık gereksiz
    db.set_setting(&format!("dir_cache_{}", acc.id), "");
    // Yeni bilgiler önbellekteki eski adları/grupları geçersiz kılar
    NAME_CACHE.lock().unwrap_or_else(|x| x.into_inner()).clear();
    GROUP_CACHE.lock().unwrap_or_else(|x| x.into_inner()).clear();
    // Açık Ad Seç pencereleri listeyi yerel kopyadan yeniler
    if let Some(app) = crate::APP.get() {
        let _ = app.emit("directory://updated", acc.id);
    }
    Ok(rows.len())
}

/// Süresi dolan hesapların adres defterini arka planda (tek iş parçacığında) yeniler
pub fn refresh_directories_if_due(db: &Db) {
    if DIR_SYNCING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let db = db.clone();
    std::thread::spawn(move || {
        for acc in db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed") {
            // Eski biçimde (kişi türü olmadan) indirilmiş adres defteri hemen yenilenir
            let fresh_format = db.setting(&format!("dir_fmt_{}", acc.id)).as_deref() == Some(DIR_FORMAT);
            let last: f64 = if fresh_format { db.setting(&format!("dir_synced_{}", acc.id)).and_then(|v| v.parse().ok()).unwrap_or(0.0) } else { 0.0 };
            // Başarısız deneme bir saat tekrarlanmaz (sunucuyu yormamak için)
            let failed: f64 = db.setting(&format!("dir_failed_{}", acc.id)).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            if crate::db::now() - last >= DIR_MAX_AGE && crate::db::now() - failed >= 3600.0 && download_directory(&db, &acc).is_err() {
                db.set_setting(&format!("dir_failed_{}", acc.id), &crate::db::now().to_string());
            }
        }
        DIR_SYNCING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// Dosya > Adres Defterini Güncelle: tüm Exchange hesaplarının adres defterini şimdi indirir
#[tauri::command]
pub async fn refresh_address_book(state: State<'_, AppState>) -> R<usize> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        while DIR_SYNCING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let mut total = 0;
        let mut last_err = None;
        for acc in db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed") {
            match download_directory(&db, &acc) {
                Ok(n) => total += n,
                Err(e) => last_err = Some(e),
            }
        }
        DIR_SYNCING.store(false, std::sync::atomic::Ordering::SeqCst);
        match (total, last_err) {
            (0, Some(e)) => Err(e),
            _ => Ok(total),
        }
    })
    .await
    .map_err(err)?
}

/// Adres defteri durumu (Dosya menüsü): son güncelleme zamanı, kayıt sayısı, güncelleniyor mu
#[tauri::command(async)]
pub fn address_book_status(state: State<AppState>) -> Value {
    let db = &state.db;
    let mut synced = 0.0f64;
    let mut count = 0i64;
    for a in db.accounts().into_iter().filter(|a| a.is_exchange()) {
        count += db.directory_count(a.id);
        let t: f64 = db.setting(&format!("dir_synced_{}", a.id)).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        synced = synced.max(t);
    }
    json!({"synced": synced, "count": count, "busy": DIR_SYNCING.load(std::sync::atomic::Ordering::SeqCst)})
}

fn dir_json(r: &crate::db::DirRow) -> Value {
    json!({"name": r.name, "email": r.email, "title": r.title, "department": r.department, "office": r.office, "phone": r.phone, "kind": r.kind})
}

/// "Ad Seç" penceresi: kurum dizini (sayfalı), kişiler ya da oda listesi.
#[tauri::command]
pub async fn directory_page(state: State<'_, AppState>, source: String, query: String, offset: usize) -> R<Value> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let q = crate::db::fold(query.trim());
        let matches = |s: &[&str]| q.is_empty() || s.iter().any(|x| crate::db::fold(x).contains(&q));
        match source.as_str() {
            "contacts" => {
                let rows: Vec<Value> = db
                    .contacts("")
                    .into_iter()
                    .filter(|c| !c.email.is_empty() && matches(&[&c.display_name, &c.email, &c.department, &c.job_title]))
                    .skip(offset)
                    .take(200)
                    .map(|c| json!({"name": c.display_name, "email": c.email, "title": c.job_title, "department": c.department, "office": c.company, "phone": c.phone_work, "kind": "Contact"}))
                    .collect();
                Ok(json!({"rows": rows, "more": false}))
            }
            "rooms" => {
                let rooms = all_rooms(&db, &query);
                // Outlook "Oda Seç": arama sözcükleri her sırada eşleşir ("ZK IFM" ~ "ZK IFM 30.KAT - ...")
                let words: Vec<String> = q.split_whitespace().map(String::from).collect();
                let rows: Vec<Value> = rooms
                    .into_iter()
                    .filter(|r| {
                        let hay = crate::db::fold(&format!("{} {} {}", r["name"].as_str().unwrap_or(""), r["email"].as_str().unwrap_or(""), r["office"].as_str().unwrap_or("")));
                        words.iter().all(|w| hay.contains(w.as_str()))
                    })
                    .map(|r| json!({"name": r["name"], "email": r["email"], "title": "", "department": r["list"], "office": r["office"].as_str().unwrap_or(""), "phone": "", "kind": "Room"}))
                    .collect();
                Ok(json!({"rows": rows, "more": false}))
            }
            _ => {
                // "gal" ya da "gal:<hesap>" (Outlook: her hesabın kendi Genel Adres Listesi)
                let want: Option<i64> = source.split_once(':').and_then(|(_, id)| id.parse().ok());
                let acc = db
                    .accounts()
                    .into_iter()
                    .filter(|a| a.is_exchange() && a.auth_state != "failed")
                    .find(|a| want.map_or(true, |w| w == a.id))
                    .ok_or_else(|| t!("error.no_account"))?;
                // Çevrimdışı adres defteri varsa arama yerelde (anında, sunucuya gitmez)
                if db.directory_count(acc.id) > 0 {
                    let rows: Vec<Value> = db.directory_search(Some(acc.id), &query, offset, 200).iter().map(dir_json).collect();
                    let more = rows.len() == 200;
                    return Ok(json!({"rows": rows, "more": more}));
                }
                // Henüz indirilmemiş: tüm adres defteri arka planda indirilip yerel kopyaya
                // kaydedilir; bu arada liste sunucudan gelir, bitince pencere yerelden yenilenir
                refresh_directories_if_due(&db);
                let key = format!("dir_cache_{}", acc.id);
                let cached = || -> Vec<Value> { db.setting(&key).and_then(|c| serde_json::from_str::<Vec<Value>>(&c).ok()).unwrap_or_default() };
                match mail::ews::pooled(&acc, |e| e.directory_page(&query, offset, 100)) {
                    Ok((rows, more)) => {
                        // Görülen dizin kayıtları saklanır: sonraki açılışta liste hemen dolu gelir
                        let mut all = cached();
                        for r in serde_json::to_value(&rows).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default() {
                            let em = r["email"].as_str().unwrap_or("").to_lowercase();
                            if !all.iter().any(|x| x["email"].as_str().unwrap_or("").to_lowercase() == em) {
                                all.push(r);
                            }
                        }
                        all.sort_by_key(|x| x["name"].as_str().unwrap_or("").to_lowercase());
                        all.truncate(5000);
                        db.set_setting(&key, &serde_json::to_string(&all).unwrap_or_default());
                        Ok(json!({"rows": rows, "more": more}))
                    }
                    Err(e) => {
                        // Sunucuya ulaşılamıyor ya da boş aramaya izin vermiyor: saklanan liste
                        let rows: Vec<Value> = cached()
                            .into_iter()
                            .filter(|r| matches(&[r["name"].as_str().unwrap_or(""), r["email"].as_str().unwrap_or(""), r["department"].as_str().unwrap_or("")]))
                            .skip(offset)
                            .take(200)
                            .collect();
                        if rows.is_empty() && offset == 0 {
                            return Err(e.to_string());
                        }
                        Ok(json!({"rows": rows, "more": false}))
                    }
                }
            }
        }
    })
    .await
    .map_err(err)?
}

/// İnternet takvimi (Outlook "Takvim Ekle > İnternet'ten"): .ics adresinden yüklenir,
/// takvim açıldıkça yenilenir. webcal:// adresleri https olarak okunur.
fn fetch_internet_calendar(db: &Db, cal: &Calendar) -> R<usize> {
    let url = cal.owner.trim().replacen("webcal://", "https://", 1);
    let mut client = mail::http::Client::new(mail::http::Auth::None);
    client.timeout = std::time::Duration::from_secs(30);
    let r = client.request("GET", &url, "", &[]).map_err(|e| e.to_string())?;
    if r.status >= 400 {
        return Err(format!("HTTP {}", r.status));
    }
    let imp = ical::parse_ics(&String::from_utf8_lossy(&r.body));
    // Takvimin önceki içeriği yenisiyle değiştirilir
    for e in db.events_in_calendar(cal.id) {
        db.delete_event(e.id);
    }
    let mut n = 0;
    for mut e in imp.events {
        e.id = 0;
        e.calendar_id = cal.id;
        e.uid = format!("{}#{}", cal.id, e.uid);
        e.remote_id = None;
        e.master_id = None;
        if db.save_event(&e).is_ok() {
            n += 1;
        }
    }
    db.set_setting(&format!("internet_cal_at_{}", cal.id), &now().to_string());
    Ok(n)
}

#[tauri::command]
pub async fn add_internet_calendar(state: State<'_, AppState>, url: String, name: String, grp: String) -> R<usize> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let u = url.trim().to_string();
        if !(u.starts_with("http://") || u.starts_with("https://") || u.starts_with("webcal://")) {
            return Err(t!("calendar.bad_url"));
        }
        let name = if name.trim().is_empty() { u.split('/').next_back().unwrap_or("Internet").trim_end_matches(".ics").to_string() } else { name.trim().to_string() };
        let id = db.save_calendar(&Calendar { name, color: "#038387".into(), visible: true, owner: u, grp, kind: "internet".into(), ..Default::default() }).map_err(err)?;
        let cal = db.calendars().into_iter().find(|c| c.id == id).ok_or("?")?;
        match fetch_internet_calendar(&db, &cal) {
            Ok(n) => Ok(n),
            Err(e) => {
                db.delete_calendar(id);
                Err(e)
            }
        }
    })
    .await
    .map_err(err)?
}

/// Görünür İnternet takvimlerini (en çok saatte bir) yeniler
#[tauri::command]
pub async fn refresh_internet_calendars(state: State<'_, AppState>, force: Option<bool>) -> R<usize> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut n = 0;
        for c in db.calendars().into_iter().filter(|c| c.kind == "internet" && c.visible) {
            let at: f64 = db.setting(&format!("internet_cal_at_{}", c.id)).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            if force != Some(true) && now() - at < 3600.0 {
                continue;
            }
            if fetch_internet_calendar(&db, &c).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    })
    .await
    .map_err(err)?
}

/// Arayüz ölçeği (Ayarlar > Genel > Yazı ve öğe boyutu): yeni pencere boyutları için
#[tauri::command(async)]
pub fn set_ui_scale(scale: f64) {
    crate::windows::set_ui_scale(scale);
}

/// Pencereyi bulunduğu ekrana göre yeniden boyutlandırır (açılışta ve çözünürlük değişince)
#[tauri::command(async)]
pub fn fit_window(window: tauri::WebviewWindow) {
    let kind = crate::windows::kind_of(window.label());
    crate::windows::place_window(&window, &kind);
}

/// Klasördeki iletilerin sırası (ileti penceresinde Önceki/Sonraki)
#[tauri::command(async)]
pub fn sibling_ids(state: State<AppState>, folder_id: i64) -> Vec<i64> {
    state.db.message_ids_ordered(folder_id)
}

/// Ekranın mantıksal boyutu (otomatik arayüz ölçeği için)
#[tauri::command(async)]
pub fn screen_info(window: tauri::WebviewWindow) -> Option<(f64, f64)> {
    crate::windows::screen_size(&window)
}
