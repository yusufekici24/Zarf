//! SQLite tabanlı yerel veri deposu (SQLite ikili dosyaya gömülüdür).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub type Result<T> = rusqlite::Result<T>;

/// İleti tablosunun sütunları: yalnızca küçük alanlar. Gövdeler ve özgün ileti `message_content`
/// tablosundadır: satırın ortasındaki büyük bir alanın ardındaki sütunu (ör. remote_id) okumak
/// için SQLite tüm taşma sayfalarını yürür; liste ve taramalar böylece ileti boyutundan bağımsızdır.
/// 1.2.0 geçişi yeni tabloyu da bununla kurar.
macro_rules! messages_columns {
    () => {
        "id INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id INTEGER NOT NULL, account_id INTEGER, uid INTEGER,
    message_id TEXT DEFAULT '', in_reply_to TEXT DEFAULT '', refs TEXT DEFAULT '',
    subject TEXT DEFAULT '', sender TEXT DEFAULT '', to_addr TEXT DEFAULT '',
    cc TEXT DEFAULT '', bcc TEXT DEFAULT '', date REAL DEFAULT 0,
    snippet TEXT DEFAULT '', is_read INTEGER DEFAULT 0, is_flagged INTEGER DEFAULT 0,
    is_draft INTEGER DEFAULT 0, is_answered INTEGER DEFAULT 0, importance INTEGER DEFAULT 1,
    categories TEXT DEFAULT '', has_attachments INTEGER DEFAULT 0, pending TEXT DEFAULT '',
    FOREIGN KEY(folder_id) REFERENCES folders(id) ON DELETE CASCADE"
    };
}

const SCHEMA: &str = concat!(
    r#"
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL, email TEXT NOT NULL, display_name TEXT DEFAULT '',
    imap_host TEXT DEFAULT '', imap_port INTEGER DEFAULT 993, imap_security TEXT DEFAULT 'ssl',
    smtp_host TEXT DEFAULT '', smtp_port INTEGER DEFAULT 465, smtp_security TEXT DEFAULT 'ssl',
    username TEXT DEFAULT '', password TEXT DEFAULT '', signature TEXT DEFAULT '',
    sync_limit INTEGER DEFAULT 500, auto_reply_enabled INTEGER DEFAULT 0,
    auto_reply_text TEXT DEFAULT '', created REAL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER, name TEXT NOT NULL, remote_name TEXT, role TEXT,
    parent_id INTEGER, uidvalidity INTEGER DEFAULT 0, position INTEGER DEFAULT 100,
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS messages ("#,
    messages_columns!(),
    r#");
CREATE INDEX IF NOT EXISTS idx_messages_folder ON messages(folder_id, date);
CREATE INDEX IF NOT EXISTS idx_messages_uid ON messages(folder_id, uid);
CREATE INDEX IF NOT EXISTS idx_messages_mid ON messages(message_id);
CREATE TABLE IF NOT EXISTS message_content (
    id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    body_text TEXT NOT NULL DEFAULT '', body_html TEXT NOT NULL DEFAULT '', raw BLOB
);
CREATE TABLE IF NOT EXISTS pending_flags (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    field TEXT NOT NULL, value INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(message_id, field)
);
CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    first_name TEXT DEFAULT '', last_name TEXT DEFAULT '', display_name TEXT DEFAULT '',
    email TEXT DEFAULT '', email2 TEXT DEFAULT '', phone_mobile TEXT DEFAULT '',
    phone_work TEXT DEFAULT '', phone_home TEXT DEFAULT '', company TEXT DEFAULT '',
    job_title TEXT DEFAULT '', department TEXT DEFAULT '', address TEXT DEFAULT '',
    website TEXT DEFAULT '', birthday TEXT DEFAULT '', notes TEXT DEFAULT '',
    categories TEXT DEFAULT '', is_favorite INTEGER DEFAULT 0,
    created REAL DEFAULT 0, modified REAL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS contact_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, members TEXT DEFAULT ''
);
CREATE TABLE IF NOT EXISTS calendars (
    id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
    color TEXT DEFAULT '#3a7bd5', visible INTEGER DEFAULT 1
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    calendar_id INTEGER DEFAULT 1, uid TEXT DEFAULT '', title TEXT DEFAULT '',
    location TEXT DEFAULT '', description TEXT DEFAULT '',
    start REAL NOT NULL, end REAL NOT NULL, all_day INTEGER DEFAULT 0,
    recurrence TEXT DEFAULT 'none', recur_interval INTEGER DEFAULT 1, recur_until REAL,
    exdates TEXT DEFAULT '', reminder_minutes INTEGER DEFAULT 15, category TEXT DEFAULT '',
    show_as TEXT DEFAULT 'busy', attendees TEXT DEFAULT '', is_private INTEGER DEFAULT 0,
    created REAL DEFAULT 0, modified REAL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_events_start ON events(start);
CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT DEFAULT '', notes TEXT DEFAULT '', start_date REAL, due_date REAL,
    reminder REAL, reminder_done INTEGER DEFAULT 0, priority INTEGER DEFAULT 1,
    status TEXT DEFAULT 'not_started', percent INTEGER DEFAULT 0, category TEXT DEFAULT '',
    message_id INTEGER, completed_at REAL, created REAL DEFAULT 0, modified REAL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    body TEXT DEFAULT '', color TEXT DEFAULT 'yellow', category TEXT DEFAULT '',
    created REAL DEFAULT 0, modified REAL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL, enabled INTEGER DEFAULT 1, account_id INTEGER,
    field TEXT DEFAULT 'from', op TEXT DEFAULT 'contains', value TEXT DEFAULT '',
    action TEXT DEFAULT 'move', action_arg TEXT DEFAULT '', stop INTEGER DEFAULT 0,
    position INTEGER DEFAULT 0
);
CREATE TABLE IF NOT EXISTS categories (name TEXT PRIMARY KEY, color TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS dismissed_reminders (key TEXT PRIMARY KEY, until REAL);
CREATE TABLE IF NOT EXISTS recipients (
    email TEXT PRIMARY KEY, name TEXT DEFAULT '', count INTEGER DEFAULT 0, last_used REAL DEFAULT 0
);
"#
);

/// Aramada ileti gövdesi (WHERE içinde `FROM messages` satırına göre)
const BODY_TEXT_SQL: &str = "(SELECT body_text FROM message_content WHERE message_content.id = messages.id)";

/// Tam metin arama dizini (FTS5). Metinler aramayla aynı katlamayla (`lyn_fold`: büyük/küçük
/// harf, Türkçe İ/ı, aksan) dizinlenir; içerik ikinci kez saklanmaz, yalnızca dizin tutulur.
/// Böyle bir dizinden satır, dizinlenmiş değerleri yeniden verilerek ('delete' komutu) çıkarılır:
/// tetikleyiciler bu yüzden eski değerleri katlar (ileti silinirken gövdesi henüz yerindedir;
/// gövde UPSERT ile yazılır, eskisi görülür). `contentless_delete` kipi bu iş yükünde (her
/// iletide çıkar + ekle) dizin büyüdükçe çok yavaşlıyor (30 bin iletide ileti başına 1 ms'den
/// 26 ms'ye); bu yol sabit kalır (~0,5 ms). Eski iletiler `build_search_index` ile arka planda
/// eklenir: henüz eklenmemiş aralıktaki (`fts_build`) iletileri tetikleyiciler atlar, kurucu
/// onları o anki hâlleriyle ekler. Sütun adları aramadaki alan adlarıdır.
/// Katlama ya da sözcük ayırma değişirse FTS_VERSION artırılır: dizin baştan kurulur.
const FTS_VERSION: &str = "1";
macro_rules! fts_row {
    ($m:literal, $body:literal) => {
        concat!(
            "lyn_fold(", $m, ".subject), lyn_fold(", $m, ".sender), lyn_fold(", $m, ".to_addr), lyn_fold(", $m, ".cc), lyn_fold(",
            $body, "), lyn_fold(", $m, ".categories), lyn_fold(", $m, ".att_names)"
        )
    };
}
macro_rules! fts_insert {
    () => {
        "INSERT INTO message_fts(rowid, subject, sender, to_addr, cc, body_text, categories, att_names)"
    };
}
macro_rules! fts_delete {
    () => {
        "INSERT INTO message_fts(message_fts, rowid, subject, sender, to_addr, cc, body_text, categories, att_names)"
    };
}
/// Satır dizinin bakımında mı (arka plan kurulumunun henüz gelmediği aralıkta değil)
macro_rules! fts_live {
    ($id:literal) => {
        concat!("NOT EXISTS (SELECT 1 FROM fts_build WHERE ", $id, " > pos AND ", $id, " <= last)")
    };
}
const FTS_TRIGGERS: &[&str] = &["message_fts_ins", "message_fts_upd", "message_fts_del", "message_fts_body", "message_fts_body_upd"];
const FTS_SCHEMA: &str = concat!(
    r#"
CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
    subject, sender, to_addr, cc, body_text, categories, att_names, content = '',
    tokenize = "unicode61 remove_diacritics 0 categories 'L* N* Co M*'"
);
CREATE TABLE IF NOT EXISTS fts_build (pos INTEGER NOT NULL, last INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS message_fts_ins AFTER INSERT ON messages WHEN "#,
    fts_live!("new.id"),
    " BEGIN\n    ",
    fts_insert!(),
    " VALUES (new.id, ",
    fts_row!("new", "''"),
    r#");
END;
CREATE TRIGGER IF NOT EXISTS message_fts_upd AFTER UPDATE OF subject, sender, to_addr, cc, categories, att_names ON messages
WHEN (old.subject IS NOT new.subject OR old.sender IS NOT new.sender OR old.to_addr IS NOT new.to_addr
    OR old.cc IS NOT new.cc OR old.categories IS NOT new.categories OR old.att_names IS NOT new.att_names) AND "#,
    fts_live!("new.id"),
    " BEGIN\n    ",
    fts_delete!(),
    " VALUES ('delete', old.id, ",
    fts_row!("old", "(SELECT body_text FROM message_content WHERE id = old.id)"),
    ");\n    ",
    fts_insert!(),
    " VALUES (new.id, ",
    fts_row!("new", "(SELECT body_text FROM message_content WHERE id = new.id)"),
    r#");
END;
CREATE TRIGGER IF NOT EXISTS message_fts_del BEFORE DELETE ON messages WHEN "#,
    fts_live!("old.id"),
    " BEGIN\n    ",
    fts_delete!(),
    " VALUES ('delete', old.id, ",
    fts_row!("old", "(SELECT body_text FROM message_content WHERE id = old.id)"),
    r#");
END;
CREATE TRIGGER IF NOT EXISTS message_fts_body AFTER INSERT ON message_content WHEN "#,
    fts_live!("new.id"),
    " BEGIN\n    ",
    fts_delete!(),
    " SELECT 'delete', m.id, ",
    fts_row!("m", "''"),
    " FROM messages m WHERE m.id = new.id;\n    ",
    fts_insert!(),
    " SELECT m.id, ",
    fts_row!("m", "new.body_text"),
    r#" FROM messages m WHERE m.id = new.id;
END;
CREATE TRIGGER IF NOT EXISTS message_fts_body_upd AFTER UPDATE OF body_text ON message_content
WHEN old.body_text IS NOT new.body_text AND "#,
    fts_live!("new.id"),
    " BEGIN\n    ",
    fts_delete!(),
    " SELECT 'delete', m.id, ",
    fts_row!("m", "old.body_text"),
    " FROM messages m WHERE m.id = new.id;\n    ",
    fts_insert!(),
    " SELECT m.id, ",
    fts_row!("m", "new.body_text"),
    r#" FROM messages m WHERE m.id = new.id;
END;
"#
);

/// Okuma bağlantısı sayısı: arayüz okumaları arka plan okumalarını (eşitleme, ek adı
/// doldurma, gövde araması) beklemesin
const READERS: usize = 3;

/// Yavaş sorgu günlüğü: bu süreyi aşan SQL ifadeleri veri klasöründeki `slow-queries.log`'a
/// yazılır (kullanıcının makinesindeki yavaşlık tahminle değil günlükten teşhis edilsin).
/// Yalnızca ifadenin metni yazılır, parametreler (ileti içeriği, adresler) yazılmaz.
const SLOW_QUERY_MS: u128 = 200;
/// Günlük bu boyutu aşınca `.1` uzantısıyla kenara alınır (en çok iki dosya)
const SLOW_LOG_MAX: u64 = 1 << 20;
static SLOW_LOG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Günlük satırı (eşiğin altındaysa None): zaman, süre, iş parçacığı ve tek satıra indirilmiş SQL
fn slow_query_line(sql: &str, d: std::time::Duration) -> Option<String> {
    if d.as_millis() < SLOW_QUERY_MS {
        return None;
    }
    let mut sql = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((i, _)) = sql.char_indices().nth(600) {
        sql.truncate(i);
        sql.push('…');
    }
    let thread = std::thread::current().name().unwrap_or("?").to_string();
    Some(format!("[{}] {} ms [{thread}] {sql}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), d.as_millis()))
}

fn append_slow_log(path: &std::path::Path, line: &str) {
    use std::io::Write;
    if std::fs::metadata(path).is_ok_and(|m| m.len() > SLOW_LOG_MAX) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// SQLite'ın her ifade bitince çağırdığı ölçüm (bağlantı başına `Connection::profile`)
fn log_slow_query(sql: &str, d: std::time::Duration) {
    if let (Some(path), Some(line)) = (SLOW_LOG.get(), slow_query_line(sql, d)) {
        append_slow_log(path, &line);
    }
}

/// Sonradan eklenen sütunlar (eski veritabanlarını yükseltmek için).
const MIGRATIONS: &[(&str, &str, &str)] = &[
    ("accounts", "kind", "TEXT DEFAULT 'imap'"),
    ("accounts", "caldav_url", "TEXT DEFAULT ''"),
    ("accounts", "carddav_url", "TEXT DEFAULT ''"),
    ("events", "etag", "TEXT DEFAULT ''"),
    ("events", "dav_raw", "TEXT DEFAULT ''"),
    ("events", "dirty", "INTEGER DEFAULT 0"),
    ("contacts", "etag", "TEXT DEFAULT ''"),
    ("contacts", "dav_raw", "TEXT DEFAULT ''"),
    ("contacts", "dirty", "INTEGER DEFAULT 0"),
    ("accounts", "auth_method", "TEXT DEFAULT 'password'"),
    ("accounts", "ews_url", "TEXT DEFAULT ''"),
    ("accounts", "auth_state", "TEXT DEFAULT 'ok'"),
    ("accounts", "auth_error", "TEXT DEFAULT ''"),
    ("accounts", "oauth_token", "TEXT DEFAULT ''"),
    ("messages", "remote_id", "TEXT"),
    ("folders", "sync_state", "TEXT DEFAULT ''"),
    ("folders", "favorite", "INTEGER DEFAULT 0"),
    ("folders", "full_sync", "INTEGER DEFAULT 0"),
    ("messages", "is_pinned", "INTEGER DEFAULT 0"),
    // Eklerin adları (satır başına bir ad): aramada ek önerileri ve "ekadı:" araması
    ("messages", "att_names", "TEXT DEFAULT ''"),
    // "RE:/YNT:" önekleri atılmış, küçük harfli konu (konuşma görünümü ve konuya göre sıralama
    // dizinden yapılır; her satırda konu yeniden çözülmez)
    ("messages", "ns", "TEXT"),
    // Konuşma: Exchange'in konuşma kimliği (yoksa boş) ve iletinin bağlandığı konuşma numarası
    // (bkz. assign_thread)
    ("messages", "conv", "TEXT NOT NULL DEFAULT ''"),
    ("messages", "tid", "INTEGER"),
    ("rules", "remote_id", "TEXT"),
    ("rules", "remote_xml", "TEXT DEFAULT ''"),
    ("rules", "complex", "INTEGER DEFAULT 0"),
    // Yerel kuralın tam modeli (JSON): boşsa basit alan/işlem/değer kullanılır
    ("rules", "spec", "TEXT DEFAULT ''"),
    // Paylaşılan posta kutusu: kimliği kullanılan ana hesap
    ("accounts", "shared_of", "INTEGER"),
    // Exchange ile eşitlenen görev, not ve kişi grupları
    ("tasks", "account_id", "INTEGER"),
    ("tasks", "remote_id", "TEXT"),
    ("tasks", "dirty", "INTEGER DEFAULT 0"),
    ("notes", "account_id", "INTEGER"),
    ("notes", "remote_id", "TEXT"),
    ("notes", "dirty", "INTEGER DEFAULT 0"),
    ("contact_groups", "account_id", "INTEGER"),
    ("contact_groups", "remote_id", "TEXT"),
    ("contact_groups", "dirty", "INTEGER DEFAULT 0"),
    ("calendars", "account_id", "INTEGER"),
    ("calendars", "remote_id", "TEXT"),
    ("events", "remote_id", "TEXT"),
    ("events", "master_id", "TEXT"),
    ("contacts", "account_id", "INTEGER"),
    ("contacts", "remote_id", "TEXT"),
    ("events", "optional_attendees", "TEXT DEFAULT ''"),
    ("events", "resources", "TEXT DEFAULT ''"),
    ("events", "organizer", "TEXT DEFAULT ''"),
    ("calendars", "owner", "TEXT DEFAULT ''"),
    ("calendars", "grp", "TEXT DEFAULT ''"),
    ("calendars", "kind", "TEXT DEFAULT ''"),
    ("events", "is_cancelled", "INTEGER DEFAULT 0"),
    ("events", "is_meeting", "INTEGER DEFAULT 0"),
    ("events", "response", "TEXT DEFAULT ''"),
    // Exchange ChangeKey: değişmeyen toplantının ayrıntıları (açıklama, katılımcılar) yeniden istenmez
    ("events", "change_key", "TEXT DEFAULT ''"),
];

pub const ROLES: [&str; 7] = ["inbox", "drafts", "sent", "outbox", "archive", "junk", "trash"];

pub fn role_order(role: &str) -> i32 {
    ROLES.iter().position(|r| *r == role).map(|p| p as i32).unwrap_or(50)
}

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ======================================================================= modeller
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_security: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub signature: String,
    pub sync_limit: i64,
    pub auto_reply_enabled: bool,
    pub auto_reply_text: String,
    /// "imap" veya "exchange" (EWS)
    pub kind: String,
    /// "password", "kerberos" (tek oturum açma), "ntlm", "oauth"
    pub auth_method: String,
    pub ews_url: String,
    /// "ok" veya "failed" (hesap kilitlenmesini önlemek için denemeler durdurulur)
    pub auth_state: String,
    pub auth_error: String,
    #[serde(skip_serializing)]
    pub oauth_token: String,
    /// Paylaşılan posta kutusu: kimliği kullanılan ana hesap
    pub shared_of: Option<i64>,
    /// Takvim sunucusu (boşsa takvim eşitlenmez)
    pub caldav_url: String,
    /// Adres defteri sunucusu
    pub carddav_url: String,
}

impl Account {
    pub fn is_exchange(&self) -> bool {
        self.kind == "exchange"
    }

    pub fn uses_kerberos(&self) -> bool {
        self.auth_method == "kerberos"
    }

    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            name: r.get("name")?,
            email: r.get("email")?,
            display_name: r.get("display_name")?,
            imap_host: r.get("imap_host")?,
            imap_port: r.get("imap_port")?,
            imap_security: r.get("imap_security")?,
            smtp_host: r.get("smtp_host")?,
            smtp_port: r.get("smtp_port")?,
            smtp_security: r.get("smtp_security")?,
            username: r.get("username")?,
            password: r.get("password")?,
            signature: r.get("signature")?,
            sync_limit: r.get("sync_limit")?,
            auto_reply_enabled: r.get::<_, i64>("auto_reply_enabled")? != 0,
            auto_reply_text: r.get("auto_reply_text")?,
            kind: r.get::<_, Option<String>>("kind")?.unwrap_or_else(|| "imap".into()),
            auth_method: r.get::<_, Option<String>>("auth_method")?.unwrap_or_else(|| "password".into()),
            ews_url: r.get::<_, Option<String>>("ews_url")?.unwrap_or_default(),
            auth_state: r.get::<_, Option<String>>("auth_state")?.unwrap_or_else(|| "ok".into()),
            auth_error: r.get::<_, Option<String>>("auth_error")?.unwrap_or_default(),
            oauth_token: r.get::<_, Option<String>>("oauth_token")?.unwrap_or_default(),
            shared_of: r.get("shared_of").unwrap_or(None),
            caldav_url: r.get::<_, Option<String>>("caldav_url")?.unwrap_or_default(),
            carddav_url: r.get::<_, Option<String>>("carddav_url")?.unwrap_or_default(),
        })
    }

    pub fn login(&self) -> &str {
        if self.username.is_empty() {
            &self.email
        } else {
            &self.username
        }
    }

    pub fn from_header(&self) -> String {
        if self.display_name.is_empty() {
            self.email.clone()
        } else {
            format!("{} <{}>", self.display_name, self.email)
        }
    }

    pub fn password_plain(&self) -> String {
        crate::secret::open(&self.password)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub id: i64,
    pub account_id: Option<i64>,
    pub name: String,
    pub remote_name: Option<String>,
    pub role: Option<String>,
    pub parent_id: Option<i64>,
    pub uidvalidity: i64,
    pub position: i64,
    pub sync_state: String,
    pub favorite: bool,
    /// Sunucudaki tüm öğe başlıkları alındı (Exchange)
    pub full_sync: bool,
}

impl Folder {
    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            account_id: r.get("account_id")?,
            name: r.get("name")?,
            remote_name: r.get("remote_name")?,
            role: r.get("role")?,
            parent_id: r.get("parent_id")?,
            uidvalidity: r.get("uidvalidity")?,
            position: r.get("position")?,
            sync_state: r.get::<_, Option<String>>("sync_state")?.unwrap_or_default(),
            favorite: r.get::<_, Option<i64>>("favorite")?.unwrap_or(0) != 0,
            full_sync: r.get::<_, Option<i64>>("full_sync").unwrap_or(None).unwrap_or(0) != 0,
        })
    }

    pub fn role_str(&self) -> &str {
        self.role.as_deref().unwrap_or("")
    }

    /// Rol klasörlerinin adı her zaman seçili dilde gösterilir.
    pub fn display_name(&self) -> String {
        match self.role.as_deref() {
            Some(r) if !r.is_empty() => crate::i18n::tr(&format!("folder.{r}")),
            _ => self.name.clone(),
        }
    }
}

/// İleti listesi satırı (gövde ve ham veri olmadan).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageRow {
    pub id: i64,
    pub folder_id: i64,
    pub account_id: Option<i64>,
    pub uid: Option<i64>,
    pub subject: String,
    pub sender: String,
    pub to_addr: String,
    pub date: f64,
    pub snippet: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub is_draft: bool,
    pub is_answered: bool,
    pub importance: i64,
    pub categories: String,
    pub has_attachments: bool,
    pub message_id: String,
    pub in_reply_to: String,
    pub remote_id: Option<String>,
    /// Sabitlenmiş (Outlook "Sabitle"): listede en üstte
    pub is_pinned: bool,
    /// Konuşmadaki ileti sayısı (tüm klasörlerde; çöp ve gereksiz hariç)
    pub thread_count: i64,
    /// Konuşma numarası (aynı konuşmadaki iletilerde aynıdır)
    pub thread_id: i64,
    /// Konuşma görünümünde: listelenen klasörde bu konuşmadaki okunmamış ileti sayısı
    pub thread_unread: i64,
}

impl MessageRow {
    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            folder_id: r.get("folder_id")?,
            account_id: r.get("account_id")?,
            uid: r.get("uid")?,
            subject: r.get("subject")?,
            sender: r.get("sender")?,
            to_addr: r.get("to_addr")?,
            date: r.get("date")?,
            snippet: r.get("snippet")?,
            is_read: r.get::<_, i64>("is_read")? != 0,
            is_flagged: r.get::<_, i64>("is_flagged")? != 0,
            is_draft: r.get::<_, i64>("is_draft")? != 0,
            is_answered: r.get::<_, i64>("is_answered")? != 0,
            importance: r.get("importance")?,
            categories: r.get("categories")?,
            has_attachments: r.get::<_, i64>("has_attachments")? != 0,
            message_id: r.get("message_id")?,
            in_reply_to: r.get("in_reply_to")?,
            remote_id: r.get("remote_id")?,
            is_pinned: r.get::<_, Option<i64>>("is_pinned").unwrap_or(None).unwrap_or(0) != 0,
            thread_count: r.get::<_, i64>("thread_count").unwrap_or(1),
            thread_id: r.get::<_, Option<i64>>("tid").unwrap_or(None).unwrap_or(0),
            thread_unread: r.get::<_, i64>("thread_unread").unwrap_or(0),
        })
    }

    pub fn category_list(&self) -> Vec<String> {
        split_list(&self.categories)
    }
}

/// Aramada karşılaştırma için: küçük harf, Türkçe i/ı/İ farkı yok ("KALAMIŞ" → "kalamiş",
/// "Kalamış" → "kalamiş")
pub fn fold(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| *c != '\u{0307}').map(|c| if c == 'ı' { 'i' } else { c }).collect()
}

/// Arama için katlama (Outlook/Exchange gibi): büyük/küçük harf, Türkçe İ/ı ve aksan farkı
/// gözetilmez ("AYDIN" ~ "aydın" ~ "aydin", "Şükrü" ~ "sukru")
pub fn search_fold(s: &str) -> String {
    fold(s)
        .chars()
        .map(|c| match c {
            'ş' => 's',
            'ğ' => 'g',
            'ü' | 'û' => 'u',
            'ö' => 'o',
            'ç' => 'c',
            'â' | 'á' | 'à' | 'ä' => 'a',
            'î' | 'í' => 'i',
            'é' | 'è' | 'ê' => 'e',
            c => c,
        })
        .collect()
}

/// Metnin sözcükleri (katlanmış; harf/rakam dışı her şey ayırıcıdır: boşluk, nokta, @, virgül...)
fn search_words(s: &str) -> Vec<String> {
    search_fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Exchange gibi sözcük başı eşleşmesi: `words` (katlanmış) metinde sözcük başlarında geçiyor mu.
/// `phrase`: sözcükler arka arkaya (ad soyad, adres, tırnaklı ifade) geçmeli.
pub fn words_match(text: &str, words: &[String], phrase: bool) -> bool {
    if words.is_empty() {
        return true;
    }
    let toks = search_words(text);
    if phrase {
        toks.windows(words.len()).any(|w| w.iter().zip(words).all(|(t, q)| t.starts_with(q.as_str())))
    } else {
        words.iter().all(|q| toks.iter().any(|t| t.starts_with(q.as_str())))
    }
}

/// SQLite LIKE ön süzgeci: LIKE yalnızca ASCII harflerde büyük/küçük harf gözetmez; Türkçe
/// harflere dönüşebilen harfler tek karakter jokeriyle (_) aranır (hızlı, geniş ön eleme)
fn like_pattern(word: &str) -> String {
    let w: String = word
        .chars()
        .map(|c| match c {
            'i' | 's' | 'g' | 'u' | 'o' | 'c' | 'a' | 'e' => '_',
            '%' | '_' | '\\' => '_',
            c => c,
        })
        .collect();
    format!("%{w}%")
}

/// Arama dizini sorgusu: sözcüklerin hepsi verilen sütunlardan birinde sözcük başı olarak geçmeli
/// (`{subject sender} : ("ahmet"* AND "aydin"*)`). Sözcükler search_words'ten gelir (katlanmış,
/// yalnızca harf/rakam) ve dizindeki metin de aynı katlamayla yazılmıştır.
fn fts_query(cols: &[&str], words: &[String]) -> String {
    let terms: Vec<String> = words.iter().map(|w| format!("\"{}\"*", w.replace('"', "\"\""))).collect();
    format!("{{{}}} : ({})", cols.join(" "), terms.join(" AND "))
}

/// Kayıtlı ad/adres çiftini Outlook gibi temizler (tırnaklar, görünmez karakterler, kodlu adlar)
fn clean_pair(name: &str, email: &str) -> (String, String) {
    let email = crate::mail::address::clean_email(email);
    let full = crate::mail::address::clean_address(&if name.trim().is_empty() { email.clone() } else { format!("{name} <{email}>") });
    let (n, e) = crate::mail::split_name_addr(&full);
    (n, if e.is_empty() { email } else { e })
}

/// Çevrimdışı adres defteri satırı
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DirRow {
    pub email: String,
    pub name: String,
    pub title: String,
    pub department: String,
    pub office: String,
    pub phone: String,
    pub kind: String,
}

impl DirRow {
    /// Dağıtım listesi / grup mu (Exchange persona türü)
    pub fn is_group(&self) -> bool {
        self.kind.ends_with("DL") || self.kind == "DistributionList" || self.kind == "GroupMailbox" || self.kind == "ModernGroup"
    }
}

/// Konuşmaya sayılan klasörler: iletinin kendi klasörü (?2) ve çöp/gereksiz dışındakiler
const THREAD_SCOPE: &str = "(folder_id = ?2 OR folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash','junk')))";

/// Bağ bilgisi olmayan "YNT: …" iletisi en çok bu kadar önceki aynı konulu iletiye bağlanır (saniye)
const SUBJECT_THREAD_WINDOW: f64 = 90.0 * 86400.0;

/// İletinin konuşma anahtarları (bkz. Db::assign_thread)
fn thread_keys(conv: &str, mid: &str, in_reply_to: &str, refs: &str, ns: &str) -> Vec<String> {
    let ids = |s: &str| -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == ',')
            .map(|x| x.trim_matches(['<', '>']))
            .filter(|x| !x.is_empty())
            .map(|x| format!("m:{x}\u{1f}{ns}"))
            .collect()
    };
    let mut keys = Vec::new();
    if !conv.is_empty() {
        keys.push(format!("x:{conv}"));
    } else {
        // Çok uzun zincirde en baştaki (kök) ve en sondaki iletiler yeter
        let r = ids(refs);
        if r.len() > 24 {
            keys.extend_from_slice(&r[..4]);
            keys.extend_from_slice(&r[r.len() - 20..]);
        } else {
            keys.extend(r);
        }
        keys.extend(ids(in_reply_to));
    }
    keys.extend(ids(mid).into_iter().take(1));
    keys.sort();
    keys.dedup();
    keys
}

const LIST_COLS: &str = "id, folder_id, account_id, uid, subject, sender, to_addr, date, snippet, \
    is_read, is_flagged, is_draft, is_answered, importance, categories, has_attachments, \
    message_id, in_reply_to, remote_id, is_pinned, tid";

/// Sunucuyla eşitlenen bayrak sütunları
pub const FLAG_FIELDS: [&str; 3] = ["is_read", "is_flagged", "is_answered"];

/// Sunucuya yazılmayı bekleyen bayrak değişikliği
#[derive(Debug, Clone, PartialEq)]
pub struct PendingFlag {
    pub message_id: i64,
    pub field: String,
    pub value: bool,
    pub folder_id: i64,
    pub uid: Option<i64>,
    pub remote_id: Option<String>,
}

/// Tam ileti.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Message {
    pub id: i64,
    pub folder_id: i64,
    pub account_id: Option<i64>,
    pub uid: Option<i64>,
    pub message_id: String,
    pub in_reply_to: String,
    pub refs: String,
    pub subject: String,
    pub sender: String,
    pub to_addr: String,
    pub cc: String,
    pub bcc: String,
    pub date: f64,
    pub snippet: String,
    pub body_text: String,
    pub body_html: String,
    #[serde(skip)]
    pub raw: Vec<u8>,
    pub is_read: bool,
    pub is_flagged: bool,
    pub is_draft: bool,
    pub is_answered: bool,
    pub importance: i64,
    pub categories: String,
    pub has_attachments: bool,
    pub pending: String,
    /// Exchange (EWS) öğe kimliği
    pub remote_id: Option<String>,
    pub is_pinned: bool,
    /// Eklerin adları (satır başına bir ad)
    pub att_names: String,
    /// Exchange konuşma kimliği (başka sunucularda boş)
    pub conv: String,
}

impl Message {
    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            folder_id: r.get("folder_id")?,
            account_id: r.get("account_id")?,
            uid: r.get("uid")?,
            message_id: r.get("message_id")?,
            in_reply_to: r.get("in_reply_to")?,
            refs: r.get("refs")?,
            subject: r.get("subject")?,
            sender: r.get("sender")?,
            to_addr: r.get("to_addr")?,
            cc: r.get("cc")?,
            bcc: r.get("bcc")?,
            date: r.get("date")?,
            snippet: r.get("snippet")?,
            body_text: r.get("body_text")?,
            body_html: r.get("body_html")?,
            raw: r.get::<_, Option<Vec<u8>>>("raw")?.unwrap_or_default(),
            is_read: r.get::<_, i64>("is_read")? != 0,
            is_flagged: r.get::<_, i64>("is_flagged")? != 0,
            is_draft: r.get::<_, i64>("is_draft")? != 0,
            is_answered: r.get::<_, i64>("is_answered")? != 0,
            importance: r.get("importance")?,
            categories: r.get("categories")?,
            has_attachments: r.get::<_, i64>("has_attachments")? != 0,
            pending: r.get("pending")?,
            remote_id: r.get("remote_id")?,
            is_pinned: r.get::<_, Option<i64>>("is_pinned").unwrap_or(None).unwrap_or(0) != 0,
            att_names: r.get::<_, Option<String>>("att_names").unwrap_or(None).unwrap_or_default(),
            conv: r.get::<_, Option<String>>("conv").unwrap_or(None).unwrap_or_default(),
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Contact {
    pub id: i64,
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub email: String,
    pub email2: String,
    pub phone_mobile: String,
    pub phone_work: String,
    pub phone_home: String,
    pub company: String,
    pub job_title: String,
    pub department: String,
    pub address: String,
    pub website: String,
    pub birthday: String,
    pub notes: String,
    pub categories: String,
    pub is_favorite: bool,
    /// Exchange kişileri için hesap ve öğe kimliği
    pub account_id: Option<i64>,
    pub remote_id: Option<String>,
}

impl Contact {
    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            first_name: r.get("first_name")?,
            last_name: r.get("last_name")?,
            display_name: r.get("display_name")?,
            email: r.get("email")?,
            email2: r.get("email2")?,
            phone_mobile: r.get("phone_mobile")?,
            phone_work: r.get("phone_work")?,
            phone_home: r.get("phone_home")?,
            company: r.get("company")?,
            job_title: r.get("job_title")?,
            department: r.get("department")?,
            address: r.get("address")?,
            website: r.get("website")?,
            birthday: r.get("birthday")?,
            notes: r.get("notes")?,
            categories: r.get("categories")?,
            is_favorite: r.get::<_, i64>("is_favorite")? != 0,
            account_id: r.get("account_id")?,
            remote_id: r.get("remote_id")?,
        })
    }

    pub fn computed_display(&self) -> String {
        if !self.display_name.trim().is_empty() {
            return self.display_name.trim().to_string();
        }
        let n = format!("{} {}", self.first_name.trim(), self.last_name.trim());
        let n = n.trim();
        if !n.is_empty() {
            n.to_string()
        } else if !self.email.is_empty() {
            self.email.clone()
        } else {
            crate::i18n::tr("contacts.unnamed")
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ContactGroup {
    pub id: i64,
    pub name: String,
    pub members: String,
    /// Exchange ile eşitlenen grubun hesabı (boş: yalnızca bu bilgisayarda)
    pub account_id: Option<i64>,
    pub remote_id: Option<String>,
    /// Sunucuya gönderilmeyi bekleyen değişiklik
    pub dirty: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Calendar {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub visible: bool,
    /// Exchange takvimleri için hesap
    pub account_id: Option<i64>,
    /// Başkasının takvimi: posta kutusu adresi (ekip, oda, paylaşılan)
    pub owner: String,
    /// Grup: "" (Takvimlerim) | "team" | "other" | "shared"
    pub grp: String,
    /// "" yerel/Exchange | "dav" CalDAV | "remote" başkasının takvimi (salt okunur) | "birthdays"
    pub kind: String,
    /// CalDAV koleksiyonunun adresi
    pub remote_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub id: i64,
    pub calendar_id: i64,
    pub uid: String,
    pub title: String,
    pub location: String,
    pub description: String,
    pub start: f64,
    pub end: f64,
    pub all_day: bool,
    pub recurrence: String,
    pub recur_interval: i64,
    pub recur_until: Option<f64>,
    pub exdates: String,
    pub reminder_minutes: i64,
    pub category: String,
    pub show_as: String,
    /// Zorunlu katılımcılar (virgülle ayrılmış adresler)
    pub attendees: String,
    /// İsteğe bağlı katılımcılar
    pub optional_attendees: String,
    /// Odalar / kaynaklar (Outlook "Oda Bulucu")
    pub resources: String,
    pub is_private: bool,
    /// Toplantıyı düzenleyen ("Ad <adres>")
    pub organizer: String,
    /// Toplantı iptal edildi (Outlook'ta beyaz kutu, "İptal edildi:")
    pub is_cancelled: bool,
    /// Katılımcılı toplantı mı
    pub is_meeting: bool,
    /// Yanıtım: "organizer" | "accept" | "tentative" | "decline" | "none" | ""
    pub response: String,
    /// Exchange öğe kimliği (yineleme için yineleme öğesi kimliği)
    pub remote_id: Option<String>,
    /// Exchange yinelenen serinin ana öğe kimliği
    pub master_id: Option<String>,
    /// Exchange ChangeKey (öğe değişince değişir)
    pub change_key: String,
}

impl Event {
    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            calendar_id: r.get("calendar_id")?,
            uid: r.get("uid")?,
            title: r.get("title")?,
            location: r.get("location")?,
            description: r.get("description")?,
            start: r.get("start")?,
            end: r.get("end")?,
            all_day: r.get::<_, i64>("all_day")? != 0,
            recurrence: r.get("recurrence")?,
            recur_interval: r.get("recur_interval")?,
            recur_until: r.get("recur_until")?,
            exdates: r.get("exdates")?,
            reminder_minutes: r.get("reminder_minutes")?,
            category: r.get("category")?,
            show_as: r.get("show_as")?,
            attendees: r.get("attendees")?,
            optional_attendees: r.get::<_, Option<String>>("optional_attendees").ok().flatten().unwrap_or_default(),
            resources: r.get::<_, Option<String>>("resources").ok().flatten().unwrap_or_default(),
            is_private: r.get::<_, i64>("is_private")? != 0,
            organizer: r.get::<_, Option<String>>("organizer").ok().flatten().unwrap_or_default(),
            is_cancelled: r.get::<_, Option<i64>>("is_cancelled").ok().flatten().unwrap_or(0) != 0,
            is_meeting: r.get::<_, Option<i64>>("is_meeting").ok().flatten().unwrap_or(0) != 0,
            response: r.get::<_, Option<String>>("response").ok().flatten().unwrap_or_default(),
            remote_id: r.get("remote_id")?,
            master_id: r.get("master_id")?,
            change_key: r.get::<_, Option<String>>("change_key").ok().flatten().unwrap_or_default(),
        })
    }

    pub fn new_default() -> Self {
        Self {
            calendar_id: 1,
            recurrence: "none".into(),
            recur_interval: 1,
            reminder_minutes: 15,
            show_as: "busy".into(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub notes: String,
    pub start_date: Option<f64>,
    pub due_date: Option<f64>,
    pub reminder: Option<f64>,
    pub reminder_done: bool,
    pub priority: i64,
    pub status: String,
    pub percent: i64,
    pub category: String,
    pub message_id: Option<i64>,
    pub completed_at: Option<f64>,
    /// Exchange ile eşitlenen görevin hesabı (boş: yalnızca bu bilgisayarda)
    pub account_id: Option<i64>,
    pub remote_id: Option<String>,
    /// Sunucuya gönderilmeyi bekleyen değişiklik
    pub dirty: bool,
}

impl Task {
    fn from_row(r: &Row) -> Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            title: r.get("title")?,
            notes: r.get("notes")?,
            start_date: r.get("start_date")?,
            due_date: r.get("due_date")?,
            reminder: r.get("reminder")?,
            reminder_done: r.get::<_, i64>("reminder_done")? != 0,
            priority: r.get("priority")?,
            status: r.get("status")?,
            percent: r.get("percent")?,
            category: r.get("category")?,
            message_id: r.get("message_id")?,
            completed_at: r.get("completed_at")?,
            account_id: r.get("account_id")?,
            remote_id: r.get("remote_id")?,
            dirty: r.get::<_, i64>("dirty")? != 0,
        })
    }

    pub fn new_default() -> Self {
        Self { priority: 1, status: "not_started".into(), ..Default::default() }
    }

    pub fn is_completed(&self) -> bool {
        self.status == "completed"
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub id: i64,
    pub body: String,
    pub color: String,
    pub category: String,
    pub modified: f64,
    /// Exchange ile eşitlenen notun hesabı (boş: yalnızca bu bilgisayarda)
    pub account_id: Option<i64>,
    pub remote_id: Option<String>,
    /// Sunucuya gönderilmeyi bekleyen değişiklik
    pub dirty: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub account_id: Option<i64>,
    pub field: String,
    pub op: String,
    pub value: String,
    pub action: String,
    pub action_arg: String,
    pub stop: bool,
    /// Exchange sunucu kuralı kimliği (kural sunucuda çalışır)
    pub remote_id: Option<String>,
    /// Basit modele sığmayan sunucu kuralı: yalnızca aç/kapat ve silme
    pub complex: bool,
    #[serde(skip)]
    pub remote_xml: String,
    /// Yerel kuralın tam modeli (bkz. rules::LocalSpec)
    pub spec: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Category {
    pub name: String,
    pub color: String,
}

pub fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

// ======================================================================= veritabanı
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// Yalnızca okuma bağlantıları (WAL: yazma sürerken de okur). Eşitleme uzun yazma
    /// yaparken ileti açma, liste, klasör ağacı gibi arayüz okumaları onu beklemez; birden
    /// çok olduğu için bir okuma da ötekini beklemez.
    readers: Arc<Vec<Arc<Mutex<Connection>>>>,
    /// Tam metin arama dizini tüm iletileri kapsıyor mu (eski iletiler arka planda dizinlenirken
    /// arama dizinsiz yoldan yapılır)
    fts_ready: Arc<std::sync::atomic::AtomicBool>,
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LYNORA_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    // Birim testleri kullanıcının gerçek veri klasörüne asla yazmaz
    if cfg!(test) {
        return std::env::temp_dir().join(format!("lynora-test-data-{}", std::process::id()));
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("lynora")
}

/// Uygulamanın eski adıyla (MailNix) oluşturulmuş veriler yeni klasöre taşınır:
/// ~/.local/share/mailnix/mailnix.db -> ~/.local/share/lynora/lynora.db (iletiler, hesaplar,
/// ayarlar korunur). Yeni klasörde veritabanı varsa dokunulmaz.
pub fn migrate_legacy_data_dir(new_dir: &std::path::Path, old_dir: &std::path::Path) {
    if new_dir.join("lynora.db").exists() || !old_dir.join("mailnix.db").exists() {
        return;
    }
    // Veritabanı olmayan klasör (ör. açılışta yazılan crash.log) taşımayı engellemesin: yoksa
    // kullanıcı verilerini kaybolmuş sanır. Silinmez, kenara alınır.
    if new_dir.exists() {
        let _ = std::fs::rename(new_dir, new_dir.with_extension("eski"));
    }
    if std::fs::rename(old_dir, new_dir).is_err() {
        return;
    }
    for ext in ["", "-wal", "-shm"] {
        let from = new_dir.join(format!("mailnix.db{ext}"));
        if from.exists() {
            let _ = std::fs::rename(&from, new_dir.join(format!("lynora.db{ext}")));
        }
    }
}

/// CalDAV/CardDAV nesnesinin sunucu durumu
#[derive(Clone, Debug, Default)]
pub struct DavState {
    pub etag: String,
    /// Sunucudaki özgün metin: yazarken yalnızca değişen özellikler güncellenir
    pub raw: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DavRow {
    pub id: i64,
    pub etag: String,
    pub dirty: bool,
}

/// İleti listesi sıralaması
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ListOrder {
    pub by: String,
    pub desc: bool,
    pub conversations: bool,
}

impl Default for ListOrder {
    fn default() -> Self {
        ListOrder { by: "date".into(), desc: true, conversations: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MsgFilter {
    #[default]
    All,
    Unread,
    Flagged,
    Attachments,
    Important,
    Category(String),
}

impl MsgFilter {
    pub fn parse(s: &str) -> Self {
        match s {
            "unread" => Self::Unread,
            "flagged" => Self::Flagged,
            "attachments" => Self::Attachments,
            "important" => Self::Important,
            _ if s.starts_with("category:") => Self::Category(s[9..].to_string()),
            _ => Self::All,
        }
    }
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = default_data_dir();
        if std::env::var_os("LYNORA_DATA_DIR").is_none() {
            if let Some(base) = dirs::data_dir() {
                migrate_legacy_data_dir(&dir, &base.join("mailnix"));
            }
        }
        let _ = std::fs::create_dir_all(&dir);
        crate::backup::apply_pending(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Self::open(dir.join("lynora.db"))
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        Self::init(conn, Some(path))
    }

    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    /// Konu sıralaması ve konuşma görünümü için "RE:/FW:" önekleri atılmış konu
    fn register_functions(conn: &Connection) -> Result<()> {
        // Arama: lyn_match(metin, "sözcük sözcük", ifade_mi) — Türkçe/aksan duyarsız sözcük başı
        conn.create_scalar_function(
            "lyn_match",
            3,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let text: String = ctx.get::<Option<String>>(0)?.unwrap_or_default();
                let q: String = ctx.get::<Option<String>>(1)?.unwrap_or_default();
                let phrase: i64 = ctx.get::<Option<i64>>(2)?.unwrap_or(0);
                let words: Vec<String> = q.split(' ').filter(|w| !w.is_empty()).map(String::from).collect();
                Ok(words_match(&text, &words, phrase != 0))
            },
        )?;
        // Arama dizini için katlanmış metin (aranan sözcükler de search_words ile aynı biçime gelir)
        conn.create_scalar_function(
            "lyn_fold",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            // Tetikleyicilerde çalışır: hiçbir değerde hata vermemeli (ileti yazılamaz olurdu)
            |ctx| {
                use rusqlite::types::ValueRef;
                let s = match ctx.get_raw(0) {
                    ValueRef::Text(t) | ValueRef::Blob(t) => String::from_utf8_lossy(t).into_owned(),
                    ValueRef::Integer(i) => i.to_string(),
                    ValueRef::Real(f) => f.to_string(),
                    ValueRef::Null => String::new(),
                };
                Ok(search_fold(&s))
            },
        )?;
        conn.create_scalar_function(
            "lyn_norm_subject",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let s: String = ctx.get::<Option<String>>(0)?.unwrap_or_default();
                Ok(crate::mail::normalize_subject(&s).to_lowercase())
            },
        )
    }

    fn open_reader(path: &std::path::Path) -> Result<Connection> {
        use rusqlite::OpenFlags;
        let mut r = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)?;
        r.execute_batch("PRAGMA temp_store = MEMORY; PRAGMA cache_size = -16000; PRAGMA busy_timeout = 5000;")?;
        Self::register_functions(&r)?;
        r.profile(Some(log_slow_query));
        Ok(r)
    }

    fn has_column(conn: &Connection, table: &str, col: &str) -> Result<bool> {
        conn.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"))?.exists([col])
    }

    /// Veritabanı eski düzende mi (1.1.0: gövdeler satırda; 1.2.0: konu sütunu yok): açılışta bir
    /// kez yapılacak geçiş uzun sürebileceği için önceden açılış ekranı gösterilir
    pub fn needs_upgrade(path: &std::path::Path) -> bool {
        use rusqlite::OpenFlags;
        path.exists()
            && Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .and_then(|c| Ok(Self::has_column(&c, "messages", "raw")? || !Self::has_column(&c, "messages", "ns")?))
                .unwrap_or(false)
    }

    /// 1.2.0 geçişi: ileti gövdeleri ve özgün ileti (body_text, body_html, raw) `message_content`
    /// tablosuna taşınır. Eski düzende bu alanlar satırın ortasındaydı; ardından gelen remote_id,
    /// att_names gibi sütunları okumak her iletide tüm taşma sayfalarını yürüyordu (liste ve
    /// taramalar 15-50 kat yavaştı). Bir kez çalışır, yarıda kesilirse kaldığı yerden sürer.
    /// Gövdeler küçük parçalar halinde taşınıp eski satırdan silindiği için boşalan sayfalar
    /// hemen yeniden kullanılır: dosya büyümez, yalnızca son adım (küçük alanlar) tek işlemdir.
    fn split_message_content(conn: &mut Connection) -> Result<bool> {
        if !Self::has_column(conn, "messages", "raw")? {
            return Ok(false);
        }
        let has_content = "(length(raw) > 0 OR body_text <> '' OR body_html <> '')";
        let max_id: i64 = conn.query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |r| r.get(0))?;
        let mut from = 0;
        while from < max_id {
            let to = from + 100;
            let tx = conn.transaction()?;
            tx.execute(
                &format!(
                    "INSERT OR REPLACE INTO message_content(id, body_text, body_html, raw) \
                     SELECT id, COALESCE(body_text, ''), COALESCE(body_html, ''), raw FROM messages \
                     WHERE id > ?1 AND id <= ?2 AND {has_content}"
                ),
                [from, to],
            )?;
            tx.execute(&format!("UPDATE messages SET body_text = '', body_html = '', raw = NULL WHERE id > ?1 AND id <= ?2 AND {has_content}"), [from, to])?;
            tx.commit()?;
            from = to;
        }
        // Silinmiş iletilerin kimlikleri yeniden verilmesin (görevler ileti kimliğini tutar)
        let seq: i64 = conn.query_row("SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'messages'), 0)", [], |r| r.get(0))?;
        // Tablo değişirken yabancı anahtar eylemleri kapalı: açıkken DROP TABLE gövdeleri de siler
        conn.execute_batch("PRAGMA foreign_keys = OFF")?;
        let rebuilt = Self::rebuild_messages(conn, seq);
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        rebuilt?;
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
        Ok(true)
    }

    /// Gövdeleri boşaltılmış eski tablonun yerine küçük alanlı yeni tablo (sütun sırası temiz
    /// kurulumdakiyle aynı; sonradan eklenen sütunlar da en sonda)
    fn rebuild_messages(conn: &mut Connection, seq: i64) -> Result<()> {
        let tx = conn.transaction()?;
        tx.execute_batch(concat!("CREATE TABLE messages_new (", messages_columns!(), ")"))?;
        for (_, col, ddl) in MIGRATIONS.iter().filter(|m| m.0 == "messages") {
            tx.execute_batch(&format!("ALTER TABLE messages_new ADD COLUMN {col} {ddl}"))?;
        }
        let cols: Vec<String> = {
            let mut st = tx.prepare("SELECT name FROM pragma_table_info('messages_new') WHERE name IN (SELECT name FROM pragma_table_info('messages'))")?;
            let cols = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>>>()?;
            cols
        };
        let list = cols.join(", ");
        tx.execute_batch(&format!(
            "INSERT INTO messages_new({list}) SELECT {list} FROM messages; DROP TABLE messages; ALTER TABLE messages_new RENAME TO messages;"
        ))?;
        if tx.execute("UPDATE sqlite_sequence SET seq = MAX(seq, ?1) WHERE name = 'messages'", [seq])? == 0 && seq > 0 {
            tx.execute("INSERT INTO sqlite_sequence(name, seq) VALUES ('messages', ?1)", [seq])?;
        }
        tx.commit()
    }

    /// 1.3.0: konuşma görünümü için arındırılmış konu (`ns`) bir kez doldurulur, konu değişince
    /// tetikleyiciyle güncellenir (yeni iletide insert_message yazar); gruplama ve sıralama dizinden
    fn init_subject_index(conn: &Connection) -> Result<()> {
        let tx = conn.unchecked_transaction()?;
        // v2 (1.7.5): önek listesi genişledi ("RE[2]:", "Cevap:", "AW:" …)
        if !tx.prepare("SELECT 1 FROM settings WHERE key = 'subject_index_v2'")?.exists([])? {
            tx.execute_batch(
                "UPDATE messages SET ns = lyn_norm_subject(subject) WHERE ns IS NULL OR ns <> lyn_norm_subject(subject); \
                 INSERT OR REPLACE INTO settings(key, value) VALUES ('subject_index_v2', '1');",
            )?;
        }
        tx.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS messages_ns AFTER UPDATE OF subject ON messages WHEN old.subject IS NOT new.subject BEGIN \
                 UPDATE messages SET ns = lyn_norm_subject(new.subject) WHERE id = new.id; \
             END; \
             CREATE INDEX IF NOT EXISTS idx_messages_ns ON messages(folder_id, ns, date);",
        )?;
        tx.commit()
    }

    /// 1.7.5: konuşmalar konuya göre değil, iletilerin birbirine bağına göre kurulur (`tid`).
    /// Eski iletiler bir kez bağlanır; sonrası insert_message ile.
    fn init_threads(conn: &mut Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS thread_keys (
                 account_id INTEGER NOT NULL, key TEXT NOT NULL, tid INTEGER NOT NULL,
                 PRIMARY KEY (account_id, key)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS idx_thread_keys_tid ON thread_keys(tid);
             DROP INDEX IF EXISTS idx_messages_thread;
             CREATE INDEX IF NOT EXISTS idx_messages_subject ON messages(account_id, ns, date);
             CREATE INDEX IF NOT EXISTS idx_messages_tid ON messages(tid, folder_id);
             CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(folder_id, tid, date, is_read);",
        )?;
        if !conn.prepare("SELECT 1 FROM settings WHERE key = 'threads_v1'")?.exists([])? {
            let tx = conn.transaction()?;
            Self::rebuild_threads(&tx)?;
            tx.execute("INSERT OR REPLACE INTO settings(key, value) VALUES ('threads_v1', '1')", [])?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Bütün konuşmaları baştan kurar (iletiler tarih sırasıyla bağlanır)
    fn rebuild_threads(c: &Connection) -> Result<()> {
        c.execute_batch("DELETE FROM thread_keys; UPDATE messages SET tid = NULL WHERE tid IS NOT NULL;")?;
        let ids: Vec<i64> = {
            let mut st = c.prepare("SELECT id FROM messages ORDER BY date, id")?;
            let ids = st.query_map([], |r| r.get(0))?.collect::<Result<Vec<i64>>>()?;
            ids
        };
        for id in ids {
            Self::assign_thread(c, id)?;
        }
        Ok(())
    }

    /// İletiyi konuşmasına bağlar (`messages.tid`). Bağ, iletinin anahtarlarıyla kurulur
    /// (`thread_keys`: anahtar → konuşma):
    /// - Exchange konuşma kimliği varsa o (sunucu yanıt zincirini ve konu değişimini zaten
    ///   hesaba katmıştır; Outlook ile aynı gruplama),
    /// - yoksa References/In-Reply-To/Message-ID zinciri, arındırılmış konuyla birlikte (konusu
    ///   değişen yanıt yeni konuşma başlatır).
    /// Hiçbir bağ bilgisi taşımayan "YNT: …" iletisi, aynı hesapta aynı konulu en yakın önceki
    /// iletinin konuşmasına katılır. Öneksiz aynı konulu iletiler (ör. her gün gelen rapor)
    /// birbirine bağlanmaz. Birden çok konuşmaya değen ileti onları birleştirir; sıra önemsizdir.
    fn assign_thread(c: &Connection, id: i64) -> Result<()> {
        type Row = (Option<i64>, String, String, String, String, String, String, f64, Option<i64>);
        let row: Option<Row> = c
            .prepare_cached(
                "SELECT account_id, COALESCE(message_id, ''), COALESCE(in_reply_to, ''), COALESCE(refs, ''), COALESCE(subject, ''), \
                 COALESCE(ns, ''), conv, COALESCE(date, 0), tid FROM messages WHERE id = ?1",
            )?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))
            .optional()?;
        let Some((account, mid, irt, refs, subject, ns, conv, date, old)) = row else { return Ok(()) };
        let acct = account.unwrap_or(0);
        let keys = thread_keys(&conv, &mid, &irt, &refs, &ns);
        let mut tids: std::collections::BTreeSet<i64> = old.into_iter().collect();
        {
            let mut st = c.prepare_cached("SELECT tid FROM thread_keys WHERE account_id = ?1 AND key = ?2")?;
            for k in &keys {
                if let Some(t) = st.query_row(params![acct, k], |r| r.get::<_, i64>(0)).optional()? {
                    tids.insert(t);
                }
            }
        }
        // Konu yedeği yalnızca Exchange kimliği olmayan iletiler arasında
        if conv.is_empty() && !ns.is_empty() {
            let linkless = irt.trim().is_empty() && refs.trim().is_empty();
            if linkless && crate::mail::has_subject_prefix(&subject) {
                let t: Option<i64> = c
                    .prepare_cached(
                        "SELECT tid FROM messages WHERE account_id IS ?1 AND ns = ?2 AND date <= ?3 AND date > ?3 - ?5 AND id <> ?4 \
                         AND conv = '' AND tid IS NOT NULL ORDER BY date DESC LIMIT 1",
                    )?
                    .query_row(params![account, ns, date, id, SUBJECT_THREAD_WINDOW], |r| r.get(0))
                    .optional()?;
                tids.extend(t);
            }
            // Bu ileti, kendisinden sonra gelmiş bağsız bir "YNT:" iletisinin en yakın öncülü olabilir
            let later: Option<(i64, String)> = c
                .prepare_cached(
                    "SELECT tid, COALESCE(subject, '') FROM messages WHERE account_id IS ?1 AND ns = ?2 AND date >= ?3 AND date < ?3 + ?5 AND id <> ?4 \
                     AND conv = '' AND tid IS NOT NULL ORDER BY date LIMIT 1",
                )?
                .query_row(params![account, ns, date, id, SUBJECT_THREAD_WINDOW], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            if let Some((t, s)) = later {
                let bare: bool = c
                    .prepare_cached("SELECT COALESCE(in_reply_to, '') = '' AND COALESCE(refs, '') = '' FROM messages WHERE tid = ?1 AND account_id IS ?2 AND ns = ?3 AND date >= ?4 ORDER BY date LIMIT 1")?
                    .query_row(params![t, account, ns, date], |r| r.get(0))
                    .optional()?
                    .unwrap_or(false);
                if bare && crate::mail::has_subject_prefix(&s) {
                    tids.insert(t);
                }
            }
        }
        let tid = match tids.iter().next() {
            Some(&t) => t,
            // Silinmiş konuşmanın numarası yeniden kullanılmaz (anahtarları duruyor olabilir)
            None => c
                .prepare_cached("SELECT MAX(COALESCE((SELECT MAX(tid) FROM messages), 0), COALESCE((SELECT MAX(tid) FROM thread_keys), 0)) + 1")?
                .query_row([], |r| r.get(0))?,
        };
        for other in tids.iter().skip(1) {
            c.prepare_cached("UPDATE messages SET tid = ?1 WHERE tid = ?2")?.execute([tid, *other])?;
            c.prepare_cached("UPDATE thread_keys SET tid = ?1 WHERE tid = ?2")?.execute([tid, *other])?;
        }
        if old != Some(tid) {
            c.prepare_cached("UPDATE messages SET tid = ?1 WHERE id = ?2")?.execute([tid, id])?;
        }
        let mut st = c.prepare_cached("INSERT INTO thread_keys(account_id, key, tid) VALUES (?1, ?2, ?3) ON CONFLICT(account_id, key) DO UPDATE SET tid = excluded.tid")?;
        for k in &keys {
            st.execute(params![acct, k, tid])?;
        }
        Ok(())
    }

    /// Exchange'in sonradan öğrenilen konuşma kimliği (ve varsa yanıt başlıkları) yazılır; ileti
    /// eski (konuya göre kurulmuş) konuşmasından ayrılıp sunucunun konuşmasına bağlanır.
    /// Döner: ileti değişti mi (kimliği zaten olan iletiye dokunulmaz).
    pub fn set_conversation(&self, id: i64, conv: &str, in_reply_to: &str, refs: &str) -> bool {
        if conv.is_empty() {
            return false;
        }
        let mut c = self.conn();
        let run = |tx: &Connection| -> Result<bool> {
            let n = tx.prepare_cached(
                "UPDATE messages SET conv = ?2, in_reply_to = CASE WHEN COALESCE(in_reply_to, '') = '' THEN ?3 ELSE in_reply_to END, \
                 refs = CASE WHEN COALESCE(refs, '') = '' THEN ?4 ELSE refs END, tid = NULL WHERE id = ?1 AND conv = ''",
            )?
            .execute(params![id, conv, in_reply_to, refs])?;
            if n == 0 {
                return Ok(false);
            }
            // İletinin kendi anahtarı eski konuşmayı gösterir: bırakılırsa ileti oraya geri bağlanır
            tx.prepare_cached(
                "DELETE FROM thread_keys WHERE (account_id, key) IN (SELECT COALESCE(account_id, 0), 'm:' || trim(message_id, '<> ') || char(31) || COALESCE(ns, '') FROM messages WHERE id = ?1)",
            )?
            .execute([id])?;
            Self::assign_thread(tx, id)?;
            Ok(true)
        };
        let Ok(tx) = c.transaction() else { return false };
        match run(&tx) {
            Ok(changed) => tx.commit().is_ok() && changed,
            Err(_) => false,
        }
    }

    /// 1.3.0: arama dizini yoksa (ya da sürümü eskiyse) kurulur. O ana kadarki iletiler dizine
    /// arka planda eklenir (`build_search_index`); nereye gelindiği `fts_build` tablosunda
    /// tutulur, uygulama kapanırsa kaldığı yerden sürer. Döner: dizin tüm iletileri kapsıyor mu.
    fn init_search_index(conn: &Connection) -> Result<bool> {
        let tx = conn.unchecked_transaction()?;
        let version: Option<String> = tx.query_row("SELECT value FROM settings WHERE key = 'fts_version'", [], |r| r.get(0)).optional()?;
        if version.as_deref() != Some(FTS_VERSION) {
            for t in FTS_TRIGGERS {
                tx.execute_batch(&format!("DROP TRIGGER IF EXISTS {t}"))?;
            }
            tx.execute_batch("DROP TABLE IF EXISTS message_fts; DROP TABLE IF EXISTS fts_build;")?;
            tx.execute_batch(FTS_SCHEMA)?;
            let last: i64 = tx.query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |r| r.get(0))?;
            if last > 0 {
                tx.execute("INSERT INTO fts_build(pos, last) VALUES (0, ?1)", [last])?;
            }
            tx.execute("INSERT OR REPLACE INTO settings(key, value) VALUES ('fts_version', ?1)", [FTS_VERSION])?;
        } else {
            // 1.2.0 geçişi ileti tablosunu yeniden kurduysa tetikleyicileri de gitmiştir
            tx.execute_batch(FTS_SCHEMA)?;
        }
        let ready = !tx.prepare("SELECT 1 FROM fts_build")?.exists([])?;
        tx.commit()?;
        Ok(ready)
    }

    /// Arama dizinine eski iletilerden bir parça ekler (arka plan işi; yazma kilidi her parçada
    /// kısa süre tutulur). Döner: iş kaldı mı. Dizin tamamlanınca arama ona geçer.
    pub fn build_search_index(&self, batch: i64) -> bool {
        use std::sync::atomic::Ordering;
        if self.fts_ready.load(Ordering::Relaxed) {
            return false;
        }
        match Self::search_index_step(&mut self.conn(), batch) {
            Ok(true) => {
                self.fts_ready.store(true, Ordering::Relaxed);
                false
            }
            Ok(false) => true,
            // Dizin kurulamıyorsa arama dizinsiz yoldan sürer
            Err(e) => {
                eprintln!("lynora arama dizini: {e}");
                false
            }
        }
    }

    /// Döner: dizin tamamlandı mı
    fn search_index_step(c: &mut Connection, batch: i64) -> Result<bool> {
        let tx = c.transaction()?;
        let Some((pos, last)) = tx.query_row("SELECT pos, last FROM fts_build", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).optional()? else {
            return Ok(true);
        };
        let to = (pos + batch).min(last);
        tx.execute(
            concat!(fts_insert!(), " SELECT m.id, ", fts_row!("m", "c.body_text"), " FROM messages m LEFT JOIN message_content c ON c.id = m.id WHERE m.id > ?1 AND m.id <= ?2"),
            [pos, to],
        )?;
        let done = to >= last;
        if done {
            tx.execute("DELETE FROM fts_build", [])?;
        } else {
            tx.execute("UPDATE fts_build SET pos = ?1", [to])?;
        }
        tx.commit()?;
        Ok(done)
    }

    fn init(mut conn: Connection, path: Option<std::path::PathBuf>) -> Result<Self> {
        // Artımlı küçültme: yeni veritabanında hemen, eskisinde ilk sıkıştırmada (VACUUM) geçerli olur
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA temp_store = MEMORY; PRAGMA cache_size = -32000; PRAGMA busy_timeout = 5000;")?;
        Self::register_functions(&conn)?;
        // Bellek içi veritabanı (testler) ölçülmez: günlük yalnızca gerçek veri klasörüne yazılır
        if let Some(p) = path.as_deref() {
            SLOW_LOG.get_or_init(|| p.with_file_name("slow-queries.log"));
            conn.profile(Some(log_slow_query));
        }
        conn.execute_batch(SCHEMA)?;
        for (table, col, ddl) in MIGRATIONS {
            if !Self::has_column(&conn, table, col)? {
                conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {ddl}"))?;
            }
        }
        // Eski tablo silindiğinde dizinleri de gider: şemadakiler yeniden kurulur (aşağıdakiler zaten kurulur)
        if Self::split_message_content(&mut conn)? {
            conn.execute_batch(SCHEMA)?;
        }
        Self::init_subject_index(&conn)?;
        Self::init_threads(&mut conn)?;
        let fts_ready = Self::init_search_index(&conn)?;
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_messages_remote ON messages(folder_id, remote_id)")?;
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_messages_unread ON messages(folder_id, is_read)")?;
        // Listenin varsayılan sırası (sabitlenmiş, tarih, kimlik) dizinden okunur: klasör açılınca
        // tüm satırlar (ve ileti gövdeleri) taranıp sıralanmaz, ilk sayfada durulur
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_messages_list ON messages(folder_id, is_pinned, date, id)")?;
        // Çevrimdışı adres defteri (Outlook OAB): kurum dizini yerelde, arama sunucuya gitmez
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS directory (
                account_id INTEGER NOT NULL, email TEXT NOT NULL, name TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '', department TEXT NOT NULL DEFAULT '', office TEXT NOT NULL DEFAULT '',
                phone TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (account_id, email)
            );
            CREATE INDEX IF NOT EXISTS idx_directory_name ON directory(account_id, name COLLATE NOCASE);",
        )?;
        // Aynı Exchange öğesi bir klasöre iki kez yazılmasın (eşzamanlı eşitleme/arama)
        let _ = conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_remote_u ON messages(folder_id, remote_id)");
        // Gönderilmiş/Silinmiş Öğeler'deki iletiler taslak sayılmaz (eski eşitlemeden kalan bayrak)
        let _ = conn.execute_batch("UPDATE messages SET is_draft = 0 WHERE is_draft = 1 AND folder_id IN (SELECT id FROM folders WHERE role IN ('sent', 'trash'))");
        // Eşitleme v2: Exchange klasörleri tüm başlıklarıyla yeniden taranır
        // (eski sürüm yalnızca en yeni N iletiyi almıştı)
        let v2: bool = conn.prepare("SELECT 1 FROM settings WHERE key = 'ews_sync_v2'")?.exists([])?;
        if !v2 {
            conn.execute_batch(
                "UPDATE folders SET sync_state = '', full_sync = 0 WHERE account_id IN (SELECT id FROM accounts WHERE kind = 'exchange'); \
                 INSERT OR REPLACE INTO settings(key, value) VALUES ('ews_sync_v2', '1');",
            )?;
        }
        // Eşitleme v3 (1.7.5): başlıklar konuşma kimliğiyle yeniden taranır (bkz. set_conversation)
        if !conn.prepare("SELECT 1 FROM settings WHERE key = 'ews_sync_v3'")?.exists([])? {
            conn.execute_batch(
                "UPDATE folders SET sync_state = '', full_sync = 0 WHERE account_id IN (SELECT id FROM accounts WHERE kind = 'exchange'); \
                 INSERT OR REPLACE INTO settings(key, value) VALUES ('ews_sync_v3', '1');",
            )?;
        }
        // Sorgu planlayıcı istatistikleri (hızlı; yalnızca gerekirse çalışır). Arama dizininin iç
        // tabloları hariç tutulur: dizin küçükken alınan istatistik ("2 satır") dizin büyüdükçe
        // FTS5'in kendi sorgularına kötü plan seçtiriyordu (ilk eşitlemede ileti başına yazma
        // 0,5 ms'den 20 ms'nin üstüne çıkıyordu); FTS5 istatistiksiz çalışır. Kısa ömürlü ayrı
        // bağlantıda yapılır: SQLite bir bağlantıya yüklenmiş tablo boyutu tahminini, istatistik
        // satırı sonradan silinse de bırakmaz. Bellek içi veritabanında (testler) gerekmez.
        if let Some(p) = path.as_deref() {
            if let Ok(c) = Connection::open(p) {
                let _ = c.execute_batch("PRAGMA busy_timeout = 5000; PRAGMA optimize = 0x10002;");
                for t in ["sqlite_stat1", "sqlite_stat4"] {
                    let _ = c.execute(&format!("DELETE FROM {t} WHERE tbl LIKE 'message!_fts!_%' ESCAPE '!'"), []);
                }
            }
        }
        let conn = Arc::new(Mutex::new(conn));
        // Bellek içi veritabanında (testler) ayrı bağlantı açılamaz: aynı bağlantı kullanılır
        let mut readers: Vec<Arc<Mutex<Connection>>> = match path.as_deref() {
            Some(p) => (0..READERS).map_while(|_| Self::open_reader(p).ok()).map(|r| Arc::new(Mutex::new(r))).collect(),
            None => vec![],
        };
        if readers.is_empty() {
            readers.push(conn.clone());
        }
        let db = Db { conn, readers: Arc::new(readers), fts_ready: Arc::new(std::sync::atomic::AtomicBool::new(fts_ready)) };
        if db.setting("schema_version").is_none() {
            db.first_run()?;
        }
        Ok(db)
    }

    /// Veritabanı dosyasının boyutu, içindeki kullanılmayan (silinen postalardan boşalan) alan
    /// ve artımlı küçültme kipinde olup olmadığı (bayt)
    pub fn storage_info(&self) -> (i64, i64, bool) {
        self.rconn()
            .query_row(
                "SELECT page_count * page_size, freelist_count * page_size, auto_vacuum FROM pragma_page_count, pragma_page_size, pragma_freelist_count, pragma_auto_vacuum",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? == 2)),
            )
            .unwrap_or((0, 0, false))
    }

    /// Veritabanını sıkıştırır (VACUUM): kullanılmayan alan dosyadan geri alınır ve dosya artımlı
    /// küçültme kipine geçer (bundan sonra boşalan alan arka planda geri verilir). Sürerken
    /// yazmalar bekler, okumalar sürer. Döner: (önceki, sonraki) boyut.
    pub fn compact(&self) -> Result<(i64, i64)> {
        let before = self.storage_info().0;
        let c = self.conn();
        // Ara kopya bellekte değil, veri klasöründe (boş yeri denetlenen diskte) kurulur: büyük
        // posta kutusunda GB'larca bellek ya da küçük bir /tmp gerekirdi
        let dir = c.path().and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_string_lossy().replace('\'', "''"))).unwrap_or_default();
        c.execute_batch(&format!("PRAGMA temp_store_directory = '{dir}'; PRAGMA temp_store = FILE; PRAGMA auto_vacuum = INCREMENTAL;"))?;
        let done = c.execute_batch("VACUUM");
        let _ = c.execute_batch("PRAGMA temp_store = MEMORY; PRAGMA temp_store_directory = ''; PRAGMA wal_checkpoint(TRUNCATE);");
        done?;
        drop(c);
        Ok((before, self.storage_info().0))
    }

    /// Artımlı küçültme kipindeki veritabanında kullanılmayan alanın bir parçasını dosyadan geri
    /// verir (arka plan işi; yazma kilidi her parçada kısa süre tutulur). Boş alan azsa (8 MB'tan
    /// az) dokunulmaz. Döner: geri verilen sayfa sayısı.
    pub fn reclaim_free_space(&self, max_pages: i64) -> i64 {
        let c = self.conn();
        let free = |c: &Connection| c.query_row("SELECT freelist_count FROM pragma_freelist_count", [], |r| r.get::<_, i64>(0)).unwrap_or(0);
        let mode: i64 = c.query_row("PRAGMA auto_vacuum", [], |r| r.get(0)).unwrap_or(0);
        let before = free(&c);
        if mode != 2 || before < 2048 {
            return 0;
        }
        // Her adımda bir sayfa geri verilir: ifade sonuna kadar adımlanmalı (execute_batch tek adım atar)
        if let Ok(mut st) = c.prepare(&format!("PRAGMA incremental_vacuum({max_pages})")) {
            if let Ok(mut rows) = st.query([]) {
                while let Ok(Some(_)) = rows.next() {}
            }
        }
        before - free(&c)
    }

    /// Veritabanının bulunduğu diskteki boş alan (bayt; bilinmiyorsa None)
    pub fn free_disk_space(&self) -> Option<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let dir = self.conn().path().and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf()))?;
            let cpath = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(cpath.as_ptr(), &mut st) } != 0 {
                return None;
            }
            Some(st.f_bavail as u64 * st.f_frsize as u64)
        }
        #[cfg(not(unix))]
        None
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bu bağlantıyla açılıştan beri yapılan satır değişikliği sayısı (eşitleme bir şey
    /// değiştirdi mi anlamak için; tüm yazmalar bu bağlantıdan yapılır)
    pub fn total_changes(&self) -> u64 {
        self.conn().query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0)).map(|v| v as u64).unwrap_or(0)
    }

    /// Okuma bağlantısı (yalnızca SELECT): boştaki ilk bağlantı; hepsi meşgulse sırayla biri beklenir
    fn rconn(&self) -> MutexGuard<'_, Connection> {
        for r in self.readers.iter() {
            match r.try_lock() {
                Ok(g) => return g,
                Err(std::sync::TryLockError::Poisoned(e)) => return e.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let i = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.readers.len();
        self.readers[i].lock().unwrap_or_else(|e| e.into_inner())
    }

    fn first_run(&self) -> Result<()> {
        self.set_setting("schema_version", "1");
        {
            let c = self.conn();
            for (name, color) in [
                ("cat.red", "#e74c3c"),
                ("cat.orange", "#e67e22"),
                ("cat.yellow", "#f1c40f"),
                ("cat.green", "#27ae60"),
                ("cat.blue", "#2980b9"),
                ("cat.purple", "#8e44ad"),
            ] {
                c.execute(
                    "INSERT OR IGNORE INTO categories(name, color) VALUES (?1, ?2)",
                    params![crate::i18n::tr(name), color],
                )?;
            }
            c.execute(
                "INSERT INTO calendars(name, color) VALUES (?1, '#3a7bd5')",
                params![crate::i18n::tr("calendar.default_name")],
            )?;
            for (pos, role) in ROLES.iter().enumerate() {
                c.execute(
                    "INSERT INTO folders(account_id, name, role, position) VALUES (NULL, ?1, ?2, ?3)",
                    params![crate::i18n::tr(&format!("folder.{role}")), role, pos as i64],
                )?;
            }
        }
        // Hoş geldiniz iletisi
        if let Some(inbox) = self.folder_by_role(None, "inbox") {
            let body = crate::i18n::tr("welcome.body");
            let raw = crate::mail::build::simple_raw(
                "Lynora <lynora@lynora.local>",
                &crate::i18n::tr("welcome.to"),
                &crate::i18n::tr("welcome.subject"),
                &body,
            );
            let mut m = crate::mail::parse::parse(&raw).into_message();
            m.folder_id = inbox.id;
            m.is_read = false;
            self.insert_message(&m)?;
        }
        Ok(())
    }

    /// İlk hesap eklenince yerel "hoş geldiniz" iletisi kaldırılır; böylece
    /// (Outlook'ta olmayan) yerel klasörler boşsa gizli kalır.
    pub fn remove_welcome(&self) {
        let _ = self.conn().execute(
            "DELETE FROM messages WHERE (sender LIKE '%<lynora@lynora.local>%' OR sender LIKE '%<mailnix@pardus.local>%' OR sender LIKE '%<lynora@pardus.local>%') AND folder_id IN \
             (SELECT id FROM folders WHERE account_id IS NULL AND role = 'inbox')",
            [],
        );
    }

    // ------------------------------------------------------------------ ayarlar
    pub fn setting(&self, key: &str) -> Option<String> {
        self.rconn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn setting_or(&self, key: &str, default: &str) -> String {
        self.setting(key).unwrap_or_else(|| default.to_string())
    }

    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.setting(key).map(|v| v == "1").unwrap_or(default)
    }

    pub fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.setting(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// Veritabanının tutarlı kopyası (uygulama çalışırken de alınabilir)
    pub fn snapshot_to(&self, path: &std::path::Path) -> Result<()> {
        self.conn().execute("VACUUM INTO ?1", params![path.to_string_lossy().to_string()]).map(|_| ())
    }

    /// Yedek özeti için kayıt sayısı; `table` koddaki sabit bir addır
    pub fn table_count(&self, table: &str) -> i64 {
        self.rconn().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap_or(0)
    }

    /// Kullanıcının tüm adresleri (küçük harf): hesaplar ve takma adlar
    pub fn my_addresses(&self) -> Vec<String> {
        let mut me: Vec<String> = self.accounts().iter().map(|a| a.email.trim().to_lowercase()).collect();
        let aliases = self.setting("identities").and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok()).unwrap_or_default();
        me.extend(aliases.iter().filter_map(|v| v.get("email").and_then(|e| e.as_str()).map(|e| e.trim().to_lowercase())));
        me
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let _ = self.conn().execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
            params![key, value],
        );
    }

    // ------------------------------------------------------------------ hesaplar
    pub fn accounts(&self) -> Vec<Account> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM accounts ORDER BY id").unwrap();
        st.query_map([], Account::from_row).unwrap().flatten().collect()
    }

    pub fn account(&self, id: i64) -> Option<Account> {
        self.rconn()
            .query_row("SELECT * FROM accounts WHERE id = ?1", [id], Account::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_account(&self, a: &Account) -> Result<i64> {
        let c = self.conn();
        if a.id > 0 {
            c.execute(
                "UPDATE accounts SET name=?1, email=?2, display_name=?3, imap_host=?4, imap_port=?5, \
                 imap_security=?6, smtp_host=?7, smtp_port=?8, smtp_security=?9, username=?10, \
                 password=?11, signature=?12, sync_limit=?13, auto_reply_enabled=?14, \
                 auto_reply_text=?15, kind=?17, auth_method=?18, ews_url=?19, auth_state=?20, auth_error=?21, \
                 oauth_token=?22, shared_of=?23, caldav_url=?24, carddav_url=?25 WHERE id=?16",
                params![a.name, a.email, a.display_name, a.imap_host, a.imap_port, a.imap_security,
                    a.smtp_host, a.smtp_port, a.smtp_security, a.username, a.password, a.signature,
                    a.sync_limit, a.auto_reply_enabled as i64, a.auto_reply_text, a.id, a.kind,
                    a.auth_method, a.ews_url, a.auth_state, a.auth_error, a.oauth_token, a.shared_of,
                    a.caldav_url, a.carddav_url],
            )?;
            Ok(a.id)
        } else {
            c.execute(
                "INSERT INTO accounts(name, email, display_name, imap_host, imap_port, imap_security, \
                 smtp_host, smtp_port, smtp_security, username, password, signature, sync_limit, \
                 auto_reply_enabled, auto_reply_text, created, kind, auth_method, ews_url, auth_state, \
                 auth_error, oauth_token, shared_of, caldav_url, carddav_url) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                params![a.name, a.email, a.display_name, a.imap_host, a.imap_port, a.imap_security,
                    a.smtp_host, a.smtp_port, a.smtp_security, a.username, a.password, a.signature,
                    a.sync_limit, a.auto_reply_enabled as i64, a.auto_reply_text, now(),
                    if a.kind.is_empty() { "imap" } else { a.kind.as_str() },
                    if a.auth_method.is_empty() { "password" } else { a.auth_method.as_str() },
                    a.ews_url, if a.auth_state.is_empty() { "ok" } else { a.auth_state.as_str() },
                    a.auth_error, a.oauth_token, a.shared_of, a.caldav_url, a.carddav_url],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    /// Ana hesabın parolası değişince paylaşılan kutuları da yeniden etkinleşir
    pub fn update_shared_credentials(&self, base: i64, password: &str, oauth_token: &str) {
        let _ = self.conn().execute(
            "UPDATE accounts SET password = ?2, oauth_token = ?3, auth_state = 'ok', auth_error = '' WHERE shared_of = ?1",
            params![base, password, oauth_token],
        );
    }

    /// Kimlik doğrulama durumunu günceller ("ok" / "failed").
    pub fn set_account_auth(&self, id: i64, state: &str, error: &str) {
        let _ = self.conn().execute(
            "UPDATE accounts SET auth_state = ?1, auth_error = ?2 WHERE id = ?3",
            params![state, error, id],
        );
    }

    pub fn delete_account(&self, id: i64) -> Result<()> {
        let c = self.conn();
        c.execute(
            "DELETE FROM messages WHERE folder_id IN (SELECT id FROM folders WHERE account_id = ?1)",
            [id],
        )?;
        c.execute("DELETE FROM folders WHERE account_id = ?1", [id])?;
        c.execute("DELETE FROM thread_keys WHERE account_id = ?1", [id])?;
        // Sunucudan eşitlenen takvim ve kişiler hesapla gider
        c.execute("DELETE FROM events WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id = ?1 AND kind = 'dav')", [id])?;
        c.execute("DELETE FROM calendars WHERE account_id = ?1 AND kind = 'dav'", [id])?;
        c.execute("DELETE FROM contacts WHERE account_id = ?1 AND account_id IN (SELECT id FROM accounts WHERE carddav_url != '')", [id])?;
        c.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        Ok(())
    }

    // ------------------------------------------------------------------ klasörler
    pub fn folders(&self, account_id: Option<i64>) -> Vec<Folder> {
        let c = self.rconn();
        let mut v: Vec<Folder> = match account_id {
            Some(a) => {
                let mut st = c.prepare("SELECT * FROM folders WHERE account_id = ?1").unwrap();
                st.query_map([a], Folder::from_row).unwrap().flatten().collect()
            }
            None => {
                let mut st = c.prepare("SELECT * FROM folders WHERE account_id IS NULL").unwrap();
                st.query_map([], Folder::from_row).unwrap().flatten().collect()
            }
        };
        v.sort_by(|a, b| {
            (role_order(a.role_str()), a.position, a.name.to_lowercase()).cmp(&(
                role_order(b.role_str()),
                b.position,
                b.name.to_lowercase(),
            ))
        });
        v
    }

    pub fn all_folders(&self) -> Vec<Folder> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM folders").unwrap();
        st.query_map([], Folder::from_row).unwrap().flatten().collect()
    }

    pub fn folder(&self, id: i64) -> Option<Folder> {
        self.rconn()
            .query_row("SELECT * FROM folders WHERE id = ?1", [id], Folder::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn folder_by_role(&self, account_id: Option<i64>, role: &str) -> Option<Folder> {
        let c = self.rconn();
        let r = match account_id {
            Some(a) => c
                .query_row(
                    "SELECT * FROM folders WHERE account_id = ?1 AND role = ?2",
                    params![a, role],
                    Folder::from_row,
                )
                .optional(),
            None => c
                .query_row(
                    "SELECT * FROM folders WHERE account_id IS NULL AND role = ?1",
                    [role],
                    Folder::from_row,
                )
                .optional(),
        };
        r.ok().flatten()
    }

    /// Hesabın rol klasörü; yoksa yerel klasör.
    pub fn folder_by_role_or_local(&self, account_id: Option<i64>, role: &str) -> Option<Folder> {
        self.folder_by_role(account_id, role).or_else(|| self.folder_by_role(None, role))
    }

    pub fn insert_folder(&self, f: &Folder) -> Result<i64> {
        let c = self.conn();
        c.execute(
            "INSERT INTO folders(account_id, name, remote_name, role, parent_id, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![f.account_id, f.name, f.remote_name, f.role, f.parent_id, f.position],
        )?;
        Ok(c.last_insert_rowid())
    }

    /// Değişen bir şey yoksa yazılmaz: eşitleme her turda klasör durumunu kaydeder ve
    /// arayüz yenilemesi `total_changes` ile gerçek bir değişiklik olup olmadığına bakar
    pub fn update_folder(&self, f: &Folder) -> Result<()> {
        self.conn().execute(
            "UPDATE folders SET name=?1, remote_name=?2, role=?3, parent_id=?4, uidvalidity=?5, \
             sync_state=?7, favorite=?8, full_sync=?9 WHERE id=?6 AND (name IS NOT ?1 OR remote_name IS NOT ?2 \
             OR role IS NOT ?3 OR parent_id IS NOT ?4 OR uidvalidity IS NOT ?5 OR sync_state IS NOT ?7 \
             OR favorite IS NOT ?8 OR full_sync IS NOT ?9)",
            params![f.name, f.remote_name, f.role, f.parent_id, f.uidvalidity, f.id, f.sync_state,
                f.favorite as i64, f.full_sync as i64],
        )?;
        Ok(())
    }

    pub fn delete_folder(&self, id: i64) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM messages WHERE folder_id = ?1", [id])?;
        c.execute("UPDATE folders SET parent_id = NULL WHERE parent_id = ?1", [id])?;
        c.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn unread_count(&self, folder_id: i64) -> i64 {
        self.rconn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE folder_id = ?1 AND is_read = 0",
                [folder_id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// Hesabın çevrimdışı adres defterini yenisiyle değiştirir (tek işlemde)
    pub fn replace_directory(&self, account_id: i64, rows: &[DirRow]) {
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return };
        let _ = tx.execute("DELETE FROM directory WHERE account_id = ?1", [account_id]);
        if let Ok(mut st) = tx.prepare_cached(
            "INSERT OR REPLACE INTO directory (account_id, email, name, title, department, office, phone, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        ) {
            for r in rows {
                let _ = st.execute(params![account_id, r.email.to_lowercase(), r.name, r.title, r.department, r.office, r.phone, r.kind]);
            }
        }
        let _ = tx.commit();
    }

    /// Hesabın çevrimdışı adres defterindeki kayıt sayısı
    pub fn directory_count(&self, account_id: i64) -> i64 {
        self.rconn().query_row("SELECT COUNT(*) FROM directory WHERE account_id = ?1", [account_id], |r| r.get(0)).unwrap_or(0)
    }

    /// Çevrimdışı adres defterinde arama (ad, adres, unvan, birim); boş aramada ilk kayıtlar
    pub fn directory_search(&self, account_id: Option<i64>, query: &str, offset: usize, limit: usize) -> Vec<DirRow> {
        let q = fold(query.trim());
        let c = self.rconn();
        let map = |r: &rusqlite::Row| Ok(DirRow { email: r.get(0)?, name: r.get(1)?, title: r.get(2)?, department: r.get(3)?, office: r.get(4)?, phone: r.get(5)?, kind: r.get(6)? });
        if q.is_empty() {
            let Ok(mut st) = c.prepare_cached("SELECT email, name, title, department, office, phone, kind FROM directory WHERE (?1 IS NULL OR account_id = ?1) ORDER BY name COLLATE NOCASE LIMIT ?2 OFFSET ?3") else { return vec![] };
            return st.query_map(params![account_id, limit as i64, offset as i64], map).map(|rows| rows.flatten().collect()).unwrap_or_default();
        }
        // Türkçe harfler (İ/ı, Ş, Ğ...) SQLite'ın lower()'ı ile eşleşmez: süzme burada,
        // büyük/küçük harf ve i/ı farkı gözetilmeden yapılır ("KALAMIŞ" ~ "Kalamış")
        let Ok(mut st) = c.prepare_cached("SELECT email, name, title, department, office, phone, kind FROM directory WHERE (?1 IS NULL OR account_id = ?1) ORDER BY name COLLATE NOCASE") else { return vec![] };
        st.query_map(params![account_id], map)
            .map(|rows| {
                rows.flatten()
                    .filter(|r| [&r.name, &r.email, &r.department, &r.title, &r.office].iter().any(|x| fold(x).contains(&q)))
                    .skip(offset)
                    .take(limit)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Adreslerin çevrimdışı adres defterindeki kayıtları (görünen ad ve grup bilgisi için)
    pub fn directory_lookup(&self, emails: &[String]) -> std::collections::HashMap<String, DirRow> {
        let c = self.rconn();
        let mut out = std::collections::HashMap::new();
        let Ok(mut st) = c.prepare_cached("SELECT email, name, title, department, office, phone, kind FROM directory WHERE email = ?1 LIMIT 1") else { return out };
        for e in emails {
            let k = e.trim().to_lowercase();
            if let Ok(r) = st.query_row([&k], |r| {
                Ok(DirRow { email: r.get(0)?, name: r.get(1)?, title: r.get(2)?, department: r.get(3)?, office: r.get(4)?, phone: r.get(5)?, kind: r.get(6)? })
            }) {
                out.insert(k, r);
            }
        }
        out
    }

    /// Gövdesi henüz inmemiş sunucu iletisi mi (ham içeriği okumadan)
    pub fn needs_content(&self, id: i64) -> bool {
        self.rconn()
            .query_row(
                "SELECT COALESCE((SELECT length(raw) FROM message_content c WHERE c.id = m.id), 0) = 0 AND m.remote_id IS NOT NULL FROM messages m WHERE m.id = ?1",
                [id],
                |r| r.get::<_, Option<bool>>(0),
            )
            .ok()
            .flatten()
            .unwrap_or(false)
    }

    /// Tüm klasörlerin (toplam, okunmamış) ileti sayıları tek sorguda (klasör ağacı için)
    pub fn folder_counts(&self) -> std::collections::HashMap<i64, (i64, i64)> {
        let c = self.rconn();
        let Ok(mut st) = c.prepare_cached("SELECT folder_id, COUNT(*), SUM(is_read = 0) FROM messages GROUP BY folder_id") else { return Default::default() };
        st.query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0)))))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Arama kutusu önerileri (Outlook gibi): kişiler (postalarda en sık geçenler önce, sonra
    /// adres defteri ve kişiler) ve adı eşleşen ekler. Döner: (kişiler, ekler)
    pub fn search_suggest(&self, q: &str) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        use serde_json::json;
        let words = search_words(q);
        let Some(longest) = words.iter().max_by_key(|w| w.chars().count()).cloned() else { return (vec![], vec![]) };
        let pat = like_pattern(&longest);
        // Kişiler: gönderenler (ileti sayısıyla). Okuma bağlantısı her sorguda ayrı alınır:
        // adres defteri/kişiler sorguları da aynı bağlantıyı kullanır (tutulursa kilitlenir)
        let mut people: Vec<(String, String, i64)> = Vec::new();
        if let Ok(mut st) = self.rconn().prepare_cached("SELECT sender, COUNT(*) FROM messages WHERE sender LIKE ?1 GROUP BY sender ORDER BY 2 DESC LIMIT 400") {
            let rows: Vec<(String, i64)> = st.query_map([&pat], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map(|r| r.flatten().collect()).unwrap_or_default();
            for (sender, n) in rows {
                let (name, email) = crate::mail::split_name_addr(&sender);
                if !email.contains('@') || !words_match(&format!("{name} {email}"), &words, false) {
                    continue;
                }
                let key = email.to_lowercase();
                match people.iter_mut().find(|p| p.1.to_lowercase() == key) {
                    Some(p) => {
                        p.2 += n;
                        if p.0.is_empty() {
                            p.0 = name;
                        }
                    }
                    None => people.push((name, email, n)),
                }
            }
        }
        // Adres defteri ve kişiler (hiç yazışılmamış olsa da)
        let mut extra: Vec<(String, String)> = self.directory_search(None, &longest, 0, 60).into_iter().map(|d| (d.name, d.email)).collect();
        extra.extend(self.contacts(&longest).into_iter().filter(|x| !x.email.is_empty()).map(|x| (x.display_name, x.email)));
        for (name, email) in extra {
            if email.contains('@') && words_match(&format!("{name} {email}"), &words, false) && !people.iter().any(|p| p.1.eq_ignore_ascii_case(&email)) {
                people.push((name, email, 0));
            }
        }
        people.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        let people = people.into_iter().take(6).map(|(name, email, n)| json!({"name": name, "email": email, "count": n})).collect();
        // Ekler: adı eşleşen dosyalar (en yeni önce, aynı ad bir kez)
        let mut atts: Vec<serde_json::Value> = Vec::new();
        if let Ok(mut st) = self.rconn().prepare_cached("SELECT id, att_names, subject, sender, date FROM messages WHERE att_names LIKE ?1 ORDER BY date DESC LIMIT 300") {
            let rows: Vec<(i64, String, String, String, f64)> = st
                .query_map([&pat], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get::<_, Option<String>>(3)?.unwrap_or_default(), r.get::<_, Option<f64>>(4)?.unwrap_or(0.0))))
                .map(|r| r.flatten().collect())
                .unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            'rows: for (id, names, subject, sender, date) in rows {
                for name in names.lines().map(str::trim).filter(|n| !n.is_empty()) {
                    if words_match(name, &words, false) && seen.insert(name.to_lowercase()) {
                        let (sn, se) = crate::mail::split_name_addr(&sender);
                        atts.push(json!({"id": id, "name": name, "subject": subject, "sender": if sn.is_empty() { se } else { sn }, "date": date}));
                        if atts.len() >= 5 {
                            break 'rows;
                        }
                    }
                }
            }
        }
        (people, atts)
    }

    /// Eski iletilerin ek adlarını doldurur (içeriği indirilmiş, eki olan iletiler). İşlenen sayı.
    pub fn backfill_att_names(&self, batch: i64) -> usize {
        let rows: Vec<(i64, Vec<u8>)> = {
            let c = self.rconn();
            let Ok(mut st) = c.prepare(
                "SELECT m.id, c.raw FROM messages m JOIN message_content c ON c.id = m.id \
                 WHERE m.has_attachments = 1 AND (m.att_names IS NULL OR m.att_names = '') AND length(c.raw) > 0 LIMIT ?1",
            ) else {
                return 0;
            };
            st.query_map([batch], |r| Ok((r.get(0)?, r.get::<_, Vec<u8>>(1)?))).map(|r| r.flatten().collect()).unwrap_or_default()
        };
        for (id, raw) in &rows {
            let p = crate::mail::parse::parse(raw);
            let names = p.real_attachments().iter().map(|a| a.name.replace('\n', " ")).filter(|n| !n.trim().is_empty()).collect::<Vec<_>>().join("\n");
            // Adı olmayan ekler de bir daha işlenmesin diye boşluk yazılır
            let _ = self.conn().execute("UPDATE messages SET att_names = ?1 WHERE id = ?2", params![if names.is_empty() { " ".to_string() } else { names }, id]);
        }
        rows.len()
    }

    /// Gelen Kutusu/Kutularındaki okunmamış iletiler (görev çubuğu sayacı, Outlook gibi)
    pub fn inbox_unread(&self) -> i64 {
        self.rconn()
            .query_row("SELECT COUNT(*) FROM messages WHERE is_read = 0 AND folder_id IN (SELECT id FROM folders WHERE role = 'inbox')", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn total_count(&self, folder_id: i64) -> i64 {
        self.rconn()
            .query_row("SELECT COUNT(*) FROM messages WHERE folder_id = ?1", [folder_id], |r| {
                r.get(0)
            })
            .unwrap_or(0)
    }

    // ------------------------------------------------------------------ iletiler
    /// `fts`: arama dizini hazır (değilse metin sütunları LIKE ile taranır)
    fn filter_sql(search: &SearchExpr, filter: &MsgFilter, fts: bool, sql: &mut String, args: &mut Vec<String>) {
        if let Some(cond) = expr_sql(search, fts, args) {
            sql.push_str(" AND ");
            sql.push_str(&cond);
        }
        match filter {
            MsgFilter::All => {}
            MsgFilter::Unread => sql.push_str(" AND is_read = 0"),
            MsgFilter::Flagged => sql.push_str(" AND is_flagged = 1"),
            MsgFilter::Attachments => sql.push_str(" AND has_attachments = 1"),
            MsgFilter::Important => sql.push_str(" AND importance = 2"),
            MsgFilter::Category(c) => {
                sql.push_str(" AND categories LIKE ?");
                args.push(format!("%{c}%"));
            }
        }
    }

    /// Aramaya uyan ileti sayısı (en çok 1'e kadar bakılır: yalnızca var/yok için)
    fn search_count(&self, folder_id: Option<i64>, search: &SearchExpr, filter: &MsgFilter) -> i64 {
        let mut args: Vec<String> = Vec::new();
        let wh = self.where_sql(folder_id, search, filter, &mut args);
        self.rconn()
            .query_row(&format!("SELECT COUNT(*) FROM (SELECT 1 FROM messages{wh} LIMIT 1)"), rusqlite::params_from_iter(args.iter()), |r| r.get(0))
            .unwrap_or(0)
    }

    /// Klasördeki tüm iletiler (arka plan işleri için; arayüz sayfalı listeyi kullanır).
    pub fn messages(&self, folder_id: Option<i64>, search: &str, filter: &MsgFilter) -> Vec<MessageRow> {
        self.messages_page(folder_id, search, filter, &ListOrder::default(), 0, i64::MAX).0
    }

    /// Kişi alanında aranan tam adres, o kişinin bilinen adlarıyla da aranır: Exchange'den yalnızca
    /// başlığı alınmış iletilerde alıcılar adlarıyla durur ("kime:ali.veli@kurum.com.tr" yine bulur)
    fn expand_people(&self, e: SearchExpr) -> SearchExpr {
        match e {
            SearchExpr::And(v) => SearchExpr::And(v.into_iter().map(|x| self.expand_people(x)).collect()),
            SearchExpr::Or(v) => SearchExpr::Or(v.into_iter().map(|x| self.expand_people(x)).collect()),
            SearchExpr::Not(x) => SearchExpr::Not(Box::new(self.expand_people(*x))),
            SearchExpr::Term(SearchTerm::Field(cols, v)) if [PEOPLE_COLS, FROM_COLS, TO_COLS, CC_COLS, BCC_COLS].contains(&cols) && v.contains('@') && !v.contains(char::is_whitespace) => {
                let addr = v.trim_matches(['<', '>']).to_string();
                let mut names: Vec<String> = {
                    let c = self.rconn();
                    let q = |sql: &str| -> Vec<String> {
                        c.prepare_cached(sql).and_then(|mut st| st.query_map([&addr], |r| r.get::<_, String>(0)).map(|r| r.flatten().collect())).unwrap_or_default()
                    };
                    let mut n = q("SELECT name FROM directory WHERE email = ?1 COLLATE NOCASE AND name <> '' LIMIT 2");
                    n.extend(q("SELECT display_name FROM contacts WHERE (email = ?1 COLLATE NOCASE OR email2 = ?1 COLLATE NOCASE) AND display_name <> '' LIMIT 2"));
                    n
                };
                if names.is_empty() {
                    names.extend(name_from_address(&addr));
                }
                names.retain(|n| search_words(n) != search_words(&addr));
                names.sort();
                names.dedup();
                let mut alts = vec![SearchExpr::Term(SearchTerm::Field(cols, v))];
                alts.extend(names.into_iter().map(|n| SearchExpr::Term(SearchTerm::Field(cols, n))));
                if alts.len() == 1 { alts.pop().unwrap() } else { SearchExpr::Or(alts) }
            }
            t => t,
        }
    }

    fn where_sql(&self, folder_id: Option<i64>, search: &SearchExpr, filter: &MsgFilter, args: &mut Vec<String>) -> String {
        let mut sql = String::from(" WHERE 1=1");
        if let Some(f) = folder_id {
            sql.push_str(&format!(" AND folder_id = {f}"));
        } else {
            // Tüm posta kutularında: çöp ve gereksiz hariç
            sql.push_str(" AND folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash','junk'))");
        }
        Self::filter_sql(&self.expand_people(search.clone()), filter, self.fts_ready.load(std::sync::atomic::Ordering::Relaxed), &mut sql, args);
        sql
    }

    /// Outlook gibi sayfalı liste: sıralama ve (isteğe bağlı) konuşma gruplaması
    /// veritabanında yapılır. Dönen ikinci değer toplam satır sayısıdır.
    pub fn messages_page(&self, folder_id: Option<i64>, search: &str, filter: &MsgFilter, order: &ListOrder, offset: i64, limit: i64) -> (Vec<MessageRow>, i64) {
        // "ahmet emre aydın" gibi birden çok düz sözcük: önce ifade olarak (ad soyad gibi arka
        // arkaya) aranır; eşleşme varsa yalnızca bunlar gösterilir (dağıtım listelerinde ayrı
        // ayrı geçen Ahmet, Emre, Aydın'lar sonuçları doldurmaz), yoksa tüm sözcükler aranır
        let search = match phrase_variant(search) {
            Some(p) if self.search_count(folder_id, &p, filter) > 0 => p,
            _ => parse_query(search),
        };
        let mut args: Vec<String> = Vec::new();
        let wh = self.where_sql(folder_id, &search, filter, &mut args);
        let dir = if order.desc { "DESC" } else { "ASC" };
        let key = match order.by.as_str() {
            "from" => format!("ltrim(sender, '\" ') COLLATE NOCASE {dir}, "),
            "to" => format!("ltrim(to_addr, '\" ') COLLATE NOCASE {dir}, "),
            "subject" => format!("ns {dir}, "),
            "importance" => format!("importance {dir}, "),
            "flag" => format!("is_flagged {dir}, "),
            "attachments" => format!("has_attachments {dir}, "),
            "read" => format!("is_read {dir}, "),
            _ => String::new(),
        };
        // Sabitlenmiş iletiler tarih sıralamasında her zaman en üstte (Outlook)
        let pin = if order.by == "date" || order.by.is_empty() { "is_pinned DESC, " } else { "" };
        let order_sql = format!(" ORDER BY {pin}{key}date {dir}, id {dir}");
        let c = self.rconn();
        let (sql, count_sql) = if order.conversations {
            // Konuşma başına en yeni ileti ve okunmamış sayısı dizinden (folder_id, tid, date,
            // is_read) bulunur; yalnızca bu satırlar okunur. SQLite: tek MAX() içeren gruplamada
            // yalın sütun (id) en büyük değerli satırdandır.
            (
                format!(
                    "SELECT {LIST_COLS}, g.u AS thread_unread FROM messages JOIN \
                     (SELECT id AS head, MAX(date) AS newest, SUM(is_read = 0) AS u FROM messages{wh} GROUP BY tid) g ON messages.id = g.head\
                     {order_sql} LIMIT {limit} OFFSET {offset}"
                ),
                format!("SELECT COUNT(DISTINCT tid) FROM messages{wh}"),
            )
        } else {
            (
                format!("SELECT {LIST_COLS} FROM messages{wh}{order_sql} LIMIT {limit} OFFSET {offset}"),
                format!("SELECT COUNT(*) FROM messages{wh}"),
            )
        };
        let mut rows: Vec<MessageRow> = match c.prepare(&sql) {
            Ok(mut st) => st
                .query_map(rusqlite::params_from_iter(args.iter()), MessageRow::from_row)
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
            Err(_) => vec![],
        };
        // Konuşmadaki ileti sayısı (diğer klasörlerdekiler, ör. gönderilen yanıtlar dahil) yalnızca
        // sayfadaki satırlar için sayılır
        if order.conversations {
            if let Ok(mut st) = c.prepare_cached(&format!("SELECT COUNT(*) FROM messages WHERE tid = ?1 AND {THREAD_SCOPE}")) {
                for m in &mut rows {
                    m.thread_count = st.query_row([m.thread_id, m.folder_id], |r| r.get(0)).unwrap_or(1);
                }
            }
        }
        // Toplam yalnızca ilk sayfada hesaplanır (sonraki sayfalarda -1)
        let total = if offset == 0 {
            if (rows.len() as i64) < limit {
                rows.len() as i64
            } else {
                c.query_row(&count_sql, rusqlite::params_from_iter(args.iter()), |r| r.get(0)).unwrap_or(0)
            }
        } else {
            -1
        };
        (rows, total)
    }

    pub fn message(&self, id: i64) -> Option<Message> {
        self.rconn()
            .query_row(
                "SELECT m.*, COALESCE(c.body_text, '') AS body_text, COALESCE(c.body_html, '') AS body_html, c.raw AS raw \
                 FROM messages m LEFT JOIN message_content c ON c.id = m.id WHERE m.id = ?1",
                [id],
                Message::from_row,
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn message_row(&self, id: i64) -> Option<MessageRow> {
        self.rconn()
            .query_row(&format!("SELECT {LIST_COLS} FROM messages WHERE id = ?1"), [id], MessageRow::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn messages_by_message_id(&self, mid: &str) -> Vec<MessageRow> {
        let c = self.rconn();
        let mut st = c
            .prepare(&format!("SELECT {LIST_COLS} FROM messages WHERE message_id = ?1"))
            .unwrap();
        st.query_map([mid], MessageRow::from_row).unwrap().flatten().collect()
    }

    /// Bir konuşmaya ait iletiler (iletinin kendi klasörü ve diğer klasörler; çöp ve gereksiz
    /// hariç): konuşma görünümündeki gruplamayla aynı anahtar (`tid`), dizinden
    pub fn conversation(&self, m: &MessageRow) -> Vec<MessageRow> {
        let c = self.rconn();
        let Ok(mut st) = c.prepare_cached(&format!("SELECT {LIST_COLS} FROM messages WHERE tid = ?1 AND {THREAD_SCOPE} ORDER BY date, id")) else {
            return vec![];
        };
        st.query_map([m.thread_id, m.folder_id], MessageRow::from_row).map(|r| r.flatten().collect()).unwrap_or_default()
    }

    pub fn insert_message(&self, m: &Message) -> Result<i64> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO messages(folder_id, account_id, uid, message_id, in_reply_to, refs, subject, \
             sender, to_addr, cc, bcc, date, snippet, is_read, is_flagged, \
             is_draft, is_answered, importance, categories, has_attachments, pending, remote_id, is_pinned, att_names, conv, ns) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,lyn_norm_subject(?7))",
            params![m.folder_id, m.account_id, m.uid, m.message_id, m.in_reply_to, m.refs, m.subject,
                m.sender, m.to_addr, m.cc, m.bcc, m.date, m.snippet,
                m.is_read as i64, m.is_flagged as i64, m.is_draft as i64, m.is_answered as i64,
                m.importance, m.categories, m.has_attachments as i64, m.pending, m.remote_id, m.is_pinned as i64, m.att_names, m.conv],
        )?;
        let id = tx.last_insert_rowid();
        Self::assign_thread(&tx, id)?;
        // Yalnızca başlığı inmiş (Exchange) iletinin gövde satırı yoktur
        if !m.raw.is_empty() || !m.body_text.is_empty() || !m.body_html.is_empty() {
            Self::write_content(&tx, id, m)?;
        }
        tx.commit()?;
        Ok(id)
    }

    fn write_content(c: &Connection, id: i64, m: &Message) -> Result<usize> {
        c.execute(
            // UPSERT (REPLACE değil): değişen gövde güncelleme tetikleyicisinde eski hâliyle görülür
            "INSERT INTO message_content(id, body_text, body_html, raw) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(id) DO UPDATE SET body_text = excluded.body_text, body_html = excluded.body_html, raw = excluded.raw",
            params![id, m.body_text, m.body_html, m.raw],
        )
    }

    /// Yalnızca başlığı bilinen (Exchange) iletinin tam içeriğini yazar.
    pub fn update_message_content(&self, id: i64, m: &Message) {
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return };
        let ns = |tx: &Connection| tx.query_row("SELECT COALESCE(ns, '') FROM messages WHERE id = ?1", [id], |r| r.get::<_, String>(0)).unwrap_or_default();
        let before = ns(&tx);
        let updated = tx.execute(
            "UPDATE messages SET sender=?1, to_addr=?2, cc=?3, bcc=?4, \
             in_reply_to=?5, refs=?6, has_attachments=?7, snippet=?8, att_names=?12, \
             message_id=CASE WHEN message_id='' THEN ?9 ELSE message_id END, \
             subject=CASE WHEN subject='' OR instr(subject, char(65533)) > 0 THEN ?11 ELSE subject END WHERE id=?10",
            params![m.sender, m.to_addr, m.cc, m.bcc, m.in_reply_to, m.refs,
                m.has_attachments as i64, m.snippet, m.message_id, id, m.subject, m.att_names],
        );
        // Konusu düzelen (bozuk kodlu) ileti eski konuşmasından ayrılır
        if ns(&tx) != before {
            let _ = tx.execute(
                "DELETE FROM thread_keys WHERE (account_id, key) IN (SELECT COALESCE(account_id, 0), 'm:' || trim(message_id, '<> ') || char(31) || ?2 FROM messages WHERE id = ?1)",
                params![id, before],
            );
            let _ = tx.execute("UPDATE messages SET tid = NULL WHERE id = ?1", [id]);
        }
        // İleti bu arada silindiyse gövde yazılmaz. Yanıt başlıkları şimdi öğrenildi: konuşma bağı
        // yenilenir.
        if matches!(updated, Ok(n) if n > 0) && Self::write_content(&tx, id, m).is_ok() && Self::assign_thread(&tx, id).is_ok() {
            let _ = tx.commit();
        }
    }

    pub fn newest_date(&self, folder_id: i64) -> Option<f64> {
        self.rconn()
            .query_row("SELECT MAX(date) FROM messages WHERE folder_id = ?1", [folder_id], |r| r.get::<_, Option<f64>>(0))
            .ok()
            .flatten()
    }

    /// En yeni n. iletinin tarihi (n'den az ileti varsa None)
    pub fn nth_newest_date(&self, folder_id: i64, n: usize) -> Option<f64> {
        self.rconn()
            .query_row(
                "SELECT date FROM messages WHERE folder_id = ?1 ORDER BY date DESC LIMIT 1 OFFSET ?2",
                params![folder_id, n.saturating_sub(1) as i64],
                |r| r.get::<_, f64>(0),
            )
            .ok()
    }

    /// Klasördeki en eski iletinin tarihi (daha eskileri yüklemek için)
    pub fn oldest_date(&self, folder_id: i64) -> Option<f64> {
        self.rconn()
            .query_row("SELECT MIN(date) FROM messages WHERE folder_id = ?1", [folder_id], |r| r.get::<_, Option<f64>>(0))
            .ok()
            .flatten()
    }

    /// İçeriği henüz indirilmemiş en yeni iletiler: (id, uzak kimlik)
    pub fn messages_without_content(&self, folder_id: i64, limit: usize) -> Vec<(i64, String)> {
        let c = self.rconn();
        let Ok(mut st) = c.prepare(
            "SELECT id, remote_id FROM messages m WHERE folder_id = ?1 AND remote_id IS NOT NULL \
             AND NOT EXISTS (SELECT 1 FROM message_content c WHERE c.id = m.id AND length(c.raw) > 0) ORDER BY date DESC LIMIT ?2",
        ) else {
            return vec![];
        };
        st.query_map(params![folder_id, limit as i64], |r| Ok((r.get(0)?, r.get(1)?))).map(|it| it.flatten().collect()).unwrap_or_default()
    }

    pub fn set_message_field(&self, id: i64, field: &str, value: impl rusqlite::ToSql) {
        let allowed = [
            "is_read", "is_flagged", "is_answered", "importance", "categories", "folder_id", "uid",
            "account_id", "pending", "snippet", "is_draft", "remote_id", "is_pinned",
        ];
        if allowed.contains(&field) {
            let _ = self
                .conn()
                .execute(&format!("UPDATE messages SET {field} = ?1 WHERE id = ?2"), params![value, id]);
        }
    }

    pub fn delete_message(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM messages WHERE id = ?1", [id]);
    }

    /// Çok iletiye aynı alan değeri tek işlemde (satır başına ayrı kayıt/disk yazımı yerine)
    pub fn set_messages_field(&self, ids: &[i64], field: &str, value: impl rusqlite::ToSql) {
        let allowed = ["is_read", "is_flagged", "is_answered", "importance", "categories", "is_pinned"];
        if ids.is_empty() || !allowed.contains(&field) {
            return;
        }
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return };
        if let Ok(mut st) = tx.prepare_cached(&format!("UPDATE messages SET {field} = ?1 WHERE id = ?2")) {
            for id in ids {
                let _ = st.execute(params![&value, id]);
            }
        }
        let _ = tx.commit();
    }

    /// Kullanıcının değiştirdiği bayrak (okundu, bayraklı, yanıtlandı): yerelde hemen yazılır,
    /// sunucuya yazılana kadar bekleyen olarak tutulur. Bekleyen alanın üzerine eşitleme
    /// sunucudaki eski değeri yazmaz; bağlantı yokken yapılan değişiklik de kaybolmaz.
    pub fn set_flags_local(&self, ids: &[i64], field: &str, value: bool) {
        if ids.is_empty() || !FLAG_FIELDS.contains(&field) {
            return;
        }
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return };
        {
            let Ok(mut set) = tx.prepare_cached(&format!("UPDATE messages SET {field} = ?1 WHERE id = ?2")) else { return };
            // Yalnızca sunucuda karşılığı olan iletiler (yerel klasörler beklemez)
            let Ok(mut pend) = tx.prepare_cached(
                "INSERT INTO pending_flags(message_id, field, value) SELECT id, ?2, ?3 FROM messages \
                 WHERE id = ?1 AND account_id IS NOT NULL AND (uid IS NOT NULL OR remote_id IS NOT NULL) \
                 ON CONFLICT(message_id, field) DO UPDATE SET value = excluded.value, attempts = 0",
            ) else {
                return;
            };
            for id in ids {
                let _ = set.execute(params![value as i64, id]);
                let _ = pend.execute(params![id, field, value as i64]);
            }
        }
        let _ = tx.commit();
    }

    /// Hesabın sunucuya yazılmayı bekleyen bayrakları
    pub fn pending_flags(&self, account_id: i64) -> Vec<PendingFlag> {
        let c = self.rconn();
        let Ok(mut st) = c.prepare_cached(
            "SELECT p.message_id, p.field, p.value, m.folder_id, m.uid, m.remote_id FROM pending_flags p \
             JOIN messages m ON m.id = p.message_id WHERE m.account_id = ?1 ORDER BY m.folder_id, p.field, p.value, p.message_id",
        ) else {
            return vec![];
        };
        st.query_map([account_id], |r| {
            Ok(PendingFlag {
                message_id: r.get(0)?,
                field: r.get(1)?,
                value: r.get::<_, i64>(2)? != 0,
                folder_id: r.get(3)?,
                uid: r.get(4)?,
                remote_id: r.get(5)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Sunucuya yazılan bayraklar bekleyenlerden düşer (bu arada yeniden değiştiyse kalır)
    pub fn clear_pending_flags(&self, done: &[PendingFlag]) {
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return };
        if let Ok(mut st) = tx.prepare_cached("DELETE FROM pending_flags WHERE message_id = ?1 AND field = ?2 AND value = ?3") {
            for p in done {
                let _ = st.execute(params![p.message_id, p.field, p.value as i64]);
            }
        }
        let _ = tx.commit();
    }

    /// Sunucunun reddettiği bayraklar: deneme sayısı artar, `max` denemeden sonra düşer
    /// (sunucudaki değer esas alınır). Düşenlerin sayısı döner.
    pub fn fail_pending_flags(&self, failed: &[PendingFlag], max: i64) -> usize {
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return 0 };
        let mut dropped = 0;
        if let (Ok(mut inc), Ok(mut del)) = (
            tx.prepare_cached("UPDATE pending_flags SET attempts = attempts + 1 WHERE message_id = ?1 AND field = ?2 AND value = ?3"),
            tx.prepare_cached("DELETE FROM pending_flags WHERE message_id = ?1 AND field = ?2 AND attempts >= ?3"),
        ) {
            for p in failed {
                let _ = inc.execute(params![p.message_id, p.field, p.value as i64]);
                dropped += del.execute(params![p.message_id, p.field, max]).unwrap_or(0);
            }
        }
        let _ = tx.commit();
        dropped
    }

    /// Sunucudaki bayrak değeri yerele yazılır; kullanıcının henüz sunucuya yazılmamış
    /// değişikliği varsa dokunulmaz. Değer gerçekten değiştiyse true.
    pub fn set_flag_from_server(&self, id: i64, field: &str, value: bool) -> bool {
        if !FLAG_FIELDS.contains(&field) {
            return false;
        }
        self.conn()
            .execute(
                &format!(
                    "UPDATE messages SET {field} = ?1 WHERE id = ?2 AND {field} != ?1 \
                     AND NOT EXISTS (SELECT 1 FROM pending_flags WHERE message_id = ?2 AND field = '{field}')"
                ),
                params![value as i64, id],
            )
            .is_ok_and(|n| n > 0)
    }

    /// Çok iletiyi tek işlemde siler
    pub fn delete_messages(&self, ids: &[i64]) {
        if ids.is_empty() {
            return;
        }
        let mut c = self.conn();
        let Ok(tx) = c.transaction() else { return };
        if let Ok(mut st) = tx.prepare_cached("DELETE FROM messages WHERE id = ?1") {
            for id in ids {
                let _ = st.execute([id]);
            }
        }
        let _ = tx.commit();
    }

    pub fn message_uids(&self, folder_id: i64) -> Vec<(i64, u32, bool, bool, bool)> {
        let c = self.rconn();
        let mut st = c
            .prepare(
                "SELECT id, uid, is_read, is_flagged, is_answered FROM messages \
                 WHERE folder_id = ?1 AND uid IS NOT NULL",
            )
            .unwrap();
        st.query_map([folder_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)? as u32,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, i64>(3)? != 0,
                r.get::<_, i64>(4)? != 0,
            ))
        })
        .unwrap()
        .flatten()
        .collect()
    }

    /// Exchange öğe kimliğine göre yerel ileti
    pub fn message_by_remote(&self, folder_id: i64, remote_id: &str) -> Option<i64> {
        self.rconn()
            .query_row("SELECT id FROM messages WHERE folder_id = ?1 AND remote_id = ?2", params![folder_id, remote_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn message_pinned(&self, id: i64) -> bool {
        self.rconn().query_row("SELECT is_pinned FROM messages WHERE id = ?1", [id], |r| r.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(0) != 0
    }

    pub fn remote_ids_in_folder(&self, folder_id: i64) -> Vec<(i64, String)> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT id, remote_id FROM messages WHERE folder_id = ?1 AND remote_id IS NOT NULL").unwrap();
        st.query_map([folder_id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect()
    }

    /// Kategori yoksa varsayılan renkle ekler (Exchange kategorileri için)
    pub fn ensure_category(&self, name: &str) {
        const COLORS: [&str; 8] = ["#e74c3c", "#e67e22", "#f1c40f", "#27ae60", "#2980b9", "#8e44ad", "#16a085", "#7f8c8d"];
        let color = COLORS[name.bytes().map(|b| b as usize).sum::<usize>() % COLORS.len()];
        let _ = self.conn().execute("INSERT OR IGNORE INTO categories(name, color) VALUES (?1, ?2)", params![name, color]);
    }

    pub fn calendar_for_account(&self, account_id: i64) -> Option<Calendar> {
        self.calendars().into_iter().find(|c| c.account_id == Some(account_id) && c.kind.is_empty())
    }

    pub fn events_in_calendar(&self, calendar_id: i64) -> Vec<Event> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM events WHERE calendar_id = ?1").unwrap();
        st.query_map([calendar_id], Event::from_row).unwrap().flatten().collect()
    }

    pub fn contacts_of_account(&self, account_id: i64) -> Vec<Contact> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM contacts WHERE account_id = ?1").unwrap();
        st.query_map([account_id], Contact::from_row).unwrap().flatten().collect()
    }

    /// Klasördeki ileti kimlikleri, listedeki varsayılan sırayla (ileti penceresinde
    /// Önceki/Sonraki için; tüm satırları yüklemeden)
    pub fn message_ids_ordered(&self, folder_id: i64) -> Vec<i64> {
        let c = self.rconn();
        let mut st = c.prepare_cached("SELECT id FROM messages WHERE folder_id = ?1 ORDER BY is_pinned DESC, date DESC, id DESC").unwrap();
        st.query_map([folder_id], |r| r.get(0)).unwrap().flatten().collect()
    }

    pub fn message_ids_in_folder(&self, folder_id: i64, unread_only: bool) -> Vec<i64> {
        let c = self.rconn();
        let sql = if unread_only {
            "SELECT id FROM messages WHERE folder_id = ?1 AND is_read = 0"
        } else {
            "SELECT id FROM messages WHERE folder_id = ?1"
        };
        let mut st = c.prepare(sql).unwrap();
        st.query_map([folder_id], |r| r.get(0)).unwrap().flatten().collect()
    }

    pub fn unread_total(&self) -> i64 {
        self.rconn()
            .query_row(
                "SELECT COUNT(*) FROM messages m JOIN folders f ON f.id = m.folder_id \
                 WHERE m.is_read = 0 AND (f.role IS NULL OR f.role NOT IN \
                 ('trash','junk','sent','drafts','outbox'))",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    // ------------------------------------------------------------------ kategoriler
    pub fn categories(&self) -> Vec<Category> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT name, color FROM categories ORDER BY name").unwrap();
        st.query_map([], |r| Ok(Category { name: r.get(0)?, color: r.get(1)? }))
            .unwrap()
            .flatten()
            .collect()
    }

    pub fn save_category(&self, old_name: Option<&str>, name: &str, color: &str) {
        let c = self.conn();
        if let Some(old) = old_name {
            let _ = c.execute(
                "UPDATE categories SET name = ?1, color = ?2 WHERE name = ?3",
                params![name, color, old],
            );
        } else {
            let _ = c.execute(
                "INSERT OR REPLACE INTO categories(name, color) VALUES (?1, ?2)",
                params![name, color],
            );
        }
    }

    pub fn delete_category(&self, name: &str) {
        let _ = self.conn().execute("DELETE FROM categories WHERE name = ?1", [name]);
    }

    // ------------------------------------------------------------------ kişiler
    pub fn contacts(&self, search: &str) -> Vec<Contact> {
        let c = self.rconn();
        if search.is_empty() {
            let mut st = c
                .prepare("SELECT * FROM contacts ORDER BY is_favorite DESC, display_name COLLATE NOCASE")
                .unwrap();
            return st.query_map([], Contact::from_row).unwrap().flatten().collect();
        }
        let like = format!("%{search}%");
        let mut st = c
            .prepare(
                "SELECT * FROM contacts WHERE display_name LIKE ?1 OR first_name LIKE ?1 OR \
                 last_name LIKE ?1 OR email LIKE ?1 OR email2 LIKE ?1 OR company LIKE ?1 OR \
                 phone_mobile LIKE ?1 OR phone_work LIKE ?1 OR categories LIKE ?1 \
                 ORDER BY display_name COLLATE NOCASE",
            )
            .unwrap();
        st.query_map([like], Contact::from_row).unwrap().flatten().collect()
    }

    pub fn contact(&self, id: i64) -> Option<Contact> {
        self.rconn()
            .query_row("SELECT * FROM contacts WHERE id = ?1", [id], Contact::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn contact_by_email(&self, email: &str) -> Option<Contact> {
        let e = email.to_lowercase();
        self.rconn()
            .query_row(
                "SELECT * FROM contacts WHERE lower(email) = ?1 OR lower(email2) = ?1",
                [e],
                Contact::from_row,
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_contact(&self, ct: &Contact) -> Result<i64> {
        let mut ct = ct.clone();
        ct.display_name = ct.computed_display();
        let c = self.conn();
        if ct.id > 0 {
            c.execute(
                "UPDATE contacts SET first_name=?1, last_name=?2, display_name=?3, email=?4, email2=?5, \
                 phone_mobile=?6, phone_work=?7, phone_home=?8, company=?9, job_title=?10, \
                 department=?11, address=?12, website=?13, birthday=?14, notes=?15, categories=?16, \
                 is_favorite=?17, modified=?18, account_id=?20, remote_id=?21 WHERE id=?19",
                params![ct.first_name, ct.last_name, ct.display_name, ct.email, ct.email2,
                    ct.phone_mobile, ct.phone_work, ct.phone_home, ct.company, ct.job_title,
                    ct.department, ct.address, ct.website, ct.birthday, ct.notes, ct.categories,
                    ct.is_favorite as i64, now(), ct.id, ct.account_id, ct.remote_id],
            )?;
            Ok(ct.id)
        } else {
            c.execute(
                "INSERT INTO contacts(first_name, last_name, display_name, email, email2, phone_mobile, \
                 phone_work, phone_home, company, job_title, department, address, website, birthday, \
                 notes, categories, is_favorite, created, modified, account_id, remote_id) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18,?19,?20)",
                params![ct.first_name, ct.last_name, ct.display_name, ct.email, ct.email2,
                    ct.phone_mobile, ct.phone_work, ct.phone_home, ct.company, ct.job_title,
                    ct.department, ct.address, ct.website, ct.birthday, ct.notes, ct.categories,
                    ct.is_favorite as i64, now(), ct.account_id, ct.remote_id],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_contact(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM contacts WHERE id = ?1", [id]);
    }

    pub fn groups(&self) -> Vec<ContactGroup> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT id, name, members, account_id, remote_id, dirty FROM contact_groups ORDER BY name").unwrap();
        st.query_map([], |r| {
            Ok(ContactGroup { id: r.get(0)?, name: r.get(1)?, members: r.get(2)?, account_id: r.get(3)?, remote_id: r.get(4)?, dirty: r.get::<_, i64>(5)? != 0 })
        })
            .unwrap()
            .flatten()
            .collect()
    }

    pub fn save_group(&self, g: &ContactGroup) -> Result<i64> {
        let c = self.conn();
        if g.id > 0 {
            c.execute(
                "UPDATE contact_groups SET name=?1, members=?2, account_id=?4, remote_id=?5, dirty=?6 WHERE id=?3",
                params![g.name, g.members, g.id, g.account_id, g.remote_id, g.dirty as i64],
            )?;
            Ok(g.id)
        } else {
            c.execute(
                "INSERT INTO contact_groups(name, members, account_id, remote_id, dirty) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![g.name, g.members, g.account_id, g.remote_id, g.dirty as i64],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_group(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM contact_groups WHERE id = ?1", [id]);
    }

    pub fn group(&self, id: i64) -> Option<ContactGroup> {
        self.groups().into_iter().find(|g| g.id == id)
    }

    pub fn mark_group_synced(&self, id: i64, remote_id: &str) {
        let _ = self.conn().execute("UPDATE contact_groups SET remote_id = ?2, dirty = 0 WHERE id = ?1", params![id, remote_id]);
    }

    /// Sunucudan silinmeyi bekleyen öğeler (`kind`: tasks, notes, groups)
    pub fn tombstones(&self, kind: &str, account_id: i64) -> Vec<String> {
        self.setting(&format!("tombstones:{kind}:{account_id}")).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn set_tombstones(&self, kind: &str, account_id: i64, list: &[String]) {
        self.set_setting(&format!("tombstones:{kind}:{account_id}"), &serde_json::to_string(list).unwrap_or_default());
    }

    pub fn add_tombstone(&self, kind: &str, account_id: i64, remote_id: &str) {
        let mut list = self.tombstones(kind, account_id);
        list.push(remote_id.to_string());
        self.set_tombstones(kind, account_id, &list);
    }

    /// Otomatik tamamlama için (ad, adres) listesi; gruplar adres listesi olarak döner.
    pub fn address_book(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for c in self.contacts("") {
            for e in [&c.email, &c.email2] {
                if !e.is_empty() {
                    out.push((c.display_name.clone(), e.clone()));
                }
            }
        }
        for g in self.groups() {
            out.push((g.name.clone(), g.members.clone()));
        }
        // Daha önce yazışılan adresler
        let c = self.rconn();
        if let Ok(mut st) = c.prepare("SELECT value FROM settings WHERE key LIKE 'recent:%'") {
            let rows: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().flatten().collect();
            for r in rows {
                let (n, a) = crate::mail::split_name_addr(&r);
                if !out.iter().any(|(_, e)| e.eq_ignore_ascii_case(&a)) {
                    out.push((n, a));
                }
            }
        }
        out
    }

    /// Gönderilen iletilerin alıcılarını otomatik tamamlama için kaydeder.
    pub fn remember_recipient(&self, name: &str, email: &str) {
        let e = crate::mail::address::clean_email(email).to_lowercase();
        if !e.contains('@') {
            return;
        }
        let _ = self.conn().execute(
            "INSERT INTO recipients(email, name, count, last_used) VALUES (?1, ?2, 1, ?3) \
             ON CONFLICT(email) DO UPDATE SET count = count + 1, last_used = ?3, \
             name = CASE WHEN ?2 != '' THEN ?2 ELSE name END",
            params![e, name.trim(), now()],
        );
    }

    pub fn forget_recipient(&self, email: &str) {
        let _ = self.conn().execute("DELETE FROM recipients WHERE email = ?1", [email.trim().to_lowercase()]);
    }

    /// Otomatik tamamlama önerileri: sık/yakın kullanılan alıcılar, kişiler, gruplar.
    /// (ad, adres, kaynak, puan)
    pub fn suggest_recipients(&self, query: &str, limit: usize) -> Vec<(String, String, String, f64)> {
        let q = fold(query.trim());
        let mut out: Vec<(String, String, String, f64)> = Vec::new();
        let n = now();
        let score_for = |name: &str, email: &str| -> Option<f64> {
            if q.is_empty() {
                return Some(0.0);
            }
            let n = fold(name);
            let e = fold(email);
            if e.starts_with(&q) || n.starts_with(&q) || n.split_whitespace().any(|w| w.starts_with(&q)) {
                Some(2.0)
            } else if e.contains(&q) || n.contains(&q) {
                Some(1.0)
            } else {
                None
            }
        };
        {
            let c = self.rconn();
            let mut st = c.prepare("SELECT email, name, count, last_used FROM recipients").unwrap();
            let rows: Vec<(String, String, i64, f64)> =
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().flatten().collect();
            for (email, name, count, last) in rows {
                // Eski otomatik tamamlama kayıtları (tırnaklı, görünmez karakterli) temiz gelir
                let (name, email) = clean_pair(&name, &email);
                if !email.contains('@') || out.iter().any(|x| x.1.eq_ignore_ascii_case(&email)) {
                    continue;
                }
                if let Some(base) = score_for(&name, &email) {
                    let recency = (1.0 - ((n - last) / (86400.0 * 90.0)).min(1.0)).max(0.0);
                    out.push((name, email, "recent".into(), base * 10.0 + (count as f64).ln_1p() * 2.0 + recency * 3.0));
                }
            }
        }
        for c in self.contacts("") {
            for e in [&c.email, &c.email2] {
                let (_, e) = clean_pair("", e);
                let e = &e;
                if !e.contains('@') {
                    continue;
                }
                if let Some(base) = score_for(&c.display_name, e) {
                    if let Some(x) = out.iter_mut().find(|x| x.1.eq_ignore_ascii_case(e)) {
                        x.3 += 4.0;
                        if x.0.is_empty() {
                            x.0 = c.display_name.clone();
                        }
                        x.2 = "contact".into();
                    } else {
                        out.push((c.display_name.clone(), e.clone(), "contact".into(), base * 10.0 + 4.0 + c.is_favorite as i64 as f64 * 3.0));
                    }
                }
            }
        }
        for g in self.groups() {
            if let Some(base) = score_for(&g.name, "") {
                out.push((g.name.clone(), g.members.clone(), "group".into(), base * 10.0 + 3.0));
            }
        }
        out.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit);
        out
    }

    // ------------------------------------------------------------------ takvim
    pub fn calendars(&self) -> Vec<Calendar> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT id, name, color, visible, account_id, owner, grp, kind, remote_id FROM calendars ORDER BY id").unwrap();
        st.query_map([], |r| {
            Ok(Calendar {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                visible: r.get::<_, i64>(3)? != 0,
                account_id: r.get(4)?,
                owner: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                grp: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                kind: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                remote_id: r.get(8)?,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    pub fn save_calendar(&self, cal: &Calendar) -> Result<i64> {
        let c = self.conn();
        if cal.id > 0 {
            c.execute(
                "UPDATE calendars SET name=?1, color=?2, visible=?3, grp=?5 WHERE id=?4",
                params![cal.name, cal.color, cal.visible as i64, cal.id, cal.grp],
            )?;
            Ok(cal.id)
        } else {
            c.execute(
                "INSERT INTO calendars(name, color, visible, account_id, owner, grp, kind, remote_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![cal.name, cal.color, cal.visible as i64, cal.account_id, cal.owner, cal.grp, cal.kind, cal.remote_id],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_calendar(&self, id: i64) {
        let c = self.conn();
        let _ = c.execute("DELETE FROM events WHERE calendar_id = ?1", [id]);
        let _ = c.execute("DELETE FROM calendars WHERE id = ?1", [id]);
    }

    pub fn events_between(&self, start: f64, end: f64) -> Vec<Event> {
        let c = self.rconn();
        let mut st = c
            .prepare(
                "SELECT * FROM events WHERE (start < ?2 AND end > ?1) OR \
                 (recurrence != 'none' AND start < ?2 AND (recur_until IS NULL OR recur_until >= ?1))",
            )
            .unwrap();
        st.query_map(params![start, end], Event::from_row).unwrap().flatten().collect()
    }

    pub fn all_events(&self) -> Vec<Event> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM events ORDER BY start").unwrap();
        st.query_map([], Event::from_row).unwrap().flatten().collect()
    }

    pub fn search_events(&self, q: &str) -> Vec<Event> {
        let c = self.rconn();
        let like = format!("%{q}%");
        let mut st = c
            .prepare(
                "SELECT * FROM events WHERE title LIKE ?1 OR location LIKE ?1 OR description LIKE ?1 \
                 ORDER BY start DESC LIMIT 500",
            )
            .unwrap();
        st.query_map([like], Event::from_row).unwrap().flatten().collect()
    }

    pub fn event(&self, id: i64) -> Option<Event> {
        self.rconn()
            .query_row("SELECT * FROM events WHERE id = ?1", [id], Event::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn event_by_uid(&self, uid: &str) -> Option<Event> {
        if uid.is_empty() {
            return None;
        }
        self.rconn()
            .query_row("SELECT * FROM events WHERE uid = ?1", [uid], Event::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_event(&self, e: &Event) -> Result<i64> {
        let mut e = e.clone();
        if e.uid.is_empty() {
            e.uid = format!("{}@lynora", uuid::Uuid::new_v4());
        }
        if e.recurrence.is_empty() {
            e.recurrence = "none".into();
        }
        let c = self.conn();
        if e.id > 0 {
            c.execute(
                "UPDATE events SET calendar_id=?1, uid=?2, title=?3, location=?4, description=?5, \
                 start=?6, end=?7, all_day=?8, recurrence=?9, recur_interval=?10, recur_until=?11, \
                 exdates=?12, reminder_minutes=?13, category=?14, show_as=?15, attendees=?16, \
                 is_private=?17, modified=?18, remote_id=?20, master_id=?21, optional_attendees=?22, \
                 resources=?23, organizer=?24, is_cancelled=?25, is_meeting=?26, response=?27, change_key=?28 WHERE id=?19",
                params![e.calendar_id, e.uid, e.title, e.location, e.description, e.start, e.end,
                    e.all_day as i64, e.recurrence, e.recur_interval, e.recur_until, e.exdates,
                    e.reminder_minutes, e.category, e.show_as, e.attendees, e.is_private as i64,
                    now(), e.id, e.remote_id, e.master_id, e.optional_attendees, e.resources,
                    e.organizer, e.is_cancelled as i64, e.is_meeting as i64, e.response, e.change_key],
            )?;
            Ok(e.id)
        } else {
            c.execute(
                "INSERT INTO events(calendar_id, uid, title, location, description, start, end, all_day, \
                 recurrence, recur_interval, recur_until, exdates, reminder_minutes, category, show_as, \
                 attendees, is_private, created, modified, remote_id, master_id, optional_attendees, resources, \
                 organizer, is_cancelled, is_meeting, response, change_key) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27)",
                params![e.calendar_id.max(1), e.uid, e.title, e.location, e.description, e.start,
                    e.end, e.all_day as i64, e.recurrence, e.recur_interval.max(1), e.recur_until,
                    e.exdates, e.reminder_minutes, e.category, e.show_as, e.attendees,
                    e.is_private as i64, now(), e.remote_id, e.master_id, e.optional_attendees, e.resources,
                    e.organizer, e.is_cancelled as i64, e.is_meeting as i64, e.response, e.change_key],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_event(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM events WHERE id = ?1", [id]);
    }

    // ------------------------------------------------------------------ CalDAV / CardDAV
    fn dav_state(&self, table: &str, id: i64) -> DavState {
        self.rconn()
            .query_row(&format!("SELECT etag, dav_raw, dirty FROM {table} WHERE id = ?1"), [id], |r| {
                Ok(DavState {
                    etag: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    raw: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    dirty: r.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
                })
            })
            .unwrap_or_default()
    }

    fn set_dav_state(&self, table: &str, id: i64, etag: &str, raw: &str) {
        let _ = self.conn().execute(&format!("UPDATE {table} SET etag = ?1, dav_raw = ?2, dirty = 0 WHERE id = ?3"), params![etag, raw, id]);
    }

    /// Sunucudaki adres → (kimlik, ETag, kirli mi)
    fn dav_index(&self, sql: &str, owner: i64) -> HashMap<String, DavRow> {
        let c = self.rconn();
        let mut st = c.prepare(sql).unwrap();
        st.query_map([owner], |r| {
            let row = DavRow { id: r.get(1)?, etag: r.get::<_, Option<String>>(2)?.unwrap_or_default(), dirty: r.get::<_, Option<i64>>(3)?.unwrap_or(0) != 0 };
            Ok((r.get(0)?, row))
        })
        .unwrap()
        .flatten()
        .collect()
    }

    pub fn event_dav(&self, id: i64) -> DavState {
        self.dav_state("events", id)
    }

    pub fn set_event_dav(&self, id: i64, etag: &str, raw: &str) {
        self.set_dav_state("events", id, etag, raw);
    }

    pub fn mark_event_dirty(&self, id: i64) {
        let _ = self.conn().execute("UPDATE events SET dirty = 1 WHERE id = ?1", [id]);
    }

    /// Yerelde değişip sunucuya henüz gönderilemeyen etkinlikler
    pub fn dirty_events(&self, calendar_id: i64) -> Vec<Event> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM events WHERE calendar_id = ?1 AND dirty = 1").unwrap();
        st.query_map([calendar_id], Event::from_row).unwrap().flatten().collect()
    }

    pub fn dav_events_index(&self, calendar_id: i64) -> HashMap<String, DavRow> {
        self.dav_index("SELECT remote_id, id, etag, dirty FROM events WHERE calendar_id = ?1 AND remote_id IS NOT NULL", calendar_id)
    }

    pub fn event_by_remote(&self, calendar_id: i64, href: &str) -> Option<i64> {
        self.rconn().query_row("SELECT id FROM events WHERE calendar_id = ?1 AND remote_id = ?2", params![calendar_id, href], |r| r.get(0)).ok()
    }

    pub fn set_event_remote(&self, id: i64, href: &str) {
        let _ = self.conn().execute("UPDATE events SET remote_id = ?1 WHERE id = ?2", params![href, id]);
    }

    /// Serinin taşınmış tek oluşum satırları (bkz. dav_sync::dav_store_event)
    pub fn delete_event_overrides(&self, calendar_id: i64, href: &str) {
        let _ = self.conn().execute("DELETE FROM events WHERE calendar_id = ?1 AND master_id = ?2 AND remote_id IS NULL", params![calendar_id, href]);
    }

    pub fn contact_dav(&self, id: i64) -> DavState {
        self.dav_state("contacts", id)
    }

    pub fn set_contact_dav(&self, id: i64, etag: &str, raw: &str) {
        self.set_dav_state("contacts", id, etag, raw);
    }

    pub fn mark_contact_dirty(&self, id: i64) {
        let _ = self.conn().execute("UPDATE contacts SET dirty = 1 WHERE id = ?1", [id]);
    }

    pub fn dirty_contacts(&self, account_id: i64) -> Vec<Contact> {
        let c = self.rconn();
        let mut st = c.prepare("SELECT * FROM contacts WHERE account_id = ?1 AND dirty = 1").unwrap();
        st.query_map([account_id], Contact::from_row).unwrap().flatten().collect()
    }

    pub fn dav_contacts_index(&self, account_id: i64) -> HashMap<String, DavRow> {
        self.dav_index("SELECT remote_id, id, etag, dirty FROM contacts WHERE account_id = ?1 AND remote_id IS NOT NULL", account_id)
    }

    pub fn set_contact_remote(&self, id: i64, href: &str) {
        let _ = self.conn().execute("UPDATE contacts SET remote_id = ?1 WHERE id = ?2", params![href, id]);
    }

    // ------------------------------------------------------------------ görevler
    pub fn tasks(&self) -> Vec<Task> {
        let c = self.rconn();
        let mut st = c
            .prepare(
                "SELECT * FROM tasks ORDER BY (status = 'completed'), \
                 CASE WHEN due_date IS NULL THEN 1 ELSE 0 END, due_date, priority DESC, id",
            )
            .unwrap();
        st.query_map([], Task::from_row).unwrap().flatten().collect()
    }

    pub fn task(&self, id: i64) -> Option<Task> {
        self.rconn()
            .query_row("SELECT * FROM tasks WHERE id = ?1", [id], Task::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn task_for_message(&self, mid: i64) -> Option<Task> {
        self.rconn()
            .query_row("SELECT * FROM tasks WHERE message_id = ?1", [mid], Task::from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_task(&self, t: &Task) -> Result<i64> {
        let mut t = t.clone();
        if t.status == "completed" {
            t.percent = 100;
            if t.completed_at.is_none() {
                t.completed_at = Some(now());
            }
        } else {
            t.completed_at = None;
        }
        let c = self.conn();
        if t.id > 0 {
            c.execute(
                "UPDATE tasks SET title=?1, notes=?2, start_date=?3, due_date=?4, reminder=?5, \
                 reminder_done=?6, priority=?7, status=?8, percent=?9, category=?10, message_id=?11, \
                 completed_at=?12, modified=?13, account_id=?15, remote_id=?16, dirty=?17 WHERE id=?14",
                params![t.title, t.notes, t.start_date, t.due_date, t.reminder, t.reminder_done as i64,
                    t.priority, t.status, t.percent, t.category, t.message_id, t.completed_at, now(), t.id,
                    t.account_id, t.remote_id, t.dirty as i64],
            )?;
            Ok(t.id)
        } else {
            c.execute(
                "INSERT INTO tasks(title, notes, start_date, due_date, reminder, reminder_done, priority, \
                 status, percent, category, message_id, completed_at, created, modified, account_id, remote_id, dirty) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13,?14,?15,?16)",
                params![t.title, t.notes, t.start_date, t.due_date, t.reminder, t.reminder_done as i64,
                    t.priority, t.status, t.percent, t.category, t.message_id, t.completed_at, now(),
                    t.account_id, t.remote_id, t.dirty as i64],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_task(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM tasks WHERE id = ?1", [id]);
    }

    /// Gönderim sürerken görev yeniden değiştirildiyse kirli kalır
    pub fn mark_task_synced(&self, sent: &Task, remote_id: &str) {
        let _ = self.conn().execute(
            "UPDATE tasks SET remote_id = ?2, dirty = CASE WHEN title = ?3 AND notes = ?4 AND status = ?5 THEN 0 ELSE dirty END WHERE id = ?1",
            params![sent.id, remote_id, sent.title, sent.notes, sent.status],
        );
    }

    // ------------------------------------------------------------------ notlar
    pub fn notes(&self) -> Vec<Note> {
        let c = self.rconn();
        let mut st = c
            .prepare("SELECT id, body, color, category, modified, account_id, remote_id, dirty FROM notes ORDER BY modified DESC")
            .unwrap();
        st.query_map([], |r| {
            Ok(Note {
                id: r.get(0)?,
                body: r.get(1)?,
                color: r.get(2)?,
                category: r.get(3)?,
                modified: r.get(4)?,
                account_id: r.get(5)?,
                remote_id: r.get(6)?,
                dirty: r.get::<_, i64>(7)? != 0,
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    pub fn save_note(&self, n: &Note) -> Result<i64> {
        let c = self.conn();
        if n.id > 0 {
            c.execute(
                "UPDATE notes SET body=?1, color=?2, category=?3, modified=?4, account_id=?6, remote_id=?7, dirty=?8 WHERE id=?5",
                params![n.body, n.color, n.category, now(), n.id, n.account_id, n.remote_id, n.dirty as i64],
            )?;
            Ok(n.id)
        } else {
            c.execute(
                "INSERT INTO notes(body, color, category, created, modified, account_id, remote_id, dirty) VALUES (?1,?2,?3,?4,?4,?5,?6,?7)",
                params![n.body, n.color, n.category, now(), n.account_id, n.remote_id, n.dirty as i64],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_note(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM notes WHERE id = ?1", [id]);
    }

    pub fn note(&self, id: i64) -> Option<Note> {
        self.notes().into_iter().find(|n| n.id == id)
    }

    /// Gönderim sürerken kullanıcı yazmaya devam ettiyse yeni metin kirli kalır
    pub fn mark_note_synced(&self, id: i64, remote_id: &str, body: &str) {
        let _ = self.conn().execute(
            "UPDATE notes SET remote_id = ?2, dirty = CASE WHEN body = ?3 THEN 0 ELSE 1 END WHERE id = ?1",
            params![id, remote_id, body],
        );
    }

    // ------------------------------------------------------------------ kurallar
    pub fn rules(&self, enabled_only: bool) -> Vec<Rule> {
        let c = self.rconn();
        let sql = if enabled_only {
            "SELECT * FROM rules WHERE enabled = 1 ORDER BY position, id"
        } else {
            "SELECT * FROM rules ORDER BY position, id"
        };
        let mut st = c.prepare(sql).unwrap();
        st.query_map([], |r| {
            Ok(Rule {
                id: r.get("id")?,
                name: r.get("name")?,
                enabled: r.get::<_, i64>("enabled")? != 0,
                account_id: r.get("account_id")?,
                field: r.get("field")?,
                op: r.get("op")?,
                value: r.get("value")?,
                action: r.get("action")?,
                action_arg: r.get("action_arg")?,
                stop: r.get::<_, i64>("stop")? != 0,
                remote_id: r.get("remote_id").unwrap_or(None),
                complex: r.get::<_, Option<i64>>("complex").unwrap_or(None).unwrap_or(0) != 0,
                remote_xml: r.get::<_, Option<String>>("remote_xml").unwrap_or(None).unwrap_or_default(),
                spec: r.get::<_, Option<String>>("spec").unwrap_or(None).unwrap_or_default(),
            })
        })
        .unwrap()
        .flatten()
        .collect()
    }

    pub fn save_rule(&self, r: &Rule) -> Result<i64> {
        let c = self.conn();
        if r.id > 0 {
            c.execute(
                "UPDATE rules SET name=?1, enabled=?2, account_id=?3, field=?4, op=?5, value=?6, \
                 action=?7, action_arg=?8, stop=?9, remote_id=?11, complex=?12, remote_xml=?13, spec=?14 WHERE id=?10",
                params![r.name, r.enabled as i64, r.account_id, r.field, r.op, r.value, r.action,
                    r.action_arg, r.stop as i64, r.id, r.remote_id, r.complex as i64, r.remote_xml, r.spec],
            )?;
            Ok(r.id)
        } else {
            c.execute(
                "INSERT INTO rules(name, enabled, account_id, field, op, value, action, action_arg, stop, remote_id, complex, remote_xml, spec) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![r.name, r.enabled as i64, r.account_id, r.field, r.op, r.value, r.action,
                    r.action_arg, r.stop as i64, r.remote_id, r.complex as i64, r.remote_xml, r.spec],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_rule(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM rules WHERE id = ?1", [id]);
    }

    // ------------------------------------------------------------------ hatırlatıcılar
    pub fn reminder_dismissed_until(&self, key: &str) -> Option<f64> {
        self.rconn()
            .query_row("SELECT until FROM dismissed_reminders WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn dismiss_reminder(&self, key: &str, until: f64) {
        let _ = self.conn().execute(
            "INSERT OR REPLACE INTO dismissed_reminders(key, until) VALUES (?1, ?2)",
            params![key, until],
        );
    }
}

// ======================================================================= arama sözdizimi
/// Outlook benzeri arama terimi
#[derive(Debug, PartialEq, Clone)]
pub enum SearchTerm {
    Field(&'static [&'static str], String),
    Sql(&'static str),
    DateAfter(i64),
    DateBefore(i64),
    /// Klasör adı ya da türü ("klasör:arşiv", "in:sent")
    Folder(String),
}

/// Çözülmüş arama: terimler VE / VEYA / DEĞİL ile bağlanır
#[derive(Debug, PartialEq, Clone)]
pub enum SearchExpr {
    And(Vec<SearchExpr>),
    Or(Vec<SearchExpr>),
    Not(Box<SearchExpr>),
    Term(SearchTerm),
}

const ALL_COLS: &[&str] = &["subject", "sender", "to_addr", "cc", "body_text", "categories", "att_names"];
/// "kişi:" (Outlook "Katılımcılar"): gönderen ya da alıcılardan biri
const PEOPLE_COLS: &[&str] = &["sender", "to_addr", "cc"];
const FROM_COLS: &[&str] = &["sender"];
const TO_COLS: &[&str] = &["to_addr", "cc"];
const CC_COLS: &[&str] = &["cc"];
const BCC_COLS: &[&str] = &["bcc"];

/// Anahtar sözcüğün (katlanmış) asıl adı: "Kimden", "FROM", "gönderen" → "from"
fn search_key(k: &str) -> Option<&'static str> {
    Some(match search_fold(k).as_str() {
        "from" | "kimden" | "gonderen" => "from",
        "to" | "kime" | "alici" => "to",
        "cc" | "bilgi" => "cc",
        "bcc" | "gizli" => "bcc",
        "subject" | "konu" => "subject",
        "participants" | "participant" | "kisi" | "katilimci" => "participants",
        "attachmentname" | "filename" | "ekadi" | "dosya" => "attachment",
        "body" | "govde" | "metin" => "body",
        "category" | "categories" | "kategori" => "category",
        "has" | "hasattachment" | "hasattachments" | "ekvar" | "ek" | "attachment" | "attachments" => "has",
        "is" | "durum" => "is",
        "importance" | "onem" => "importance",
        "after" | "since" | "sonra" => "after",
        "before" | "once" => "before",
        "on" | "tarih" | "received" | "date" | "sent" => "on",
        "folder" | "in" | "klasor" => "folder",
        _ => return None,
    })
}

#[derive(Debug, PartialEq)]
enum Tok {
    Open,
    Close,
    Or,
    And,
    Not,
    /// "kimden:(" — parantezdeki sözcüklerin hepsi bu alanda aranır
    Group(&'static str),
    Word(Option<&'static str>, String),
}

/// Aramayı parçalara ayırır: tırnak içi tek parça, "anahtar: değer" (iki noktadan sonra boşluk
/// olabilir), parantez, VEYA/OR, VE/AND, DEĞİL/NOT (büyük harfle) ve baştaki "-".
fn search_tokens(q: &str) -> Vec<Tok> {
    let ch: Vec<char> = q.chars().collect();
    let (mut out, mut i, mut depth) = (Vec::new(), 0usize, 0usize);
    while i < ch.len() {
        let c = ch[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' {
            out.push(Tok::Open);
            depth += 1;
            i += 1;
            continue;
        }
        if c == ')' && depth > 0 {
            out.push(Tok::Close);
            depth -= 1;
            i += 1;
            continue;
        }
        if c == '-' && ch.get(i + 1).is_some_and(|n| n.is_alphanumeric() || *n == '"' || *n == '(') {
            out.push(Tok::Not);
            i += 1;
            continue;
        }
        // Sözcük
        let (mut cur, mut key, mut quoted, mut any_quote, mut inner) = (String::new(), None, false, false, 0usize);
        let mut group = false;
        while i < ch.len() {
            let c = ch[i];
            if c == '"' {
                quoted = !quoted;
                any_quote = true;
            } else if quoted {
                cur.push(c);
            } else if c.is_whitespace() {
                break;
            } else if c == ':' && key.is_none() && !any_quote && search_key(&cur).is_some() {
                key = search_key(&cur);
                cur.clear();
                // "kimden: ali" — değer boşluktan sonra da gelebilir
                let mut j = i + 1;
                while ch.get(j).is_some_and(|n| n.is_whitespace()) {
                    j += 1;
                }
                if ch.get(j) == Some(&'(') {
                    group = true;
                    i = j;
                    break;
                }
                if j < ch.len() {
                    i = j;
                    continue;
                }
            } else if c == '(' {
                inner += 1;
                cur.push(c);
            } else if c == ')' {
                if inner == 0 && depth > 0 {
                    break;
                }
                inner = inner.saturating_sub(1);
                cur.push(c);
            } else {
                cur.push(c);
            }
            i += 1;
        }
        if group {
            out.push(Tok::Group(key.unwrap_or("")));
            continue;
        }
        if key.is_none() && !any_quote {
            match cur.as_str() {
                "OR" | "VEYA" | "|" | "||" => {
                    out.push(Tok::Or);
                    continue;
                }
                "AND" | "VE" | "&" | "&&" => {
                    out.push(Tok::And);
                    continue;
                }
                "NOT" | "DEĞİL" | "DEGIL" => {
                    out.push(Tok::Not);
                    continue;
                }
                _ => {}
            }
        }
        // "tarih:bu hafta", "on:last month" — iki sözcüklü dönem tırnaksız da yazılabilir
        if matches!(key, Some("on" | "after" | "before")) && !any_quote && matches!(search_fold(&cur).as_str(), "bu" | "gecen" | "this" | "last") {
            let rest: String = ch[i..].iter().collect();
            if let Some(next) = rest.split_whitespace().next().filter(|n| matches!(search_fold(n).as_str(), "hafta" | "ay" | "yil" | "week" | "month" | "year")) {
                cur.push_str(next);
                i += rest[..rest.find(next).unwrap_or(0)].chars().count() + next.chars().count();
            }
        }
        if !cur.trim().is_empty() {
            out.push(Tok::Word(key, cur));
        }
    }
    out
}

fn day_ts(d: chrono::NaiveDate) -> i64 {
    use chrono::{Local, TimeZone};
    // Yaz saati geçişinde gece yarısı yoksa günün ilk geçerli anı
    d.and_hms_opt(0, 0, 0).and_then(|t| Local.from_local_datetime(&t).earliest()).map(|t| t.timestamp()).unwrap_or(0)
}

/// Tarih değeri → [başlangıç, bitiş) günleri: "2026-03-05", "05.03.2026", "2026-03", "03.2026",
/// "2026", "bugün", "dün", "bu hafta", "geçen ay", "mart", "7g" (son 7 gün), "2h", "3a", "1y"
fn parse_period(v: &str, today: chrono::NaiveDate) -> Option<(chrono::NaiveDate, chrono::NaiveDate)> {
    use chrono::{Datelike, Duration, Months, NaiveDate};
    let v: String = search_fold(v).chars().filter(|c| !c.is_whitespace() && *c != '_').collect();
    let day = |d: NaiveDate| Some((d, d + Duration::days(1)));
    let month = |y: i32, m: u32| {
        let s = NaiveDate::from_ymd_opt(y, m, 1)?;
        Some((s, s.checked_add_months(Months::new(1))?))
    };
    let year = |y: i32| Some((NaiveDate::from_ymd_opt(y, 1, 1)?, NaiveDate::from_ymd_opt(y + 1, 1, 1)?));
    let week = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    match v.as_str() {
        "today" | "bugun" => return day(today),
        "yesterday" | "dun" => return day(today - Duration::days(1)),
        "thisweek" | "buhafta" => return Some((week, week + Duration::days(7))),
        "lastweek" | "gecenhafta" => return Some((week - Duration::days(7), week)),
        "thismonth" | "buay" => return month(today.year(), today.month()),
        "lastmonth" | "gecenay" => {
            let p = today.with_day(1)?.checked_sub_months(Months::new(1))?;
            return month(p.year(), p.month());
        }
        "thisyear" | "buyil" => return year(today.year()),
        "lastyear" | "gecenyil" => return year(today.year() - 1),
        _ => {}
    }
    const MONTHS: [&[&str]; 12] = [
        &["ocak", "january", "jan"], &["subat", "february", "feb"], &["mart", "march", "mar"], &["nisan", "april", "apr"],
        &["mayis", "may"], &["haziran", "june", "jun"], &["temmuz", "july", "jul"], &["agustos", "august", "aug"],
        &["eylul", "september", "sep"], &["ekim", "october", "oct"], &["kasim", "november", "nov"], &["aralik", "december", "dec"],
    ];
    if let Some(m) = MONTHS.iter().position(|n| n.contains(&v.as_str())) {
        // Henüz gelmemiş ay geçen yılındır
        let m = m as u32 + 1;
        return month(if m > today.month() { today.year() - 1 } else { today.year() }, m);
    }
    // Göreli: son N gün/hafta/ay/yıl
    if v.len() >= 2 && v.is_char_boundary(v.len() - 1) {
        let (n, unit) = v.split_at(v.len() - 1);
        if let Ok(n) = n.parse::<u32>() {
            let end = today + Duration::days(1);
            let start = match unit {
                "d" | "g" => Some(today - Duration::days(n as i64)),
                "w" | "h" => Some(today - Duration::days(7 * n as i64)),
                "m" | "a" => today.checked_sub_months(Months::new(n)),
                "y" => today.checked_sub_months(Months::new(12 * n)),
                _ => None,
            };
            if let Some(s) = start {
                return Some((s, end));
            }
        }
    }
    for f in ["%Y-%m-%d", "%d.%m.%Y", "%d/%m/%Y", "%Y/%m/%d"] {
        if let Ok(d) = NaiveDate::parse_from_str(&v, f) {
            return day(d);
        }
    }
    let nums: Vec<&str> = v.split(['-', '.', '/']).collect();
    let n = |s: &str| s.parse::<u32>().ok();
    match nums.as_slice() {
        [y] if y.len() == 4 => year(n(y).filter(|y| (1980..2200).contains(y))? as i32),
        [y, m] if y.len() == 4 => month(n(y)? as i32, n(m)?),
        [m, y] if y.len() == 4 => month(n(y)? as i32, n(m)?),
        _ => None,
    }
}

/// Tarih terimi: `key` after/before/on; değer dönem, karşılaştırma (">=2026-01-01") ya da
/// aralık ("2026-01-01..2026-02-15") olabilir
fn date_terms(key: &str, val: &str, today: chrono::NaiveDate) -> Option<Vec<SearchTerm>> {
    let p = |v: &str| parse_period(v, today);
    let (after, before) = (|d| SearchTerm::DateAfter(day_ts(d)), |d| SearchTerm::DateBefore(day_ts(d)));
    let v = val.trim();
    Some(match key {
        "after" => vec![after(p(v)?.0)],
        "before" => vec![before(p(v)?.0)],
        _ => {
            if let Some((a, b)) = v.split_once("..") {
                vec![after(p(a)?.0), before(p(b)?.1)]
            } else if let Some(r) = v.strip_prefix(">=") {
                vec![after(p(r)?.0)]
            } else if let Some(r) = v.strip_prefix("<=") {
                vec![before(p(r)?.1)]
            } else if let Some(r) = v.strip_prefix('>') {
                vec![after(p(r)?.1)]
            } else if let Some(r) = v.strip_prefix('<') {
                vec![before(p(r)?.0)]
            } else {
                let (s, e) = p(v)?;
                vec![after(s), before(e)]
            }
        }
    })
}

/// Tek terim; anlaşılmayan değer ("is:xyz", "tarih:abc") düz metin olarak aranır
fn search_term(key: Option<&'static str>, val: &str, today: chrono::NaiveDate) -> Option<SearchExpr> {
    use SearchTerm::*;
    let field = |cols: &'static [&'static str]| Some(SearchExpr::Term(Field(cols, val.to_string())));
    let sql = |s: &'static str| Some(SearchExpr::Term(Sql(s)));
    let Some(key) = key else { return field(ALL_COLS) };
    let lv = search_fold(val.trim());
    let literal = || Some(SearchExpr::Term(Field(ALL_COLS, format!("{key}:{val}"))));
    match key {
        "from" => field(FROM_COLS),
        "to" => field(TO_COLS),
        "cc" => field(CC_COLS),
        "bcc" => field(BCC_COLS),
        "subject" => field(&["subject"]),
        "participants" => field(PEOPLE_COLS),
        "attachment" => field(&["att_names"]),
        "body" => field(&["body_text"]),
        "category" => field(&["categories"]),
        "has" => sql(if matches!(lv.as_str(), "yes" | "true" | "evet" | "var" | "attachment" | "attachments" | "ek" | "1") { "has_attachments = 1" } else { "has_attachments = 0" }),
        "is" => match lv.as_str() {
            "unread" | "okunmamis" | "okunmadi" => sql("is_read = 0"),
            "read" | "okunmus" | "okundu" => sql("is_read = 1"),
            "flagged" | "bayrakli" | "isaretli" => sql("is_flagged = 1"),
            "unflagged" | "bayraksiz" => sql("is_flagged = 0"),
            "important" | "onemli" => sql("importance = 2"),
            "replied" | "answered" | "yanitlandi" | "yanitlanmis" => sql("is_answered = 1"),
            "unreplied" | "unanswered" | "yanitlanmadi" | "yanitlanmamis" => sql("is_answered = 0"),
            "draft" | "taslak" => sql("is_draft = 1"),
            "pinned" | "sabit" | "sabitlenmis" => sql("is_pinned = 1"),
            _ => literal(),
        },
        "importance" => match lv.as_str() {
            "high" | "yuksek" => sql("importance = 2"),
            "low" | "dusuk" => sql("importance = 0"),
            "normal" => sql("importance = 1"),
            _ => literal(),
        },
        "after" | "before" | "on" => match date_terms(key, val, today) {
            Some(mut t) if t.len() == 1 => t.pop().map(SearchExpr::Term),
            Some(t) => Some(SearchExpr::And(t.into_iter().map(SearchExpr::Term).collect())),
            None => literal(),
        },
        "folder" => Some(SearchExpr::Term(Folder(val.to_string()))),
        _ => field(ALL_COLS),
    }
}

struct SearchParser {
    toks: Vec<Tok>,
    pos: usize,
    today: chrono::NaiveDate,
}

impl SearchParser {
    fn or(&mut self, key: Option<&'static str>) -> Option<SearchExpr> {
        let mut alts = Vec::new();
        loop {
            alts.extend(self.and(key));
            if self.toks.get(self.pos) != Some(&Tok::Or) {
                break;
            }
            self.pos += 1;
        }
        match alts.len() {
            0 => None,
            1 => alts.pop(),
            _ => Some(SearchExpr::Or(alts)),
        }
    }

    fn and(&mut self, key: Option<&'static str>) -> Option<SearchExpr> {
        let mut items = Vec::new();
        while let Some(t) = self.toks.get(self.pos) {
            match t {
                Tok::Or | Tok::Close => break,
                Tok::And => self.pos += 1,
                _ => match self.unary(key) {
                    // Aynı düzeydeki VE'ler tek listede
                    Some(SearchExpr::And(v)) => items.extend(v),
                    Some(e) => items.push(e),
                    None => {}
                },
            }
        }
        match items.len() {
            0 => None,
            1 => items.pop(),
            _ => Some(SearchExpr::And(items)),
        }
    }

    fn unary(&mut self, key: Option<&'static str>) -> Option<SearchExpr> {
        let t = self.toks.get(self.pos)?;
        self.pos += 1;
        match t {
            Tok::Not => self.unary(key).map(|e| SearchExpr::Not(Box::new(e))),
            Tok::Open => self.group(key),
            Tok::Group(k) => {
                let k = *k;
                self.pos += 1; // "("
                self.group(Some(k))
            }
            Tok::Word(k, v) => search_term(k.or(key), v, self.today),
            _ => None,
        }
    }

    fn group(&mut self, key: Option<&'static str>) -> Option<SearchExpr> {
        let e = self.or(key);
        if self.toks.get(self.pos) == Some(&Tok::Close) {
            self.pos += 1;
        }
        e
    }
}

fn parse_query_at(q: &str, today: chrono::NaiveDate) -> SearchExpr {
    let mut p = SearchParser { toks: search_tokens(q), pos: 0, today };
    let mut items = Vec::new();
    // Fazladan ")" aramayı kesmez
    while p.pos < p.toks.len() {
        items.extend(p.or(None));
        if p.toks.get(p.pos) == Some(&Tok::Close) {
            p.pos += 1;
        }
    }
    match items.len() {
        1 => items.pop().unwrap(),
        _ => SearchExpr::And(items),
    }
}

/// `kimden:ali konu:"aylık rapor" (fatura VEYA makbuz) -taslak tarih:bu hafta` gibi aramaları çözer.
/// Türkçe ve İngilizce anahtar sözcükler (büyük/küçük harf ayırmadan; "Kimden: ali" de olur);
/// yan yana terimler VE'dir.
pub fn parse_query(q: &str) -> SearchExpr {
    parse_query_at(q, chrono::Local::now().date_naive())
}

/// En üst düzeydeki terimler (sınamalar için)
#[cfg(test)]
pub fn parse_search(q: &str) -> Vec<SearchTerm> {
    match parse_query(q) {
        SearchExpr::And(v) => v.into_iter().filter_map(|e| if let SearchExpr::Term(t) = e { Some(t) } else { None }).collect(),
        SearchExpr::Term(t) => vec![t],
        _ => vec![],
    }
}

/// Anahtarsız düz sözcükler (en az iki) tek bir ifadede birleştirilir: `ahmet emre aydın
/// is:unread` → `"ahmet emre aydın" is:unread`. Birleştirilecek sözcük yoksa None.
pub fn phrase_variant(q: &str) -> Option<SearchExpr> {
    if q.contains('"') {
        return None;
    }
    let SearchExpr::And(items) = parse_query(q) else { return None };
    let plain = |e: &SearchExpr| matches!(e, SearchExpr::Term(SearchTerm::Field(c, _)) if *c == ALL_COLS);
    let words: Vec<&str> = items.iter().filter_map(|e| if let SearchExpr::Term(SearchTerm::Field(c, v)) = e { (*c == ALL_COLS).then_some(v.as_str()) } else { None }).collect();
    if words.len() < 2 {
        return None;
    }
    let mut out = vec![SearchExpr::Term(SearchTerm::Field(ALL_COLS, words.join(" ")))];
    out.extend(items.iter().filter(|e| !plain(e)).cloned());
    Some(if out.len() == 1 { out.pop().unwrap() } else { SearchExpr::And(out) })
}

/// Aramayı Exchange'in sorgu diline (AQS) çevirir (sunucuda arama için).
pub fn search_to_aqs(q: &str) -> String {
    expr_to_aqs(&parse_query(q))
}

pub fn expr_to_aqs(e: &SearchExpr) -> String {
    let quote = |v: &str| if v.contains(char::is_whitespace) { format!("\"{}\"", v.replace('"', "")) } else { v.to_string() };
    let day = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()).unwrap_or_default();
    let list = |v: &[SearchExpr], sep: &str, top: bool| {
        let parts: Vec<String> = v.iter().map(|x| aqs_part(x)).filter(|s| !s.is_empty()).collect();
        if parts.len() > 1 && !top { format!("({})", parts.join(sep)) } else { parts.join(sep) }
    };
    fn aqs_part(e: &SearchExpr) -> String {
        match e {
            SearchExpr::And(v) if v.len() > 1 => format!("({})", expr_to_aqs(e)),
            _ => expr_to_aqs(e),
        }
    }
    match e {
        SearchExpr::And(v) => list(v, " ", true),
        SearchExpr::Or(v) => list(v, " OR ", false),
        SearchExpr::Not(x) => {
            let s = aqs_part(x);
            if s.is_empty() { s } else { format!("NOT {s}") }
        }
        SearchExpr::Term(t) => match t {
            SearchTerm::Field(cols, v) if *cols == ALL_COLS => quote(v),
            SearchTerm::Field(cols, v) if *cols == PEOPLE_COLS => format!("participants:{}", quote(v)),
            SearchTerm::Field(cols, v) => {
                let key = match cols.first().copied() {
                    Some("sender") => "from",
                    Some("to_addr") => "to",
                    Some("cc") => "cc",
                    Some("bcc") => "bcc",
                    Some("subject") => "subject",
                    Some("body_text") => "body",
                    Some("categories") => "category",
                    Some("att_names") => "attachment",
                    _ => "",
                };
                if key.is_empty() { quote(v) } else { format!("{key}:{}", quote(v)) }
            }
            SearchTerm::Sql("has_attachments = 1") => "hasattachment:true".into(),
            SearchTerm::Sql("has_attachments = 0") => "hasattachment:false".into(),
            SearchTerm::Sql("is_read = 0") => "isread:false".into(),
            SearchTerm::Sql("is_read = 1") => "isread:true".into(),
            SearchTerm::Sql("is_flagged = 1") => "isflagged:true".into(),
            SearchTerm::Sql("is_flagged = 0") => "isflagged:false".into(),
            SearchTerm::Sql("importance = 2") => "importance:high".into(),
            SearchTerm::Sql("importance = 1") => "importance:normal".into(),
            SearchTerm::Sql("importance = 0") => "importance:low".into(),
            // Yanıtlandı/taslak/sabit ve klasör sunucu sorgusuna çevrilmez (yerelde süzülür)
            SearchTerm::Sql(_) | SearchTerm::Folder(_) => String::new(),
            SearchTerm::DateAfter(t) => format!("received:>={}", day(*t)),
            SearchTerm::DateBefore(t) => format!("received:<{}", day(*t)),
        },
    }
}

/// Klasör türü adı ("gelen", "sent", "çöp") → rol
fn folder_role(v: &str) -> &'static str {
    match search_fold(v.trim()).as_str() {
        "inbox" | "gelen" | "gelenkutusu" => "inbox",
        "sent" | "gonderilmis" | "gonderilen" | "giden" => "sent",
        "drafts" | "draft" | "taslak" | "taslaklar" => "drafts",
        "trash" | "deleted" | "silinmis" | "silinen" | "cop" => "trash",
        "junk" | "spam" | "gereksiz" => "junk",
        "archive" | "arsiv" => "archive",
        _ => "",
    }
}

/// Aramanın SQL koşulu (boş arama: None). `args`: soru işaretlerinin değerleri, sırayla.
fn expr_sql(e: &SearchExpr, fts: bool, args: &mut Vec<String>) -> Option<String> {
    let list = |v: &[SearchExpr], sep: &str, args: &mut Vec<String>| {
        let parts: Vec<String> = v.iter().filter_map(|x| expr_sql(x, fts, args)).collect();
        (!parts.is_empty()).then(|| format!("({})", parts.join(sep)))
    };
    match e {
        SearchExpr::And(v) => list(v, " AND ", args),
        SearchExpr::Or(v) => list(v, " OR ", args),
        SearchExpr::Not(x) => expr_sql(x, fts, args).map(|s| format!("({s} IS NOT 1)")),
        SearchExpr::Term(SearchTerm::Sql(cond)) => Some(format!("({cond})")),
        SearchExpr::Term(SearchTerm::DateAfter(ts)) => Some(format!("(date >= {ts})")),
        SearchExpr::Term(SearchTerm::DateBefore(ts)) => Some(format!("(date < {ts})")),
        SearchExpr::Term(SearchTerm::Folder(v)) => {
            let words = search_words(v);
            if words.is_empty() {
                return None;
            }
            args.push(words.join(" "));
            args.push(folder_role(v).to_string());
            Some("(folder_id IN (SELECT id FROM folders WHERE lyn_match(name, ?, 0) OR role = ?))".into())
        }
        SearchExpr::Term(SearchTerm::Field(cols, v)) => {
            // Exchange/Outlook gibi: sözcük başı eşleşmesi, Türkçe harf ve aksan duyarsız.
            // Birden çok sözcüklü değer (tırnaklı ifade, e-posta adresi) aynı alanda arka
            // arkaya geçmeli. Önce hızlı ön eleme (dizin ya da en uzun sözcükle LIKE),
            // sonra kesin denetim.
            let words = search_words(v);
            let longest = words.iter().max_by_key(|w| w.chars().count())?;
            let phrase = words.len() > 1;
            let col = |c: &'static str| -> &'static str { if c == "body_text" { BODY_TEXT_SQL } else { c } };
            let exact: Vec<String> = cols.iter().map(|c| format!("lyn_match({}, ?, {})", col(c), phrase as i32)).collect();
            let mut sql;
            // Gizli alıcılar dizinde yok
            if fts && cols.iter().all(|c| ALL_COLS.contains(c)) {
                // Dizin sözcük başı eşleşmesini zaten kesin yapar; yalnızca sözcüklerin
                // arka arkaya geçmesi (ifade) satırın kendisinde denetlenir
                sql = String::from("id IN (SELECT rowid FROM message_fts WHERE message_fts MATCH ?)");
                args.push(fts_query(cols, &words));
                if !phrase {
                    return Some(format!("({sql})"));
                }
                sql.push_str(&format!(" AND ({})", exact.join(" OR ")));
            } else {
                let pre: Vec<String> = cols.iter().map(|c| format!("{} LIKE ?", col(c))).collect();
                sql = format!("({}) AND ({})", pre.join(" OR "), exact.join(" OR "));
                for _ in cols.iter() {
                    args.push(like_pattern(longest));
                }
            }
            for _ in cols.iter() {
                args.push(words.join(" "));
            }
            Some(format!("({sql})"))
        }
    }
}

/// "ad.soyad@kurum.com.tr" → "ad soyad" (adres defterinde adı bulunmayan kişi için tahmin)
fn name_from_address(addr: &str) -> Option<String> {
    let parts: Vec<&str> = addr.split('@').next()?.split(['.', '_', '-']).filter(|p| !p.is_empty()).collect();
    (parts.len() >= 2 && parts.iter().all(|p| p.chars().count() >= 2 && p.chars().all(char::is_alphabetic))).then(|| parts.join(" "))
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn word_prefix_search() {
        let w = |q: &str| search_words(q);
        assert!(words_match("Ahmet Emre AYDIN <ahmet.aydin@kurum.com.tr>", &w("ahmet emre aydın"), true));
        assert!(words_match("Ahmet Emre Aydın", &w("AHMET EMRE AYDIN"), true));
        assert!(!words_match("Ahmet Yılmaz, Emre Kaya, Ali Aydın", &w("ahmet emre aydın"), true));
        assert!(words_match("Ahmet Yılmaz, Emre Kaya, Ali Aydın", &w("ahmet emre aydın"), false));
        assert!(!words_match("Kemre Bey", &w("emre"), false));
        assert!(words_match("Şükrü Çağlar", &w("sukru caglar"), true));
        assert_eq!(phrase_variant("ahmet emre aydın is:unread"), Some(parse_query("\"ahmet emre aydın\" is:unread")));
        assert_eq!(phrase_variant("ahmet"), None);
        let db = Db::open_memory().unwrap();
        let n: bool = db.rconn().query_row("SELECT lyn_match('Ahmet Emre AYDIN', 'ahmet emre aydin', 1)", [], |r| r.get(0)).unwrap();
        assert!(n);
    }

    #[test]
    fn dav_state_and_index() {
        let db = Db::open_memory().unwrap();
        let cal = db.save_calendar(&Calendar { name: "İş".into(), kind: "dav".into(), remote_id: Some("/c/".into()), ..Default::default() }).unwrap();
        assert_eq!(db.calendars().into_iter().find(|c| c.id == cal).unwrap().remote_id.as_deref(), Some("/c/"));
        let id = db.save_event(&Event { calendar_id: cal, start: 1.0, end: 2.0, remote_id: Some("/c/1.ics".into()), ..Event::new_default() }).unwrap();
        db.set_event_dav(id, "\"e1\"", "RAW");
        assert_eq!(db.event_dav(id).raw, "RAW");
        assert!(db.dirty_events(cal).is_empty());
        db.mark_event_dirty(id);
        assert_eq!(db.dirty_events(cal).len(), 1);
        let idx = db.dav_events_index(cal);
        assert!(idx["/c/1.ics"].dirty && idx["/c/1.ics"].etag == "\"e1\"");
        // Yazılınca kirli işareti kalkar; arayüzden gelen kayıt sunucu durumuna dokunmaz
        db.set_event_dav(id, "\"e2\"", "RAW2");
        let mut e = db.event(id).unwrap();
        e.title = "x".into();
        db.save_event(&e).unwrap();
        assert_eq!((db.event_dav(id).etag.as_str(), db.event_dav(id).dirty), ("\"e2\"", false));
        // Serinin taşınmış oluşumu seriyle birlikte yenilenir, seri satırı silinmez
        db.save_event(&Event { calendar_id: cal, start: 1.0, end: 2.0, master_id: Some("/c/1.ics".into()), ..Event::new_default() }).unwrap();
        db.delete_event_overrides(cal, "/c/1.ics");
        assert_eq!(db.events_in_calendar(cal).len(), 1);
    }

    #[test]
    fn migration_is_not_blocked_by_a_dir_without_database() {
        let root = std::env::temp_dir().join(format!("lynora-mig-empty-{}", std::process::id()));
        let (old, new) = (root.join("mailnix"), root.join("lynora"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("mailnix.db"), b"veri").unwrap();
        std::fs::write(new.join("crash.log"), b"gunluk").unwrap();
        migrate_legacy_data_dir(&new, &old);
        assert_eq!(std::fs::read(new.join("lynora.db")).unwrap(), b"veri");
        assert!(!old.exists());
        assert_eq!(std::fs::read(root.join("lynora.eski/crash.log")).unwrap(), b"gunluk", "kenara alınmalı, silinmemeli");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Eski adla (MailNix) oluşturulmuş veri klasörü yeni ada taşınır; yeni klasörde veritabanı varsa dokunulmaz
    #[test]
    fn migrates_legacy_data_dir() {
        let root = std::env::temp_dir().join(format!("lynora-mig-{}", std::process::id()));
        let (old, new) = (root.join("mailnix"), root.join("lynora"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(old.join("photos")).unwrap();
        {
            let db = Db::open(old.join("mailnix.db")).unwrap();
            db.set_setting("eski", "veri");
        }
        std::fs::write(old.join(".key"), [7u8; 32]).unwrap();
        migrate_legacy_data_dir(&new, &old);
        assert!(!old.exists());
        assert!(new.join("lynora.db").exists() && !new.join("mailnix.db").exists());
        assert!(new.join(".key").exists() && new.join("photos").is_dir());
        assert_eq!(Db::open(new.join("lynora.db")).unwrap().setting("eski").as_deref(), Some("veri"));
        // İkinci kez: yeni veritabanı var, eski (yeniden oluşmuş) klasöre dokunulmaz
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("mailnix.db"), b"x").unwrap();
        migrate_legacy_data_dir(&new, &old);
        assert!(old.join("mailnix.db").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Dosya veritabanında okuma bağlantısı yazılanları görür, yazma sürerken de okur
    #[test]
    fn reader_connection_sees_writes() {
        let dir = std::env::temp_dir().join(format!("lynora-db-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = Db::open(dir.join("t.db")).unwrap();
        assert_eq!(db.readers.len(), READERS);
        assert!(db.readers.iter().all(|r| !Arc::ptr_eq(&db.conn, r)));
        // Bir okuma sürerken öteki okuma başka bağlantıdan yapılır (beklemez)
        let busy = db.rconn();
        assert_eq!(db.setting("yok"), None);
        drop(busy);
        db.set_setting("x", "1");
        assert_eq!(db.setting("x").as_deref(), Some("1"));
        let guard = db.conn();
        guard.execute_batch("BEGIN; INSERT OR REPLACE INTO settings(key, value) VALUES ('x', '2');").unwrap();
        // Yazma işlemi açıkken okuma beklemez (son işlenmiş değeri görür)
        assert_eq!(db.setting("x").as_deref(), Some("1"));
        guard.execute_batch("COMMIT").unwrap();
        drop(guard);
        assert_eq!(db.setting("x").as_deref(), Some("2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 1.1.0 düzeni: gövdeler ve özgün ileti messages satırının ortasında
    const OLD_MESSAGES: &str = "CREATE TABLE messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        folder_id INTEGER NOT NULL, account_id INTEGER, uid INTEGER,
        message_id TEXT DEFAULT '', in_reply_to TEXT DEFAULT '', refs TEXT DEFAULT '',
        subject TEXT DEFAULT '', sender TEXT DEFAULT '', to_addr TEXT DEFAULT '',
        cc TEXT DEFAULT '', bcc TEXT DEFAULT '', date REAL DEFAULT 0,
        snippet TEXT DEFAULT '', body_text TEXT DEFAULT '', body_html TEXT DEFAULT '',
        raw BLOB, is_read INTEGER DEFAULT 0, is_flagged INTEGER DEFAULT 0,
        is_draft INTEGER DEFAULT 0, is_answered INTEGER DEFAULT 0, importance INTEGER DEFAULT 1,
        categories TEXT DEFAULT '', has_attachments INTEGER DEFAULT 0, pending TEXT DEFAULT '',
        FOREIGN KEY(folder_id) REFERENCES folders(id) ON DELETE CASCADE
    );
    ALTER TABLE messages ADD COLUMN remote_id TEXT;
    ALTER TABLE messages ADD COLUMN is_pinned INTEGER DEFAULT 0;
    ALTER TABLE messages ADD COLUMN att_names TEXT DEFAULT '';";

    /// Eski veritabanı açılışta bir kez yeni düzene geçer: kimlikler, gövdeler, bayraklar, arama,
    /// silme ve kimlik sayacı korunur; yarıda kesilmiş geçiş kaldığı yerden tamamlanır.
    #[test]
    fn splits_message_content_from_old_layout() {
        let dir = std::env::temp_dir().join(format!("lynora-split-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
                 INSERT INTO settings VALUES ('schema_version', '1');
                 CREATE TABLE folders (id INTEGER PRIMARY KEY AUTOINCREMENT, account_id INTEGER, name TEXT NOT NULL,
                     remote_name TEXT, role TEXT, parent_id INTEGER, uidvalidity INTEGER DEFAULT 0, position INTEGER DEFAULT 100);
                 INSERT INTO folders(id, name, role) VALUES (1, 'Gelen Kutusu', 'inbox');",
            )
            .unwrap();
            c.execute_batch(OLD_MESSAGES).unwrap();
            for i in 1..=260i64 {
                c.execute(
                    "INSERT INTO messages(id, folder_id, subject, body_text, body_html, raw, remote_id, is_read, att_names) VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![i, format!("Konu {i}"), format!("gövde metni anahtar{i}"), format!("<p>{i}</p>"), vec![i as u8; 5000], format!("r{i}"), i % 2, format!("ek{i}.pdf")],
                )
                .unwrap();
            }
            // Yalnızca başlığı inmiş ileti; en yeni kimlikler silinmiş (sayaç 260'ta kalır)
            c.execute_batch("UPDATE messages SET body_text = '', body_html = '', raw = X'' WHERE id = 250; DELETE FROM messages WHERE id > 250;").unwrap();
            // Yarıda kesilmiş geçiş: ilk 100 ileti taşınmış
            c.execute_batch(
                "CREATE TABLE message_content (id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
                     body_text TEXT NOT NULL DEFAULT '', body_html TEXT NOT NULL DEFAULT '', raw BLOB);
                 INSERT INTO message_content SELECT id, body_text, body_html, raw FROM messages WHERE id <= 100;
                 UPDATE messages SET body_text = '', body_html = '', raw = NULL WHERE id <= 100;",
            )
            .unwrap();
        }
        assert!(Db::needs_upgrade(&path));
        let db = Db::open(&path).unwrap();
        assert!(!Db::needs_upgrade(&path));
        {
            let c = db.conn();
            assert!(!Db::has_column(&c, "messages", "raw").unwrap() && !Db::has_column(&c, "messages", "body_text").unwrap());
            assert_eq!(c.query_row("SELECT COUNT(*) FROM message_content", [], |r| r.get::<_, i64>(0)).unwrap(), 249);
            let idx: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_messages_folder', 'idx_messages_list', 'idx_messages_remote_u')", [], |r| r.get(0)).unwrap();
            assert_eq!(idx, 3, "eski tabloyla giden dizinler yeniden kurulmalı");
        }
        for id in [42, 177] {
            let m = db.message(id).unwrap();
            assert_eq!((m.subject, m.body_text, m.body_html), (format!("Konu {id}"), format!("gövde metni anahtar{id}"), format!("<p>{id}</p>")));
            assert_eq!(m.raw, vec![id as u8; 5000]);
            assert_eq!((m.remote_id.as_deref(), m.is_read, m.att_names), (Some(format!("r{id}").as_str()), id % 2 == 1, format!("ek{id}.pdf")));
        }
        assert!(db.needs_content(250) && !db.needs_content(42));
        // Konuşma anahtarı (arındırılmış konu) eski iletiler için de doldurulur
        assert_eq!(db.conn().query_row("SELECT COUNT(*) FROM messages WHERE ns IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(db.conversation(&db.message_row(177).unwrap()).iter().map(|r| r.id).collect::<Vec<_>>(), vec![177]);
        assert_eq!(db.messages_without_content(1, 10), vec![(250, "r250".to_string())]);
        // Gövdede arama (anahtarsız sözcük de gövdeye bakar)
        assert_eq!(db.messages(Some(1), "gövde:anahtar177", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(1), "anahtar42", &MsgFilter::All).len(), 1);
        // Arama dizini eski iletiler için arka planda kurulur; o zamana kadar dizinsiz aranır
        assert!(!db.fts_ready.load(std::sync::atomic::Ordering::Relaxed));
        while db.build_search_index(100) {}
        assert!(db.fts_ready.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(db.messages(Some(1), "gövde:anahtar177", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(1), "anahtar42", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(1), "ekadı:ek99", &MsgFilter::All).len(), 1);
        // Silinen iletinin gövdesi de gider; yeni ileti silinmişlerin kimliğini almaz
        db.delete_message(42);
        assert_eq!(db.conn().query_row("SELECT COUNT(*) FROM message_content WHERE id = 42", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let id = db.insert_message(&Message { folder_id: 1, subject: "yeni".into(), body_text: "yeni gövde".into(), raw: b"x".to_vec(), ..Default::default() }).unwrap();
        assert!(id > 260);
        assert_eq!(db.message(id).unwrap().body_text, "yeni gövde");
        // Sonradan inen gövde
        db.update_message_content(250, &Message { body_text: "indi".into(), raw: b"From: a\r\n\r\nindi".to_vec(), ..Default::default() });
        assert!(!db.needs_content(250));
        assert_eq!(db.message(250).unwrap().body_text, "indi");
        assert_eq!(db.messages(Some(1), "gövde:indi", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(1), "gövde:yeni", &MsgFilter::All)[0].id, id);
        // Klasör silinince iletileri ve gövdeleri gider
        db.delete_folder(1).unwrap();
        assert_eq!(db.conn().query_row("SELECT COUNT(*) FROM message_content", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        drop(db);
        // İkinci açılışta geçiş yeniden çalışmaz
        assert!(Db::open(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Arama dizini dizinsiz taramayla aynı sonucu verir ve ileti/gövde değiştikçe güncel kalır
    #[test]
    fn search_index_matches_scan_and_follows_changes() {
        use std::sync::atomic::Ordering;
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        let rows = [
            ("Bütçe toplantısı", "Ahmet Emre AYDIN <ahmet.aydin@kurum.com.tr>", "ŞÜKRÜ Bey, İSTANBUL ofisindeki kalamış projesi", "Kırmızı", "Teklif_2026.xlsx"),
            ("RE: Aylık rapor", "Ayşe Işık <ayse.isik@x.com>", "Rapor ektedir. Ahmet Bey'e iletildi.", "", "rapor.pdf"),
            ("Çay saati", "Veli <veli@x.com>", "aydın ışık çiçek ÇİÇEK emre", "Mavi", ""),
            ("Fatura 2026/11", "muhasebe@firma.com.tr", "Ödeme e-posta ile: odeme@firma.com.tr", "", "fatura-11.pdf"),
        ];
        let mut ids = vec![];
        for (subject, sender, body, cat, att) in rows {
            let m = Message { folder_id: inbox, subject: subject.into(), sender: sender.into(), to_addr: "ben@kurum.com.tr".into(), body_text: body.into(), raw: b"x".to_vec(), categories: cat.into(), att_names: att.into(), ..Default::default() };
            ids.push(db.insert_message(&m).unwrap());
        }
        let run = |q: &str| {
            let mut v: Vec<i64> = db.messages(Some(inbox), q, &MsgFilter::All).iter().map(|m| m.id).filter(|id| ids.contains(id)).collect();
            v.sort();
            v
        };
        let queries = [
            "aydın", "AYDIN", "aydin", "ahmet emre aydın", "\"emre aydin\"", "\"aydin emre\"", "ahmet.aydin@kurum.com.tr", "kişi:ahmet", "sukru", "İstanbul",
            "ıstanbul", "kalamis", "konu:butce", "konu:rapor", "gövde:ekte", "gövde:iletil", "çiçek", "cicek", "kategori:kirmizi", "ekadı:teklif",
            "ekadı:2026", "fatura 2026", "odeme@firma", "kimden:muhasebe", "kime:ben", "rapor is:unread", "yok", "a", "2026", "e-posta", "bey'e",
        ];
        assert!(db.fts_ready.load(Ordering::Relaxed));
        let indexed: Vec<Vec<i64>> = queries.iter().map(|q| run(q)).collect();
        db.fts_ready.store(false, Ordering::Relaxed);
        let scanned: Vec<Vec<i64>> = queries.iter().map(|q| run(q)).collect();
        db.fts_ready.store(true, Ordering::Relaxed);
        for (q, (a, b)) in queries.iter().zip(indexed.iter().zip(&scanned)) {
            assert_eq!(a, b, "dizinli ve dizinsiz arama farklı: {q}");
        }
        assert_eq!(run("aydın"), vec![ids[0], ids[2]]);
        assert_eq!(run("ahmet emre aydın"), vec![ids[0]], "ad soyad arka arkaya geçmeli");
        assert_eq!(run("kalamis"), vec![ids[0]]);
        // Konu, kategori, ek adı ve gövde değişince dizin güncellenir
        db.set_message_field(ids[2], "categories", "Acil");
        assert_eq!(run("kategori:acil"), vec![ids[2]]);
        assert!(run("kategori:mavi").is_empty());
        db.update_message_content(ids[3], &Message { subject: "yok sayılır".into(), sender: "tahsilat@firma.com.tr".into(), body_text: "havale bilgisi".into(), raw: b"y".to_vec(), ..Default::default() });
        assert_eq!(run("kimden:tahsilat"), vec![ids[3]]);
        assert_eq!(run("gövde:havale"), vec![ids[3]]);
        assert!(run("gövde:odeme").is_empty() && run("kimden:muhasebe").is_empty());
        assert_eq!(run("konu:fatura"), vec![ids[3]], "konu değişmedi, dizinde kalmalı");
        // Silinen ileti ve klasörüyle silinen iletiler dizinden çıkar
        db.delete_message(ids[0]);
        assert!(run("kalamis").is_empty());
        let n: i64 = db.conn().query_row("SELECT COUNT(*) FROM message_fts WHERE message_fts MATCH 'cicek'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let other = db.folder_by_role(None, "archive").map(|f| f.id).unwrap_or(inbox);
        db.set_message_field(ids[2], "folder_id", other);
        db.delete_folder(other).unwrap();
        let n: i64 = db.conn().query_row("SELECT COUNT(*) FROM message_fts WHERE message_fts MATCH 'cicek'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    /// Rastgele ekleme/güncelleme/silme dizisinden sonra canlı dizin, aynı verilerden sıfırdan
    /// kurulan dizinle sözcük sözcük aynıdır (silmede yanlış eski değer verilirse eski sözcükler
    /// dizinde kalırdı)
    #[test]
    fn search_index_stays_exact_under_random_changes() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        let archive = db.folder_by_role(None, "archive").unwrap().id;
        let vocab = ["ali", "ayşe", "IŞIK", "çiçek", "Şükrü", "rapor", "bütçe", "toplantı", "fatura", "İzmir", "ödeme", "acil", "proje", "teklif"];
        let mut seed = 0x9e3779b97f4a7c15u64;
        let mut rnd = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        let mut text = |k: usize| (0..k).map(|_| vocab[rnd(vocab.len())]).collect::<Vec<_>>().join(" ");
        let mut ids: Vec<i64> = Vec::new();
        for i in 0..120 {
            let body = if i % 2 == 0 { text(6) } else { String::new() };
            let m = Message { folder_id: if i % 3 == 0 { archive } else { inbox }, subject: text(3), sender: text(2), cc: text(1), categories: text(1), att_names: text(1), raw: if body.is_empty() { vec![] } else { b"x".to_vec() }, body_text: body, ..Default::default() };
            ids.push(db.insert_message(&m).unwrap());
        }
        for step in 0..400 {
            let id = ids[step * 7 % ids.len()];
            match step % 6 {
                0 => db.set_message_field(id, "categories", text(1)),
                1 => db.update_message_content(id, &Message { sender: text(2), cc: text(1), att_names: text(1), body_text: text(5), raw: b"y".to_vec(), ..Default::default() }),
                2 => {
                    let _ = db.conn().execute("UPDATE messages SET subject = ?1 WHERE id = ?2", params![text(3), id]);
                }
                3 => db.set_message_field(id, "is_read", 1),
                4 if step % 24 == 4 => db.delete_message(id),
                _ => {
                    let m = Message { folder_id: inbox, subject: text(2), body_text: text(4), raw: b"z".to_vec(), ..Default::default() };
                    ids.push(db.insert_message(&m).unwrap());
                }
            }
        }
        db.delete_folder(archive).unwrap();
        let c = db.conn();
        c.execute_batch("INSERT INTO message_fts(message_fts) VALUES ('integrity-check')").unwrap();
        // Aynı verilerden sıfırdan kurulan karşılaştırma dizini
        c.execute_batch(concat!(
            "CREATE VIRTUAL TABLE fts_check USING fts5(subject, sender, to_addr, cc, body_text, categories, att_names, content = '', ",
            "tokenize = \"unicode61 remove_diacritics 0 categories 'L* N* Co M*'\"); ",
            "INSERT INTO fts_check(rowid, subject, sender, to_addr, cc, body_text, categories, att_names) SELECT m.id, ",
            fts_row!("m", "c.body_text"),
            " FROM messages m LEFT JOIN message_content c ON c.id = m.id;"
        ))
        .unwrap();
        let hits = |table: &str, q: &str| -> Vec<i64> {
            c.prepare(&format!("SELECT rowid FROM {table} WHERE {table} MATCH ?1 ORDER BY rowid")).unwrap().query_map([q], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
        };
        let mut total = 0;
        for w in vocab {
            for col in ["subject", "sender", "cc", "body_text", "categories", "att_names"] {
                let q = format!("{{{col}}} : \"{}\"", search_fold(w));
                let live = hits("message_fts", &q);
                assert_eq!(live, hits("fts_check", &q), "{q}");
                total += live.len();
            }
        }
        assert!(total > 500, "anlamlı sayıda eşleşme olmalı: {total}");
    }

    /// Dizinsiz (1.3.0 öncesi) veritabanında dizin arka planda kurulur; bu arada değişen ve
    /// yeni gelen iletiler iki kez dizinlenmez, eski içerikleri eşleşmez
    #[test]
    fn search_index_builds_in_background() {
        use std::sync::atomic::Ordering;
        let dir = std::env::temp_dir().join(format!("lynora-fts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        {
            let db = Db::open(&path).unwrap();
            for i in 0..450 {
                db.insert_message(&Message { folder_id: 1, subject: format!("konu{i}son ortak"), body_text: format!("gövde{i}son"), raw: b"x".to_vec(), ..Default::default() }).unwrap();
            }
            // 1.3.0 öncesi gibi: dizin yok
            db.conn().execute_batch("DELETE FROM settings WHERE key = 'fts_version'").unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert!(!db.fts_ready.load(Ordering::Relaxed));
        let id = |q: &str| db.messages(Some(1), q, &MsgFilter::All).iter().map(|m| m.id).collect::<Vec<_>>();
        let first = id("konu:konu7son")[0];
        // Kurulum başladıktan sonra değişen eski ileti ve yeni ileti
        assert!(db.build_search_index(200));
        let late = id("konu:konu400son")[0];
        db.update_message_content(late, &Message { body_text: "değişti".into(), raw: b"y".to_vec(), ..Default::default() });
        let fresh = db.insert_message(&Message { folder_id: 1, subject: "taze".into(), ..Default::default() }).unwrap();
        while db.build_search_index(200) {}
        assert!(db.fts_ready.load(Ordering::Relaxed));
        assert_eq!(id("konu:konu7son"), vec![first]);
        assert_eq!(id("gövde:değişti"), vec![late]);
        assert!(id("gövde:gövde400son").is_empty());
        assert_eq!(id("taze"), vec![fresh]);
        assert_eq!(id("ortak").len(), 450);
        let n: i64 = db.conn().query_row("SELECT COUNT(*) FROM message_fts WHERE message_fts MATCH 'ortak'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 450, "aynı ileti dizinde bir kez");
        drop(db);
        // Yeniden açılışta dizin yeniden kurulmaz; iç tablolarının planlayıcı istatistiği tutulmaz
        let db = Db::open(&path).unwrap();
        assert!(db.fts_ready.load(Ordering::Relaxed));
        let stats: i64 = db.conn().query_row("SELECT COUNT(*) FROM sqlite_stat1 WHERE tbl LIKE 'message_fts%'", [], |r| r.get(0)).unwrap_or(0);
        assert_eq!(stats, 0);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Konuşma: arındırılmış konusu aynı iletiler (tüm klasörler, çöp hariç); konu değişince güncellenir
    #[test]
    fn conversation_by_normalized_subject() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        let sent = db.folder_by_role(None, "sent").unwrap().id;
        let trash = db.folder_by_role(None, "trash").unwrap().id;
        let add = |folder: i64, subject: &str, date: f64| db.insert_message(&Message { folder_id: folder, subject: subject.into(), date, ..Default::default() }).unwrap();
        let a = add(inbox, "Bütçe Toplantısı", 1.0);
        let b = add(sent, "RE: bütçe toplantısı", 2.0);
        let c = add(inbox, "YNT: Fw: Bütçe toplantısı", 3.0);
        let _ = add(trash, "RE: Bütçe toplantısı", 4.0);
        let other = add(inbox, "Bütçe toplantısı notları", 5.0);
        let ids = |m: i64| db.conversation(&db.message_row(m).unwrap()).iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(c), vec![a, b, c]);
        assert_eq!(ids(other), vec![other]);
        // Konuşma görünümünde gelen kutusu: konuşma tek satır; sayı gönderilen yanıtı da kapsar
        // (çöptekini kapsamaz), okunmamışlar yalnızca bu klasörden
        let conv = ListOrder { conversations: true, ..ListOrder::default() };
        let (rows, total) = db.messages_page(Some(inbox), "", &MsgFilter::All, &conv, 0, 50);
        let head = rows.iter().find(|r| r.id == c).unwrap();
        assert_eq!((head.thread_count, head.thread_unread, rows.iter().any(|r| r.id == a)), (3, 2, false));
        assert_eq!(total as usize, rows.len());
        // Çöp kutusunda kendi iletisi de görünür
        let (rows, _) = db.messages_page(Some(trash), "", &MsgFilter::All, &conv, 0, 50);
        assert_eq!(rows[0].thread_count, 4);
        assert_eq!(db.conversation(&rows[0]).len(), 4);
    }

    /// Konuşmalar yanıt başlıklarıyla kurulur; konu yalnızca başlıksız yanıtlarda yedektir
    #[test]
    fn threads_follow_reply_headers() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        let add = |subject: &str, mid: &str, irt: &str, refs: &str, conv: &str, date: f64| {
            let m = Message { folder_id: inbox, subject: subject.into(), message_id: mid.into(), in_reply_to: irt.into(), refs: refs.into(), conv: conv.into(), date, ..Default::default() };
            db.insert_message(&m).unwrap()
        };
        let tid = |id: i64| db.message_row(id).unwrap().thread_id;
        // Her gün gelen aynı konulu rapor: ayrı konuşmalar
        let (r1, r2) = (add("Günlük rapor", "<r1@x>", "", "", "", 1.0), add("Günlük rapor", "<r2@x>", "", "", "", 2.0));
        assert_ne!(tid(r1), tid(r2));
        // Başlıksız yanıt en yakın önceki aynı konulu iletiye bağlanır
        let r3 = add("YNT: Günlük rapor", "<r3@x>", "", "", "", 3.0);
        assert_eq!(tid(r3), tid(r2));
        // Zincir: yanıt kökten önce gelse de (sıra önemsiz) aynı konuşma
        let b = add("RE: Plan", "<b@x>", "<a@x>", "<a@x>", "", 11.0);
        let c = add("RE: Plan", "<c@x>", "<b@x>", "<a@x> <b@x>", "", 12.0);
        let a = add("Plan", "a@x", "", "", "", 10.0);
        assert_eq!((tid(a), tid(b)), (tid(c), tid(c)));
        // İki ayrı parça ortak ata gelince birleşir
        let e = add("RE: Sözleşme", "<e@x>", "<d@x>", "<root@x> <d@x>", "", 21.0);
        let f = add("RE: Sözleşme", "<f@x>", "<g@x>", "<g@x>", "", 22.0);
        assert_ne!(tid(e), tid(f));
        let g = add("RE: Sözleşme", "<g@x>", "<root@x>", "<root@x>", "", 20.0);
        assert_eq!((tid(e), tid(f)), (tid(g), tid(g)));
        // Konusu değiştirilen yanıt yeni konuşma başlatır
        let h = add("Bambaşka konu", "<h@x>", "<c@x>", "<a@x> <b@x> <c@x>", "", 13.0);
        assert_ne!(tid(h), tid(a));
        // Exchange kimliği: konu farklı olsa da aynı konuşma; aynı konulu başka kimlik ayrı
        let (x1, x2, x3) = (add("Teklif", "", "", "", "AAQk1", 30.0), add("YNT: Teklif (güncel)", "", "", "", "AAQk1", 31.0), add("YNT: Teklif", "", "", "", "AAQk2", 32.0));
        assert_eq!(tid(x1), tid(x2));
        assert_ne!(tid(x1), tid(x3));
        // Kimliği sonradan öğrenilen ileti konuya göre kurulmuş konuşmadan ayrılır
        let (y1, y2) = (add("Davet", "<y1@x>", "", "", "", 40.0), add("RE: Davet", "<y2@x>", "", "", "", 41.0));
        assert_eq!(tid(y1), tid(y2));
        assert!(db.set_conversation(y2, "AAQk9", "", ""));
        assert!(!db.set_conversation(y2, "AAQk7", "", ""));
        assert_ne!(tid(y1), tid(y2));
        assert!(db.set_conversation(y1, "AAQk9", "", ""));
        assert_eq!(tid(y1), tid(y2));
        // Baştan kurulunca aynı gruplar çıkar
        let groups = |db: &Db| {
            let ids = [r1, r2, r3, a, b, c, e, f, g, h, x1, x2, x3, y1, y2];
            let t: Vec<i64> = ids.iter().map(|&i| db.message_row(i).unwrap().thread_id).collect();
            t.iter().map(|x| t.iter().position(|y| y == x).unwrap()).collect::<Vec<_>>()
        };
        let before = groups(&db);
        Db::rebuild_threads(&db.conn()).unwrap();
        assert_eq!(groups(&db), before);
    }

    /// Eski veritabanının bir kerelik konuşma kurulumu yeterince hızlı mı (elle: --ignored)
    #[test]
    #[ignore]
    fn rebuild_threads_speed() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        {
            let mut c = db.conn();
            let tx = c.transaction().unwrap();
            for i in 0..100_000i64 {
                let (k, n) = (i / 4, i % 4);
                let subj = if n == 0 { format!("Konu {}", k % 5000) } else { format!("RE: Konu {}", k % 5000) };
                let refs = if n > 1 { format!("<m{}@x>", i - 1) } else { String::new() };
                tx.execute(
                    "INSERT INTO messages(folder_id, account_id, subject, ns, message_id, refs, date) VALUES (?1, 1, ?2, lyn_norm_subject(?2), ?3, ?4, ?5)",
                    params![inbox, subj, format!("<m{i}@x>"), refs, i as f64 * 60.0],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        let t = std::time::Instant::now();
        Db::rebuild_threads(&db.conn()).unwrap();
        let n: i64 = db.conn().query_row("SELECT COUNT(DISTINCT tid) FROM messages WHERE account_id = 1", [], |r| r.get(0)).unwrap();
        eprintln!("rebuild 100k: {:?}, {n} konuşma", t.elapsed());
        assert_eq!(n, 25_000);
    }

    /// Kullanıcının bayrak değişikliği sunucuya yazılana kadar bekler: eşitleme bu arada
    /// sunucudaki eski değeri geri yazmaz (okunan iletinin yeniden kalın görünmesi)
    #[test]
    fn pending_flags_block_stale_server_values() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        let add = |acc: Option<i64>, uid: Option<i64>, rid: Option<&str>| {
            db.insert_message(&Message { folder_id: inbox, account_id: acc, uid, remote_id: rid.map(String::from), ..Default::default() }).unwrap()
        };
        let (imap, ews, local, other) = (add(Some(1), Some(7), None), add(Some(1), None, Some("AAMk")), add(None, None, None), add(Some(2), Some(8), None));
        db.set_flags_local(&[imap, ews, local], "is_read", true);
        let read = |id| db.message_row(id).unwrap().is_read;
        assert!(read(imap) && read(ews) && read(local));
        // Yerel klasördeki ileti sunucuya gitmez
        let p = db.pending_flags(1);
        assert_eq!(p.iter().map(|p| (p.message_id, p.field.as_str(), p.value, p.uid)).collect::<Vec<_>>(), vec![(imap, "is_read", true, Some(7)), (ews, "is_read", true, None)]);
        assert_eq!(p[1].remote_id.as_deref(), Some("AAMk"));
        // Eşitleme okundu isteğinden önce alınmış "okunmadı" bilgisini getirir: yazılmaz
        assert!(!db.set_flag_from_server(imap, "is_read", false));
        assert!(read(imap));
        // Başka alan sunucudan güncellenir
        assert!(db.set_flag_from_server(imap, "is_flagged", true));
        // Gönderim sürerken yeniden değişen değer, eski gönderim bitince bekleyenlerden düşmez
        db.set_flags_local(&[ews], "is_read", false);
        db.clear_pending_flags(&p);
        assert_eq!(db.pending_flags(1).iter().map(|p| (p.message_id, p.value)).collect::<Vec<_>>(), vec![(ews, false)]);
        // Yazılan değerden sonra sunucu yine esas
        assert!(db.set_flag_from_server(imap, "is_read", false));
        assert!(!read(imap));
        // Sunucu üç kez reddederse vazgeçilir
        let p = db.pending_flags(1);
        assert_eq!((db.fail_pending_flags(&p, 3), db.fail_pending_flags(&p, 3)), (0, 0));
        assert_eq!(db.fail_pending_flags(&p, 3), 1);
        assert!(db.pending_flags(1).is_empty());
        // Yeni değişiklik deneme sayısını sıfırlar; ileti silinince bekleyen de silinir
        db.set_flags_local(&[other], "is_flagged", true);
        assert_eq!(db.fail_pending_flags(&db.pending_flags(2), 3), 0);
        db.set_flags_local(&[other], "is_flagged", false);
        assert_eq!(db.fail_pending_flags(&db.pending_flags(2), 2), 0);
        db.delete_message(other);
        assert!(db.pending_flags(2).is_empty());
    }

    /// Silinen postaların boşalttığı alan sıkıştırmayla geri alınır; sonrasında dosya artımlı
    /// küçültme kipindedir ve boş alan arka plan işiyle geri verilir
    #[test]
    fn compaction_and_incremental_reclaim() {
        let dir = std::env::temp_dir().join(format!("lynora-vacuum-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        // 1.2.0 ve öncesi gibi artımlı kipte olmayan dosya
        Connection::open(&path).unwrap().execute_batch("PRAGMA auto_vacuum = NONE; CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);").unwrap();
        let db = Db::open(&path).unwrap();
        let fill = |n: usize| -> Vec<i64> {
            (0..n).map(|i| db.insert_message(&Message { folder_id: 1, subject: format!("s{i}"), raw: vec![7u8; 40_000], body_text: "x".into(), ..Default::default() }).unwrap()).collect()
        };
        let ids = fill(200);
        db.delete_messages(&ids);
        let _ = db.conn().execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
        let (size, free, incremental) = db.storage_info();
        assert!(!incremental && free > 7_000_000 && size >= free, "{size} {free}");
        assert_eq!(db.reclaim_free_space(10_000), 0, "artımlı kip değilken dokunulmaz");
        let (before, after) = db.compact().unwrap();
        assert!(after < before / 2, "{before} → {after}");
        let (_, free, incremental) = db.storage_info();
        assert!(incremental && free == 0);
        let ids = fill(400);
        db.delete_messages(&ids);
        let freed = db.reclaim_free_space(1_000);
        assert_eq!(freed, 1_000);
        while db.reclaim_free_space(1_000) > 0 {}
        assert!(db.storage_info().1 < 2048 * 4096);
        assert_eq!(db.messages(Some(1), "", &MsgFilter::All).iter().filter(|m| m.subject.starts_with('s')).count(), 0);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Değişmeyen klasör yeniden yazılmaz: eşitleme sonrası arayüz yenilemesi gerçek değişikliğe bakar
    #[test]
    fn unchanged_folder_is_not_rewritten() {
        let db = Db::open_memory().unwrap();
        let mut f = db.folder_by_role(None, "inbox").unwrap();
        let before = db.total_changes();
        db.update_folder(&f).unwrap();
        assert_eq!(db.total_changes(), before);
        f.sync_state = "durum".into();
        db.update_folder(&f).unwrap();
        assert_eq!(db.total_changes(), before + 1);
        assert_eq!(db.folder(f.id).unwrap().sync_state, "durum");
    }

    /// Eşiği aşan sorgu tek satırda, kısaltılmış ve parametresiz yazılır; büyüyen günlük kenara alınır
    #[test]
    fn slow_query_log() {
        use std::time::Duration;
        assert!(slow_query_line("SELECT 1", Duration::from_millis(199)).is_none());
        let line = slow_query_line("SELECT *\n   FROM messages\tWHERE id = ?1", Duration::from_millis(250)).unwrap();
        assert!(line.contains(" 250 ms [") && line.ends_with("] SELECT * FROM messages WHERE id = ?1\n"), "{line}");
        let long = slow_query_line(&"ğ".repeat(5000), Duration::from_secs(1)).unwrap();
        assert!(long.ends_with("…\n") && long.chars().count() < 700);
        let dir = std::env::temp_dir().join(format!("lynora-slowlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slow-queries.log");
        std::fs::write(&path, vec![b'x'; (SLOW_LOG_MAX + 1) as usize]).unwrap();
        append_slow_log(&path, "yeni\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "yeni\n");
        assert_eq!(std::fs::metadata(dir.join("slow-queries.log.1")).unwrap().len(), SLOW_LOG_MAX + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Arama dili: VEYA/DEĞİL/parantez, "anahtar: değer", alan grubu, tarih dönemleri
    #[test]
    fn search_language() {
        use SearchExpr::*;
        let f = |cols: &'static [&'static str], v: &str| Term(SearchTerm::Field(cols, v.into()));
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(); // salı
        let p = |q: &str| parse_query_at(q, today);
        let d = |y, m, dd| day_ts(chrono::NaiveDate::from_ymd_opt(y, m, dd).unwrap());
        let range = |a, b| And(vec![Term(SearchTerm::DateAfter(a)), Term(SearchTerm::DateBefore(b))]);
        // Anahtar büyük/küçük harf ayırmaz; iki noktadan sonra boşluk olabilir
        assert_eq!(p("From: ali To:veli"), And(vec![f(FROM_COLS, "ali"), f(TO_COLS, "veli")]));
        assert_eq!(p("KİMDEN: \"ali veli\""), f(FROM_COLS, "ali veli"));
        assert_eq!(p("kimden:"), And(vec![]));
        assert_eq!(p("gizli:ali"), f(BCC_COLS, "ali"));
        // VEYA, DEĞİL, parantez; küçük harfli "ve"/"or" sıradan sözcüktür
        assert_eq!(p("fatura OR makbuz"), Or(vec![f(ALL_COLS, "fatura"), f(ALL_COLS, "makbuz")]));
        assert_eq!(p("kimden:ali (fatura VEYA makbuz) -taslak"), And(vec![f(FROM_COLS, "ali"), Or(vec![f(ALL_COLS, "fatura"), f(ALL_COLS, "makbuz")]), Not(Box::new(f(ALL_COLS, "taslak")))]));
        assert_eq!(p("NOT from:ali"), Not(Box::new(f(FROM_COLS, "ali"))));
        assert_eq!(p("ali ve veli"), And(vec![f(ALL_COLS, "ali"), f(ALL_COLS, "ve"), f(ALL_COLS, "veli")]));
        // Alan grubu: parantezdeki sözcükler o alanda
        assert_eq!(p("from:(ali OR veli) rapor"), And(vec![Or(vec![f(FROM_COLS, "ali"), f(FROM_COLS, "veli")]), f(ALL_COLS, "rapor")]));
        // Sözcüğün içindeki tire ve parantez ayırmaz; kapanmamış parantez aramayı bozmaz
        assert_eq!(p("e-posta Teklif(2)"), And(vec![f(ALL_COLS, "e-posta"), f(ALL_COLS, "Teklif(2)")]));
        assert_eq!(p("(ali veli"), And(vec![f(ALL_COLS, "ali"), f(ALL_COLS, "veli")]));
        assert_eq!(p("ali) veli"), And(vec![f(ALL_COLS, "ali)"), f(ALL_COLS, "veli")]));
        // Tarihler
        assert_eq!(p("tarih:bugün"), range(d(2026, 11, 10), d(2026, 11, 11)));
        assert_eq!(p("tarih:bu hafta rapor"), And(vec![Term(SearchTerm::DateAfter(d(2026, 11, 9))), Term(SearchTerm::DateBefore(d(2026, 11, 16))), f(ALL_COLS, "rapor")]));
        assert_eq!(p("on:\"last month\""), range(d(2026, 10, 1), d(2026, 11, 1)));
        assert_eq!(p("received:2026-01-15..2026-02-01"), range(d(2026, 1, 15), d(2026, 2, 2)));
        assert_eq!(p("tarih:>=01.03.2026"), Term(SearchTerm::DateAfter(d(2026, 3, 1))));
        assert_eq!(p("tarih:2025"), range(d(2025, 1, 1), d(2026, 1, 1)));
        assert_eq!(p("tarih:aralık"), range(d(2025, 12, 1), d(2026, 1, 1)));
        assert_eq!(p("sonra:7g"), Term(SearchTerm::DateAfter(d(2026, 11, 3))));
        assert_eq!(p("tarih:yarın"), f(ALL_COLS, "on:yarın"));
        // Durumlar
        assert_eq!(p("is:replied önem:yüksek klasör:arşiv"), And(vec![Term(SearchTerm::Sql("is_answered = 1")), Term(SearchTerm::Sql("importance = 2")), Term(SearchTerm::Folder("arşiv".into()))]));
        // Sunucu sorgusu
        assert_eq!(search_to_aqs("from:(ali OR veli) -taslak (fatura VEYA makbuz) is:replied"), "(from:ali OR from:veli) NOT taslak (fatura OR makbuz)");
        assert_eq!(expr_to_aqs(&p("NOT (ali veli) tarih:bugün")), "NOT (ali veli) received:>=2026-11-10 received:<2026-11-11");
    }

    /// Arama ağacı hem dizinle hem dizinsiz aynı sonucu verir; adres kişinin adıyla da bulunur
    #[test]
    fn search_expressions_filter_rows() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap().id;
        let sent = db.folder_by_role(None, "sent").unwrap().id;
        let _ = db.conn().execute("DELETE FROM messages", []);
        let add = |folder: i64, subject: &str, sender: &str, to: &str, bcc: &str| {
            db.insert_message(&Message { folder_id: folder, subject: subject.into(), sender: sender.into(), to_addr: to.into(), bcc: bcc.into(), date: 100.0, ..Default::default() }).unwrap()
        };
        let a = add(inbox, "Fatura ekim", "Ali Veli <ali.veli@kurum.com.tr>", "Ben", "");
        let b = add(inbox, "Makbuz", "Ayşe Kaya <ayse@kurum.com.tr>", "Ali Veli", "");
        let c = add(sent, "Fatura taslak", "Ben <ben@kurum.com.tr>", "Mehmet Öz", "gizli.kisi@x.com");
        let d = add(inbox, "Toplantı", "Zeynep Demir <zeynep@kurum.com.tr>", "Çağrı Şen", "");
        db.conn().execute("INSERT INTO directory(account_id, email, name) VALUES (1, 'cagri.s@kurum.com.tr', 'Çağrı Şen')", []).unwrap();
        for fts in [true, false] {
            db.fts_ready.store(fts, std::sync::atomic::Ordering::Relaxed);
            let ids = |q: &str| {
                let mut v: Vec<i64> = db.messages(None, q, &MsgFilter::All).iter().map(|m| m.id).collect();
                v.sort();
                v
            };
            assert_eq!(ids("fatura OR makbuz"), vec![a, b, c], "fts={fts}");
            assert_eq!(ids("fatura -taslak"), vec![a]);
            assert_eq!(ids("(fatura VEYA makbuz) NOT from:ali"), vec![b, c]);
            assert_eq!(ids("from:(ali OR zeynep)"), vec![a, d]);
            assert_eq!(ids("Kimden: ali"), vec![a]);
            assert_eq!(ids("bcc:gizli"), vec![c]);
            assert_eq!(ids("klasör:gönderilmiş"), vec![c]);
            assert_eq!(ids("in:inbox -fatura"), vec![b, d]);
            // Alıcı başlıkta yalnızca adıyla duruyor: adresle arama yine bulur
            assert_eq!(ids("to:ali.veli@kurum.com.tr"), vec![b]);
            assert_eq!(ids("kime:cagri.s@kurum.com.tr"), vec![d]);
            assert_eq!(ids("kişi:ali.veli@kurum.com.tr"), vec![a, b]);
            assert_eq!(ids("tarih:2020"), Vec::<i64>::new());
        }
    }

    #[test]
    fn parses_outlook_style_queries() {
        let t = parse_search(r#"kimden:ali konu:"aylık rapor" has:attachment fatura is:unread"#);
        assert_eq!(t[0], SearchTerm::Field(&["sender"], "ali".into()));
        assert_eq!(t[1], SearchTerm::Field(&["subject"], "aylık rapor".into()));
        assert_eq!(t[2], SearchTerm::Sql("has_attachments = 1"));
        assert_eq!(t[3], SearchTerm::Field(ALL_COLS, "fatura".into()));
        assert_eq!(t[4], SearchTerm::Sql("is_read = 0"));
        // e-posta adresi anahtar sözcük sanılmamalı
        assert_eq!(parse_search("ali@kurum.gov.tr")[0], SearchTerm::Field(ALL_COLS, "ali@kurum.gov.tr".into()));
        assert!(matches!(parse_search("after:2026-01-15")[0], SearchTerm::DateAfter(_)));
        assert_eq!(parse_search("ek:var")[0], SearchTerm::Sql("has_attachments = 1"));
        assert_eq!(search_to_aqs(r#"kimden:ali konu:"aylık rapor" has:attachment fatura"#), r#"from:ali subject:"aylık rapor" hasattachment:true fatura"#);
    }

    #[test]
    fn search_runs_against_db() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap();
        for (subj, from, att) in [("Aylık rapor", "Ali <ali@x.com>", true), ("Toplantı", "Veli <veli@x.com>", false)] {
            let m = Message { folder_id: inbox.id, subject: subj.into(), sender: from.into(), has_attachments: att, ..Default::default() };
            db.insert_message(&m).unwrap();
        }
        assert_eq!(db.messages(Some(inbox.id), "kimden:ali", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(inbox.id), "has:attachment", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(inbox.id), "from:veli konu:toplantı", &MsgFilter::All).len(), 1);
        assert_eq!(db.messages(Some(inbox.id), "from:veli konu:rapor", &MsgFilter::All).len(), 0);
    }

    #[test]
    fn suggestions_people_and_attachments() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap();
        for (from, att) in [("Ahmet Efe AYDIN <ahmet.aydin@x.com>", "Bütçe 2026.xlsx"), ("Ahmet Efe AYDIN <ahmet.aydin@x.com>", ""), ("Mehmet Kaya <mk@x.com>", "rapor.pdf")] {
            db.insert_message(&Message { folder_id: inbox.id, sender: from.into(), att_names: att.into(), has_attachments: !att.is_empty(), ..Default::default() }).unwrap();
        }
        let (people, atts) = db.search_suggest("ahmet");
        assert_eq!(people.len(), 1);
        assert_eq!(people[0]["count"], 2);
        assert!(atts.is_empty());
        let (_, atts) = db.search_suggest("butce");
        assert_eq!(atts[0]["name"], "Bütçe 2026.xlsx");
        assert_eq!(db.messages(Some(inbox.id), "kişi:ahmet.aydin@x.com", &MsgFilter::All).len(), 2);
        assert_eq!(db.messages(Some(inbox.id), "ekadı:rapor", &MsgFilter::All).len(), 1);
    }

    #[test]
    fn bulk_writes_counts_and_content_check() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap();
        let (t0, u0) = db.folder_counts().get(&inbox.id).copied().unwrap_or((0, 0));
        let ids: Vec<i64> = (0..5)
            .map(|i| db.insert_message(&Message { folder_id: inbox.id, subject: format!("s{i}"), remote_id: (i == 0).then(|| "r0".to_string()), ..Default::default() }).unwrap())
            .collect();
        let before = db.total_changes();
        db.set_messages_field(&ids[..3], "is_read", 1);
        assert!(db.total_changes() > before);
        assert_eq!(db.folder_counts().get(&inbox.id).copied(), Some((t0 + 5, u0 + 2)));
        // Gövdesi olmayan sunucu iletisi; yerel iletinin gövdesi indirilmez
        assert!(db.needs_content(ids[0]));
        assert!(!db.needs_content(ids[1]));
        // İzin verilmeyen alan yazılmaz
        db.set_messages_field(&ids, "folder_id", 999);
        assert_eq!(db.message_row(ids[0]).unwrap().folder_id, inbox.id);
        db.delete_messages(&ids[3..]);
        assert_eq!(db.folder_counts().get(&inbox.id).copied(), Some((t0 + 3, u0)));
        // Liste, sabitlenmiş önce ve tarihe göre dizinden okunur
        let order = ListOrder { by: "date".into(), desc: true, conversations: false };
        assert_eq!(db.messages_page(Some(inbox.id), "", &MsgFilter::All, &order, 0, 50).0.len() as i64, t0 + 3);
        let conv = ListOrder { by: "date".into(), desc: true, conversations: true };
        assert_eq!(db.messages_page(Some(inbox.id), "", &MsgFilter::All, &conv, 0, 50).0.len() as i64, t0 + 3);
    }

    #[test]
    fn turkish_fold_search() {
        assert_eq!(fold("KALAMIŞ"), fold("Kalamış"));
        assert_eq!(fold("İSTANBUL"), fold("ıstanbul"));
    }

    #[test]
    fn offline_address_book() {
        let db = Db::open_memory().unwrap();
        let row = |e: &str, n: &str, k: &str| DirRow { email: e.into(), name: n.into(), department: "Bilgi Teknolojileri".into(), kind: k.into(), ..Default::default() };
        db.replace_directory(7, &[row("Ali.Veli@x.com", "Ali Veli", "Person"), row("bt@x.com", "BT Destek", "DistributionList"), row("ayse@x.com", "Ayşe Kaya", "Person")]);
        assert_eq!(db.directory_count(7), 3);
        assert_eq!(db.directory_search(Some(7), "veli", 0, 50).len(), 1);
        assert_eq!(db.directory_search(None, "", 0, 2).len(), 2);
        assert_eq!(db.directory_search(Some(7), "teknoloji", 0, 50).len(), 3);
        // % ve _ arama karakteri olarak değil, düz metin olarak aranır
        assert_eq!(db.directory_search(Some(7), "%", 0, 50).len(), 0);
        let found = db.directory_lookup(&["ali.veli@x.com".into(), "BT@x.com".into(), "yok@x.com".into()]);
        assert_eq!(found.len(), 2);
        assert!(found["bt@x.com"].is_group());
        assert!(!found["ali.veli@x.com"].is_group());
        // Yeni indirme eskisinin yerini alır
        db.replace_directory(7, &[row("ayse@x.com", "Ayşe Kaya", "Person")]);
        assert_eq!(db.directory_count(7), 1);
    }

    #[test]
    fn paged_sorted_and_conversations() {
        let db = Db::open_memory().unwrap();
        let inbox = db.folder_by_role(None, "inbox").unwrap();
        let _ = db.conn().execute("DELETE FROM messages", []);
        for i in 0..1200 {
            let subj = if i % 3 == 0 { format!("Konu {}", i / 3) } else { format!("RE: Konu {}", i / 3) };
            let m = Message { folder_id: inbox.id, subject: subj, sender: format!("K{} <k@x.com>", i % 7), date: 1000.0 + i as f64, ..Default::default() };
            db.insert_message(&m).unwrap();
        }
        let o = ListOrder::default();
        let (p1, total) = db.messages_page(Some(inbox.id), "", &MsgFilter::All, &o, 0, 500);
        assert_eq!((p1.len(), total), (500, 1200));
        assert_eq!(p1[0].date, 2199.0);
        let (p3, t3) = db.messages_page(Some(inbox.id), "", &MsgFilter::All, &o, 1000, 500);
        assert_eq!((p3.len(), t3), (200, -1));
        assert_eq!(p3.last().unwrap().date, 1000.0);
        // Konuya göre artan, RE: önekleri yok sayılır
        let by_subj = ListOrder { by: "subject".into(), desc: false, conversations: false };
        let (s, _) = db.messages_page(Some(inbox.id), "", &MsgFilter::All, &by_subj, 0, 3);
        assert!(s.iter().all(|m| crate::mail::normalize_subject(&m.subject) == "Konu 0"));
        // Konuşma görünümü: konu başına tek satır ve sayı
        let conv = ListOrder { conversations: true, ..ListOrder::default() };
        let (c, ct) = db.messages_page(Some(inbox.id), "", &MsgFilter::All, &conv, 0, 500);
        assert_eq!(ct, 400);
        assert_eq!(c.len(), 400);
        assert!(c.iter().all(|m| m.thread_count == 3));
        // Arama sayfalı listede de çalışır
        let (f, _) = db.messages_page(Some(inbox.id), "kimden:K3", &MsgFilter::All, &o, 0, 5000);
        assert!(!f.is_empty() && f.iter().all(|m| m.sender.starts_with("K3")));
    }
}
