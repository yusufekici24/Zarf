//! SQLite tabanlı yerel veri deposu (SQLite ikili dosyaya gömülüdür).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub type Result<T> = rusqlite::Result<T>;

const SCHEMA: &str = r#"
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
CREATE TABLE IF NOT EXISTS messages (
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
CREATE INDEX IF NOT EXISTS idx_messages_folder ON messages(folder_id, date);
CREATE INDEX IF NOT EXISTS idx_messages_uid ON messages(folder_id, uid);
CREATE INDEX IF NOT EXISTS idx_messages_mid ON messages(message_id);
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
"#;

/// Sonradan eklenen sütunlar (eski veritabanlarını yükseltmek için).
const MIGRATIONS: &[(&str, &str, &str)] = &[
    ("accounts", "kind", "TEXT DEFAULT 'imap'"),
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
    /// Konuşma görünümünde konudaki ileti sayısı
    pub thread_count: i64,
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

const LIST_COLS: &str = "id, folder_id, account_id, uid, subject, sender, to_addr, date, snippet, \
    is_read, is_flagged, is_draft, is_answered, importance, categories, has_attachments, \
    message_id, in_reply_to, remote_id, is_pinned";

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
    /// "" yerel/Exchange | "remote" başkasının takvimi (salt okunur) | "birthdays"
    pub kind: String,
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
    /// Yalnızca okuma bağlantısı (WAL: yazma sürerken de okur). Eşitleme uzun yazma
    /// yaparken ileti açma, liste, klasör ağacı gibi arayüz okumaları onu beklemez.
    reader: Arc<Mutex<Connection>>,
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MAILNIX_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("mailnix")
}

/// Uygulamanın eski adıyla (Lynora) oluşturulmuş veriler yeni klasöre taşınır:
/// ~/.local/share/lynora/lynora.db -> ~/.local/share/mailnix/mailnix.db (iletiler, hesaplar,
/// ayarlar korunur). Yeni klasör zaten varsa dokunulmaz.
pub fn migrate_legacy_data_dir(new_dir: &std::path::Path, old_dir: &std::path::Path) {
    if new_dir.exists() || !old_dir.join("lynora.db").exists() {
        return;
    }
    if std::fs::rename(old_dir, new_dir).is_err() {
        return;
    }
    for ext in ["", "-wal", "-shm"] {
        let from = new_dir.join(format!("lynora.db{ext}"));
        if from.exists() {
            let _ = std::fs::rename(&from, new_dir.join(format!("mailnix.db{ext}")));
        }
    }
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
        if std::env::var_os("MAILNIX_DATA_DIR").is_none() {
            if let Some(base) = dirs::data_dir() {
                migrate_legacy_data_dir(&dir, &base.join("lynora"));
            }
        }
        let _ = std::fs::create_dir_all(&dir);
        crate::backup::apply_pending(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Self::open(dir.join("mailnix.db"))
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
        let r = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)?;
        r.execute_batch("PRAGMA temp_store = MEMORY; PRAGMA cache_size = -32000; PRAGMA busy_timeout = 5000;")?;
        Self::register_functions(&r)?;
        Ok(r)
    }

    fn init(conn: Connection, path: Option<std::path::PathBuf>) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA temp_store = MEMORY; PRAGMA cache_size = -32000; PRAGMA busy_timeout = 5000;")?;
        Self::register_functions(&conn)?;
        conn.execute_batch(SCHEMA)?;
        for (table, col, ddl) in MIGRATIONS {
            let exists: bool = conn
                .prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"))?
                .exists([col])?;
            if !exists {
                conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {ddl}"))?;
            }
        }
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
        // Sorgu planlayıcı istatistikleri (hızlı; yalnızca gerekirse çalışır)
        let _ = conn.execute_batch("PRAGMA optimize = 0x10002;");
        let conn = Arc::new(Mutex::new(conn));
        // Bellek içi veritabanında (testler) ayrı bağlantı açılamaz: aynı bağlantı kullanılır
        let reader = match path.as_deref().map(Self::open_reader) {
            Some(Ok(r)) => Arc::new(Mutex::new(r)),
            _ => conn.clone(),
        };
        let db = Db { conn, reader };
        if db.setting("schema_version").is_none() {
            db.first_run()?;
        }
        Ok(db)
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bu bağlantıyla açılıştan beri yapılan satır değişikliği sayısı (eşitleme bir şey
    /// değiştirdi mi anlamak için; tüm yazmalar bu bağlantıdan yapılır)
    pub fn total_changes(&self) -> u64 {
        self.conn().query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0)).map(|v| v as u64).unwrap_or(0)
    }

    /// Okuma bağlantısı (yalnızca SELECT)
    fn rconn(&self) -> MutexGuard<'_, Connection> {
        self.reader.lock().unwrap_or_else(|e| e.into_inner())
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
                "MailNix <mailnix@pardus.local>",
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
            "DELETE FROM messages WHERE (sender LIKE '%<mailnix@pardus.local>%' OR sender LIKE '%<lynora@pardus.local>%') AND folder_id IN \
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
                 oauth_token=?22, shared_of=?23 WHERE id=?16",
                params![a.name, a.email, a.display_name, a.imap_host, a.imap_port, a.imap_security,
                    a.smtp_host, a.smtp_port, a.smtp_security, a.username, a.password, a.signature,
                    a.sync_limit, a.auto_reply_enabled as i64, a.auto_reply_text, a.id, a.kind,
                    a.auth_method, a.ews_url, a.auth_state, a.auth_error, a.oauth_token, a.shared_of],
            )?;
            Ok(a.id)
        } else {
            c.execute(
                "INSERT INTO accounts(name, email, display_name, imap_host, imap_port, imap_security, \
                 smtp_host, smtp_port, smtp_security, username, password, signature, sync_limit, \
                 auto_reply_enabled, auto_reply_text, created, kind, auth_method, ews_url, auth_state, \
                 auth_error, oauth_token, shared_of) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
                params![a.name, a.email, a.display_name, a.imap_host, a.imap_port, a.imap_security,
                    a.smtp_host, a.smtp_port, a.smtp_security, a.username, a.password, a.signature,
                    a.sync_limit, a.auto_reply_enabled as i64, a.auto_reply_text, now(),
                    if a.kind.is_empty() { "imap" } else { a.kind.as_str() },
                    if a.auth_method.is_empty() { "password" } else { a.auth_method.as_str() },
                    a.ews_url, if a.auth_state.is_empty() { "ok" } else { a.auth_state.as_str() },
                    a.auth_error, a.oauth_token, a.shared_of],
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

    pub fn update_folder(&self, f: &Folder) -> Result<()> {
        self.conn().execute(
            "UPDATE folders SET name=?1, remote_name=?2, role=?3, parent_id=?4, uidvalidity=?5, \
             sync_state=?7, favorite=?8, full_sync=?9 WHERE id=?6",
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
            .query_row("SELECT COALESCE(length(raw), 0) = 0 AND remote_id IS NOT NULL FROM messages WHERE id = ?1", [id], |r| r.get::<_, Option<bool>>(0))
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
            let Ok(mut st) = c.prepare("SELECT id, raw FROM messages WHERE has_attachments = 1 AND (att_names IS NULL OR att_names = '') AND raw IS NOT NULL AND length(raw) > 0 LIMIT ?1") else { return 0 };
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
    fn filter_sql(search: &str, filter: &MsgFilter, sql: &mut String, args: &mut Vec<String>) {
        for term in parse_search(search) {
            match term {
                SearchTerm::Field(cols, v) => {
                    // Exchange/Outlook gibi: sözcük başı eşleşmesi, Türkçe harf ve aksan duyarsız.
                    // Birden çok sözcüklü değer (tırnaklı ifade, e-posta adresi) aynı alanda arka
                    // arkaya geçmeli. Önce hızlı LIKE ön elemesi (en uzun sözcükle), sonra kesin denetim.
                    let words = search_words(&v);
                    let Some(longest) = words.iter().max_by_key(|w| w.chars().count()) else { continue };
                    let phrase = words.len() > 1;
                    let pre: Vec<String> = cols.iter().map(|c| format!("{c} LIKE ?")).collect();
                    let exact: Vec<String> = cols.iter().map(|c| format!("lyn_match({c}, ?, {})", phrase as i32)).collect();
                    sql.push_str(&format!(" AND ({}) AND ({})", pre.join(" OR "), exact.join(" OR ")));
                    for _ in cols {
                        args.push(like_pattern(longest));
                    }
                    for _ in cols {
                        args.push(words.join(" "));
                    }
                }
                SearchTerm::Sql(cond) => {
                    sql.push_str(" AND ");
                    sql.push_str(cond);
                }
                SearchTerm::DateAfter(ts) => sql.push_str(&format!(" AND date >= {ts}")),
                SearchTerm::DateBefore(ts) => sql.push_str(&format!(" AND date < {ts}")),
            }
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
    fn search_count(&self, folder_id: Option<i64>, search: &str, filter: &MsgFilter) -> i64 {
        let mut args: Vec<String> = Vec::new();
        let wh = Self::where_sql(folder_id, search, filter, &mut args);
        self.rconn()
            .query_row(&format!("SELECT COUNT(*) FROM (SELECT 1 FROM messages{wh} LIMIT 1)"), rusqlite::params_from_iter(args.iter()), |r| r.get(0))
            .unwrap_or(0)
    }

    /// Klasördeki tüm iletiler (arka plan işleri için; arayüz sayfalı listeyi kullanır).
    pub fn messages(&self, folder_id: Option<i64>, search: &str, filter: &MsgFilter) -> Vec<MessageRow> {
        self.messages_page(folder_id, search, filter, &ListOrder::default(), 0, i64::MAX).0
    }

    fn where_sql(folder_id: Option<i64>, search: &str, filter: &MsgFilter, args: &mut Vec<String>) -> String {
        let mut sql = String::from(" WHERE 1=1");
        if let Some(f) = folder_id {
            sql.push_str(&format!(" AND folder_id = {f}"));
        } else {
            // Tüm posta kutularında: çöp ve gereksiz hariç
            sql.push_str(" AND folder_id NOT IN (SELECT id FROM folders WHERE role IN ('trash','junk'))");
        }
        Self::filter_sql(search, filter, &mut sql, args);
        sql
    }

    /// Outlook gibi sayfalı liste: sıralama ve (isteğe bağlı) konuşma gruplaması
    /// veritabanında yapılır. Dönen ikinci değer toplam satır sayısıdır.
    pub fn messages_page(&self, folder_id: Option<i64>, search: &str, filter: &MsgFilter, order: &ListOrder, offset: i64, limit: i64) -> (Vec<MessageRow>, i64) {
        // "ahmet emre aydın" gibi birden çok düz sözcük: önce ifade olarak (ad soyad gibi arka
        // arkaya) aranır; eşleşme varsa yalnızca bunlar gösterilir (dağıtım listelerinde ayrı
        // ayrı geçen Ahmet, Emre, Aydın'lar sonuçları doldurmaz), yoksa tüm sözcükler aranır
        let phrased;
        let search = match phrase_variant(search) {
            Some(p) if self.search_count(folder_id, &p, filter) > 0 => {
                phrased = p;
                phrased.as_str()
            }
            _ => search,
        };
        let mut args: Vec<String> = Vec::new();
        let wh = Self::where_sql(folder_id, search, filter, &mut args);
        let dir = if order.desc { "DESC" } else { "ASC" };
        let key = match order.by.as_str() {
            "from" => format!("ltrim(sender, '\" ') COLLATE NOCASE {dir}, "),
            "to" => format!("ltrim(to_addr, '\" ') COLLATE NOCASE {dir}, "),
            "subject" => format!("lyn_norm_subject(subject) {dir}, "),
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
            (
                format!(
                    "SELECT * FROM (SELECT {LIST_COLS}, COUNT(*) OVER (PARTITION BY ns) AS thread_count, \
                     ROW_NUMBER() OVER (PARTITION BY ns ORDER BY date DESC) AS rn FROM \
                     (SELECT {LIST_COLS}, lyn_norm_subject(subject) AS ns FROM messages{wh})) WHERE rn = 1{order_sql} LIMIT {limit} OFFSET {offset}"
                ),
                format!("SELECT COUNT(DISTINCT lyn_norm_subject(subject)) FROM messages{wh}"),
            )
        } else {
            (
                format!("SELECT {LIST_COLS} FROM messages{wh}{order_sql} LIMIT {limit} OFFSET {offset}"),
                format!("SELECT COUNT(*) FROM messages{wh}"),
            )
        };
        let rows: Vec<MessageRow> = match c.prepare(&sql) {
            Ok(mut st) => st
                .query_map(rusqlite::params_from_iter(args.iter()), MessageRow::from_row)
                .map(|r| r.flatten().collect())
                .unwrap_or_default(),
            Err(_) => vec![],
        };
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
            .query_row("SELECT * FROM messages WHERE id = ?1", [id], Message::from_row)
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

    /// Bir konuşmaya ait iletiler (Message-ID / In-Reply-To zinciri ve konu).
    pub fn conversation(&self, m: &MessageRow) -> Vec<MessageRow> {
        let base = crate::mail::normalize_subject(&m.subject);
        let c = self.rconn();
        let mut st = c
            .prepare(&format!(
                "SELECT {LIST_COLS} FROM messages WHERE folder_id NOT IN \
                 (SELECT id FROM folders WHERE role IN ('trash','junk')) AND \
                 (message_id = ?1 OR in_reply_to = ?1 OR message_id = ?2 OR subject LIKE ?3) \
                 ORDER BY date"
            ))
            .unwrap();
        st.query_map(params![m.message_id, m.in_reply_to, format!("%{base}")], MessageRow::from_row)
            .unwrap()
            .flatten()
            .filter(|r| crate::mail::normalize_subject(&r.subject) == base)
            .collect()
    }

    pub fn insert_message(&self, m: &Message) -> Result<i64> {
        let c = self.conn();
        c.execute(
            "INSERT INTO messages(folder_id, account_id, uid, message_id, in_reply_to, refs, subject, \
             sender, to_addr, cc, bcc, date, snippet, body_text, body_html, raw, is_read, is_flagged, \
             is_draft, is_answered, importance, categories, has_attachments, pending, remote_id, is_pinned, att_names) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27)",
            params![m.folder_id, m.account_id, m.uid, m.message_id, m.in_reply_to, m.refs, m.subject,
                m.sender, m.to_addr, m.cc, m.bcc, m.date, m.snippet, m.body_text, m.body_html, m.raw,
                m.is_read as i64, m.is_flagged as i64, m.is_draft as i64, m.is_answered as i64,
                m.importance, m.categories, m.has_attachments as i64, m.pending, m.remote_id, m.is_pinned as i64, m.att_names],
        )?;
        Ok(c.last_insert_rowid())
    }

    /// Yalnızca başlığı bilinen (Exchange) iletinin tam içeriğini yazar.
    pub fn update_message_content(&self, id: i64, m: &Message) {
        let _ = self.conn().execute(
            "UPDATE messages SET raw=?1, body_text=?2, body_html=?3, sender=?4, to_addr=?5, cc=?6, bcc=?7, \
             in_reply_to=?8, refs=?9, has_attachments=?10, snippet=?11, att_names=?15, \
             message_id=CASE WHEN message_id='' THEN ?12 ELSE message_id END, \
             subject=CASE WHEN subject='' OR instr(subject, char(65533)) > 0 THEN ?14 ELSE subject END WHERE id=?13",
            params![m.raw, m.body_text, m.body_html, m.sender, m.to_addr, m.cc, m.bcc, m.in_reply_to, m.refs,
                m.has_attachments as i64, m.snippet, m.message_id, id, m.subject, m.att_names],
        );
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
            "SELECT id, remote_id FROM messages WHERE folder_id = ?1 AND remote_id IS NOT NULL \
             AND (raw IS NULL OR length(raw) = 0) ORDER BY date DESC LIMIT ?2",
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
        let mut st = c.prepare("SELECT id, name, color, visible, account_id, owner, grp, kind FROM calendars ORDER BY id").unwrap();
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
                "INSERT INTO calendars(name, color, visible, account_id, owner, grp, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![cal.name, cal.color, cal.visible as i64, cal.account_id, cal.owner, cal.grp, cal.kind],
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
            e.uid = format!("{}@mailnix", uuid::Uuid::new_v4());
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
                 resources=?23, organizer=?24, is_cancelled=?25, is_meeting=?26, response=?27 WHERE id=?19",
                params![e.calendar_id, e.uid, e.title, e.location, e.description, e.start, e.end,
                    e.all_day as i64, e.recurrence, e.recur_interval, e.recur_until, e.exdates,
                    e.reminder_minutes, e.category, e.show_as, e.attendees, e.is_private as i64,
                    now(), e.id, e.remote_id, e.master_id, e.optional_attendees, e.resources,
                    e.organizer, e.is_cancelled as i64, e.is_meeting as i64, e.response],
            )?;
            Ok(e.id)
        } else {
            c.execute(
                "INSERT INTO events(calendar_id, uid, title, location, description, start, end, all_day, \
                 recurrence, recur_interval, recur_until, exdates, reminder_minutes, category, show_as, \
                 attendees, is_private, created, modified, remote_id, master_id, optional_attendees, resources, \
                 organizer, is_cancelled, is_meeting, response) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
                params![e.calendar_id.max(1), e.uid, e.title, e.location, e.description, e.start,
                    e.end, e.all_day as i64, e.recurrence, e.recur_interval.max(1), e.recur_until,
                    e.exdates, e.reminder_minutes, e.category, e.show_as, e.attendees,
                    e.is_private as i64, now(), e.remote_id, e.master_id, e.optional_attendees, e.resources,
                    e.organizer, e.is_cancelled as i64, e.is_meeting as i64, e.response],
            )?;
            Ok(c.last_insert_rowid())
        }
    }

    pub fn delete_event(&self, id: i64) {
        let _ = self.conn().execute("DELETE FROM events WHERE id = ?1", [id]);
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
#[derive(Debug, PartialEq)]
pub enum SearchTerm {
    Field(&'static [&'static str], String),
    Sql(&'static str),
    DateAfter(i64),
    DateBefore(i64),
}

const ALL_COLS: &[&str] = &["subject", "sender", "to_addr", "cc", "body_text", "categories", "att_names"];
/// "kişi:" (Outlook "Katılımcılar"): gönderen ya da alıcılardan biri
const PEOPLE_COLS: &[&str] = &["sender", "to_addr", "cc"];

/// Anahtarsız düz sözcükler (en az iki) tek bir ifadede birleştirilir: `ahmet emre aydın
/// is:unread` → `"ahmet emre aydın" is:unread`. Birleştirilecek sözcük yoksa None.
pub fn phrase_variant(q: &str) -> Option<String> {
    if q.contains('"') {
        return None;
    }
    let toks: Vec<&str> = q.split_whitespace().collect();
    let plain: Vec<&str> = toks.iter().copied().filter(|t| matches!(parse_search(t).as_slice(), [SearchTerm::Field(c, _)] if *c == ALL_COLS)).collect();
    if plain.len() < 2 {
        return None;
    }
    let rest: Vec<&str> = toks.iter().copied().filter(|t| !plain.contains(t)).collect();
    let mut out = format!("\"{}\"", plain.join(" "));
    for r in rest {
        out.push(' ');
        out.push_str(r);
    }
    Some(out)
}

/// Aramayı Exchange'in sorgu diline (AQS) çevirir (sunucuda arama için).
pub fn search_to_aqs(q: &str) -> String {
    let quote = |v: &str| if v.contains(char::is_whitespace) { format!("\"{}\"", v.replace('"', "")) } else { v.to_string() };
    let day = |t: i64| chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
    parse_search(q)
        .into_iter()
        .map(|t| match t {
            SearchTerm::Field(cols, v) if cols == ALL_COLS => quote(&v),
            SearchTerm::Field(cols, v) if cols == PEOPLE_COLS => format!("participants:{}", quote(&v)),
            SearchTerm::Field(cols, v) => {
                let key = match cols.first().copied() {
                    Some("sender") => "from",
                    Some("to_addr") => "to",
                    Some("cc") => "cc",
                    Some("subject") => "subject",
                    Some("body_text") => "body",
                    Some("categories") => "category",
                    Some("att_names") => "attachment",
                    _ => "",
                };
                if key.is_empty() { quote(&v) } else { format!("{key}:{}", quote(&v)) }
            }
            SearchTerm::Sql("has_attachments = 1") => "hasattachment:true".into(),
            SearchTerm::Sql("has_attachments = 0") => "hasattachment:false".into(),
            SearchTerm::Sql("is_read = 0") => "isread:false".into(),
            SearchTerm::Sql("is_read = 1") => "isread:true".into(),
            SearchTerm::Sql("is_flagged = 1") => "isflagged:true".into(),
            SearchTerm::Sql("importance = 2") => "importance:high".into(),
            SearchTerm::Sql(_) => String::new(),
            SearchTerm::DateAfter(t) => format!("received:>={}", day(t)),
            SearchTerm::DateBefore(t) => format!("received:<{}", day(t)),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_day(v: &str) -> Option<i64> {
    use chrono::{Local, NaiveDate, TimeZone};
    let d = NaiveDate::parse_from_str(v, "%Y-%m-%d").or_else(|_| NaiveDate::parse_from_str(v, "%d.%m.%Y")).ok()?;
    Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).single().map(|t| t.timestamp())
}

/// "kimden:ali konu:"aylık rapor" has:attachment fatura" gibi aramaları çözer.
/// Türkçe ve İngilizce anahtar sözcükler desteklenir; kalan sözcüklerin hepsi aranır.
pub fn parse_search(q: &str) -> Vec<SearchTerm> {
    // Sözcüklere ayır (tırnak içi tek parça)
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    let mut out = Vec::new();
    for tok in toks {
        let (key, val) = match tok.split_once(':') {
            Some((k, v)) if !v.is_empty() && !k.contains('@') => (k.to_lowercase(), v.to_string()),
            _ => (String::new(), tok.clone()),
        };
        let lv = val.to_lowercase();
        let yes = matches!(lv.as_str(), "yes" | "true" | "evet" | "var" | "attachment" | "attachments" | "ek");
        let term = match key.as_str() {
            "from" | "kimden" | "gönderen" => SearchTerm::Field(&["sender"], val),
            "to" | "kime" | "alıcı" => SearchTerm::Field(&["to_addr", "cc"], val),
            "cc" | "bilgi" => SearchTerm::Field(&["cc"], val),
            "subject" | "konu" => SearchTerm::Field(&["subject"], val),
            "participants" | "kişi" | "kisi" | "katılımcı" => SearchTerm::Field(PEOPLE_COLS, val),
            "attachmentname" | "filename" | "ekadı" | "ekadi" | "dosya" => SearchTerm::Field(&["att_names"], val),
            "body" | "gövde" | "metin" => SearchTerm::Field(&["body_text"], val),
            "category" | "categories" | "kategori" => SearchTerm::Field(&["categories"], val),
            "has" | "hasattachment" | "hasattachments" | "ekvar" | "ek" | "attachment" | "attachments" => {
                if yes || lv == "attachment" { SearchTerm::Sql("has_attachments = 1") } else { SearchTerm::Sql("has_attachments = 0") }
            }
            "is" | "durum" => match lv.as_str() {
                "unread" | "okunmamış" | "okunmadı" => SearchTerm::Sql("is_read = 0"),
                "read" | "okunmuş" | "okundu" => SearchTerm::Sql("is_read = 1"),
                "flagged" | "bayraklı" | "işaretli" => SearchTerm::Sql("is_flagged = 1"),
                "important" | "önemli" => SearchTerm::Sql("importance = 2"),
                _ => SearchTerm::Field(ALL_COLS, tok.clone()),
            },
            "after" | "since" | "sonra" | "received>" => match parse_day(&val) {
                Some(t) => SearchTerm::DateAfter(t),
                None => SearchTerm::Field(ALL_COLS, tok.clone()),
            },
            "before" | "önce" | "received<" => match parse_day(&val) {
                Some(t) => SearchTerm::DateBefore(t),
                None => SearchTerm::Field(ALL_COLS, tok.clone()),
            },
            "on" | "tarih" | "received" => match parse_day(&val) {
                Some(t) => {
                    out.push(SearchTerm::DateAfter(t));
                    SearchTerm::DateBefore(t + 86400)
                }
                None => SearchTerm::Field(ALL_COLS, tok.clone()),
            },
            _ => SearchTerm::Field(ALL_COLS, tok.clone()),
        };
        out.push(term);
    }
    out
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
        assert_eq!(phrase_variant("ahmet emre aydın is:unread").as_deref(), Some("\"ahmet emre aydın\" is:unread"));
        assert_eq!(phrase_variant("ahmet"), None);
        let db = Db::open_memory().unwrap();
        let n: bool = db.rconn().query_row("SELECT lyn_match('Ahmet Emre AYDIN', 'ahmet emre aydin', 1)", [], |r| r.get(0)).unwrap();
        assert!(n);
    }

    /// Eski adla (Lynora) oluşturulmuş veri klasörü yeni ada taşınır; yeni klasör varsa dokunulmaz
    #[test]
    fn migrates_legacy_data_dir() {
        let root = std::env::temp_dir().join(format!("mailnix-mig-{}", std::process::id()));
        let (old, new) = (root.join("lynora"), root.join("mailnix"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(old.join("photos")).unwrap();
        {
            let db = Db::open(old.join("lynora.db")).unwrap();
            db.set_setting("eski", "veri");
        }
        std::fs::write(old.join(".key"), [7u8; 32]).unwrap();
        migrate_legacy_data_dir(&new, &old);
        assert!(!old.exists());
        assert!(new.join("mailnix.db").exists() && !new.join("lynora.db").exists());
        assert!(new.join(".key").exists() && new.join("photos").is_dir());
        assert_eq!(Db::open(new.join("mailnix.db")).unwrap().setting("eski").as_deref(), Some("veri"));
        // İkinci kez: yeni klasör var, eski (yeniden oluşmuş) klasöre dokunulmaz
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("lynora.db"), b"x").unwrap();
        migrate_legacy_data_dir(&new, &old);
        assert!(old.join("lynora.db").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Dosya veritabanında okuma bağlantısı yazılanları görür, yazma sürerken de okur
    #[test]
    fn reader_connection_sees_writes() {
        let dir = std::env::temp_dir().join(format!("mailnix-db-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db = Db::open(dir.join("t.db")).unwrap();
        assert!(!Arc::ptr_eq(&db.conn, &db.reader));
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
            let subj = if i % 3 == 0 { format!("RE: Konu {}", i / 3) } else { format!("Konu {}", i / 3) };
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
