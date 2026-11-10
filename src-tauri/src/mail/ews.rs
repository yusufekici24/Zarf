//! Exchange Web Services (EWS) istemcisi — şirket içi Exchange 2013/2016/2019 ve
//! Exchange Online (OAuth ile).

use super::http::{Auth, Client};
use super::xml::{self, esc, Element};
use super::{MailError, MailResult};
use crate::db::{Account, Contact, ContactGroup, Event, Note, Task};
use base64::Engine;

const SOAP_HEAD: &str = r#"<?xml version="1.0" encoding="utf-8"?><soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/" xmlns:t="http://schemas.microsoft.com/exchange/services/2006/types" xmlns:m="http://schemas.microsoft.com/exchange/services/2006/messages"><soap:Header><t:RequestServerVersion Version="Exchange2013"/></soap:Header><soap:Body>"#;
const SOAP_TAIL: &str = "</soap:Body></soap:Envelope>";

pub struct Ews {
    pub url: String,
    http: Client,
    uses_password: bool,
    /// Paylaşılan posta kutusu: kendi kimliğimizle başka bir kutu açılır
    mailbox: Option<String>,
}

// ---------------------------------------------------------------------- bağlantı havuzu
// Outlook gibi: Exchange'e açılan TLS bağlantısı ve kimlik doğrulaması (NTLM/Kerberos
// bağlantı başına yapılır) her istekte yeniden kurulmaz; istemciler hesap başına
// havuzda tutulur ve iş parçacıkları arasında paylaşılır.
struct Pooled {
    ews: Ews,
    key: String,
    born: std::time::Instant,
}

static POOL: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<i64, Vec<Pooled>>>> =
    std::sync::LazyLock::new(Default::default);

fn pool_key(acc: &Account) -> String {
    format!("{}|{}|{}|{}|{}", acc.ews_url, acc.auth_method, acc.username, acc.password, acc.oauth_token.len())
}

/// Hesabın havuzdaki istemcilerini atar (hesap ayarları değişince).
pub fn pool_clear(account_id: i64) {
    POOL.lock().unwrap_or_else(|e| e.into_inner()).remove(&account_id);
}

/// Havuzdan bir istemci alıp işlemi çalıştırır; başarılıysa istemci havuza geri döner.
pub fn pooled<T>(acc: &Account, f: impl FnOnce(&mut Ews) -> MailResult<T>) -> MailResult<T> {
    let key = pool_key(acc);
    // OAuth belirteçleri süreli: o istemciler daha kısa yaşar
    let max_age = std::time::Duration::from_secs(if acc.auth_method == "oauth" { 600 } else { 1800 });
    let taken = {
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        let list = pool.entry(acc.id).or_default();
        list.retain(|p| p.key == key && p.born.elapsed() < max_age);
        list.pop()
    };
    let (mut ews, born) = match taken {
        Some(p) => (p.ews, p.born),
        None => (Ews::for_account(acc)?, std::time::Instant::now()),
    };
    let r = f(&mut ews);
    if r.is_ok() {
        let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
        let list = pool.entry(acc.id).or_default();
        if list.len() < 6 {
            list.push(Pooled { ews, key, born });
        }
    }
    r
}

/// Dizin satırı (Ad Seç penceresi)
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub email: String,
    pub title: String,
    pub department: String,
    pub office: String,
    pub phone: String,
    /// "Person", "DistributionList", "Room", "Mailbox", "PublicDL"...
    pub kind: String,
}

/// Kişi kartı bilgisi
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ContactCard {
    pub name: String,
    pub email: String,
    /// "Mailbox", "PublicDL", "GroupMailbox"...
    pub kind: String,
    pub job_title: String,
    pub department: String,
    pub company: String,
    pub office: String,
    pub manager: String,
    /// (tür, numara) — "BusinessPhone", "MobilePhone"...
    pub phones: Vec<(String, String)>,
    /// Grup üyeleri ("Ad <adres>")
    pub members: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EwsFolder {
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub class: String,
    pub role: Option<String>,
}

/// İçerik indirilmeden listelemek için öğe başlığı
#[derive(Debug, Clone, Default)]
pub struct EwsHeader {
    pub id: String,
    pub subject: String,
    pub sender: String,
    pub to: String,
    pub cc: String,
    pub date: f64,
    pub is_read: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    pub importance: i64,
    pub categories: Vec<String>,
    pub message_id: String,
    pub preview: String,
    pub is_draft: bool,
    pub item_class: String,
    pub pinned: bool,
    /// Sunucunun konuşma kimliği (base64; PidTagConversationId, yoksa ConversationIndex'teki GUID)
    pub conversation: String,
    pub in_reply_to: String,
    pub references: String,
}

/// Başlık için istenen alanlar
const HEADER_PROPS: &str = r#"<t:AdditionalProperties><t:FieldURI FieldURI="item:Subject"/><t:FieldURI FieldURI="message:From"/><t:FieldURI FieldURI="item:DisplayTo"/><t:FieldURI FieldURI="item:DisplayCc"/><t:FieldURI FieldURI="item:DateTimeReceived"/><t:FieldURI FieldURI="message:IsRead"/><t:FieldURI FieldURI="item:Flag"/><t:FieldURI FieldURI="item:HasAttachments"/><t:FieldURI FieldURI="item:Importance"/><t:FieldURI FieldURI="item:Categories"/><t:FieldURI FieldURI="message:InternetMessageId"/><t:FieldURI FieldURI="item:Preview"/><t:FieldURI FieldURI="item:IsDraft"/><t:FieldURI FieldURI="item:ItemClass"/><t:ExtendedFieldURI PropertyTag="0x0F01" PropertyType="SystemTime"/><t:ExtendedFieldURI PropertyTag="0x3013" PropertyType="Binary"/><t:ExtendedFieldURI PropertyTag="0x0071" PropertyType="Binary"/><t:ExtendedFieldURI PropertyTag="0x1042" PropertyType="String"/><t:ExtendedFieldURI PropertyTag="0x1039" PropertyType="String"/></t:AdditionalProperties>"#;

/// Outlook/OWA "Sabitle": PidTagRenewTime ve PidTagReceivedOrRenewTime 4500 yılına ayarlanır
const PIN_TIME: &str = "4500-09-01T00:00:00Z";

impl EwsHeader {
    fn from_element(it: &Element) -> Option<EwsHeader> {
        let id = it.child("ItemId").and_then(|i| i.attr("Id"))?.to_string();
        let sender = it
            .child("From")
            .and_then(|f| f.child("Mailbox"))
            .map(|mb| {
                let (n, e) = (mb.text_of("Name"), mb.text_of("EmailAddress"));
                if e.contains('@') {
                    if n.is_empty() || n == e { e } else { format!("{} <{e}>", n.replace(['"', '<', '>'], "")) }
                } else {
                    n
                }
            })
            .unwrap_or_default();
        // Genişletilmiş özellik (MAPI etiketi) değeri
        let ext = |tag: u32| {
            it.children_named("ExtendedProperty")
                .find(|p| p.child("ExtendedFieldURI").and_then(|u| u.attr("PropertyTag")).and_then(|t| u32::from_str_radix(t.trim_start_matches("0x"), 16).ok()) == Some(tag))
                .map(|p| p.text_of("Value").trim().to_string())
                .unwrap_or_default()
        };
        // Konuşma: PidTagConversationId; yoksa PidTagConversationIndex'in 6-22. baytları (aynı GUID)
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut conversation = ext(0x3013).split_whitespace().collect::<String>();
        if conversation.is_empty() {
            if let Some(g) = b64.decode(ext(0x0071).split_whitespace().collect::<String>()).ok().filter(|b| b.len() >= 22) {
                conversation = b64.encode(&g[6..22]);
            }
        }
        Some(EwsHeader {
            id,
            conversation,
            in_reply_to: ext(0x1042),
            references: ext(0x1039),
            subject: it.text_of("Subject"),
            sender,
            to: it.text_of("DisplayTo").replace("; ", ", "),
            cc: it.text_of("DisplayCc").replace("; ", ", "),
            date: ews_to_ts(&it.text_of("DateTimeReceived")).unwrap_or(0.0),
            is_read: it.child("IsRead").map(|x| x.text.trim() == "true").unwrap_or(true),
            flagged: it.find("FlagStatus").map(|x| x.text.trim() == "Flagged").unwrap_or(false),
            has_attachments: it.child("HasAttachments").map(|x| x.text.trim() == "true").unwrap_or(false),
            importance: match it.text_of("Importance").as_str() {
                "High" => 2,
                "Low" => 0,
                _ => 1,
            },
            categories: it.child("Categories").map(|c| c.children.iter().map(|s| s.text.clone()).collect()).unwrap_or_default(),
            message_id: it.text_of("InternetMessageId").trim_matches(|c| c == '<' || c == '>').to_string(),
            preview: it.text_of("Preview"),
            is_draft: it.child("IsDraft").map(|x| x.text.trim() == "true").unwrap_or(false),
            item_class: it.text_of("ItemClass"),
            pinned: it.children_named("ExtendedProperty").any(|p| {
                let tag = p.child("ExtendedFieldURI").and_then(|u| u.attr("PropertyTag")).unwrap_or("").to_lowercase();
                // OWA/Outlook sabitlemesi: alınma/yenilenme zamanı uzak geleceğe (4500 yılı) alınır
                (tag == "0xf01" || tag == "0x0f01") && p.text_of("Value").get(..4).and_then(|y| y.parse::<u32>().ok()).is_some_and(|y| y >= 4000)
            }),
        })
    }

    /// Meeting request vb. takvim iletisi mi
    pub fn is_meeting(&self) -> bool {
        self.item_class.starts_with("IPM.Schedule.Meeting")
    }
}

#[derive(Debug, Default)]
pub struct SyncChanges {
    pub state: String,
    pub done: bool,
    /// Yeni öğelerin başlıkları
    pub creates: Vec<EwsHeader>,
    /// (kimlik, okundu, bayraklı, sabitlenmiş)
    pub updates: Vec<(String, Option<bool>, Option<bool>, Option<bool>)>,
    pub deletes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EwsMessage {
    pub id: String,
    pub mime: Vec<u8>,
    pub is_read: bool,
    pub flagged: bool,
    pub categories: Vec<String>,
}

fn ts_to_ews(ts: f64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

/// Takvim görünümü parçası (31 gün) ve parça başına en çok öğe (Exchange'in varsayılan 1000'lik
/// sınırının altında: aşan parça ikiye bölünür)
const CAL_CHUNK_SECS: f64 = 31.0 * 86400.0;
const CAL_MAX: usize = 500;
/// Daha fazla bölünmeyen en kısa aralık
const CAL_MIN_SECS: f64 = 3600.0;

/// [start, end) aralığını `chunk` uzunluğunda parçalarla çeker. `fetch` (öğeler, tamamı geldi mi)
/// döndürür; tamamı gelmeyen parça ikiye bölünür (en kısa aralıkta gelen kabul edilir).
fn fetch_split<T>(start: f64, end: f64, chunk: f64, fetch: &mut dyn FnMut(f64, f64) -> MailResult<(Vec<T>, bool)>) -> MailResult<Vec<T>> {
    let mut out = Vec::new();
    let mut stack: Vec<(f64, f64)> = Vec::new();
    let mut s = start;
    while s < end {
        let e = (s + chunk).min(end);
        stack.push((s, e));
        s = e;
    }
    // Sıra korunur: bölünen parçanın ilk yarısı önce
    stack.reverse();
    while let Some((s, e)) = stack.pop() {
        let (items, complete) = fetch(s, e)?;
        if complete || e - s <= CAL_MIN_SECS {
            out.extend(items);
        } else {
            let mid = s + ((e - s) / 2.0).floor();
            stack.push((mid, e));
            stack.push((s, mid));
        }
    }
    Ok(out)
}

pub fn ews_to_ts(s: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.timestamp() as f64)
}

fn item_ids(ids: &[String]) -> String {
    ids.iter().map(|i| format!(r#"<t:ItemId Id="{}"/>"#, esc(i))).collect()
}

/// Ayrılmış klasör adları (kimlik yerine kullanılabilir)
const DIST: &[&str] = &["inbox", "sentitems", "drafts", "deleteditems", "junkemail", "outbox", "calendar", "contacts", "msgfolderroot", "tasks", "notes", "archivemsgfolderroot"];

fn folder_ref(id: &str) -> String {
    if DIST.contains(&id) {
        format!(r#"<t:DistinguishedFolderId Id="{id}"/>"#)
    } else {
        format!(r#"<t:FolderId Id="{}"/>"#, esc(id))
    }
}

impl Ews {
    pub fn for_account(acc: &Account) -> MailResult<Ews> {
        if acc.ews_url.is_empty() {
            return Err(MailError(crate::t!("error.no_ews_url")));
        }
        // Kullanıcı adı boşsa bilgisayara giriş yapılan etki alanı kullanıcısı, o da yoksa e-posta adresi
        let user = if !acc.username.is_empty() {
            acc.username.clone()
        } else {
            Some(super::gssapi::default_login()).filter(|l| !l.is_empty()).unwrap_or_else(|| acc.email.clone())
        };
        let (auth, uses_password) = match acc.auth_method.as_str() {
            "kerberos" => (Auth::Negotiate, false),
            "oauth" => (Auth::Bearer(crate::oauth::access_token(acc)?), false),
            "ntlm" => (Auth::Ntlm { user, password: acc.password_plain() }, true),
            "basic" => (Auth::Basic { user, password: acc.password_plain() }, true),
            _ => (Auth::Auto { user, password: acc.password_plain(), kerberos: false }, true),
        };
        let mailbox = acc.shared_of.map(|_| acc.email.clone());
        Ok(Ews { url: acc.ews_url.clone(), http: Client::new(auth), uses_password, mailbox })
    }

    /// Klasör başvurusu: paylaşılan kutuda ayrılmış klasörler o kutuya yönlendirilir
    fn folder(&self, id: &str) -> String {
        match &self.mailbox {
            Some(m) if DIST.contains(&id) => {
                format!(r#"<t:DistinguishedFolderId Id="{id}"><t:Mailbox><t:EmailAddress>{}</t:EmailAddress></t:Mailbox></t:DistinguishedFolderId>"#, esc(m))
            }
            _ => folder_ref(id),
        }
    }

    /// Hesap kaydı olmadan (otomatik kurulum sırasında) istemci oluşturur.
    pub fn with_auth(url: &str, auth: Auth, uses_password: bool) -> Ews {
        let mut http = Client::new(auth);
        http.timeout = std::time::Duration::from_secs(15);
        Ews { url: url.to_string(), http, uses_password, mailbox: None }
    }

    /// SOAP isteği gönderir; yanıt gövdesini (Body) döndürür.
    pub fn call(&mut self, body: &str) -> MailResult<Element> {
        let req = format!("{SOAP_HEAD}{body}{SOAP_TAIL}");
        let r = self.http.request("POST", &self.url, "text/xml; charset=utf-8", req.as_bytes())?;
        if r.status == 401 {
            let msg = crate::t!("error.ews_auth");
            // Yalnızca parola kullanılıyorsa kilitlenme koruması devreye girer
            return Err(if self.uses_password { MailError::auth(msg) } else { MailError(msg) });
        }
        let doc = xml::parse(&r.body).map_err(MailError)?;
        if let Some(f) = doc.find("Fault") {
            let msg = f.find("faultstring").map(|x| x.text.clone()).unwrap_or_else(|| "SOAP fault".into());
            return Err(MailError(format!("Exchange: {msg}")));
        }
        if r.status >= 400 {
            return Err(MailError(format!("Exchange: HTTP {}", r.status)));
        }
        // İlk hata veren yanıt iletisi
        let mut msgs = Vec::new();
        collect_response_messages(&doc, &mut msgs);
        for m in &msgs {
            if m.attr("ResponseClass") == Some("Error") {
                let code = m.text_of("ResponseCode");
                // Silinmiş öğe vb. hatalar toplu işlemlerde göz ardı edilebilir
                if code == "ErrorItemNotFound" && msgs.len() > 1 {
                    continue;
                }
                return Err(MailError(format!("Exchange: {} ({code})", m.text_of("MessageText"))));
            }
        }
        Ok(doc)
    }

    // ------------------------------------------------------------------ klasörler
    pub fn list_folders(&mut self) -> MailResult<Vec<EwsFolder>> {
        let dist = ["inbox", "sentitems", "drafts", "deleteditems", "junkemail", "msgfolderroot"];
        let ids: String = dist.iter().map(|d| self.folder(d)).collect();
        let doc = self.call(&format!(r#"<m:GetFolder><m:FolderShape><t:BaseShape>IdOnly</t:BaseShape></m:FolderShape><m:FolderIds>{ids}</m:FolderIds></m:GetFolder>"#))?;
        let mut roles = std::collections::HashMap::new();
        let mut root_id = String::new();
        let mut fids = Vec::new();
        doc.find_all("FolderId", &mut fids);
        for (i, f) in fids.iter().enumerate().take(dist.len()) {
            let id = f.attr("Id").unwrap_or("").to_string();
            match dist[i] {
                "inbox" => roles.insert(id, "inbox"),
                "sentitems" => roles.insert(id, "sent"),
                "drafts" => roles.insert(id, "drafts"),
                "deleteditems" => roles.insert(id, "trash"),
                "junkemail" => roles.insert(id, "junk"),
                _ => {
                    root_id = id;
                    None
                }
            };
        }
        let root = self.folder("msgfolderroot");
        let doc = self.call(&format!(r#"<m:FindFolder Traversal="Deep"><m:FolderShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties><t:FieldURI FieldURI="folder:DisplayName"/><t:FieldURI FieldURI="folder:ParentFolderId"/><t:FieldURI FieldURI="folder:FolderClass"/></t:AdditionalProperties></m:FolderShape><m:ParentFolderIds>{root}</m:ParentFolderIds></m:FindFolder>"#))?;
        let mut folders_el = Vec::new();
        doc.find_all("Folder", &mut folders_el);
        let mut out = Vec::new();
        for f in folders_el {
            let id = f.child("FolderId").and_then(|x| x.attr("Id")).unwrap_or("").to_string();
            let parent = f.child("ParentFolderId").and_then(|x| x.attr("Id")).unwrap_or("").to_string();
            let class = f.text_of("FolderClass");
            if !(class.is_empty() || class.starts_with("IPF.Note")) {
                continue;
            }
            let name = f.text_of("DisplayName");
            let mut role = roles.get(&id).map(|r| r.to_string());
            if role.is_none() && parent == root_id && ["archive", "arşiv", "arsiv"].contains(&name.to_lowercase().as_str()) {
                role = Some("archive".into());
            }
            out.push(EwsFolder { id, parent_id: if parent == root_id { String::new() } else { parent }, name, class, role });
        }
        Ok(out)
    }

    pub fn create_folder(&mut self, parent: &str, name: &str) -> MailResult<String> {
        let parent = if parent.is_empty() { "msgfolderroot" } else { parent };
        let doc = self.call(&format!(r#"<m:CreateFolder><m:ParentFolderId>{}</m:ParentFolderId><m:Folders><t:Folder><t:DisplayName>{}</t:DisplayName></t:Folder></m:Folders></m:CreateFolder>"#, folder_ref(parent), esc(name)))?;
        Ok(doc.find("FolderId").and_then(|f| f.attr("Id")).unwrap_or("").to_string())
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) -> MailResult<()> {
        self.call(&format!(r#"<m:UpdateFolder><m:FolderChanges><t:FolderChange>{}<t:Updates><t:SetFolderField><t:FieldURI FieldURI="folder:DisplayName"/><t:Folder><t:DisplayName>{}</t:DisplayName></t:Folder></t:SetFolderField></t:Updates></t:FolderChange></m:FolderChanges></m:UpdateFolder>"#, folder_ref(id), esc(name)))?;
        Ok(())
    }

    pub fn delete_folder(&mut self, id: &str) -> MailResult<()> {
        self.call(&format!(r#"<m:DeleteFolder DeleteType="MoveToDeletedItems"><m:FolderIds>{}</m:FolderIds></m:DeleteFolder>"#, folder_ref(id)))?;
        Ok(())
    }

    pub fn empty_folder(&mut self, id: &str) -> MailResult<()> {
        self.call(&format!(r#"<m:EmptyFolder DeleteType="HardDelete" DeleteSubFolders="false"><m:FolderIds>{}</m:FolderIds></m:EmptyFolder>"#, folder_ref(id)))?;
        Ok(())
    }

    // ------------------------------------------------------------------ eşitleme
    pub fn sync_items(&mut self, folder_id: &str, state: &str) -> MailResult<SyncChanges> {
        let st = if state.is_empty() { String::new() } else { format!("<m:SyncState>{}</m:SyncState>", esc(state)) };
        let doc = self.call(&format!(r#"<m:SyncFolderItems><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape>{HEADER_PROPS}</m:ItemShape><m:SyncFolderId>{}</m:SyncFolderId>{st}<m:MaxChangesReturned>256</m:MaxChangesReturned></m:SyncFolderItems>"#, folder_ref(folder_id)))?;
        let mut ch = SyncChanges {
            state: doc.find("SyncState").map(|s| s.text.clone()).unwrap_or_default(),
            done: doc.find("IncludesLastItemInRange").map(|s| s.text.trim() == "true").unwrap_or(true),
            ..Default::default()
        };
        if let Some(changes) = doc.find("Changes") {
            for c in &changes.children {
                let item = c.children.iter().find(|x| x.name != "ItemId" && x.name != "IsRead");
                let id_of = |e: &Element| e.find("ItemId").and_then(|i| i.attr("Id")).map(String::from);
                match c.name.as_str() {
                    "Create" => {
                        if let Some(h) = item.and_then(EwsHeader::from_element) {
                            ch.creates.push(h);
                        }
                    }
                    "Update" => {
                        if let Some(it) = item {
                            if let Some(id) = id_of(it) {
                                let read = it.child("IsRead").map(|x| x.text.trim() == "true");
                                let flag = it.find("FlagStatus").map(|x| x.text.trim() == "Flagged");
                                let pinned = EwsHeader::from_element(it).map(|h| h.pinned);
                                ch.updates.push((id, read, flag, pinned));
                            }
                        }
                    }
                    "ReadFlagChange" => {
                        if let Some(id) = id_of(c) {
                            ch.updates.push((id, c.child("IsRead").map(|x| x.text.trim() == "true"), None, None));
                        }
                    }
                    "Delete" => {
                        if let Some(id) = id_of(c) {
                            ch.deletes.push(id);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(ch)
    }

    /// Klasördeki en yeni öğeler (alınma zamanına göre azalan). `before` verilirse
    /// o zamandan eski olanlar; `query` verilirse sunucuda arama (AQS).
    pub fn find_items(&mut self, folder_id: &str, before: Option<f64>, query: Option<&str>, max: usize) -> MailResult<(Vec<EwsHeader>, bool)> {
        let restriction = match before {
            Some(t) if query.is_none() => format!(
                r#"<m:Restriction><t:IsLessThan><t:FieldURI FieldURI="item:DateTimeReceived"/><t:FieldURIOrConstant><t:Constant Value="{}"/></t:FieldURIOrConstant></t:IsLessThan></m:Restriction>"#,
                ts_to_ews(t)
            ),
            _ => String::new(),
        };
        let qs = query.map(|q| format!("<m:QueryString>{}</m:QueryString>", esc(q))).unwrap_or_default();
        let doc = self.call(&format!(
            r#"<m:FindItem Traversal="Shallow"><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape>{HEADER_PROPS}</m:ItemShape><m:IndexedPageItemView MaxEntriesReturned="{max}" Offset="0" BasePoint="Beginning"/>{restriction}<m:SortOrder><t:FieldOrder Order="Descending"><t:FieldURI FieldURI="item:DateTimeReceived"/></t:FieldOrder></m:SortOrder><m:ParentFolderIds>{}</m:ParentFolderIds>{qs}</m:FindItem>"#,
            folder_ref(folder_id)
        ))?;
        let mut out = Vec::new();
        let mut more = false;
        if let Some(root) = doc.find("RootFolder") {
            more = root.attr("IncludesLastItemInRange") == Some("false");
            if let Some(items) = root.child("Items") {
                out.extend(items.children.iter().filter_map(EwsHeader::from_element));
            }
        }
        Ok((out, more))
    }

    pub fn get_messages(&mut self, ids: &[String]) -> MailResult<Vec<EwsMessage>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(10) {
            let doc = self.call(&format!(r#"<m:GetItem><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:IncludeMimeContent>true</t:IncludeMimeContent><t:AdditionalProperties><t:FieldURI FieldURI="message:IsRead"/><t:FieldURI FieldURI="item:Flag"/><t:FieldURI FieldURI="item:Categories"/></t:AdditionalProperties></m:ItemShape><m:ItemIds>{}</m:ItemIds></m:GetItem>"#, item_ids(chunk)))?;
            let mut items = Vec::new();
            doc.find_all("Items", &mut items);
            for container in items {
                for it in &container.children {
                    let Some(id) = it.child("ItemId").and_then(|i| i.attr("Id")) else { continue };
                    let mime = it
                        .child("MimeContent")
                        .and_then(|m| base64::engine::general_purpose::STANDARD.decode(m.text.split_whitespace().collect::<String>()).ok())
                        .unwrap_or_default();
                    if mime.is_empty() {
                        continue;
                    }
                    out.push(EwsMessage {
                        id: id.to_string(),
                        mime,
                        is_read: it.child("IsRead").map(|x| x.text.trim() == "true").unwrap_or(true),
                        flagged: it.find("FlagStatus").map(|x| x.text.trim() == "Flagged").unwrap_or(false),
                        categories: it.child("Categories").map(|c| c.children.iter().map(|s| s.text.clone()).collect()).unwrap_or_default(),
                    });
                }
            }
        }
        Ok(out)
    }

    fn update_items(&mut self, ids: &[String], field: &str, value_xml: &str) -> MailResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let changes: String = ids
            .iter()
            .map(|i| format!(r#"<t:ItemChange><t:ItemId Id="{}"/><t:Updates><t:SetItemField><t:FieldURI FieldURI="{field}"/><t:Message>{value_xml}</t:Message></t:SetItemField></t:Updates></t:ItemChange>"#, esc(i)))
            .collect();
        self.call(&format!(r#"<m:UpdateItem MessageDisposition="SaveOnly" ConflictResolution="AlwaysOverwrite" SuppressReadReceipts="true"><m:ItemChanges>{changes}</m:ItemChanges></m:UpdateItem>"#))?;
        Ok(())
    }

    pub fn set_read(&mut self, ids: &[String], read: bool) -> MailResult<()> {
        self.update_items(ids, "message:IsRead", &format!("<t:IsRead>{read}</t:IsRead>"))
    }

    /// Sabitle / sabitlemeyi kaldır (Outlook ve OWA ile aynı özellikler)
    /// Klasörde sabitlenmiş öğelerin kimlikleri (OWA'da/başka istemcide sabitlenenler de)
    pub fn pinned_ids(&mut self, folder_id: &str) -> MailResult<Vec<String>> {
        let doc = self.call(&format!(
            r#"<m:FindItem Traversal="Shallow"><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape></m:ItemShape><m:IndexedPageItemView MaxEntriesReturned="500" Offset="0" BasePoint="Beginning"/><m:Restriction><t:IsGreaterThan><t:ExtendedFieldURI PropertyTag="0x0F01" PropertyType="SystemTime"/><t:FieldURIOrConstant><t:Constant Value="4000-01-01T00:00:00Z"/></t:FieldURIOrConstant></t:IsGreaterThan></m:Restriction><m:ParentFolderIds>{}</m:ParentFolderIds></m:FindItem>"#,
            folder_ref(folder_id)
        ))?;
        let mut ids = Vec::new();
        doc.find_all("ItemId", &mut ids);
        Ok(ids.iter().filter_map(|i| i.attr("Id").map(String::from)).collect())
    }

    pub fn set_pinned(&mut self, ids: &[String], pinned: bool) -> MailResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let props = ["0x0F01", "0x0F02"];
        let changes: String = ids
            .iter()
            .map(|id| {
                let ops: String = props
                    .iter()
                    .map(|tag| {
                        let uri = format!(r#"<t:ExtendedFieldURI PropertyTag="{tag}" PropertyType="SystemTime"/>"#);
                        if pinned {
                            format!(r#"<t:SetItemField>{uri}<t:Message><t:ExtendedProperty>{uri}<t:Value>{PIN_TIME}</t:Value></t:ExtendedProperty></t:Message></t:SetItemField>"#)
                        } else {
                            format!("<t:DeleteItemField>{uri}</t:DeleteItemField>")
                        }
                    })
                    .collect();
                format!(r#"<t:ItemChange><t:ItemId Id="{}"/><t:Updates>{ops}</t:Updates></t:ItemChange>"#, esc(id))
            })
            .collect();
        self.call(&format!(r#"<m:UpdateItem ConflictResolution="AlwaysOverwrite" MessageDisposition="SaveOnly" SuppressReadReceipts="true"><m:ItemChanges>{changes}</m:ItemChanges></m:UpdateItem>"#))?;
        Ok(())
    }

    pub fn set_flag(&mut self, ids: &[String], flagged: bool) -> MailResult<()> {
        let status = if flagged { "Flagged" } else { "NotFlagged" };
        self.update_items(ids, "item:Flag", &format!("<t:Flag><t:FlagStatus>{status}</t:FlagStatus></t:Flag>"))
    }

    pub fn set_categories(&mut self, ids: &[String], cats: &[String]) -> MailResult<()> {
        let inner: String = cats.iter().map(|c| format!("<t:String>{}</t:String>", esc(c))).collect();
        self.update_items(ids, "item:Categories", &format!("<t:Categories>{inner}</t:Categories>"))
    }

    pub fn move_items(&mut self, ids: &[String], to: &str) -> MailResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.call(&format!(r#"<m:MoveItem><m:ToFolderId>{}</m:ToFolderId><m:ItemIds>{}</m:ItemIds></m:MoveItem>"#, folder_ref(to), item_ids(ids)))?;
        Ok(())
    }

    pub fn delete_items(&mut self, ids: &[String], hard: bool) -> MailResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let t = if hard { "HardDelete" } else { "MoveToDeletedItems" };
        self.call(&format!(r#"<m:DeleteItem DeleteType="{t}" SendMeetingCancellations="SendToNone" AffectedTaskOccurrences="AllOccurrences"><m:ItemIds>{}</m:ItemIds></m:DeleteItem>"#, item_ids(ids)))?;
        Ok(())
    }

    /// Ham iletiyi klasöre kaydeder (içe aktarma / taşıma). Okundu ve "gönderilmiş"
    /// olarak işaretlenir; aksi halde Exchange onu gönderilmemiş taslak sanar.
    pub fn save_mime(&mut self, folder: &str, mime: &[u8], read: bool, draft: bool) -> MailResult<String> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(mime);
        let flags = if draft { 8 + 1 } else if read { 1 } else { 0 };
        let doc = self.call(&format!(r#"<m:CreateItem MessageDisposition="SaveOnly"><m:SavedItemFolderId>{}</m:SavedItemFolderId><m:Items><t:Message><t:MimeContent CharacterSet="UTF-8">{b64}</t:MimeContent><t:ExtendedProperty><t:ExtendedFieldURI PropertyTag="0x0E07" PropertyType="Integer"/><t:Value>{flags}</t:Value></t:ExtendedProperty></t:Message></m:Items></m:CreateItem>"#, folder_ref(folder)))?;
        Ok(doc.find("ItemId").and_then(|i| i.attr("Id")).unwrap_or("").to_string())
    }

    /// İletiyi gönderir ve Gönderilmiş Öğeler'e kopyasını kaydeder.
    pub fn send_mime(&mut self, mime: &[u8]) -> MailResult<()> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(mime);
        // PR_MESSAGE_FLAGS = okundu: MIME ile oluşturulan iletide "gönderilmedi" bayrağı
        // kalırsa Gönderilmiş Öğeler'deki kopya Outlook'ta da taslak görünüyordu
        let sent = self.folder("sentitems");
        self.call(&format!(r#"<m:CreateItem MessageDisposition="SendAndSaveCopy"><m:SavedItemFolderId>{sent}</m:SavedItemFolderId><m:Items><t:Message><t:MimeContent CharacterSet="UTF-8">{b64}</t:MimeContent><t:ExtendedProperty><t:ExtendedFieldURI PropertyTag="0x0E07" PropertyType="Integer"/><t:Value>1</t:Value></t:ExtendedProperty></t:Message></m:Items></m:CreateItem>"#))?;
        Ok(())
    }

    // ------------------------------------------------------------------ genel adres listesi
    pub fn resolve_names(&mut self, query: &str) -> MailResult<Vec<(String, String)>> {
        Ok(self.resolve_names_typed(query)?.into_iter().map(|(n, e, _)| (n, e)).collect())
    }

    /// Ad çözümleme; üçüncü değer posta kutusu türü ("Mailbox", "PublicDL", "PrivateDL"...)
    pub fn resolve_names_typed(&mut self, query: &str) -> MailResult<Vec<(String, String, String)>> {
        let doc = match self.call(&format!(r#"<m:ResolveNames ReturnFullContactData="false" SearchScope="ActiveDirectoryContacts"><m:UnresolvedEntry>{}</m:UnresolvedEntry></m:ResolveNames>"#, esc(query))) {
            Ok(d) => d,
            // Eşleşme yoksa EWS hata döndürür
            Err(e) if e.message().contains("ErrorNameResolutionNoResults") => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut res = Vec::new();
        doc.find_all("Resolution", &mut res);
        Ok(res
            .iter()
            .filter_map(|r| {
                let mb = r.child("Mailbox")?;
                let email = mb.text_of("EmailAddress");
                if !email.contains('@') {
                    return None;
                }
                Some((mb.text_of("Name"), email, mb.text_of("MailboxType")))
            })
            .collect())
    }

    /// Kişi kartı (Outlook'ta ada tıklayınca açılan kart): dizindeki tam kişi bilgisi
    pub fn contact_card(&mut self, email: &str) -> MailResult<Option<ContactCard>> {
        let doc = match self.call(&format!(r#"<m:ResolveNames ReturnFullContactData="true" SearchScope="ActiveDirectoryContacts"><m:UnresolvedEntry>smtp:{}</m:UnresolvedEntry></m:ResolveNames>"#, esc(email))) {
            Ok(d) => d,
            Err(e) if e.message().contains("ErrorNameResolutionNoResults") => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut res = Vec::new();
        doc.find_all("Resolution", &mut res);
        let pick = res
            .iter()
            .find(|r| r.child("Mailbox").map(|m| m.text_of("EmailAddress").eq_ignore_ascii_case(email)).unwrap_or(false))
            .or(res.first());
        let Some(r) = pick else { return Ok(None) };
        let mb = r.child("Mailbox");
        let mut c = ContactCard {
            name: mb.map(|m| m.text_of("Name")).unwrap_or_default(),
            email: mb.map(|m| m.text_of("EmailAddress")).filter(|e| e.contains('@')).unwrap_or_else(|| email.to_string()),
            kind: mb.map(|m| m.text_of("MailboxType")).unwrap_or_default(),
            ..Default::default()
        };
        if let Some(ct) = r.child("Contact") {
            let dn = ct.text_of("DisplayName");
            if !dn.is_empty() {
                c.name = dn;
            }
            c.job_title = ct.text_of("JobTitle");
            c.department = ct.text_of("Department");
            c.company = ct.text_of("CompanyName");
            c.office = ct.text_of("OfficeLocation");
            c.manager = ct.text_of("Manager");
            if let Some(ph) = ct.child("PhoneNumbers") {
                for e in &ph.children {
                    let v = e.text.trim();
                    if !v.is_empty() {
                        c.phones.push((e.attr("Key").unwrap_or("").to_string(), v.to_string()));
                    }
                }
            }
        }
        Ok(Some(c))
    }

    /// Kurum dizini (Outlook "Ad Seç: Genel Adres Listesi"): sayfalı liste ve arama.
    /// Exchange 2013+ FindPeople (dizin klasörü); desteklenmezse ResolveNames ile arama.
    pub fn directory_page(&mut self, query: &str, offset: usize, limit: usize) -> MailResult<(Vec<DirEntry>, bool)> {
        let q = query.trim();
        match self.find_people(q, offset, limit) {
            Ok((rows, total, n)) => {
                let more = if total > 0 { offset + n < total } else { n >= limit };
                Ok((rows, more))
            }
            Err(first) => {
                // Eski sunucu ya da boş aramaya izin vermeyen dizin: ResolveNames (en çok 100
                // sonuç). Boş aramada "SMTP:" dizindeki ilk kişileri döndürür (liste dolu açılır).
                if offset > 0 {
                    return Ok((vec![], false));
                }
                let rq = if q.is_empty() { "SMTP:" } else { q };
                match self.resolve_names_dir(rq) {
                    Ok((rows, _)) => Ok((rows, false)),
                    Err(e) => Err(if q.is_empty() { first } else { e }),
                }
            }
        }
    }

    /// FindPeople ile dizin sayfası. Döner: (satırlar, sunucunun bildirdiği toplam (0 = yok), ham kişi sayısı)
    pub fn find_people(&mut self, q: &str, offset: usize, limit: usize) -> MailResult<(Vec<DirEntry>, usize, usize)> {
        let qs = if q.is_empty() { String::new() } else { format!("<m:QueryString>{}</m:QueryString>", esc(q)) };
        let body = format!(
            r#"<m:FindPeople><m:PersonaShape><t:BaseShape>Default</t:BaseShape><t:AdditionalProperties><t:FieldURI FieldURI="persona:Title"/><t:FieldURI FieldURI="persona:Department"/><t:FieldURI FieldURI="persona:OfficeLocations"/><t:FieldURI FieldURI="persona:BusinessPhoneNumbers"/><t:FieldURI FieldURI="persona:EmailAddress"/><t:FieldURI FieldURI="persona:PersonaType"/></t:AdditionalProperties></m:PersonaShape><m:IndexedPageItemView BasePoint="Beginning" MaxEntriesReturned="{limit}" Offset="{offset}"/><m:ParentFolderId><t:DistinguishedFolderId Id="directory"/></m:ParentFolderId>{qs}</m:FindPeople>"#
        );
        let doc = self.call(&body)?;
        let mut ps = Vec::new();
        doc.find_all("Persona", &mut ps);
        let first_val = |e: Option<&Element>, inner: &str| -> String {
            e.and_then(|x| {
                let mut v = Vec::new();
                x.find_all(inner, &mut v);
                v.into_iter().map(|y| y.text.trim().to_string()).find(|t| !t.is_empty())
            })
            .unwrap_or_default()
        };
        let rows: Vec<DirEntry> = ps
            .iter()
            .filter_map(|p| {
                let email = p.child("EmailAddress").map(|e| e.text_of("EmailAddress")).unwrap_or_default();
                if !email.contains('@') {
                    return None;
                }
                Some(DirEntry {
                    name: p.text_of("DisplayName"),
                    email,
                    title: p.text_of("Title"),
                    department: p.text_of("Department"),
                    office: first_val(p.child("OfficeLocations"), "Value"),
                    phone: first_val(p.child("BusinessPhoneNumbers"), "Number"),
                    kind: p.text_of("PersonaType"),
                })
            })
            .collect();
        let total: usize = doc.find("TotalNumberOfPeopleInView").and_then(|t| t.text.trim().parse().ok()).unwrap_or(0);
        Ok((rows, total, ps.len()))
    }

    /// ResolveNames ile dizin araması (sunucu en çok 100 sonuç verir).
    /// Döner: (satırlar, sonuç kesildi mi — daha fazla eşleşme var)
    pub fn resolve_names_dir(&mut self, q: &str) -> MailResult<(Vec<DirEntry>, bool)> {
        let doc = match self.call(&format!(r#"<m:ResolveNames ReturnFullContactData="true" SearchScope="ActiveDirectory"><m:UnresolvedEntry>{}</m:UnresolvedEntry></m:ResolveNames>"#, esc(q))) {
            Ok(d) => d,
            Err(e) if e.message().contains("ErrorNameResolutionNoResults") => return Ok((vec![], false)),
            Err(e) => return Err(e),
        };
        let mut res = Vec::new();
        doc.find_all("Resolution", &mut res);
        let truncated = doc.find("ResolutionSet").map(|r| r.attr("IncludesLastItemInRange") == Some("false")).unwrap_or(false) || res.len() >= 100;
        let rows = res
            .iter()
            .filter_map(|r| {
                let mb = r.child("Mailbox")?;
                let email = mb.text_of("EmailAddress");
                if !email.contains('@') {
                    return None;
                }
                let ct = r.child("Contact");
                let phone = ct
                    .and_then(|c| c.child("PhoneNumbers"))
                    .and_then(|p| p.children.iter().find(|e| e.attr("Key") == Some("BusinessPhone")).map(|e| e.text.trim().to_string()))
                    .unwrap_or_default();
                Some(DirEntry {
                    name: ct.map(|c| c.text_of("DisplayName")).filter(|n| !n.is_empty()).unwrap_or_else(|| mb.text_of("Name")),
                    email,
                    title: ct.map(|c| c.text_of("JobTitle")).unwrap_or_default(),
                    department: ct.map(|c| c.text_of("Department")).unwrap_or_default(),
                    office: ct.map(|c| c.text_of("OfficeLocation")).unwrap_or_default(),
                    phone,
                    kind: mb.text_of("MailboxType"),
                })
            })
            .collect();
        Ok((rows, truncated))
    }

    /// Dağıtım listesinin üyeleri (Outlook'ta gruptaki "+")
    pub fn expand_dl(&mut self, email: &str) -> MailResult<Vec<(String, String, String)>> {
        let doc = self.call(&format!(r#"<m:ExpandDL><m:Mailbox><t:EmailAddress>{}</t:EmailAddress></m:Mailbox></m:ExpandDL>"#, esc(email)))?;
        let mut mbs = Vec::new();
        doc.find_all("Mailbox", &mut mbs);
        Ok(mbs
            .iter()
            .filter_map(|m| {
                let e = m.text_of("EmailAddress");
                e.contains('@').then(|| (m.text_of("Name"), e, m.text_of("MailboxType")))
            })
            .collect())
    }

    // ------------------------------------------------------------------ anlık bildirimler
    /// Tüm klasörler için akış aboneliği (Outlook gibi: yeni posta anında gelir).
    pub fn subscribe_streaming(&mut self) -> MailResult<String> {
        let doc = self.call(
            r#"<m:Subscribe><m:StreamingSubscriptionRequest SubscribeToAllFolders="true"><t:EventTypes><t:EventType>NewMailEvent</t:EventType><t:EventType>CreatedEvent</t:EventType><t:EventType>DeletedEvent</t:EventType><t:EventType>ModifiedEvent</t:EventType><t:EventType>MovedEvent</t:EventType><t:EventType>CopiedEvent</t:EventType></t:EventTypes></m:StreamingSubscriptionRequest></m:Subscribe>"#,
        )?;
        let id = doc.find("SubscriptionId").map(|x| x.text.trim().to_string()).unwrap_or_default();
        if id.is_empty() {
            return Err(MailError("Exchange: abonelik kimliği yok".into()));
        }
        Ok(id)
    }

    /// Akıştaki olayları bekler; her bildirimde değişen klasör kimlikleri `on_change`'e
    /// verilir (`false` dönerse akış kapanır). Sunucu bağlantıyı `minutes` sonra kapatır.
    /// Dönen değer: abonelik hâlâ geçerli mi.
    pub fn stream_events(&mut self, subscription: &str, minutes: u32, mut on_change: impl FnMut(Vec<String>, bool) -> bool) -> MailResult<bool> {
        let req = format!(
            "{SOAP_HEAD}<m:GetStreamingEvents><m:SubscriptionIds><t:SubscriptionId>{}</t:SubscriptionId></m:SubscriptionIds><m:ConnectionTimeout>{minutes}</m:ConnectionTimeout></m:GetStreamingEvents>{SOAP_TAIL}",
            esc(subscription)
        );
        let mut buf: Vec<u8> = Vec::new();
        let mut valid = true;
        let url = self.url.clone();
        let idle = std::time::Duration::from_secs(minutes as u64 * 60 + 120);
        let status = self.http.post_streaming(&url, "text/xml; charset=utf-8", req.as_bytes(), idle, |chunk| {
            buf.extend_from_slice(chunk);
            // Yanıt birden çok SOAP zarfı olarak akar
            let close = regex::bytes::Regex::new(r"</([A-Za-z0-9_]+:)?Envelope\s*>").unwrap();
            loop {
                let Some(m) = close.find(&buf) else { break };
                let one = String::from_utf8_lossy(&buf[..m.end()]).to_string();
                buf.drain(..m.end());
                let Ok(doc) = xml::parse(one.as_bytes()) else { continue };
                if doc.find("ErrorCode").map(|c| c.text.contains("SubscriptionNotFound") || c.text.contains("InvalidSubscription")).unwrap_or(false)
                    || one.contains("ErrorSubscriptionNotFound")
                    || one.contains("ErrorInvalidSubscription")
                {
                    valid = false;
                    return false;
                }
                let mut ids: Vec<String> = Vec::new();
                let mut new_mail = false;
                for kind in ["NewMailEvent", "CreatedEvent", "DeletedEvent", "ModifiedEvent", "MovedEvent", "CopiedEvent"] {
                    let mut evs = Vec::new();
                    doc.find_all(kind, &mut evs);
                    if kind == "NewMailEvent" && !evs.is_empty() {
                        new_mail = true;
                    }
                    for e in evs {
                        for tag in ["ParentFolderId", "OldParentFolderId"] {
                            if let Some(id) = e.child(tag).and_then(|f| f.attr("Id")) {
                                ids.push(id.to_string());
                            }
                        }
                        // Klasörün kendisi değiştiyse (ör. okunmamış sayısı)
                        if let Some(id) = e.child("FolderId").and_then(|f| f.attr("Id")) {
                            ids.push(id.to_string());
                        }
                    }
                }
                ids.sort();
                ids.dedup();
                if (!ids.is_empty() || new_mail) && !on_change(ids, new_mail) {
                    return false;
                }
                if one.contains("<m:ConnectionStatus>Closed") || one.contains(">Closed</") {
                    return false;
                }
            }
            true
        })?;
        if status == 401 {
            return Err(if self.uses_password { MailError::auth(crate::t!("error.ews_auth")) } else { MailError(crate::t!("error.ews_auth")) });
        }
        if status >= 400 {
            return Err(MailError(format!("Exchange: HTTP {status}")));
        }
        Ok(valid)
    }

    // ------------------------------------------------------------------ odalar
    /// Kuruluştaki oda listeleri ve odalar (Outlook "Oda Bulucu"): (liste adı, oda adı, adres)
    pub fn rooms(&mut self) -> MailResult<Vec<(String, String, String)>> {
        let doc = match self.call("<m:GetRoomLists/>") {
            Ok(d) => d,
            Err(e) if e.is_auth() => return Err(e),
            Err(_) => return Ok(vec![]),
        };
        let mut lists = Vec::new();
        doc.find_all("Address", &mut lists);
        let lists: Vec<(String, String)> = lists.iter().map(|a| (a.text_of("Name"), a.text_of("EmailAddress"))).filter(|(_, e)| e.contains('@')).collect();
        let mut out = Vec::new();
        for (lname, laddr) in lists {
            let Ok(d) = self.call(&format!(r#"<m:GetRooms><m:RoomList><t:EmailAddress>{}</t:EmailAddress></m:RoomList></m:GetRooms>"#, esc(&laddr))) else { continue };
            let mut rooms = Vec::new();
            d.find_all("Id", &mut rooms);
            for r in rooms {
                let email = r.text_of("EmailAddress");
                if email.contains('@') {
                    out.push((lname.clone(), r.text_of("Name"), email));
                }
            }
        }
        Ok(out)
    }

    /// Kişilerin/odaların meşguliyet durumu (Zamanlama Yardımcısı). Her adres için
    /// birleştirilmiş durum dizisi (0 boş, 1 belirsiz, 2 meşgul, 3 ofis dışı, 4 veri yok),
    /// `interval` dakikalık dilimlerle.
    pub fn free_busy(&mut self, emails: &[String], start: f64, end: f64, interval: u32) -> MailResult<Vec<(String, String)>> {
        if emails.is_empty() {
            return Ok(vec![]);
        }
        let mbx: String = emails
            .iter()
            .map(|e| format!("<t:MailboxData><t:Email><t:Address>{}</t:Address></t:Email><t:AttendeeType>Required</t:AttendeeType><t:ExcludeConflicts>false</t:ExcludeConflicts></t:MailboxData>", esc(e)))
            .collect();
        let tz = legacy_tz(start);
        let fmt = |t: f64| chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M:%S").to_string()).unwrap_or_default();
        let doc = self.call(&format!(
            r#"<m:GetUserAvailabilityRequest>{tz}<m:MailboxDataArray>{mbx}</m:MailboxDataArray><t:FreeBusyViewOptions><t:TimeWindow><t:StartTime>{}</t:StartTime><t:EndTime>{}</t:EndTime></t:TimeWindow><t:MergedFreeBusyIntervalInMinutes>{interval}</t:MergedFreeBusyIntervalInMinutes><t:RequestedView>MergedOnly</t:RequestedView></t:FreeBusyViewOptions></m:GetUserAvailabilityRequest>"#,
            fmt(start),
            fmt(end)
        ))?;
        let mut resp = Vec::new();
        doc.find_all("FreeBusyResponse", &mut resp);
        Ok(emails.iter().cloned().zip(resp.iter().map(|r| r.find("MergedFreeBusy").map(|m| m.text.trim().to_string()).unwrap_or_default())).collect())
    }

    /// Başkasının takviminden ayrıntılı meşguliyet (klasör izni yoksa Outlook da bunu kullanır):
    /// her öğe için başlangıç/bitiş ve durum; izin veriliyorsa konu ve konum.
    pub fn availability_events(&mut self, email: &str, start: f64, end: f64) -> MailResult<Vec<Event>> {
        let tz = legacy_tz(start);
        let fmt = |t: f64| chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M:%S").to_string()).unwrap_or_default();
        let doc = self.call(&format!(
            r#"<m:GetUserAvailabilityRequest>{tz}<m:MailboxDataArray><t:MailboxData><t:Email><t:Address>{}</t:Address></t:Email><t:AttendeeType>Required</t:AttendeeType><t:ExcludeConflicts>false</t:ExcludeConflicts></t:MailboxData></m:MailboxDataArray><t:FreeBusyViewOptions><t:TimeWindow><t:StartTime>{}</t:StartTime><t:EndTime>{}</t:EndTime></t:TimeWindow><t:MergedFreeBusyIntervalInMinutes>30</t:MergedFreeBusyIntervalInMinutes><t:RequestedView>Detailed</t:RequestedView></t:FreeBusyViewOptions></m:GetUserAvailabilityRequest>"#,
            esc(email),
            fmt(start),
            fmt(end)
        ))?;
        if let Some(rc) = doc.find("ResponseMessage").filter(|m| m.attr("ResponseClass") == Some("Error")) {
            return Err(MailError(format!("Exchange: {}", rc.text_of("MessageText"))));
        }
        Ok(availability_events_of(&doc))
    }

    /// Posta kutusu doluluğu (Outlook durum çubuğundaki "Posta kutusu: %"): kullanılan bayt ve
    /// kota (bayt; sunucu bildirmiyorsa 0). Kota sırasıyla gönderme engeli, alma engeli ve
    /// depolama sınırı özelliklerinden alınır (değerler KB).
    pub fn mailbox_usage(&mut self) -> MailResult<(u64, u64)> {
        let doc = self.call(r#"<m:GetFolder><m:FolderShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties><t:ExtendedFieldURI PropertyTag="0x0E08" PropertyType="Long"/><t:ExtendedFieldURI PropertyTag="0x666E" PropertyType="Integer"/><t:ExtendedFieldURI PropertyTag="0x666A" PropertyType="Integer"/><t:ExtendedFieldURI PropertyTag="0x3FF5" PropertyType="Integer"/></t:AdditionalProperties></m:FolderShape><m:FolderIds><t:DistinguishedFolderId Id="root"/></m:FolderIds></m:GetFolder>"#)?;
        let mut props = Vec::new();
        doc.find_all("ExtendedProperty", &mut props);
        let val = |tag: &str| -> Option<u64> {
            props
                .iter()
                // Sunucu etiketi "0xe08" gibi farklı yazabilir: sayı olarak karşılaştırılır
                .find(|p| p.child("ExtendedFieldURI").and_then(|u| u.attr("PropertyTag")).and_then(|t| u32::from_str_radix(t.trim_start_matches("0x"), 16).ok()) == u32::from_str_radix(&tag[2..], 16).ok())
                .and_then(|p| p.text_of("Value").trim().parse::<i64>().ok())
                .filter(|v| *v > 0)
                .map(|v| v as u64)
        };
        let used = val("0x0E08").unwrap_or(0);
        let quota = val("0x666E").or_else(|| val("0x666A")).or_else(|| val("0x3FF5")).map(|kb| kb * 1024).unwrap_or(0);
        Ok((used, quota))
    }

    /// Zamanlama Yardımcısı (Outlook gibi ayrıntılı): her kişi için birleşik meşguliyet
    /// dizisi ve izin veriliyorsa öğelerin konu/konum bilgisi. Döner: (adres, birleşik, öğeler)
    pub fn free_busy_detailed(&mut self, emails: &[String], start: f64, end: f64, interval: u32) -> MailResult<Vec<(String, String, Vec<Event>)>> {
        if emails.is_empty() {
            return Ok(vec![]);
        }
        let mbx: String = emails
            .iter()
            .map(|e| format!("<t:MailboxData><t:Email><t:Address>{}</t:Address></t:Email><t:AttendeeType>Required</t:AttendeeType><t:ExcludeConflicts>false</t:ExcludeConflicts></t:MailboxData>", esc(e)))
            .collect();
        let tz = legacy_tz(start);
        let fmt = |t: f64| chrono::DateTime::from_timestamp(t as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M:%S").to_string()).unwrap_or_default();
        let doc = self.call(&format!(
            r#"<m:GetUserAvailabilityRequest>{tz}<m:MailboxDataArray>{mbx}</m:MailboxDataArray><t:FreeBusyViewOptions><t:TimeWindow><t:StartTime>{}</t:StartTime><t:EndTime>{}</t:EndTime></t:TimeWindow><t:MergedFreeBusyIntervalInMinutes>{interval}</t:MergedFreeBusyIntervalInMinutes><t:RequestedView>DetailedMerged</t:RequestedView></t:FreeBusyViewOptions></m:GetUserAvailabilityRequest>"#,
            fmt(start),
            fmt(end)
        ))?;
        let mut resp = Vec::new();
        doc.find_all("FreeBusyResponse", &mut resp);
        Ok(emails
            .iter()
            .cloned()
            .zip(resp.iter().map(|r| (r.find("MergedFreeBusy").map(|m| m.text.trim().to_string()).unwrap_or_default(), availability_events_of(r))))
            .map(|(e, (m, ev))| (e, m, ev))
            .collect())
    }

    /// Kuruluş bilgisi (Outlook "Ekip" takvim grubu için): yönetici ve doğrudan bağlılar.
    /// Döner: (görünen ad, yönetici (ad, adres), doğrudan bağlılar [(ad, adres)])
    pub fn org_info(&mut self, email: &str) -> MailResult<(String, Option<(String, String)>, Vec<(String, String)>)> {
        let doc = self.call(&format!(
            r#"<m:ResolveNames ReturnFullContactData="true" SearchScope="ActiveDirectory" ContactDataShape="AllProperties"><m:UnresolvedEntry>smtp:{}</m:UnresolvedEntry></m:ResolveNames>"#,
            esc(email)
        ))?;
        let Some(r) = doc.find("Resolution") else { return Ok((String::new(), None, vec![])) };
        let name = r.child("Contact").map(|c| c.text_of("DisplayName")).filter(|n| !n.is_empty()).or_else(|| r.child("Mailbox").map(|m| m.text_of("Name"))).unwrap_or_default();
        let boxes = |el: Option<&Element>| -> Vec<(String, String)> {
            let mut list = Vec::new();
            if let Some(e) = el {
                let mut mbs = Vec::new();
                e.find_all("Mailbox", &mut mbs);
                for m in mbs {
                    list.push((m.text_of("Name"), m.text_of("EmailAddress")));
                }
            }
            list
        };
        let ct = r.child("Contact");
        let manager = boxes(ct.and_then(|c| c.child("ManagerMailbox"))).into_iter().next();
        let reports = boxes(ct.and_then(|c| c.child("DirectReports")));
        // Adres X500 (EX) biçimindeyse SMTP adresine çevrilir
        let mut smtp = |(n, a): (String, String)| -> Option<(String, String)> {
            if a.contains('@') {
                return Some((n, a));
            }
            let q = if a.is_empty() { n.clone() } else { a.clone() };
            let r = self.resolve_names_typed(&q).ok()?;
            let (rn, ra, _) = r.into_iter().next()?;
            Some((if n.is_empty() { rn } else { n }, ra))
        };
        let manager = manager.and_then(&mut smtp);
        let reports = reports.into_iter().filter_map(&mut smtp).collect();
        Ok((name, manager, reports))
    }

    // ------------------------------------------------------------------ takvim
    /// Hesabın (paylaşılan kutuda o kutunun) takvimi, ayrıntısız
    pub fn calendar_summary(&mut self, start: f64, end: f64) -> MailResult<Vec<Event>> {
        let mailbox = self.mailbox.clone();
        self.calendar_summary_of(mailbox.as_deref(), start, end)
    }

    /// Takvim görünümü; `mailbox` verilirse başkasının (ekip, oda, paylaşılan) takvimi
    pub fn calendar_view_of(&mut self, mailbox: Option<&str>, start: f64, end: f64) -> MailResult<Vec<Event>> {
        let mut events = self.calendar_summary_of(mailbox, start, end)?;
        self.calendar_details(&mut events);
        Ok(events)
    }

    /// Takvim görünümü, ayrıntısız (açıklama ve katılımcılar yok). Exchange tek istekte en çok
    /// `EWSFindCountLimit` (varsayılan 1000) öğe döndürür; fazlasında hata verir ya da listeyi
    /// keser. Toplantısı yoğun takvimlerde (tekrarlayan toplantılar) aralık bu yüzden parçalar
    /// halinde istenir, dolan parça ikiye bölünür.
    pub fn calendar_summary_of(&mut self, mailbox: Option<&str>, start: f64, end: f64) -> MailResult<Vec<Event>> {
        let folder = match mailbox {
            Some(m) => format!(r#"<t:DistinguishedFolderId Id="calendar"><t:Mailbox><t:EmailAddress>{}</t:EmailAddress></t:Mailbox></t:DistinguishedFolderId>"#, esc(m)),
            None => r#"<t:DistinguishedFolderId Id="calendar"/>"#.to_string(),
        };
        let all = fetch_split(start, end, CAL_CHUNK_SECS, &mut |s, e| self.calendar_chunk(&folder, s, e))?;
        // Parça sınırına denk gelen toplantı iki parçada da gelir
        let mut seen = std::collections::HashSet::new();
        Ok(all.into_iter().filter(|e| seen.insert(e.remote_id.clone())).collect())
    }

    /// Aralığın tek isteği: (olaylar, tamamı geldi mi)
    fn calendar_chunk(&mut self, folder: &str, start: f64, end: f64) -> MailResult<(Vec<Event>, bool)> {
        let doc = match self.call(&format!(r#"<m:FindItem Traversal="Shallow"><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties><t:FieldURI FieldURI="item:Subject"/><t:FieldURI FieldURI="calendar:Start"/><t:FieldURI FieldURI="calendar:End"/><t:FieldURI FieldURI="calendar:Location"/><t:FieldURI FieldURI="calendar:IsAllDayEvent"/><t:FieldURI FieldURI="item:ReminderIsSet"/><t:FieldURI FieldURI="item:ReminderMinutesBeforeStart"/><t:FieldURI FieldURI="calendar:LegacyFreeBusyStatus"/><t:FieldURI FieldURI="calendar:IsRecurring"/><t:FieldURI FieldURI="item:Categories"/><t:FieldURI FieldURI="item:Sensitivity"/><t:FieldURI FieldURI="calendar:UID"/><t:FieldURI FieldURI="calendar:Organizer"/><t:FieldURI FieldURI="calendar:IsCancelled"/><t:FieldURI FieldURI="calendar:IsMeeting"/><t:FieldURI FieldURI="calendar:MyResponseType"/></t:AdditionalProperties></m:ItemShape><m:CalendarView MaxEntriesReturned="{CAL_MAX}" StartDate="{}" EndDate="{}"/><m:ParentFolderIds>{folder}</m:ParentFolderIds></m:FindItem>"#, ts_to_ews(start), ts_to_ews(end))) {
            Ok(d) => d,
            // Sınır aşıldı: parça bölünür
            Err(e) if e.message().contains("ErrorExceededFindCountLimit") || e.message().contains("ErrorCalendarViewRangeTooBig") => return Ok((vec![], false)),
            Err(e) => return Err(e),
        };
        let mut items = Vec::new();
        doc.find_all("CalendarItem", &mut items);
        let complete = items.len() < CAL_MAX && doc.find("RootFolder").and_then(|r| r.attr("IncludesLastItemInRange")) != Some("false");
        Ok((items.iter().filter_map(|it| calendar_item_to_event(it)).collect(), complete))
    }

    /// Açıklama ve katılımcılar (50'şer öğelik isteklerle)
    pub fn calendar_details(&mut self, events: &mut [Event]) {
        let ids: Vec<String> = events.iter().filter_map(|e| e.remote_id.clone()).collect();
        for chunk in ids.chunks(50) {
            let Ok(doc) = self.call(&format!(r#"<m:GetItem><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:BodyType>Text</t:BodyType><t:AdditionalProperties><t:FieldURI FieldURI="item:Body"/><t:FieldURI FieldURI="calendar:RequiredAttendees"/><t:FieldURI FieldURI="calendar:OptionalAttendees"/><t:FieldURI FieldURI="calendar:Resources"/></t:AdditionalProperties></m:ItemShape><m:ItemIds>{}</m:ItemIds></m:GetItem>"#, item_ids(chunk))) else { continue };
            let mut det = Vec::new();
            doc.find_all("CalendarItem", &mut det);
            for d in det {
                let Some(id) = d.child("ItemId").and_then(|i| i.attr("Id")) else { continue };
                if let Some(e) = events.iter_mut().find(|e| e.remote_id.as_deref() == Some(id)) {
                    e.description = d.text_of("Body").trim().to_string();
                    let list = |name: &str| {
                        let mut addrs = Vec::new();
                        if let Some(c) = d.child(name) {
                            c.find_all("EmailAddress", &mut addrs);
                        }
                        addrs.iter().map(|a| a.text.trim().to_string()).filter(|a| a.contains('@')).collect::<Vec<_>>().join(",")
                    };
                    e.attendees = list("RequiredAttendees");
                    e.optional_attendees = list("OptionalAttendees");
                    e.resources = list("Resources");
                }
            }
        }
    }

    fn event_xml(e: &Event, include_recurrence: bool) -> String {
        let mut x = format!("<t:Subject>{}</t:Subject>", esc(&e.title));
        if e.is_private {
            x.push_str("<t:Sensitivity>Private</t:Sensitivity>");
        }
        x.push_str(&format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, esc(&e.description)));
        if !e.category.is_empty() {
            x.push_str(&format!("<t:Categories><t:String>{}</t:String></t:Categories>", esc(&e.category)));
        }
        if e.reminder_minutes >= 0 {
            x.push_str(&format!("<t:ReminderIsSet>true</t:ReminderIsSet><t:ReminderMinutesBeforeStart>{}</t:ReminderMinutesBeforeStart>", e.reminder_minutes));
        } else {
            x.push_str("<t:ReminderIsSet>false</t:ReminderIsSet>");
        }
        x.push_str(&format!("<t:Start>{}</t:Start><t:End>{}</t:End>", ts_to_ews(e.start), ts_to_ews(e.end)));
        x.push_str(&format!("<t:IsAllDayEvent>{}</t:IsAllDayEvent>", e.all_day));
        let fb = match e.show_as.as_str() {
            "free" => "Free",
            "tentative" => "Tentative",
            "oof" => "OOF",
            "elsewhere" => "WorkingElsewhere",
            _ => "Busy",
        };
        x.push_str(&format!("<t:LegacyFreeBusyStatus>{fb}</t:LegacyFreeBusyStatus>"));
        x.push_str(&format!("<t:Location>{}</t:Location>", esc(&e.location)));
        x.push_str(&Self::attendees_xml(e));
        if include_recurrence && e.recurrence != "none" {
            x.push_str(&recurrence_xml(e));
        }
        x
    }

    /// Zorunlu, isteğe bağlı katılımcılar ve odalar (şema sırasıyla)
    fn attendees_xml(e: &Event) -> String {
        let mut x = String::new();
        for (tag, list) in [("RequiredAttendees", &e.attendees), ("OptionalAttendees", &e.optional_attendees), ("Resources", &e.resources)] {
            let att = crate::db::split_list(list);
            if att.is_empty() {
                continue;
            }
            x.push_str(&format!("<t:{tag}>"));
            for a in att {
                x.push_str(&format!("<t:Attendee><t:Mailbox><t:EmailAddress>{}</t:EmailAddress></t:Mailbox></t:Attendee>", esc(&a)));
            }
            x.push_str(&format!("</t:{tag}>"));
        }
        x
    }

    fn has_invitees(e: &Event) -> bool {
        !(e.attendees.trim().is_empty() && e.optional_attendees.trim().is_empty() && e.resources.trim().is_empty())
    }

    pub fn create_event(&mut self, e: &Event) -> MailResult<String> {
        let send = if !Self::has_invitees(e) { "SendToNone" } else { "SendToAllAndSaveCopy" };
        let calendar = self.folder("calendar");
        let doc = self.call(&format!(r#"<m:CreateItem SendMeetingInvitations="{send}"><m:SavedItemFolderId>{calendar}</m:SavedItemFolderId><m:Items><t:CalendarItem>{}</t:CalendarItem></m:Items></m:CreateItem>"#, Self::event_xml(e, true)))?;
        Ok(doc.find("ItemId").and_then(|i| i.attr("Id")).unwrap_or("").to_string())
    }

    pub fn update_event(&mut self, id: &str, e: &Event) -> MailResult<()> {
        let send = if !Self::has_invitees(e) { "SendToNone" } else { "SendToChangedAndSaveCopy" };
        let fields: Vec<(&str, String)> = vec![
            ("item:Subject", format!("<t:Subject>{}</t:Subject>", esc(&e.title))),
            ("item:Body", format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, esc(&e.description))),
            ("calendar:Start", format!("<t:Start>{}</t:Start>", ts_to_ews(e.start))),
            ("calendar:End", format!("<t:End>{}</t:End>", ts_to_ews(e.end))),
            ("calendar:IsAllDayEvent", format!("<t:IsAllDayEvent>{}</t:IsAllDayEvent>", e.all_day)),
            ("calendar:Location", format!("<t:Location>{}</t:Location>", esc(&e.location))),
            ("item:ReminderIsSet", format!("<t:ReminderIsSet>{}</t:ReminderIsSet>", e.reminder_minutes >= 0)),
        ];
        let mut updates: String = fields
            .iter()
            .map(|(f, v)| format!(r#"<t:SetItemField><t:FieldURI FieldURI="{f}"/><t:CalendarItem>{v}</t:CalendarItem></t:SetItemField>"#))
            .collect();
        // Katılımcılar / odalar: boşsa alan silinir
        for (uri, tag, list) in [
            ("calendar:RequiredAttendees", "RequiredAttendees", &e.attendees),
            ("calendar:OptionalAttendees", "OptionalAttendees", &e.optional_attendees),
            ("calendar:Resources", "Resources", &e.resources),
        ] {
            let att = crate::db::split_list(list);
            if att.is_empty() {
                updates.push_str(&format!(r#"<t:DeleteItemField><t:FieldURI FieldURI="{uri}"/></t:DeleteItemField>"#));
            } else {
                let inner: String = att.iter().map(|a| format!("<t:Attendee><t:Mailbox><t:EmailAddress>{}</t:EmailAddress></t:Mailbox></t:Attendee>", esc(a))).collect();
                updates.push_str(&format!(r#"<t:SetItemField><t:FieldURI FieldURI="{uri}"/><t:CalendarItem><t:{tag}>{inner}</t:{tag}></t:CalendarItem></t:SetItemField>"#));
            }
        }
        let fb = match e.show_as.as_str() {
            "free" => "Free",
            "tentative" => "Tentative",
            "oof" => "OOF",
            "elsewhere" => "WorkingElsewhere",
            _ => "Busy",
        };
        updates.push_str(&format!(r#"<t:SetItemField><t:FieldURI FieldURI="calendar:LegacyFreeBusyStatus"/><t:CalendarItem><t:LegacyFreeBusyStatus>{fb}</t:LegacyFreeBusyStatus></t:CalendarItem></t:SetItemField>"#));
        if e.reminder_minutes >= 0 {
            updates.push_str(&format!(r#"<t:SetItemField><t:FieldURI FieldURI="item:ReminderMinutesBeforeStart"/><t:CalendarItem><t:ReminderMinutesBeforeStart>{}</t:ReminderMinutesBeforeStart></t:CalendarItem></t:SetItemField>"#, e.reminder_minutes));
        }
        self.call(&format!(r#"<m:UpdateItem ConflictResolution="AlwaysOverwrite" SendMeetingInvitationsOrCancellations="{send}"><m:ItemChanges><t:ItemChange><t:ItemId Id="{}"/><t:Updates>{updates}</t:Updates></t:ItemChange></m:ItemChanges></m:UpdateItem>"#, esc(id)))?;
        Ok(())
    }

    /// `series`: yinelenen serinin tamamını sil
    pub fn delete_event(&mut self, occurrence_id: &str, series: bool, notify: bool) -> MailResult<()> {
        let target = if series {
            format!(r#"<t:RecurringMasterItemId OccurrenceId="{}"/>"#, esc(occurrence_id))
        } else {
            format!(r#"<t:ItemId Id="{}"/>"#, esc(occurrence_id))
        };
        let send = if notify { "SendToAllAndSaveCopy" } else { "SendToNone" };
        self.call(&format!(r#"<m:DeleteItem DeleteType="MoveToDeletedItems" SendMeetingCancellations="{send}"><m:ItemIds>{target}</m:ItemIds></m:DeleteItem>"#))?;
        Ok(())
    }

    /// Toplantı daveti yanıtı: "accept", "tentative", "decline"
    /// `send` = false: Outlook "Yanıt Gönderme" — takvim güncellenir, düzenleyiciye ileti gitmez
    /// (yanıt taslak olarak oluşur ve hemen silinir)
    pub fn respond(&mut self, item_id: &str, kind: &str, comment: &str, send: bool) -> MailResult<()> {
        let tag = match kind {
            "accept" => "AcceptItem",
            "tentative" => "TentativelyAcceptItem",
            _ => "DeclineItem",
        };
        let body = if comment.is_empty() { String::new() } else { format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, esc(comment)) };
        let disp = if send { "SendAndSaveCopy" } else { "SaveOnly" };
        let doc = self.call(&format!(r#"<m:CreateItem MessageDisposition="{disp}"><m:Items><t:{tag}>{body}<t:ReferenceItemId Id="{}"/></t:{tag}></m:Items></m:CreateItem>"#, esc(item_id)))?;
        if !send {
            let mut ids = Vec::new();
            doc.find_all("ItemId", &mut ids);
            for id in ids.iter().filter_map(|i| i.attr("Id")) {
                let _ = self.call(&format!(r#"<m:DeleteItem DeleteType="HardDelete"><m:ItemIds><t:ItemId Id="{}"/></m:ItemIds></m:DeleteItem>"#, esc(id)));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ kişiler
    pub fn contacts(&mut self) -> MailResult<Vec<Contact>> {
        let folder = self.folder("contacts");
        let doc = self.call(&format!(r#"<m:FindItem Traversal="Shallow"><m:ItemShape><t:BaseShape>AllProperties</t:BaseShape></m:ItemShape><m:IndexedPageItemView MaxEntriesReturned="5000" Offset="0" BasePoint="Beginning"/><m:ParentFolderIds>{folder}</m:ParentFolderIds></m:FindItem>"#))?;
        let mut items = Vec::new();
        doc.find_all("Contact", &mut items);
        Ok(items
            .iter()
            .map(|c| {
                let entry = |parent: &str, key: &str| -> String {
                    c.child(parent)
                        .map(|p| p.children.iter().find(|e| e.attr("Key") == Some(key)).map(|e| e.text.trim().to_string()).unwrap_or_default())
                        .unwrap_or_default()
                };
                Contact {
                    first_name: c.text_of("GivenName"),
                    last_name: c.text_of("Surname"),
                    display_name: c.text_of("DisplayName"),
                    email: entry("EmailAddresses", "EmailAddress1").trim_start_matches("SMTP:").to_string(),
                    email2: entry("EmailAddresses", "EmailAddress2").trim_start_matches("SMTP:").to_string(),
                    phone_mobile: entry("PhoneNumbers", "MobilePhone"),
                    phone_work: entry("PhoneNumbers", "BusinessPhone"),
                    phone_home: entry("PhoneNumbers", "HomePhone"),
                    company: c.text_of("CompanyName"),
                    job_title: c.text_of("JobTitle"),
                    department: c.text_of("Department"),
                    remote_id: c.child("ItemId").and_then(|i| i.attr("Id")).map(String::from),
                    ..Default::default()
                }
            })
            .collect())
    }

    // ------------------------------------------------------------------ görevler, notlar, gruplar
    /// Klasördeki öğelerin tam hali. FindItem gövde ve üye listesi döndürmediği için kimlikler
    /// alınıp GetItem ile parça parça okunur. `kinds`: okunacak öğe türleri.
    fn read_folder(&mut self, folder: &str, kinds: &[&str]) -> MailResult<Vec<Element>> {
        let parent = self.folder(folder);
        let doc = self.call(&format!(r#"<m:FindItem Traversal="Shallow"><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape></m:ItemShape><m:IndexedPageItemView MaxEntriesReturned="5000" Offset="0" BasePoint="Beginning"/><m:ParentFolderIds>{parent}</m:ParentFolderIds></m:FindItem>"#))?;
        let mut items = Vec::new();
        for k in kinds {
            doc.find_all(k, &mut items);
        }
        let ids: Vec<String> = items.iter().filter_map(|i| remote_id(i)).collect();
        let mut docs = Vec::new();
        for part in ids.chunks(100) {
            docs.push(self.call(&format!(r#"<m:GetItem><m:ItemShape><t:BaseShape>AllProperties</t:BaseShape><t:BodyType>Text</t:BodyType></m:ItemShape><m:ItemIds>{}</m:ItemIds></m:GetItem>"#, item_ids(part)))?);
        }
        Ok(docs)
    }

    fn create_item(&mut self, folder: &str, item: &str) -> MailResult<String> {
        let parent = self.folder(folder);
        let doc = self.call(&format!(r#"<m:CreateItem MessageDisposition="SaveOnly"><m:SavedItemFolderId>{parent}</m:SavedItemFolderId><m:Items>{item}</m:Items></m:CreateItem>"#))?;
        doc.find("ItemId").and_then(|i| i.attr("Id")).map(String::from).ok_or_else(|| MailError("Exchange: ItemId".into()))
    }

    fn update_item(&mut self, id: &str, updates: &str) -> MailResult<()> {
        self.call(&format!(r#"<m:UpdateItem MessageDisposition="SaveOnly" ConflictResolution="AlwaysOverwrite"><m:ItemChanges><t:ItemChange><t:ItemId Id="{}"/><t:Updates>{updates}</t:Updates></t:ItemChange></m:ItemChanges></m:UpdateItem>"#, esc(id)))?;
        Ok(())
    }

    pub fn tasks(&mut self) -> MailResult<Vec<Task>> {
        let docs = self.read_folder("tasks", &["Task"])?;
        let mut out = Vec::new();
        for doc in &docs {
            let mut items = Vec::new();
            doc.find_all("Task", &mut items);
            out.extend(items.into_iter().map(task_from_xml));
        }
        Ok(out)
    }

    pub fn create_task(&mut self, t: &Task) -> MailResult<String> {
        self.create_item("tasks", &format!("<t:Task>{}</t:Task>", task_fields(t)))
    }

    pub fn update_task(&mut self, id: &str, t: &Task) -> MailResult<()> {
        let set = |uri: &str, value: String| format!(r#"<t:SetItemField><t:FieldURI FieldURI="{uri}"/><t:Task>{value}</t:Task></t:SetItemField>"#);
        let clear = |uri: &str| format!(r#"<t:DeleteItemField><t:FieldURI FieldURI="{uri}"/></t:DeleteItemField>"#);
        let date = |uri: &str, tag: &str, v: Option<f64>| match v {
            Some(d) => set(uri, format!("<t:{tag}>{}</t:{tag}>", ts_to_ews(d))),
            None => clear(uri),
        };
        let mut u = set("item:Subject", format!("<t:Subject>{}</t:Subject>", esc(&t.title)));
        u += &set("item:Body", format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, esc(&t.notes)));
        u += &set("item:Importance", format!("<t:Importance>{}</t:Importance>", task_importance(t)));
        u += &if t.category.is_empty() { clear("item:Categories") } else { set("item:Categories", format!("<t:Categories><t:String>{}</t:String></t:Categories>", esc(&t.category))) };
        u += &date("task:DueDate", "DueDate", t.due_date);
        u += &date("task:StartDate", "StartDate", t.start_date);
        u += &set("task:PercentComplete", format!("<t:PercentComplete>{}</t:PercentComplete>", task_percent(t)));
        u += &set("task:Status", format!("<t:Status>{}</t:Status>", task_status_out(&t.status)));
        self.update_item(id, &u)
    }

    pub fn notes(&mut self) -> MailResult<Vec<Note>> {
        // Yapışkan notlar (IPM.StickyNote) ileti olarak döner
        let docs = self.read_folder("notes", &["Message", "Item"])?;
        let mut out = Vec::new();
        for doc in &docs {
            let mut items = Vec::new();
            doc.find_all("Message", &mut items);
            doc.find_all("Item", &mut items);
            out.extend(items.into_iter().map(note_from_xml));
        }
        Ok(out)
    }

    pub fn create_note(&mut self, body: &str) -> MailResult<String> {
        self.create_item(
            "notes",
            &format!(r#"<t:Message><t:ItemClass>IPM.StickyNote</t:ItemClass><t:Subject>{}</t:Subject><t:Body BodyType="Text">{}</t:Body></t:Message>"#, esc(&note_subject(body)), esc(body)),
        )
    }

    pub fn update_note(&mut self, id: &str, body: &str) -> MailResult<()> {
        let u = format!(
            r#"<t:SetItemField><t:FieldURI FieldURI="item:Subject"/><t:Message><t:Subject>{}</t:Subject></t:Message></t:SetItemField><t:SetItemField><t:FieldURI FieldURI="item:Body"/><t:Message><t:Body BodyType="Text">{}</t:Body></t:Message></t:SetItemField>"#,
            esc(&note_subject(body)),
            esc(body)
        );
        self.update_item(id, &u)
    }

    pub fn contact_groups(&mut self) -> MailResult<Vec<ContactGroup>> {
        let docs = self.read_folder("contacts", &["DistributionList"])?;
        let mut out = Vec::new();
        for doc in &docs {
            let mut items = Vec::new();
            doc.find_all("DistributionList", &mut items);
            out.extend(items.into_iter().map(group_from_xml));
        }
        Ok(out)
    }

    pub fn create_group(&mut self, g: &ContactGroup) -> MailResult<String> {
        self.create_item(
            "contacts",
            &format!("<t:DistributionList><t:DisplayName>{}</t:DisplayName>{}</t:DistributionList>", esc(&g.name), members_xml(&g.members)),
        )
    }

    pub fn update_group(&mut self, id: &str, g: &ContactGroup) -> MailResult<()> {
        let u = format!(
            r#"<t:SetItemField><t:FieldURI FieldURI="contacts:DisplayName"/><t:DistributionList><t:DisplayName>{}</t:DisplayName></t:DistributionList></t:SetItemField><t:SetItemField><t:FieldURI FieldURI="distributionlist:Members"/><t:DistributionList>{}</t:DistributionList></t:SetItemField>"#,
            esc(&g.name),
            members_xml(&g.members)
        );
        self.update_item(id, &u)
    }

    // ------------------------------------------------------------------ ofis dışı
    pub fn set_oof(&mut self, email: &str, enabled: bool, text: &str) -> MailResult<()> {
        let state = if enabled { "Enabled" } else { "Disabled" };
        let msg = esc(text);
        self.call(&format!(r#"<m:SetUserOofSettingsRequest><t:Mailbox><t:Address>{}</t:Address></t:Mailbox><t:UserOofSettings><t:OofState>{state}</t:OofState><t:ExternalAudience>All</t:ExternalAudience><t:InternalReply><t:Message>{msg}</t:Message></t:InternalReply><t:ExternalReply><t:Message>{msg}</t:Message></t:ExternalReply></t:UserOofSettings></m:SetUserOofSettingsRequest>"#, esc(email)))?;
        Ok(())
    }

    /// Kullanıcının (AD/Exchange) fotoğrafı; yoksa None.
    pub fn get_user_photo(&mut self, email: &str) -> MailResult<Option<Vec<u8>>> {
        // Önce REST uç noktası (Outlook da bunu kullanır; şirket içi Exchange 2013+ ve
        // Exchange Online'da çalışır, SOAP'tan hızlıdır)
        if let Some(base) = self.url.to_lowercase().rfind("/exchange.asmx").map(|i| self.url[..i].to_string()) {
            let url = format!("{base}/s/GetUserPhoto?email={}&size=HR96x96", super::http::url_encode(email));
            match self.http.request("GET", &url, "", &[]) {
                Ok(r) if r.status == 200 && r.body.len() > 64 && !r.body.starts_with(b"<") => return Ok(Some(r.body)),
                Ok(r) if r.status == 401 && self.uses_password => return Err(MailError::auth(crate::t!("error.ews_auth"))),
                // 404: fotoğraf yok
                Ok(r) if r.status == 404 => return Ok(None),
                _ => {}
            }
        }
        match self.call(&format!(r#"<m:GetUserPhoto><m:Email>{}</m:Email><m:SizeRequested>HR96x96</m:SizeRequested></m:GetUserPhoto>"#, esc(email))) {
            Ok(doc) => {
                let data = doc.find("PictureData").map(|p| p.text.split_whitespace().collect::<String>()).unwrap_or_default();
                if data.is_empty() {
                    return Ok(None);
                }
                Ok(base64::engine::general_purpose::STANDARD.decode(data).ok())
            }
            Err(e) if e.is_auth() => Err(e),
            // Fotoğraf yok / kullanıcı bulunamadı / sunucu desteklemiyor
            Err(_) => Ok(None),
        }
    }

    // ------------------------------------------------------------------ sunucu kuralları
    pub fn get_rules(&mut self, mailbox: &str) -> MailResult<Vec<ServerRule>> {
        let doc = self.call(&format!(r#"<m:GetInboxRules><m:MailboxSmtpAddress>{}</m:MailboxSmtpAddress></m:GetInboxRules>"#, esc(mailbox)))?;
        let mut list = Vec::new();
        doc.find_all("Rule", &mut list);
        Ok(list.into_iter().filter_map(ServerRule::from_element).collect())
    }

    /// Kural işlemleri (oluştur/değiştir/sil). `remove_outlook_blob`: Outlook masaüstünde
    /// oluşturulmuş istemci kuralları varsa Exchange bunu ister (o kurallar silinir).
    pub fn update_rules(&mut self, mailbox: &str, operations: &str, remove_outlook_blob: bool) -> MailResult<()> {
        self.call(&format!(
            r#"<m:UpdateInboxRules><m:MailboxSmtpAddress>{}</m:MailboxSmtpAddress><m:RemoveOutlookRuleBlob>{remove_outlook_blob}</m:RemoveOutlookRuleBlob><m:Operations>{operations}</m:Operations></m:UpdateInboxRules>"#,
            esc(mailbox)
        ))?;
        Ok(())
    }

    /// Bağlantıyı sınar: gelen kutusu erişilebilir mi?
    pub fn test(&mut self) -> MailResult<usize> {
        Ok(self.list_folders()?.len())
    }
}

fn remote_id(item: &Element) -> Option<String> {
    item.child("ItemId").and_then(|i| i.attr("Id")).map(String::from)
}

fn task_status_out(status: &str) -> &'static str {
    match status {
        "in_progress" => "InProgress",
        "completed" => "Completed",
        "waiting" => "WaitingOnOthers",
        "deferred" => "Deferred",
        _ => "NotStarted",
    }
}

fn task_importance(t: &Task) -> &'static str {
    match t.priority {
        2 => "High",
        0 => "Low",
        _ => "Normal",
    }
}

/// Exchange yüzde ile durumu birlikte denetler: uyumsuz çift reddedilir
fn task_percent(t: &Task) -> i64 {
    match t.status.as_str() {
        "completed" => 100,
        "not_started" => 0,
        _ => t.percent.clamp(0, 99),
    }
}

/// Alanlar EWS şemasındaki sırayla yazılır
fn task_fields(t: &Task) -> String {
    let mut x = format!(r#"<t:Subject>{}</t:Subject><t:Body BodyType="Text">{}</t:Body>"#, esc(&t.title), esc(&t.notes));
    if !t.category.is_empty() {
        x += &format!("<t:Categories><t:String>{}</t:String></t:Categories>", esc(&t.category));
    }
    x += &format!("<t:Importance>{}</t:Importance>", task_importance(t));
    if let Some(r) = t.reminder {
        x += &format!("<t:ReminderDueBy>{}</t:ReminderDueBy><t:ReminderIsSet>true</t:ReminderIsSet>", ts_to_ews(r));
    }
    if let Some(d) = t.due_date {
        x += &format!("<t:DueDate>{}</t:DueDate>", ts_to_ews(d));
    }
    x += &format!("<t:PercentComplete>{}</t:PercentComplete>", task_percent(t));
    if let Some(d) = t.start_date {
        x += &format!("<t:StartDate>{}</t:StartDate>", ts_to_ews(d));
    }
    x + &format!("<t:Status>{}</t:Status>", task_status_out(&t.status))
}

fn task_from_xml(it: &Element) -> Task {
    let ts = |name: &str| ews_to_ts(&it.text_of(name));
    let status = match it.text_of("Status").as_str() {
        "InProgress" => "in_progress",
        "Completed" => "completed",
        "WaitingOnOthers" => "waiting",
        "Deferred" => "deferred",
        _ => "not_started",
    };
    Task {
        title: it.text_of("Subject"),
        notes: it.text_of("Body").replace("\r\n", "\n").trim_end().to_string(),
        start_date: ts("StartDate"),
        due_date: ts("DueDate"),
        reminder: if it.text_of("ReminderIsSet") == "true" { ts("ReminderDueBy") } else { None },
        priority: match it.text_of("Importance").as_str() {
            "High" => 2,
            "Low" => 0,
            _ => 1,
        },
        status: status.to_string(),
        percent: it.text_of("PercentComplete").trim().parse::<f64>().unwrap_or(0.0) as i64,
        category: it.child("Categories").and_then(|c| c.children.first()).map(|c| c.text.trim().to_string()).unwrap_or_default(),
        completed_at: ts("CompleteDate"),
        remote_id: remote_id(it),
        ..Default::default()
    }
}

/// Notun konusu: ilk satırın kopyası (Outlook'taki not listesi bunu gösterir)
fn note_subject(body: &str) -> String {
    body.lines().next().unwrap_or("").chars().take(255).collect()
}

fn note_from_xml(it: &Element) -> Note {
    let body = it.text_of("Body");
    let body = if body.trim().is_empty() { it.text_of("Subject") } else { body };
    Note {
        body: body.replace("\r\n", "\n").trim_end().to_string(),
        modified: ews_to_ts(&it.text_of("LastModifiedTime")).unwrap_or(0.0),
        remote_id: remote_id(it),
        ..Default::default()
    }
}

fn members_xml(members: &str) -> String {
    let list: String = super::split_addresses(members)
        .iter()
        .map(|a| {
            let (name, email) = super::split_name_addr(a);
            let name = if name.is_empty() { email.clone() } else { name };
            format!(
                "<t:Member><t:Mailbox><t:Name>{}</t:Name><t:EmailAddress>{}</t:EmailAddress><t:RoutingType>SMTP</t:RoutingType><t:MailboxType>OneOff</t:MailboxType></t:Mailbox></t:Member>",
                esc(&name),
                esc(&email)
            )
        })
        .collect();
    format!("<t:Members>{list}</t:Members>")
}

fn group_from_xml(it: &Element) -> ContactGroup {
    let mut boxes = Vec::new();
    it.find_all("Mailbox", &mut boxes);
    let members: Vec<String> = boxes
        .iter()
        .filter_map(|b| {
            let email = b.text_of("EmailAddress");
            let name = b.text_of("Name");
            if email.trim().is_empty() {
                None
            } else if name.is_empty() || name == email {
                Some(email)
            } else {
                Some(format!("{name} <{email}>"))
            }
        })
        .collect();
    ContactGroup { name: it.text_of("DisplayName"), members: members.join(", "), remote_id: remote_id(it), ..Default::default() }
}

fn collect_response_messages<'a>(e: &'a Element, out: &mut Vec<&'a Element>) {
    for c in &e.children {
        if c.name.ends_with("ResponseMessage") || c.attr("ResponseClass").is_some() {
            out.push(c);
        } else {
            collect_response_messages(c, out);
        }
    }
}

fn calendar_item_to_event(it: &Element) -> Option<Event> {
    let id = it.child("ItemId")?.attr("Id")?.to_string();
    let change_key = it.child("ItemId").and_then(|i| i.attr("ChangeKey")).unwrap_or("").to_string();
    let start = ews_to_ts(&it.text_of("Start"))?;
    let end = ews_to_ts(&it.text_of("End")).unwrap_or(start + 1800.0);
    let reminder = if it.text_of("ReminderIsSet") == "true" { it.text_of("ReminderMinutesBeforeStart").parse().unwrap_or(15) } else { -1 };
    Some(Event {
        uid: it.text_of("UID"),
        title: it.text_of("Subject"),
        location: it.text_of("Location"),
        start,
        end,
        all_day: it.text_of("IsAllDayEvent") == "true",
        reminder_minutes: reminder,
        show_as: match it.text_of("LegacyFreeBusyStatus").as_str() {
            "Free" => "free",
            "Tentative" => "tentative",
            "OOF" => "oof",
            "WorkingElsewhere" => "elsewhere",
            _ => "busy",
        }
        .into(),
        category: it.child("Categories").and_then(|c| c.children.first()).map(|s| s.text.clone()).unwrap_or_default(),
        is_private: it.text_of("Sensitivity") == "Private",
        organizer: it
            .child("Organizer")
            .and_then(|o| o.child("Mailbox"))
            .map(|m| {
                let (n, e) = (m.text_of("Name"), m.text_of("EmailAddress"));
                if e.contains('@') && !n.is_empty() && n != e { format!("{} <{e}>", n.replace(['"', ',', ';'], " ").trim()) } else if e.contains('@') { e } else { n }
            })
            .unwrap_or_default(),
        is_cancelled: it.text_of("IsCancelled") == "true",
        is_meeting: it.text_of("IsMeeting") == "true",
        response: match it.text_of("MyResponseType").as_str() {
            "Organizer" => "organizer",
            "Accept" => "accept",
            "Tentative" => "tentative",
            "Decline" => "decline",
            "NoResponseReceived" | "Unknown" => "none",
            _ => "",
        }
        .into(),
        master_id: if it.text_of("IsRecurring") == "true" { Some(id.clone()) } else { None },
        remote_id: Some(id),
        change_key,
        ..Event::new_default()
    })
}

fn recurrence_xml(e: &Event) -> String {
    let start = chrono::DateTime::from_timestamp(e.start as i64, 0).map(|d| d.with_timezone(&chrono::Local)).unwrap();
    use chrono::Datelike;
    let days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let interval = e.recur_interval.max(1);
    let pattern = match e.recurrence.as_str() {
        "daily" => format!("<t:DailyRecurrence><t:Interval>{interval}</t:Interval></t:DailyRecurrence>"),
        "weekdays" => "<t:WeeklyRecurrence><t:Interval>1</t:Interval><t:DaysOfWeek>Monday Tuesday Wednesday Thursday Friday</t:DaysOfWeek></t:WeeklyRecurrence>".into(),
        "weekly" => format!("<t:WeeklyRecurrence><t:Interval>{interval}</t:Interval><t:DaysOfWeek>{}</t:DaysOfWeek></t:WeeklyRecurrence>", days[start.weekday().num_days_from_monday() as usize]),
        "monthly" => format!("<t:AbsoluteMonthlyRecurrence><t:Interval>{interval}</t:Interval><t:DayOfMonth>{}</t:DayOfMonth></t:AbsoluteMonthlyRecurrence>", start.day()),
        "yearly" => format!("<t:AbsoluteYearlyRecurrence><t:DayOfMonth>{}</t:DayOfMonth><t:Month>{}</t:Month></t:AbsoluteYearlyRecurrence>", start.day(), months[start.month0() as usize]),
        _ => return String::new(),
    };
    let sd = start.format("%Y-%m-%d");
    let range = match e.recur_until {
        Some(u) => format!(
            "<t:EndDateRecurrence><t:StartDate>{sd}</t:StartDate><t:EndDate>{}</t:EndDate></t:EndDateRecurrence>",
            chrono::DateTime::from_timestamp(u as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()).unwrap_or_default()
        ),
        None => format!("<t:NoEndRecurrence><t:StartDate>{sd}</t:StartDate></t:NoEndRecurrence>"),
    };
    format!("<t:Recurrence>{pattern}{range}</t:Recurrence>")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yoğun takvim (460 günde saat başı toplantı): Exchange 500'den fazlasını keser, 1000'den
    /// fazlasında hata verir. Parçalı istekle hepsi, sırayla ve bir kez gelir.
    #[test]
    fn dense_calendar_is_fetched_in_chunks() {
        let (start, end) = (0.0, 460.0 * 86400.0);
        let all: Vec<f64> = (0..(460 * 24)).map(|h| h as f64 * 3600.0).collect();
        let mut calls = 0;
        let got = fetch_split(start, end, CAL_CHUNK_SECS, &mut |s, e| {
            calls += 1;
            let hit: Vec<f64> = all.iter().copied().filter(|t| *t >= s && *t < e).collect();
            if hit.len() > 1000 {
                return Ok((vec![], false)); // ErrorExceededFindCountLimit
            }
            let complete = hit.len() < CAL_MAX;
            Ok((hit.into_iter().take(CAL_MAX).collect(), complete))
        })
        .unwrap();
        assert_eq!(got, all);
        assert!(calls < 80, "{calls} istek");
        // Seyrek takvimde parça başına tek istek
        let mut calls = 0;
        let got = fetch_split(start, end, CAL_CHUNK_SECS, &mut |_, _| {
            calls += 1;
            Ok((vec![1], true))
        })
        .unwrap();
        assert_eq!((got.len(), calls), (15, 15));
        // Bölünemeyecek kadar dar aralıkta gelen kabul edilir (sonsuz döngü olmaz)
        let got = fetch_split(0.0, 1800.0, CAL_CHUNK_SECS, &mut |_, _| Ok((vec![7], false))).unwrap();
        assert_eq!(got, vec![7]);
        // Ağ hatası olduğu gibi döner
        assert!(fetch_split::<u8>(start, end, CAL_CHUNK_SECS, &mut |_, _| Err(MailError::net("yok"))).unwrap_err().is_net());
    }

    /// Konuşma kimliği ve yanıt başlıkları genişletilmiş özelliklerden okunur
    #[test]
    fn header_reads_conversation_properties() {
        let prop = |tag: &str, ty: &str, v: &str| format!(r#"<ExtendedProperty><ExtendedFieldURI PropertyTag="{tag}" PropertyType="{ty}"/><Value>{v}</Value></ExtendedProperty>"#);
        let parse = |props: &str| {
            let xml = format!(r#"<Message><ItemId Id="AAMk1"/><Subject>YNT: Plan</Subject>{props}</Message>"#);
            EwsHeader::from_element(xml::parse(xml.as_bytes()).unwrap().find("Message").unwrap()).unwrap()
        };
        let index: Vec<u8> = (0u8..27).collect();
        let b64 = base64::engine::general_purpose::STANDARD;
        let h = parse(&(prop("0x3013", "Binary", "q83vEjRWeJAAAAAAAAAAAA==") + &prop("0x71", "Binary", &b64.encode(&index)) + &prop("0x1042", "String", "&lt;a@x&gt;") + &prop("0x1039", "String", "&lt;r@x&gt; &lt;a@x&gt;")));
        assert_eq!((h.conversation.as_str(), h.in_reply_to.as_str(), h.references.as_str()), ("q83vEjRWeJAAAAAAAAAAAA==", "<a@x>", "<r@x> <a@x>"));
        // Kimlik yoksa dizindeki GUID
        let h = parse(&prop("0x0071", "Binary", &b64.encode(&index)));
        assert_eq!(h.conversation, b64.encode(&index[6..22]));
        assert!(!h.pinned);
        assert_eq!(parse("").conversation, "");
    }

    #[test]
    fn calendar_item_keeps_change_key() {
        let xml = r#"<CalendarItem><ItemId Id="AAMkA" ChangeKey="DwAAAB"/><Subject>Yönetim kurulu</Subject><Start>2026-11-06T07:00:00Z</Start><End>2026-11-06T08:00:00Z</End><IsRecurring>true</IsRecurring></CalendarItem>"#;
        let doc = xml::parse(xml.as_bytes()).unwrap();
        let e = calendar_item_to_event(doc.find("CalendarItem").unwrap()).unwrap();
        assert_eq!((e.remote_id.as_deref(), e.change_key.as_str(), e.master_id.as_deref()), (Some("AAMkA"), "DwAAAB", Some("AAMkA")));
        assert_eq!((e.start, e.end), (1793948400.0, 1793952000.0));
    }

    #[test]
    fn task_xml_roundtrip() {
        let xml = r#"<Task><ItemId Id="abc"/><Subject>Rapor</Subject><Body>Satır 1&#13;
Satır 2</Body><Categories><String>İş</String></Categories><Importance>High</Importance><ReminderIsSet>false</ReminderIsSet><DueDate>2026-03-15T09:00:00Z</DueDate><PercentComplete>50</PercentComplete><Status>InProgress</Status></Task>"#;
        let doc = xml::parse(xml.as_bytes()).unwrap();
        let t = task_from_xml(doc.find("Task").unwrap());
        assert_eq!(t.title, "Rapor");
        assert_eq!(t.notes, "Satır 1\nSatır 2");
        assert_eq!((t.priority, t.percent, t.status.as_str()), (2, 50, "in_progress"));
        assert_eq!(t.category, "İş");
        assert_eq!(t.remote_id.as_deref(), Some("abc"));
        assert!(t.due_date.is_some() && t.reminder.is_none());
        let out = task_fields(&t);
        assert!(out.find("<t:Categories>").unwrap() < out.find("<t:Importance>").unwrap());
        assert!(out.find("<t:DueDate>").unwrap() < out.find("<t:PercentComplete>").unwrap());
        assert!(out.contains("<t:Status>InProgress</t:Status>") && out.contains("<t:PercentComplete>50</t:PercentComplete>"));
    }

    #[test]
    fn task_percent_follows_status() {
        let mut t = Task { status: "completed".into(), percent: 10, ..Default::default() };
        assert_eq!(task_percent(&t), 100);
        t.status = "not_started".into();
        assert_eq!(task_percent(&t), 0);
        t.status = "in_progress".into();
        t.percent = 100;
        assert_eq!(task_percent(&t), 99);
    }

    #[test]
    fn group_members_roundtrip() {
        let x = members_xml("Ali Veli <ali@x.com>, veli@y.com");
        assert!(x.contains("<t:Name>Ali Veli</t:Name><t:EmailAddress>ali@x.com</t:EmailAddress>"));
        assert!(x.contains("<t:Name>veli@y.com</t:Name><t:EmailAddress>veli@y.com</t:EmailAddress>"));
        let xml = r#"<DistributionList><ItemId Id="g1"/><DisplayName>Ekip</DisplayName><Members><Member><Mailbox><Name>Ali Veli</Name><EmailAddress>ali@x.com</EmailAddress></Mailbox></Member><Member><Mailbox><Name>veli@y.com</Name><EmailAddress>veli@y.com</EmailAddress></Mailbox></Member></Members></DistributionList>"#;
        let doc = xml::parse(xml.as_bytes()).unwrap();
        let g = group_from_xml(doc.find("DistributionList").unwrap());
        assert_eq!(g.name, "Ekip");
        assert_eq!(g.members, "Ali Veli <ali@x.com>, veli@y.com");
    }

    #[test]
    fn note_uses_first_line_as_subject() {
        assert_eq!(note_subject("Alışveriş\nsüt\nekmek"), "Alışveriş");
        let doc = xml::parse(br#"<Message><ItemId Id="n1"/><Subject>Konu</Subject><Body> </Body></Message>"#).unwrap();
        let n = note_from_xml(doc.find("Message").unwrap());
        assert_eq!(n.body, "Konu");
    }

    #[test]
    fn shared_mailbox_routes_distinguished_folders() {
        let mut e = Ews::with_auth("https://x/ews", Auth::Negotiate, false);
        assert_eq!(e.folder("inbox"), r#"<t:DistinguishedFolderId Id="inbox"/>"#);
        e.mailbox = Some("ortak@x.com".into());
        assert!(e.folder("inbox").contains("<t:EmailAddress>ortak@x.com</t:EmailAddress>"));
        assert_eq!(e.folder("AAMk="), r#"<t:FolderId Id="AAMk="/>"#);
    }

    #[test]
    fn calendar_parse_and_recurrence() {
        let x = br#"<CalendarItem><ItemId Id="AQ==" ChangeKey="x"/><Subject>Haftalik</Subject><Start>2026-10-13T07:00:00Z</Start><End>2026-10-13T08:00:00Z</End><IsAllDayEvent>false</IsAllDayEvent><ReminderIsSet>true</ReminderIsSet><ReminderMinutesBeforeStart>10</ReminderMinutesBeforeStart><LegacyFreeBusyStatus>Tentative</LegacyFreeBusyStatus><IsRecurring>true</IsRecurring><Location>Oda 1</Location></CalendarItem>"#;
        let doc = xml::parse(x).unwrap();
        let e = calendar_item_to_event(doc.find("CalendarItem").unwrap()).unwrap();
        assert_eq!(e.title, "Haftalik");
        assert_eq!(e.reminder_minutes, 10);
        assert_eq!(e.show_as, "tentative");
        assert_eq!(e.master_id.as_deref(), Some("AQ=="));
        assert_eq!(e.end - e.start, 3600.0);
        let r = recurrence_xml(&Event { recurrence: "weekly".into(), recur_interval: 2, ..e.clone() });
        assert!(r.contains("<t:Interval>2</t:Interval>") && r.contains("Tuesday") && r.contains("NoEndRecurrence"));
    }
}

/// Exchange sunucu kuralı (Lynora'nın basit kural modeline eşlenmiş hali)
#[derive(Debug, Clone, Default)]
pub struct ServerRule {
    pub id: String,
    pub name: String,
    pub priority: i64,
    pub enabled: bool,
    pub field: String,
    pub op: String,
    pub value: String,
    pub action: String,
    /// Taşıma için sunucu klasör kimliği, kategori adı ya da iletme adresi
    pub action_arg: String,
    pub stop: bool,
    /// Tek koşul / tek eylem modeline sığmıyor (yalnızca aç-kapat ve silme)
    pub complex: bool,
    /// Koşullar, istisnalar ve eylemler (değiştirmeden geri yazmak için)
    pub body_xml: String,
}

impl ServerRule {
    fn from_element(r: &Element) -> Option<ServerRule> {
        let id = r.text_of("RuleId");
        if id.is_empty() {
            return None;
        }
        let mut sr = ServerRule {
            id,
            name: r.text_of("DisplayName"),
            priority: r.text_of("Priority").parse().unwrap_or(1),
            enabled: r.text_of("IsEnabled") == "true",
            op: "contains".into(),
            ..Default::default()
        };
        let strings = |e: &Element| e.children.iter().map(|c| if c.children.is_empty() { c.text.clone() } else { c.find("EmailAddress").map(|x| x.text.clone()).unwrap_or_default() }).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        let conds: Vec<&Element> = r.child("Conditions").map(|c| c.children.iter().collect()).unwrap_or_default();
        let exceptions = r.child("Exceptions").map(|e| !e.children.is_empty()).unwrap_or(false);
        let mut complex = conds.len() != 1 || exceptions;
        if let Some(c) = conds.first() {
            let vals = strings(c);
            complex |= vals.len() != 1;
            sr.value = vals.join(", ");
            sr.field = match c.name.as_str() {
                "ContainsSenderStrings" => "from",
                "FromAddresses" => {
                    sr.op = "equals".into();
                    "from"
                }
                "ContainsSubjectStrings" => "subject",
                "ContainsBodyStrings" => "body",
                "ContainsSubjectOrBodyStrings" => "any",
                "SentToAddresses" => {
                    sr.op = "equals".into();
                    "to"
                }
                "ContainsRecipientStrings" => "to",
                _ => {
                    complex = true;
                    "any"
                }
            }
            .into();
        }
        let actions: Vec<&Element> = r.child("Actions").map(|a| a.children.iter().collect()).unwrap_or_default();
        let mut main = Vec::new();
        for a in &actions {
            match a.name.as_str() {
                "StopProcessingRules" => sr.stop = a.text == "true",
                _ => main.push(*a),
            }
        }
        complex |= main.len() != 1;
        if let Some(a) = main.first() {
            sr.action = match a.name.as_str() {
                "MoveToFolder" => {
                    sr.action_arg = a.find("FolderId").or(a.find("DistinguishedFolderId")).and_then(|f| f.attr("Id")).unwrap_or("").to_string();
                    "move"
                }
                "Delete" => "delete",
                "MarkAsRead" => "mark_read",
                "AssignCategories" => {
                    sr.action_arg = strings(a).join(",");
                    "category"
                }
                "MarkImportance" => "important",
                "ForwardToRecipients" | "RedirectToRecipients" => {
                    sr.action_arg = strings(a).join(", ");
                    "forward"
                }
                _ => {
                    complex = true;
                    "notify"
                }
            }
            .into();
        }
        sr.complex = complex;
        sr.body_xml = ["Conditions", "Exceptions", "Actions"].iter().filter_map(|n| r.child(n)).map(|e| e.to_xml("t")).collect();
        Some(sr)
    }

    /// Basit modelden koşul ve eylem XML'i
    pub fn build_body(field: &str, op: &str, value: &str, action: &str, action_arg: &str, stop: bool) -> Result<String, String> {
        let strings = |vals: &[&str]| vals.iter().map(|v| format!("<t:String>{}</t:String>", esc(v.trim()))).collect::<String>();
        let addrs = |v: &str| v.split([',', ';']).filter(|x| !x.trim().is_empty()).map(|x| format!("<t:Address><t:EmailAddress>{}</t:EmailAddress></t:Address>", esc(x.trim()))).collect::<String>();
        let cond = match (field, op) {
            ("from", "equals") => format!("<t:FromAddresses>{}</t:FromAddresses>", addrs(value)),
            ("from", _) => format!("<t:ContainsSenderStrings>{}</t:ContainsSenderStrings>", strings(&[value])),
            ("to", "equals") => format!("<t:SentToAddresses>{}</t:SentToAddresses>", addrs(value)),
            ("to", _) => format!("<t:ContainsRecipientStrings>{}</t:ContainsRecipientStrings>", strings(&[value])),
            ("subject", _) => format!("<t:ContainsSubjectStrings>{}</t:ContainsSubjectStrings>", strings(&[value])),
            ("body", _) => format!("<t:ContainsBodyStrings>{}</t:ContainsBodyStrings>", strings(&[value])),
            _ => format!("<t:ContainsSubjectOrBodyStrings>{}</t:ContainsSubjectOrBodyStrings>", strings(&[value])),
        };
        let act = match action {
            "move" | "junk" => format!("<t:MoveToFolder>{}</t:MoveToFolder>", folder_ref(if action == "junk" { "junkemail" } else { action_arg })),
            "delete" => "<t:Delete>true</t:Delete>".into(),
            "mark_read" => "<t:MarkAsRead>true</t:MarkAsRead>".into(),
            "category" => format!("<t:AssignCategories>{}</t:AssignCategories>", strings(&action_arg.split(',').collect::<Vec<_>>())),
            "important" => "<t:MarkImportance>High</t:MarkImportance>".into(),
            "forward" => format!("<t:ForwardToRecipients>{}</t:ForwardToRecipients>", addrs(action_arg)),
            _ => return Err(action.to_string()),
        };
        let stop = if stop { "<t:StopProcessingRules>true</t:StopProcessingRules>" } else { "" };
        // "İçermiyorsa": koşulsuz kural + istisna
        if op == "not_contains" {
            return Ok(format!("<t:Conditions/><t:Exceptions>{cond}</t:Exceptions><t:Actions>{act}{stop}</t:Actions>"));
        }
        Ok(format!("<t:Conditions>{cond}</t:Conditions><t:Actions>{act}{stop}</t:Actions>"))
    }
}

/// GetUserAvailability saat dilimi (eski biçim). Yaz saati olmayan dilimde (Türkiye) geçişler
/// Month/DayOrder = 0 ile "yok" bildirilir (Microsoft EWS kitaplığı gibi); iki geçişin aynı
/// tarih verilmesi Exchange'de "The specified time zone isn't valid" hatası veriyordu.
fn legacy_tz(at: f64) -> String {
    let off = chrono::DateTime::from_timestamp(at as i64, 0)
        .map(|d| d.with_timezone(&chrono::Local).offset().local_minus_utc())
        .unwrap_or_else(|| chrono::Local::now().offset().local_minus_utc())
        / 60;
    let none = "<t:Bias>0</t:Bias><t:Time>00:00:00</t:Time><t:DayOrder>0</t:DayOrder><t:Month>0</t:Month><t:DayOfWeek>Sunday</t:DayOfWeek>";
    format!("<t:TimeZone><t:Bias>{}</t:Bias><t:StandardTime>{none}</t:StandardTime><t:DaylightTime>{none}</t:DaylightTime></t:TimeZone>", -off)
}

/// GetUserAvailability yanıtındaki CalendarEvent öğeleri
fn availability_events_of(doc: &Element) -> Vec<Event> {
    let parse_local = |s: &str| {
        chrono::NaiveDateTime::parse_from_str(s.trim().get(..19).unwrap_or(""), "%Y-%m-%dT%H:%M:%S")
            .ok()
            .and_then(|n| n.and_local_timezone(chrono::Local).single())
            .map(|d| d.timestamp() as f64)
    };
    let mut evs = Vec::new();
    doc.find_all("CalendarEvent", &mut evs);
    evs
        .iter()
        .filter_map(|c| {
            let s = parse_local(&c.text_of("StartTime"))?;
            let e = parse_local(&c.text_of("EndTime")).unwrap_or(s + 1800.0);
            let det = c.child("CalendarEventDetails");
            let show_as = match c.text_of("BusyType").as_str() {
                "Free" => "free",
                "Tentative" => "tentative",
                "OOF" => "oof",
                "WorkingElsewhere" => "elsewhere",
                "NoData" => return None,
                _ => "busy",
            };
            Some(Event {
                title: det.map(|d| d.text_of("Subject")).unwrap_or_default(),
                location: det.map(|d| d.text_of("Location")).unwrap_or_default(),
                start: s,
                end: e,
                all_day: e - s >= 86400.0 && (s as i64) % 3600 == 0,
                show_as: show_as.into(),
                is_private: det.map(|d| d.text_of("IsPrivate") == "true").unwrap_or(false),
                is_meeting: det.map(|d| d.text_of("IsMeeting") == "true").unwrap_or(false),
                master_id: det.filter(|d| d.text_of("IsRecurring") == "true").map(|_| "r".to_string()),
                reminder_minutes: -1,
                ..Event::new_default()
            })
        })
    .collect()
}
