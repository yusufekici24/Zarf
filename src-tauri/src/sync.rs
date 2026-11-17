//! Posta hizmeti: yerel veritabanı ile IMAP/SMTP sunucuları arasında eşitleme.
//!
//! Arayüz komutları yerel veritabanını anında günceller; sunucu işlemleri tek
//! bir arka plan iş parçacığında sırayla yürütülür. Sonuçlar olay olarak
//! arayüze bildirilir.

use crate::db::{now, Db, Folder, Message};
use crate::mail::dav::{Dav, Kind};
use crate::mail::imap_client::{flags_contain, Imap};
use crate::mail::{self, parse, MailError, MailResult};
use crate::t;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub type Emitter = Arc<dyn Fn(&str, Value) + Send + Sync>;

type Job = Box<dyn FnOnce(&mut Worker) -> MailResult<()> + Send>;

/// İş kuyruğu: kullanıcının beklediği işler (tıklanan klasör, Gönder/Al) öne
/// alınır; uzun eşitleme işleri küçük parçalar halinde arkada yürür.
pub(crate) struct Queue {
    items: Mutex<VecDeque<(Job, String, Option<String>)>>,
    cv: Condvar,
    pending: AtomicUsize,
}

impl Queue {
    /// Aynı anahtarlı iş zaten bekliyorsa yenisi eklenmez (öne alınması istenirse öne taşınır).
    pub(crate) fn push(&self, key: Option<String>, front: bool, desc: String, job: Job) {
        let mut q = self.items.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(k) = &key {
            if let Some(pos) = q.iter().position(|(_, _, kk)| kk.as_deref() == Some(k.as_str())) {
                if front && pos > 0 {
                    if let Some(item) = q.remove(pos) {
                        q.push_front(item);
                    }
                }
                return;
            }
        }
        self.pending.fetch_add(1, Ordering::SeqCst);
        if front {
            q.push_front((job, desc, key));
        } else {
            q.push_back((job, desc, key));
        }
        self.cv.notify_one();
    }

    fn pop(&self) -> (Job, String, Option<String>) {
        let mut q = self.items.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(item) = q.pop_front() {
                return item;
            }
            q = self.cv.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }
}

pub struct Worker {
    pub db: Db,
    pub(crate) conns: HashMap<i64, Imap>,
    pub(crate) ews: HashMap<i64, crate::mail::ews::Ews>,
    pub(crate) emit: Emitter,
    /// Kullanıcı işleri (gönder, bayrak, açılan klasör, yeni posta denetimi)
    pub(crate) fg: Arc<Queue>,
    /// Uzun arka plan işleri (tam tarama, gövde ön yükleme, takvim, kişiler)
    pub(crate) bg: Arc<Queue>,
    /// Tam tarama sırasında sunucuda görülen öğeler (klasör → kimlikler); iki iş
    /// parçacığı arasında paylaşılır
    pub(crate) enum_seen: Arc<Mutex<HashMap<i64, HashSet<String>>>>,
    /// Aynı klasörün iki iş parçacığında aynı anda eşitlenmemesi için kilitler
    pub(crate) folder_locks: Arc<Mutex<HashMap<i64, Arc<Mutex<()>>>>>,
}

impl Worker {
    /// Arka planda sürecek bir iş ekler (ör. sonraki sayfa).
    pub(crate) fn defer(&self, key: String, job: impl FnOnce(&mut Worker) -> MailResult<()> + Send + 'static) {
        self.bg.push(Some(key), false, String::new(), Box::new(job));
    }

    /// İşi `wait` sonra arka plan kuyruğuna ekler: sunucu meşgulken (istek sınırı) tarama
    /// bırakılmaz, sunucunun istediği süre dolunca kaldığı yerden sürer. Aynı anahtar beklerken
    /// yenisi eklenmez.
    pub(crate) fn defer_after(&self, wait: std::time::Duration, key: String, job: impl FnOnce(&mut Worker) -> MailResult<()> + Send + 'static) {
        static WAITING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
        if !WAITING.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(key.clone()) {
            return;
        }
        let bg = self.bg.clone();
        let _ = std::thread::Builder::new().name("lynora-defer".into()).spawn(move || {
            std::thread::sleep(wait);
            if let Some(w) = WAITING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                w.remove(&key);
            }
            bg.push(Some(key), false, String::new(), Box::new(job));
        });
    }

    /// Kullanıcının beklediği bir işi (ör. Gelen Kutusu) ön kuyruğa ekler.
    pub(crate) fn defer_fg(&self, key: String, job: impl FnOnce(&mut Worker) -> MailResult<()> + Send + 'static) {
        self.fg.push(Some(key), false, String::new(), Box::new(job));
    }

    pub(crate) fn folder_lock(&self, folder_id: i64) -> Arc<Mutex<()>> {
        let mut m = self.folder_locks.lock().unwrap_or_else(|e| e.into_inner());
        m.entry(folder_id).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }
}

#[derive(Clone)]
pub struct MailService {
    pub db: Db,
    queue: Arc<Queue>,
    bg: Arc<Queue>,
    emit: Emitter,
    /// Arayüzde açık olan klasör (yeni posta denetiminde öncelikli)
    active_folder: Arc<AtomicI64>,
    /// Anlık bildirim (EWS akış aboneliği) bağlantısı açık olan hesaplar
    push_live: Arc<Mutex<HashSet<i64>>>,
    /// Anlık bildirim iş parçacığı çalışan hesaplar
    push_threads: Arc<Mutex<HashSet<i64>>>,
}

/// Bir kuyruğu işleyen iş parçacığını başlatır. `lane_status`: yalnızca ön kuyruk
/// durum çubuğuna yazar; arka plan kuyruğu yalnızca "mail://bg" olayı gönderir.
fn spawn_worker(name: &str, mut worker: Worker, queue: Arc<Queue>, foreground: bool) {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || loop {
            let (job, desc, _key) = queue.pop();
            if foreground && !desc.is_empty() {
                (worker.emit)("mail://status", json!(desc));
            }
            if !foreground && queue.pending.load(Ordering::SeqCst) == 1 {
                (worker.emit)("mail://bg", json!(true));
            }
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut worker)));
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) if e.is_net() => {
                    // Kurum ağı dışında / bağlantı yok: Outlook gibi hata penceresi gösterilmez,
                    // durum çubuğunda "Bağlantı kesildi" yazar; bağlantı gelince yeniden denenir
                    if !mail::net::work_offline() {
                        mail::net::set_disconnected(true);
                    }
                    worker.conns.clear();
                    worker.ews.clear();
                }
                Ok(Err(e)) if e.is_busy() => {
                    // Exchange istek sınırı: bağlantılar sağlamdır, süre dolunca kendiliğinden
                    // sürer. Her işte yinelenmesin diye kullanıcıya seyrek bildirilir.
                    static NOTICE: Mutex<Option<std::time::Instant>> = Mutex::new(None);
                    let mut last = NOTICE.lock().unwrap_or_else(|e| e.into_inner());
                    if foreground && last.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(900)) {
                        *last = Some(std::time::Instant::now());
                        (worker.emit)("mail://error", json!(e.message()));
                    } else {
                        eprintln!("lynora: {}", e.message());
                    }
                }
                Ok(Err(e)) => {
                    // Arka plan hataları (ör. bağlantı koptu) kullanıcıyı rahatsız etmez;
                    // bir sonraki denetimde yeniden denenir
                    if foreground {
                        (worker.emit)("mail://error", json!(e.message()));
                    } else {
                        eprintln!("lynora arka plan: {}", e.message());
                    }
                    // Bağlantı hatasında önbellekteki bağlantıları at
                    worker.conns.clear();
                    worker.ews.clear();
                }
                Err(_) => (worker.emit)("mail://error", json!(t!("error.internal"))),
            }
            // Açıklamalı iş bitince durum metni silinir (kuyrukta başka iş olsa bile
            // "eşitleniyor" yazısı takılı kalmaz)
            if foreground && !desc.is_empty() {
                (worker.emit)("mail://status", json!(""));
            }
            if queue.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                for (_, c) in worker.conns.drain() {
                    c.logout();
                }
                if foreground {
                    (worker.emit)("mail://status", json!(""));
                } else {
                    worker.ews.clear();
                    (worker.emit)("mail://bg", json!(false));
                }
            }
        })
        .expect("posta iş parçacığı");
}

fn strip_bcc(raw: &[u8]) -> Vec<u8> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 2)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| p + 1))
        .unwrap_or(raw.len());
    let head = &raw[..end];
    let mut out = Vec::with_capacity(raw.len());
    let mut skipping = false;
    for line in head.split_inclusive(|&b| b == b'\n') {
        let cont = line.first().map(|b| *b == b' ' || *b == b'\t').unwrap_or(false);
        if cont {
            if !skipping {
                out.extend_from_slice(line);
            }
            continue;
        }
        skipping = line.len() >= 4 && line[..4].eq_ignore_ascii_case(b"bcc:");
        if !skipping {
            out.extend_from_slice(line);
        }
    }
    out.extend_from_slice(&raw[end..]);
    out
}

impl MailService {
    pub fn new(db: Db, emit: Emitter) -> Self {
        let mk = || Arc::new(Queue { items: Mutex::new(VecDeque::new()), cv: Condvar::new(), pending: AtomicUsize::new(0) });
        let (queue, bg) = (mk(), mk());
        let enum_seen = Arc::new(Mutex::new(HashMap::new()));
        let folder_locks = Arc::new(Mutex::new(HashMap::new()));
        let worker = |db: &Db| Worker {
            db: db.clone(),
            conns: HashMap::new(),
            ews: HashMap::new(),
            emit: emit.clone(),
            fg: queue.clone(),
            bg: bg.clone(),
            enum_seen: enum_seen.clone(),
            folder_locks: folder_locks.clone(),
        };
        db.reset_stuck_sending();
        spawn_worker("lynora-mail", worker(&db), queue.clone(), true);
        spawn_worker("lynora-mail-bg", worker(&db), bg.clone(), false);
        mail::net::set_work_offline(db.setting("work_offline").as_deref() == Some("1"));
        let svc = MailService {
            db,
            queue,
            bg,
            emit,
            active_folder: Arc::new(AtomicI64::new(0)),
            push_live: Arc::new(Mutex::new(HashSet::new())),
            push_threads: Arc::new(Mutex::new(HashSet::new())),
        };
        svc.spawn_net_monitor();
        svc
    }

    /// Bağlantı durumunu arayüze bildirir; bağlantı kesikken sunucuları yoklar ve
    /// bağlantı gelince eşitlemeyi ve Giden Kutusu'nu kendiliğinden çalıştırır.
    fn spawn_net_monitor(&self) {
        let svc = self.clone();
        std::thread::Builder::new()
            .name("lynora-net".into())
            .spawn(move || {
                let mut last = String::new();
                let mut last_probe = std::time::Instant::now() - std::time::Duration::from_secs(3600);
                loop {
                    let state = if mail::net::work_offline() {
                        "work_offline"
                    } else if mail::net::is_disconnected() {
                        "disconnected"
                    } else {
                        "online"
                    };
                    if state != last {
                        (svc.emit)("net://state", json!(state));
                        last = state.to_string();
                    }
                    if state == "disconnected" && last_probe.elapsed() >= std::time::Duration::from_secs(20) {
                        last_probe = std::time::Instant::now();
                        if svc.probe_servers() {
                            mail::net::set_disconnected(false);
                            (svc.emit)("net://state", json!("online"));
                            last = "online".into();
                            svc.sync_all();
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            })
            .ok();
    }

    /// Hesap sunucularından birine TCP bağlantısı kurulabiliyor mu (kısa zaman aşımı)
    fn probe_servers(&self) -> bool {
        for a in self.db.accounts() {
            let (host, port) = if a.is_exchange() {
                match mail::http::Url::parse(&a.ews_url) {
                    Ok(u) => (u.host, u.port),
                    Err(_) => continue,
                }
            } else {
                (a.imap_host.clone(), a.imap_port as u16)
            };
            if host.is_empty() {
                continue;
            }
            mail::net::forget(&host);
            if mail::tls::tcp_connect(&host, port, std::time::Duration::from_secs(5)).is_ok() {
                return true;
            }
        }
        false
    }

    pub fn busy(&self) -> bool {
        self.queue.pending.load(Ordering::SeqCst) > 0 || self.bg.pending.load(Ordering::SeqCst) > 0
    }

    fn submit(&self, desc: String, job: impl FnOnce(&mut Worker) -> MailResult<()> + Send + 'static) {
        self.queue.push(None, false, desc, Box::new(job));
    }

    pub(crate) fn submit_keyed(&self, key: String, front: bool, desc: String, job: impl FnOnce(&mut Worker) -> MailResult<()> + Send + 'static) {
        self.queue.push(Some(key), front, desc, Box::new(job));
    }

    fn is_ews(&self, account_id: Option<i64>) -> bool {
        account_id.and_then(|a| self.db.account(a)).map(|a| a.is_exchange()).unwrap_or(false)
    }

    fn changed(&self, folder: Option<i64>) {
        (self.emit)("mail://changed", json!({ "folder": folder }));
    }

    // ------------------------------------------------------------------ eşitleme
    pub fn sync_all(&self) {
        for a in self.db.accounts() {
            // Parolası reddedilmiş hesaplar, kullanıcı yeni parolayı girene kadar
            // otomatik olarak denenmez (AD hesap kilitlenmesini önler).
            if a.auth_state == "failed" {
                continue;
            }
            self.sync_account(a.id);
        }
        self.process_outbox();
    }

    pub fn sync_account(&self, account_id: i64) {
        let Some(acc) = self.db.account(account_id) else { return };
        let front = acc.is_exchange();
        self.submit_keyed(format!("acc:{account_id}"), front, t!("status.syncing", acc.name), move |w| w.sync_account(account_id));
        self.sync_dav(account_id);
    }

    /// Takvim ve kişiler arka planda eşitlenir; posta sunucusundan bağımsızdır
    pub fn sync_dav(&self, account_id: i64) {
        if self.db.account(account_id).is_some_and(|a| !a.caldav_url.trim().is_empty() || !a.carddav_url.trim().is_empty()) {
            // Takvim sunucusuna ulaşılamaması postayı "bağlantı kesildi" durumuna düşürmez
            self.bg.push(Some(format!("dav:{account_id}")), false, String::new(), Box::new(move |w: &mut Worker| crate::pim_sync::quiet(w.dav_sync_account(account_id))));
        }
    }

    /// Klasörü hemen eşitler (kullanıcı tıkladığında; kuyruğun önüne alınır).
    pub fn sync_folder(&self, folder_id: i64) {
        self.sync_folder_ex(folder_id, false);
    }

    /// `quiet`: arka plan denetimi (durum çubuğunda "eşitleniyor" gösterilmez)
    fn sync_folder_ex(&self, folder_id: i64, quiet: bool) {
        let Some(f) = self.db.folder(folder_id) else { return };
        let Some(acc) = f.account_id else { return };
        let ews = self.is_ews(Some(acc));
        let desc = if quiet { String::new() } else { t!("status.syncing", f.display_name()) };
        self.submit_keyed(format!("f:{folder_id}"), !quiet, desc, move |w| {
            let f = w.db.folder(folder_id).ok_or_else(|| MailError("folder".into()))?;
            let before = w.db.total_changes();
            let new = if ews { w.ews_sync_folder(acc, &f)? } else { w.sync_folder(acc, &f)? };
            // Arka plan denetiminde hiçbir şey değişmediyse arayüz yenilenmez (her 15 sn'de
            // klasör ağacı ve liste boş yere yeniden yükleniyordu)
            if !quiet || !new.is_empty() || w.db.total_changes() != before {
                w.emit_changed(Some(folder_id));
            }
            w.emit_new(new);
            Ok(())
        });
    }

    /// Arayüzde açılan klasör: kaydedilir ve hemen eşitlenir.
    pub fn set_active_folder(&self, folder_id: i64) {
        self.active_folder.store(folder_id, Ordering::SeqCst);
        self.sync_folder(folder_id);
    }

    /// Yeni posta denetimi (her dakika): Exchange hesaplarında Gelen Kutusu ve açık klasör.
    /// Değişiklik yoksa sunucuya tek küçük istek gider.
    pub fn quick_check(&self) {
        self.quick_check_ex(true);
    }

    /// `all`: anlık bildirimi açık hesaplar da denetlenir (seyrek güvenlik denetimi)
    pub fn quick_check_ex(&self, all: bool) {
        let active = self.active_folder.load(Ordering::SeqCst);
        let live = self.push_live.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for a in self.db.accounts().into_iter().filter(|a| a.is_exchange() && a.auth_state != "failed" && (all || !live.contains(&a.id))) {
            if let Some(inbox) = self.db.folder_by_role(Some(a.id), "inbox") {
                self.sync_folder_ex(inbox.id, true);
            }
        }
        if active > 0 && self.db.folder(active).and_then(|f| f.account_id).map(|a| self.is_ews(Some(a))).unwrap_or(false) {
            self.sync_folder_ex(active, true);
        }
    }

    /// Anlık bildirim iş parçacıklarını başlatır: Exchange akış aboneliği, IMAP IDLE (Outlook
    /// gibi yeni posta sunucuya düştüğü anda gelir; zamanlayıcı yalnızca yedek denetimdir).
    pub fn ensure_push(&self) {
        // Akış aboneliği yalnızca kendi kutumuz için açılabilir: paylaşılan kutular zamanlayıcıyla denetlenir
        for a in self.db.accounts().into_iter().filter(|a| (a.is_exchange() || !a.imap_host.trim().is_empty()) && a.shared_of.is_none() && a.auth_state != "failed") {
            let id = a.id;
            let exchange = a.is_exchange();
            if !self.push_threads.lock().unwrap_or_else(|e| e.into_inner()).insert(id) {
                continue;
            }
            let svc = self.clone();
            let _ = std::thread::Builder::new().name(format!("lynora-push-{id}")).spawn(move || {
                if exchange {
                    svc.push_loop(id);
                } else {
                    svc.imap_push_loop(id);
                }
                svc.push_live.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                svc.push_threads.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            });
        }
    }

    fn push_loop(&self, account_id: i64) {
        let mut backoff = 20u64;
        let mut sub: Option<String> = None;
        loop {
            let Some(acc) = self.db.account(account_id) else { return };
            if !acc.is_exchange() || acc.auth_state == "failed" {
                return;
            }
            let mut ews = match crate::mail::ews::Ews::for_account(&acc) {
                Ok(e) => e,
                Err(_) => return,
            };
            // Abonelik (bağlantıyı da doğrular; akış aynı bağlantıda açılır)
            let sid = match sub.clone() {
                // Var olan abonelikte bağlantıyı doğrulamak için hafif bir istek
                Some(s) => match ews.call(r#"<m:GetFolder><m:FolderShape><t:BaseShape>IdOnly</t:BaseShape></m:FolderShape><m:FolderIds><t:DistinguishedFolderId Id="inbox"/></m:FolderIds></m:GetFolder>"#) {
                    Ok(_) => s,
                    Err(e) => {
                        if e.is_auth() {
                            self.db.set_account_auth(account_id, "failed", e.message());
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(backoff));
                        backoff = (backoff * 2).min(300);
                        continue;
                    }
                },
                None => match ews.subscribe_streaming() {
                    Ok(s) => s,
                    Err(e) => {
                        if e.is_auth() {
                            self.db.set_account_auth(account_id, "failed", e.message());
                            return;
                        }
                        // Sunucu desteklemiyorsa (Exchange 2010 öncesi) zamanlayıcı yeterli
                        if e.message().contains("ErrorInvalidRequest") || e.message().contains("ErrorSchemaValidation") {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(backoff));
                        backoff = (backoff * 2).min(300);
                        continue;
                    }
                },
            };
            sub = Some(sid.clone());
            self.push_live.lock().unwrap_or_else(|e| e.into_inner()).insert(account_id);
            // Bağlantı kopmuşken gelenleri kaçırmamak için bir kez denetle
            if let Some(inbox) = self.db.folder_by_role(Some(account_id), "inbox") {
                self.sync_folder_ex(inbox.id, true);
            }
            let me = self.clone();
            let r = ews.stream_events(&sid, 29, |folders, _new_mail| {
                let local = me.db.folders(Some(account_id));
                let mut any = false;
                for rid in &folders {
                    if let Some(f) = local.iter().find(|f| f.remote_name.as_deref() == Some(rid.as_str())) {
                        me.sync_folder_ex(f.id, true);
                        any = true;
                    }
                }
                // Takvim/kişi klasörleri yerel posta klasörü değildir
                if !any && !folders.is_empty() {
                    let key = format!("cal:{account_id}");
                    me.submit_keyed(key, false, String::new(), move |w| w.ews_sync_calendar(account_id));
                }
                true
            });
            self.push_live.lock().unwrap_or_else(|e| e.into_inner()).remove(&account_id);
            match r {
                Ok(valid) => {
                    backoff = 20;
                    if !valid {
                        sub = None;
                    }
                }
                Err(e) => {
                    if e.is_auth() {
                        self.db.set_account_auth(account_id, "failed", e.message());
                        return;
                    }
                    sub = None;
                    std::thread::sleep(std::time::Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(300);
                }
            }
        }
    }

    /// IMAP IDLE: yeni posta sunucuya düştüğü anda Gelen Kutusu eşitlenir. Eşitleme bağlantısı
    /// kilitlenmesin diye ayrı bağlantı kullanılır; reddedilen parola yeniden denenmez.
    fn imap_push_loop(&self, account_id: i64) {
        let mut backoff = 20u64;
        loop {
            let Some(acc) = self.db.account(account_id) else { return };
            if acc.auth_state == "failed" {
                return;
            }
            let Some(inbox) = self.db.folder_by_role(Some(account_id), "inbox") else {
                std::thread::sleep(std::time::Duration::from_secs(backoff));
                continue;
            };
            let Some(remote) = inbox.remote_name.clone() else { return };
            let started = std::time::Instant::now();
            let r = self.imap_idle(&acc, inbox.id, &remote);
            self.push_live.lock().unwrap_or_else(|e| e.into_inner()).remove(&account_id);
            match r {
                Ok(true) => return,
                // IDLE desteklemeyen sunucuya her turda yeniden bağlanılmaz: zamanlayıcı yeterli
                Ok(false) => {
                    while self.db.account(account_id).is_some() {
                        std::thread::sleep(std::time::Duration::from_secs(3600));
                    }
                    return;
                }
                Err(e) if e.is_auth() => {
                    self.db.set_account_auth(account_id, "failed", e.message());
                    return;
                }
                Err(_) => {
                    if started.elapsed().as_secs() > 300 {
                        backoff = 20;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(300);
                }
            }
        }
    }

    /// Bağlantı kopana kadar Gelen Kutusu'nu izler. Döner: false sunucu IDLE desteklemiyor,
    /// true hesap silindi ya da parolası reddedildi.
    fn imap_idle(&self, acc: &crate::db::Account, inbox: i64, remote: &str) -> MailResult<bool> {
        let mut imap = Imap::connect(acc, &acc.password_plain())?;
        if !imap.has_cap("IDLE") {
            imap.logout();
            return Ok(false);
        }
        imap.select(remote)?;
        self.push_live.lock().unwrap_or_else(|e| e.into_inner()).insert(acc.id);
        // Bağlantı kopmuşken gelenleri kaçırmamak için bir kez denetle
        self.sync_folder_ex(inbox, true);
        loop {
            // Süre kısa tutulur: kopan bağlantı (uyku, VPN) en geç bu kadar sürede anlaşılır
            if imap.idle_wait(std::time::Duration::from_secs(9 * 60))? {
                self.sync_folder_ex(inbox, true);
            }
            if mail::net::work_offline() {
                return Err(MailError::net(t!("net.working_offline")));
            }
            if !self.db.account(acc.id).is_some_and(|a| a.auth_state != "failed") {
                return Ok(true);
            }
        }
    }

    /// Klasörde daha eski iletileri sunucudan getirir (liste sonuna kaydırınca).
    pub fn load_older(&self, folder_id: i64) {
        let Some(f) = self.db.folder(folder_id) else { return };
        let Some(acc) = f.account_id else { return };
        if !self.is_ews(Some(acc)) {
            (self.emit)("mail://older", json!({"folder": folder_id, "more": false}));
            return;
        }
        self.submit_keyed(format!("older:{folder_id}"), true, t!("status.loading_older"), move |w| w.ews_load_older(acc, folder_id));
    }

    // ------------------------------------------------------------------ bayraklar
    pub fn set_read(&self, ids: &[i64], value: bool) {
        self.set_flag(ids, "is_read", value);
    }

    pub fn set_flagged(&self, ids: &[i64], value: bool) {
        self.set_flag(ids, "is_flagged", value);
    }

    /// Sabitle: Exchange'de sunucuya yazılır (Outlook'ta da üstte görünür), IMAP'te yereldir.
    pub fn set_pinned(&self, ids: &[i64], value: bool) {
        let mut ews: HashMap<i64, Vec<String>> = HashMap::new();
        for &id in ids {
            let Some(m) = self.db.message_row(id) else { continue };
            self.db.set_message_field(id, "is_pinned", value as i64);
            if let (Some(rid), Some(acc)) = (m.remote_id.clone(), m.account_id) {
                if self.is_ews(Some(acc)) {
                    ews.entry(acc).or_default().push(rid);
                }
            }
        }
        self.changed(None);
        for (acc, rids) in ews {
            self.submit_keyed(format!("pin:{acc}:{}", rids.join(",")), true, String::new(), move |w| w.with_ews(acc, |e| e.set_pinned(&rids, value)));
        }
    }

    pub fn set_answered(&self, ids: &[i64]) {
        self.set_flag(ids, "is_answered", true);
    }

    /// Yerelde hemen yazılır; sunucuya hesap başına tek işle gider (bkz. `Worker::push_flags`).
    fn set_flag(&self, ids: &[i64], column: &str, value: bool) {
        self.db.set_flags_local(ids, column, value);
        let accounts: HashSet<i64> = ids.iter().filter_map(|&id| self.db.message_row(id)?.account_id).collect();
        for acc in accounts {
            self.submit_keyed(format!("flags:{acc}"), false, String::new(), move |w| w.push_flags(acc));
        }
        self.changed(None);
    }

    pub fn set_categories(&self, ids: &[i64], cats: &[String]) {
        self.db.set_messages_field(ids, "categories", cats.join(","));
        // Exchange kategorileri sunucuda da güncellenir (hesap başına tek istek)
        let mut ews: HashMap<i64, Vec<String>> = HashMap::new();
        for &id in ids {
            if let Some(m) = self.db.message_row(id) {
                if let (Some(rid), Some(acc)) = (m.remote_id, m.account_id) {
                    ews.entry(acc).or_default().push(rid);
                }
            }
        }
        for (acc, rids) in ews {
            let cats = cats.to_vec();
            self.submit(String::new(), move |w| w.with_ews(acc, |e| e.set_categories(&rids, &cats)));
        }
        self.changed(None);
    }

    pub fn set_importance(&self, ids: &[i64], imp: i64) {
        self.db.set_messages_field(ids, "importance", imp);
        self.changed(None);
    }

    // ------------------------------------------------------------------ taşıma / silme
    pub fn move_messages(&self, ids: &[i64], dest_id: i64) {
        let Some(dest) = self.db.folder(dest_id) else { return };
        // Aynı hesap içindeki taşımalar toplanır: ileti başına bir sunucu isteği ve bir hedef
        // klasör eşitlemesi yerine tek istek (EWS'de 100'lük parçalar) ve tek eşitleme
        let mut ews_moves: HashMap<(i64, String), Vec<(i64, String)>> = HashMap::new();
        let mut imap_moves: HashMap<(i64, String, String), Vec<(i64, u32)>> = HashMap::new();
        for &id in ids {
            let Some(m) = self.db.message_row(id) else { continue };
            if m.folder_id == dest_id {
                continue;
            }
            let Some(src) = self.db.folder(m.folder_id) else { continue };
            // ---- Exchange
            let src_ews = m.remote_id.is_some() && self.is_ews(src.account_id);
            let dst_ews = dest.remote_name.is_some() && self.is_ews(dest.account_id);
            if src_ews || dst_ews {
                self.db.set_message_field(id, "folder_id", dest_id);
                self.db.set_message_field(id, "account_id", dest.account_id);
                if src_ews && dst_ews && src.account_id == dest.account_id {
                    let (acc, rid, to) = (src.account_id.unwrap(), m.remote_id.clone().unwrap(), dest.remote_name.clone().unwrap());
                    ews_moves.entry((acc, to)).or_default().push((id, rid));
                } else {
                    self.db.set_message_field(id, "remote_id", None::<String>);
                    if dst_ews || dest.remote_name.is_some() {
                        self.submit(String::new(), move |w| w.append_existing(id, dest_id));
                    }
                    if src_ews {
                        let (acc, rid) = (src.account_id.unwrap(), m.remote_id.clone().unwrap());
                        self.submit(String::new(), move |w| w.with_ews(acc, |e| e.delete_items(&[rid], false)));
                    } else if m.uid.is_some() && src.remote_name.is_some() {
                        let (acc, uid, sname) = (src.account_id.unwrap(), m.uid.unwrap() as u32, src.remote_name.clone().unwrap());
                        self.db.set_message_field(id, "uid", None::<i64>);
                        self.submit(String::new(), move |w| {
                            let imap = w.imap(acc)?;
                            imap.select(&sname)?;
                            imap.delete(&[uid])
                        });
                    }
                }
                continue;
            }
            let remote_src = m.uid.is_some() && src.remote_name.is_some();
            let remote_dst = dest.remote_name.is_some();
            self.db.set_message_field(id, "folder_id", dest_id);
            self.db.set_message_field(id, "account_id", dest.account_id);
            self.db.set_message_field(id, "uid", None::<i64>);
            if remote_src && remote_dst && src.account_id == dest.account_id {
                let (acc, uid, sname, dname) =
                    (src.account_id.unwrap(), m.uid.unwrap() as u32, src.remote_name.clone().unwrap(), dest.remote_name.clone().unwrap());
                imap_moves.entry((acc, sname, dname)).or_default().push((id, uid));
            } else {
                if remote_dst {
                    self.submit(String::new(), move |w| w.append_existing(id, dest_id));
                }
                if remote_src {
                    let (acc, uid, sname) = (src.account_id.unwrap(), m.uid.unwrap() as u32, src.remote_name.clone().unwrap());
                    self.submit(String::new(), move |w| {
                        let imap = w.imap(acc)?;
                        imap.select(&sname)?;
                        imap.delete(&[uid])
                    });
                }
            }
        }
        for ((acc, to), items) in ews_moves {
            self.submit(String::new(), move |w| {
                for part in items.chunks(100) {
                    let rids: Vec<String> = part.iter().map(|(_, r)| r.clone()).collect();
                    w.with_ews(acc, |e| e.move_items(&rids, &to))?;
                    // Yerel kopyalar, hedef klasör eşitlenince sunucudakilerle değişir
                    for (id, _) in part {
                        w.db.delete_message(*id);
                    }
                }
                let f = w.db.folder(dest_id).ok_or_else(|| MailError("folder".into()))?;
                w.ews_sync_folder(acc, &f)?;
                w.emit_changed(Some(dest_id));
                Ok(())
            });
        }
        for ((acc, sname, dname), items) in imap_moves {
            self.submit(String::new(), move |w| {
                let uids: Vec<u32> = items.iter().map(|(_, u)| *u).collect();
                let imap = w.imap(acc)?;
                imap.select(&sname)?;
                imap.move_to(&uids, &dname)?;
                // Yerel kopyalar, hedef klasör eşitlenince sunucudakilerle değişir
                for (id, _) in &items {
                    w.db.delete_message(*id);
                }
                let f = w.db.folder(dest_id).ok_or_else(|| MailError("folder".into()))?;
                w.sync_folder(acc, &f)?;
                w.emit_changed(Some(dest_id));
                Ok(())
            });
        }
        self.changed(None);
    }

    /// Silinmiş Öğeler'e taşır; zaten oradaysa veya `permanent` ise kalıcı siler.
    pub fn delete(&self, ids: &[i64], permanent: bool) {
        let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
        for &id in ids {
            if let Some(m) = self.db.message_row(id) {
                groups.entry(m.folder_id).or_default().push(id);
            }
        }
        for (fid, mids) in groups {
            let Some(f) = self.db.folder(fid) else { continue };
            let trash = self.db.folder_by_role_or_local(f.account_id, "trash");
            let perm = permanent || trash.is_none() || f.role.as_deref() == Some("trash") || f.role.as_deref() == Some("outbox");
            if perm {
                let mut uids = Vec::new();
                let mut rids = Vec::new();
                for id in &mids {
                    if let Some(m) = self.db.message_row(*id) {
                        if let Some(u) = m.uid {
                            uids.push(u as u32);
                        }
                        if let Some(r) = m.remote_id {
                            rids.push(r);
                        }
                    }
                }
                self.db.delete_messages(&mids);
                if let (Some(acc), false) = (f.account_id, rids.is_empty()) {
                    self.submit(String::new(), move |w| {
                        for part in rids.chunks(100) {
                            w.with_ews(acc, |e| e.delete_items(part, true))?;
                        }
                        Ok(())
                    });
                }
                if let (Some(acc), Some(remote), false) = (f.account_id, f.remote_name.clone(), uids.is_empty()) {
                    self.submit(String::new(), move |w| {
                        let imap = w.imap(acc)?;
                        imap.select(&remote)?;
                        imap.delete(&uids)
                    });
                }
            } else {
                self.move_messages(&mids, trash.unwrap().id);
            }
        }
        self.changed(None);
    }

    pub fn empty_folder(&self, folder_id: i64) {
        let ids = self.db.message_ids_in_folder(folder_id, false);
        self.delete(&ids, true);
    }

    pub fn mark_folder_read(&self, folder_id: i64) {
        let ids = self.db.message_ids_in_folder(folder_id, true);
        self.set_read(&ids, true);
    }

    // ------------------------------------------------------------------ klasörler
    pub fn create_folder(&self, account_id: Option<i64>, name: String, parent_id: Option<i64>) {
        match account_id {
            None => {
                let _ = self.db.insert_folder(&Folder { account_id: None, name, parent_id, position: 100, ..Default::default() });
                self.changed(None);
            }
            Some(acc) if self.is_ews(Some(acc)) => {
                let parent = parent_id.and_then(|p| self.db.folder(p)).and_then(|p| p.remote_name).unwrap_or_default();
                self.submit(t!("status.creating_folder", name), move |w| {
                    w.with_ews(acc, |e| e.create_folder(&parent, &name))?;
                    w.ews_sync_folder_list(acc)?;
                    w.emit_changed(None);
                    Ok(())
                });
            }
            Some(acc) => {
                let parent = parent_id.and_then(|p| self.db.folder(p));
                self.submit(t!("status.creating_folder", name), move |w| {
                    let imap = w.imap(acc)?;
                    let delim = imap.list_folders()?.first().map(|f| f.delim.clone()).unwrap_or_else(|| "/".into());
                    let enc = mail::imap_client::utf7_encode(&name);
                    let remote = match parent.and_then(|p| p.remote_name) {
                        Some(pr) => format!("{pr}{delim}{enc}"),
                        None => enc,
                    };
                    imap.create(&remote)?;
                    w.sync_folder_list(acc)?;
                    w.emit_changed(None);
                    Ok(())
                });
            }
        }
    }

    pub fn delete_folder(&self, folder_id: i64) {
        let Some(f) = self.db.folder(folder_id) else { return };
        if f.role.is_some() {
            return;
        }
        if let (Some(acc), Some(remote)) = (f.account_id, f.remote_name.clone()) {
            if self.is_ews(Some(acc)) {
                self.submit(String::new(), move |w| w.with_ews(acc, |e| e.delete_folder(&remote)));
            } else {
                self.submit(String::new(), move |w| w.imap(acc)?.delete_folder(&remote));
            }
        }
        let _ = self.db.delete_folder(folder_id);
        self.changed(None);
    }

    pub fn rename_folder(&self, folder_id: i64, new_name: String) {
        let Some(mut f) = self.db.folder(folder_id) else { return };
        if f.role.is_some() {
            return;
        }
        f.name = new_name.clone();
        let _ = self.db.update_folder(&f);
        if let (Some(acc), Some(old)) = (f.account_id, f.remote_name.clone()) {
            if self.is_ews(Some(acc)) {
                self.submit(String::new(), move |w| w.with_ews(acc, |e| e.rename_folder(&old, &new_name)));
                self.changed(None);
                return;
            }
            self.submit(String::new(), move |w| {
                let imap = w.imap(acc)?;
                let delim = imap
                    .list_folders()?
                    .into_iter()
                    .find(|x| x.remote == old)
                    .map(|x| x.delim)
                    .unwrap_or_else(|| "/".into());
                let prefix = old.rsplit_once(delim.as_str()).map(|(p, _)| format!("{p}{delim}")).unwrap_or_default();
                imap.rename(&old, &format!("{prefix}{}", mail::imap_client::utf7_encode(&new_name)))?;
                w.sync_folder_list(acc)?;
                w.emit_changed(None);
                Ok(())
            });
        }
        self.changed(None);
    }

    // ------------------------------------------------------------------ gönderme
    /// İletiyi Giden Kutusu'na koyar; zamanı geldiyse hemen gönderir.
    pub fn send(&self, account_id: i64, raw: Vec<u8>, send_at: Option<f64>) -> Option<i64> {
        let outbox = self.db.folder_by_role(None, "outbox")?;
        let mut m = parse::parse(&raw).into_message();
        m.folder_id = outbox.id;
        m.is_read = true;
        m.pending = format!("send:{}:{}", send_at.unwrap_or(0.0), account_id);
        let id = self.db.insert_message(&m).ok()?;
        self.changed(Some(outbox.id));
        match send_at.map(|t| t - now()) {
            Some(wait) if wait > 0.0 => self.send_later(id, wait),
            _ => self.submit_keyed(format!("send:{id}"), true, t!("status.sending"), move |w| w.send_outbox_item(id)),
        }
        Some(id)
    }

    /// Gönderimi geri alma süresi (birkaç saniye): ileti süre dolunca tam zamanında gönderilir.
    /// 30 sn'lik genel Giden Kutusu denetimini beklemez; uzun zamanlanmış gönderimler (saatler
    /// sonra) o denetime kalır. Geri alınmış ya da gönderilmiş ileti `send_outbox_item`'da atlanır.
    fn send_later(&self, id: i64, wait: f64) {
        if wait > 600.0 {
            return;
        }
        let svc = self.clone();
        let _ = std::thread::Builder::new().name("lynora-send-later".into()).spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(wait));
            svc.submit_keyed(format!("send:{id}"), true, t!("status.sending"), move |w| w.send_outbox_item(id));
        });
    }

    pub fn process_outbox(&self) {
        let Some(outbox) = self.db.folder_by_role(None, "outbox") else { return };
        let due: Vec<i64> = self
            .db
            .messages(Some(outbox.id), "", &crate::db::MsgFilter::All)
            .into_iter()
            .filter_map(|r| self.db.message(r.id))
            .filter(|m| {
                let p: Vec<&str> = m.pending.split(':').collect();
                p.len() == 3 && p[0] == "send" && p[1].parse::<f64>().unwrap_or(0.0) <= now() && !m.snippet.starts_with("[!]")
            })
            .map(|m| m.id)
            .collect();
        for id in due {
            self.submit_keyed(format!("send:{id}"), true, t!("status.sending"), move |w| w.send_outbox_item(id));
        }
    }

    pub fn retry_outbox_item(&self, id: i64) {
        if let Some(m) = self.db.message(id) {
            let (_, body) = (m.snippet.clone(), m.body_text.clone());
            self.db.set_message_field(id, "snippet", mail::make_snippet(&body, 200));
            self.submit_keyed(format!("send:{id}"), true, t!("status.sending"), move |w| w.send_outbox_item(id));
        }
    }

    // ------------------------------------------------------------------ taslaklar
    pub fn save_draft(&self, account_id: Option<i64>, raw: Vec<u8>, old_id: Option<i64>) -> Option<i64> {
        let drafts = self.db.folder_by_role_or_local(account_id, "drafts")?;
        if let Some(old) = old_id {
            self.discard_draft(old);
        }
        let mut m = parse::parse(&raw).into_message();
        m.folder_id = drafts.id;
        m.account_id = drafts.account_id;
        m.is_read = true;
        m.is_draft = true;
        m.pending = format!("draft:{}", account_id.map(|a| a.to_string()).unwrap_or_default());
        let id = self.db.insert_message(&m).ok()?;
        if let (Some(acc), Some(remote), true) = (drafts.account_id, drafts.remote_name.clone(), self.is_ews(drafts.account_id)) {
            let did = drafts.id;
            self.submit(String::new(), move |w| {
                let raw = w.db.message(id).map(|m| m.raw).unwrap_or_default();
                let rid = w.with_ews(acc, |e| e.save_mime(&remote, &raw, true, true))?;
                w.db.set_message_field(id, "remote_id", rid);
                w.emit_changed(Some(did));
                Ok(())
            });
        } else if let (Some(acc), Some(remote)) = (drafts.account_id, drafts.remote_name.clone()) {
            self.submit(String::new(), move |w| {
                let raw = w.db.message(id).map(|m| m.raw).unwrap_or_default();
                if raw.is_empty() {
                    return Ok(());
                }
                w.imap(acc)?.append(&remote, &raw, true, true)?;
                // Sunucudaki kopyayı UID ile eşleştirmek için klasörü eşitle,
                // yerel taslağı kaldır
                w.db.delete_message(id);
                let f = w.db.folder_by_role(Some(acc), "drafts").ok_or_else(|| MailError("drafts".into()))?;
                w.sync_folder(acc, &f)?;
                w.emit_changed(Some(f.id));
                Ok(())
            });
        }
        self.changed(Some(drafts.id));
        Some(id)
    }

    pub fn discard_draft(&self, id: i64) {
        let Some(m) = self.db.message_row(id) else { return };
        self.db.delete_message(id);
        if let (Some(rid), Some(acc)) = (m.remote_id.clone(), m.account_id) {
            self.submit(String::new(), move |w| w.with_ews(acc, |e| e.delete_items(&[rid], true)));
        }
        if let (Some(uid), Some(acc)) = (m.uid, m.account_id) {
            if let Some(remote) = self.db.folder(m.folder_id).and_then(|f| f.remote_name) {
                self.submit(String::new(), move |w| {
                    let imap = w.imap(acc)?;
                    imap.select(&remote)?;
                    imap.delete(&[uid as u32])
                });
            }
        }
        self.changed(None);
    }

    // ------------------------------------------------------------------ Exchange takvim / toplantı
    /// Exchange takvimindeki etkinliği sunucuya yazar (oluştur/güncelle) ve yeniden eşitler.
    pub fn push_event(&self, account_id: i64, event_id: i64) {
        if !self.is_ews(Some(account_id)) {
            return self.dav_push_event(account_id, event_id);
        }
        self.submit(String::new(), move |w| {
            let Some(e) = w.db.event(event_id) else { return Ok(()) };
            match e.remote_id.clone() {
                Some(rid) => w.with_ews(account_id, |c| c.update_event(&rid, &e))?,
                None => {
                    let rid = w.with_ews(account_id, |c| c.create_event(&e))?;
                    // Yerel kaydı sunucudaki öğeye bağla (kimlik değişmesin)
                    let mut e = e;
                    e.remote_id = Some(rid);
                    let _ = w.db.save_event(&e);
                }
            }
            w.ews_sync_calendar(account_id)?;
            // Kullanıcının kendi işleminden sonra takvim her zaman yenilenir
            (w.emit)("calendar://changed", json!({}));
            Ok(())
        });
    }

    pub fn delete_remote_event(&self, account_id: i64, remote_id: String, series: bool, notify: bool) {
        self.submit(String::new(), move |w| {
            w.with_ews(account_id, |c| c.delete_event(&remote_id, series, notify))?;
            w.ews_sync_calendar(account_id)?;
            (w.emit)("calendar://changed", json!({}));
            Ok(())
        });
    }

    /// Toplantı davetine Exchange üzerinden yanıt
    pub fn respond_ews(&self, account_id: i64, item_id: String, kind: String, comment: String, send: bool) {
        self.submit(t!("status.sending"), move |w| {
            w.with_ews(account_id, |c| c.respond(&item_id, &kind, &comment, send))?;
            w.ews_sync_calendar(account_id)?;
            (w.emit)("calendar://changed", json!({}));
            if send {
                (w.emit)("mail://sent", json!(""));
            }
            Ok(())
        });
    }

    // ------------------------------------------------------------------ içe aktarma
    pub fn import_raw(&self, folder_id: i64, raw: Vec<u8>, read: bool) -> Option<i64> {
        let f = self.db.folder(folder_id)?;
        let mut m = parse::parse(&raw).into_message();
        m.folder_id = folder_id;
        m.account_id = f.account_id;
        m.is_read = read;
        let id = self.db.insert_message(&m).ok()?;
        if f.remote_name.is_some() {
            self.submit(String::new(), move |w| w.append_existing(id, folder_id));
        }
        self.changed(Some(folder_id));
        Some(id)
    }

    pub fn test_account(account: &crate::db::Account, password: &str) -> (bool, bool, String) {
        let mut lines = Vec::new();
        if account.is_exchange() {
            let mut acc = account.clone();
            acc.password = crate::secret::encrypt_local(password);
            let r = crate::mail::ews::Ews::for_account(&acc).and_then(|mut e| e.test());
            return match r {
                Ok(n) => (true, true, format!("✔ {}", t!("account.test_ews_ok", n))),
                Err(e) => (false, false, format!("✘ {e}")),
            };
        }
        let imap_ok = match Imap::connect(account, password) {
            Ok(mut c) => {
                let n = c.list_folders().map(|v| v.len()).unwrap_or(0);
                c.logout();
                lines.push(format!("✔ {}", t!("account.test_imap_ok", n)));
                true
            }
            Err(e) => {
                lines.push(format!("✘ {e}"));
                false
            }
        };
        let smtp_ok = match mail::smtp::send_test(account, password) {
            Ok(()) => {
                lines.push(format!("✔ {}", t!("account.test_smtp_ok")));
                true
            }
            Err(e) => {
                lines.push(format!("✘ {e}"));
                false
            }
        };
        for (url, kind, name) in [(&account.caldav_url, Kind::Calendar, "CalDAV"), (&account.carddav_url, Kind::Contacts, "CardDAV")] {
            if !url.trim().is_empty() {
                let found = Dav::for_account(account, url, Some(password)).and_then(|mut d| d.discover(kind));
                lines.push(match found {
                    Ok(v) => format!("✔ {}", t!("account.test_dav_ok", name, v.len())),
                    Err(e) => format!("✘ {name}: {e}"),
                });
            }
        }
        (imap_ok, smtp_ok, lines.join("\n"))
    }
}

impl Worker {
    /// Parolası reddedilmiş hesaba yeniden parola gönderilmez.
    pub fn check_paused(&self, acc: &crate::db::Account) -> MailResult<()> {
        if acc.auth_state == "failed" {
            return Err(MailError(t!("error.auth_paused", acc.email)));
        }
        Ok(())
    }

    /// Kimlik hatası ise hesabı duraklatır ve arayüze bildirir; hatayı geri verir.
    pub fn note_auth(&self, acc: &crate::db::Account, e: MailError) -> MailError {
        if e.is_auth() {
            self.db.set_account_auth(acc.id, "failed", e.message());
            (self.emit)("account://auth_failed", json!({"id": acc.id, "email": acc.email, "error": e.message()}));
            return MailError(t!("error.auth_failed_paused", acc.email, e.message()));
        }
        e
    }

    /// SMTP ile gönderim (duraklatma denetimiyle).
    pub fn smtp_send(&self, acc: &crate::db::Account, rcpts: &[String], raw: &[u8]) -> MailResult<()> {
        self.check_paused(acc)?;
        mail::smtp::send(acc, &acc.password_plain(), rcpts, raw).map_err(|e| self.note_auth(acc, e))
    }

    pub(crate) fn emit_changed(&self, folder: Option<i64>) {
        (self.emit)("mail://changed", json!({ "folder": folder }));
    }

    pub(crate) fn emit_new(&self, msgs: Vec<Message>) {
        if msgs.is_empty() {
            return;
        }
        let list: Vec<Value> = msgs
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "subject": m.subject,
                    "sender": mail::display_name(&m.sender),
                    "email": mail::split_name_addr(&m.sender).1,
                    "preview": m.snippet.chars().take(160).collect::<String>(),
                    "date": m.date,
                    "importance": m.importance,
                    "has_attachments": m.has_attachments,
                })
            })
            .collect();
        (self.emit)("mail://new", json!(list));
    }

    pub fn imap(&mut self, account_id: i64) -> MailResult<&mut Imap> {
        let alive = self.conns.get_mut(&account_id).map(|c| c.noop()).unwrap_or(false);
        if !alive {
            if let Some(c) = self.conns.remove(&account_id) {
                c.logout();
            }
            let acc = self.db.account(account_id).ok_or_else(|| MailError(t!("error.no_account")))?;
            self.check_paused(&acc)?;
            let c = Imap::connect(&acc, &acc.password_plain()).map_err(|e| self.note_auth(&acc, e))?;
            self.conns.insert(account_id, c);
        }
        Ok(self.conns.get_mut(&account_id).unwrap())
    }

    /// Bekleyen bayrak değişikliklerini sunucuya yazar (klasör, alan ve değer başına tek istek).
    /// Bağlantı yoksa ya da hesap duraklatılmışsa kayıtlar kalır, sonraki eşitlemede yeniden
    /// denenir; sunucu üç kez reddederse sunucudaki değer esas alınır.
    pub(crate) fn push_flags(&mut self, account_id: i64) -> MailResult<()> {
        let pending = self.db.pending_flags(account_id);
        let Some(acc) = self.db.account(account_id) else { return Ok(()) };
        if pending.is_empty() || acc.auth_state == "failed" {
            return Ok(());
        }
        let ews = acc.is_exchange();
        let mut groups: Vec<Vec<crate::db::PendingFlag>> = Vec::new();
        for p in pending {
            match groups.last_mut() {
                Some(g) if g[0].folder_id == p.folder_id && g[0].field == p.field && g[0].value == p.value => g.push(p),
                _ => groups.push(vec![p]),
            }
        }
        let mut first_err = None;
        for g in groups {
            let (field, value) = (g[0].field.clone(), g[0].value);
            let res = if ews {
                let rids: Vec<String> = g.iter().filter_map(|p| p.remote_id.clone()).collect();
                match field.as_str() {
                    "is_read" if !rids.is_empty() => self.with_ews(account_id, |e| e.set_read(&rids, value)),
                    "is_flagged" if !rids.is_empty() => self.with_ews(account_id, |e| e.set_flag(&rids, value)),
                    // Exchange "yanıtlandı" bilgisini gönderilen yanıttan kendisi işler
                    _ => Ok(()),
                }
            } else {
                let uids: Vec<u32> = g.iter().filter_map(|p| p.uid.map(|u| u as u32)).collect();
                let flag = match field.as_str() {
                    "is_read" => "\\Seen",
                    "is_flagged" => "\\Flagged",
                    _ => "\\Answered",
                };
                match self.db.folder(g[0].folder_id).and_then(|f| f.remote_name) {
                    Some(remote) if !uids.is_empty() => (|| -> MailResult<()> {
                        let imap = self.imap(account_id)?;
                        imap.select(&remote)?;
                        imap.store_flag(&uids, flag, value)
                    })(),
                    _ => Ok(()),
                }
            };
            match res {
                Ok(()) => self.db.clear_pending_flags(&g),
                Err(e) if e.is_net() || self.db.account(account_id).is_some_and(|a| a.auth_state == "failed") => return Err(e),
                Err(e) => {
                    self.db.fail_pending_flags(&g, 3);
                    first_err.get_or_insert(e);
                }
            }
        }
        first_err.map_or(Ok(()), Err)
    }

    fn sync_account(&mut self, account_id: i64) -> MailResult<()> {
        // Önce bekleyen bayraklar: sunucudan okunan bayraklar kullanıcının son değişikliğini içersin
        if let Err(e) = self.push_flags(account_id) {
            if e.is_net() {
                return Err(e);
            }
            (self.emit)("mail://error", json!(e.message()));
        }
        if self.db.account(account_id).map(|a| a.is_exchange()).unwrap_or(false) {
            return self.ews_sync_account(account_id);
        }
        let before = self.db.total_changes();
        self.sync_folder_list(account_id)?;
        let mut new = Vec::new();
        for f in self.db.folders(Some(account_id)) {
            if f.remote_name.is_none() {
                continue;
            }
            match self.sync_folder(account_id, &f) {
                Ok(n) => new.extend(n),
                Err(e) => (self.emit)("mail://error", json!(e.message())),
            }
        }
        // Hiçbir şey değişmediyse arayüz yenilenmez
        if !new.is_empty() || self.db.total_changes() != before {
            self.emit_changed(None);
        }
        self.emit_new(new);
        Ok(())
    }

    fn sync_folder_list(&mut self, account_id: i64) -> MailResult<()> {
        let remote = self.imap(account_id)?.list_folders()?;
        let existing: HashMap<String, Folder> = self
            .db
            .folders(Some(account_id))
            .into_iter()
            .filter_map(|f| f.remote_name.clone().map(|r| (r, f)))
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut used_roles = std::collections::HashSet::new();
        let mut by_path: HashMap<String, (i64, String)> = HashMap::new();
        for rf in &remote {
            let mut role = rf.role.clone();
            if let Some(r) = &role {
                if !used_roles.insert(r.clone()) {
                    role = None;
                }
            }
            let leaf = rf.path.rsplit(rf.delim.as_str()).next().unwrap_or(&rf.path).to_string();
            seen.insert(rf.remote.clone());
            let id = if let Some(f) = existing.get(&rf.remote) {
                let mut f = f.clone();
                f.name = leaf;
                f.role = role;
                let _ = self.db.update_folder(&f);
                f.id
            } else {
                self.db
                    .insert_folder(&Folder {
                        account_id: Some(account_id),
                        name: leaf,
                        remote_name: Some(rf.remote.clone()),
                        role,
                        position: 100,
                        ..Default::default()
                    })
                    .unwrap_or(0)
            };
            by_path.insert(rf.path.clone(), (id, rf.delim.clone()));
        }
        for (path, (id, delim)) in &by_path {
            let parent = if !delim.is_empty() && path.contains(delim.as_str()) {
                path.rsplit_once(delim.as_str()).and_then(|(p, _)| by_path.get(p)).map(|x| x.0)
            } else {
                None
            };
            if let Some(mut f) = self.db.folder(*id) {
                f.parent_id = if f.role.is_some() { None } else { parent };
                let _ = self.db.update_folder(&f);
            }
        }
        for (name, f) in existing {
            if !seen.contains(&name) {
                let _ = self.db.delete_folder(f.id);
            }
        }
        Ok(())
    }

    pub fn sync_folder(&mut self, account_id: i64, folder: &Folder) -> MailResult<Vec<Message>> {
        let acc = self.db.account(account_id).ok_or_else(|| MailError(t!("error.no_account")))?;
        let limit = acc.sync_limit.max(0) as usize;
        let Some(remote) = folder.remote_name.clone() else { return Ok(vec![]) };
        let db = self.db.clone();
        let imap = self.imap(account_id)?;
        let (_, uidvalidity) = imap.select(&remote)?;
        if folder.uidvalidity != 0 && uidvalidity != 0 && folder.uidvalidity != uidvalidity as i64 {
            let _ = db.conn().execute("DELETE FROM messages WHERE folder_id = ?1 AND uid IS NOT NULL", [folder.id]);
        }
        if uidvalidity != 0 && folder.uidvalidity != uidvalidity as i64 {
            let mut f = folder.clone();
            f.uidvalidity = uidvalidity as i64;
            let _ = db.update_folder(&f);
        }
        let server = imap.search_all()?;
        let window: Vec<u32> = if limit > 0 && server.len() > limit { server[server.len() - limit..].to_vec() } else { server.clone() };
        let server_set: std::collections::HashSet<u32> = server.iter().copied().collect();
        let local: HashMap<u32, (i64, bool, bool, bool)> =
            db.message_uids(folder.id).into_iter().map(|(id, uid, r, f, a)| (uid, (id, r, f, a))).collect();
        for (uid, (id, ..)) in &local {
            if !server_set.contains(uid) {
                db.delete_message(*id);
            }
        }
        let existing: Vec<u32> = window.iter().copied().filter(|u| local.contains_key(u)).collect();
        for (uid, flags) in imap.fetch_flags(&existing)? {
            if let Some((id, r, f, a)) = local.get(&uid) {
                let (nr, nf, na) = (flags_contain(&flags, "\\Seen"), flags_contain(&flags, "\\Flagged"), flags_contain(&flags, "\\Answered"));
                // Sunucuya henüz yazılmamış yerel değişikliğin üzerine yazılmaz
                for (field, server, was) in [("is_read", nr, *r), ("is_flagged", nf, *f), ("is_answered", na, *a)] {
                    if server != was {
                        db.set_flag_from_server(*id, field, server);
                    }
                }
            }
        }
        let new_uids: Vec<u32> = window.iter().copied().filter(|u| !local.contains_key(u)).collect();
        let fetched = imap.fetch_messages(&new_uids)?;
        let is_inbox = folder.role.as_deref() == Some("inbox");
        let rules = if is_inbox { db.rules(true) } else { vec![] };
        let mut new_msgs = Vec::new();
        for f in fetched {
            let parsed = parse::parse(&f.raw);
            let (auto, list) = (parsed.auto_submitted, parsed.list_id);
            let mut m = parsed.into_message();
            m.folder_id = folder.id;
            m.account_id = Some(account_id);
            m.uid = Some(f.uid as i64);
            m.is_read = flags_contain(&f.flags, "\\Seen");
            m.is_flagged = flags_contain(&f.flags, "\\Flagged");
            m.is_answered = flags_contain(&f.flags, "\\Answered");
            m.is_draft = flags_contain(&f.flags, "\\Draft");
            let Ok(id) = db.insert_message(&m) else { continue };
            m.id = id;
            if is_inbox && !m.is_read {
                let mut notify = true;
                if !rules.is_empty() {
                    notify = self.apply_rules(&rules, &m, account_id, folder)?;
                }
                if notify {
                    new_msgs.push(m.clone());
                }
                if !auto && !list {
                    self.auto_reply(&acc, &m);
                }
            }
        }
        Ok(new_msgs)
    }

    fn apply_rules(&mut self, rules: &[crate::db::Rule], m: &Message, account_id: i64, folder: &Folder) -> MailResult<bool> {
        let mut notify = true;
        let uid = m.uid.unwrap_or(0) as u32;
        let src_remote = folder.remote_name.clone().unwrap_or_default();
        let me = self.db.my_addresses();
        for (action, arg) in crate::rules::evaluate(rules, m, Some(account_id), &me) {
            match action.as_str() {
                "mark_read" => {
                    self.db.set_message_field(m.id, "is_read", 1);
                    let imap = self.imap(account_id)?;
                    imap.select(&src_remote)?;
                    imap.store_flag(&[uid], "\\Seen", true)?;
                    notify = false;
                }
                "mark_unread" => {
                    self.db.set_message_field(m.id, "is_read", 0);
                    let imap = self.imap(account_id)?;
                    imap.select(&src_remote)?;
                    imap.store_flag(&[uid], "\\Seen", false)?;
                }
                "flag" => {
                    self.db.set_message_field(m.id, "is_flagged", 1);
                    let imap = self.imap(account_id)?;
                    imap.select(&src_remote)?;
                    imap.store_flag(&[uid], "\\Flagged", true)?;
                }
                "unflag" => {
                    self.db.set_message_field(m.id, "is_flagged", 0);
                    let imap = self.imap(account_id)?;
                    imap.select(&src_remote)?;
                    imap.store_flag(&[uid], "\\Flagged", false)?;
                }
                "important" => self.db.set_message_field(m.id, "importance", 2),
                "importance" => self.db.set_message_field(m.id, "importance", crate::rules::importance_level(&arg)),
                "desktop_notify" => (self.emit)("mail://rule-notify", json!({ "title": arg, "body": m.subject })),
                "copy" => {
                    let dest = arg.parse::<i64>().ok().and_then(|id| self.db.folder(id));
                    if let (Some(dest), Some(full)) = (dest, self.db.message(m.id)) {
                        match dest.remote_name.as_deref().filter(|_| dest.account_id == Some(account_id)) {
                            Some(remote) => self.imap(account_id)?.append(remote, &full.raw, full.is_read, false)?,
                            None => {
                                let mut copy = parse::parse(&full.raw).into_message();
                                copy.folder_id = dest.id;
                                copy.account_id = dest.account_id;
                                copy.is_read = full.is_read;
                                let _ = self.db.insert_message(&copy);
                            }
                        }
                    }
                }
                "permanent_delete" => {
                    let imap = self.imap(account_id)?;
                    imap.select(&src_remote)?;
                    imap.delete(&[uid])?;
                    self.db.delete_message(m.id);
                    return Ok(false);
                }
                "category" => {
                    let mut cats = crate::db::split_list(&self.db.message_row(m.id).map(|r| r.categories).unwrap_or_default());
                    if !arg.is_empty() && !cats.contains(&arg) {
                        cats.push(arg.clone());
                    }
                    self.db.set_message_field(m.id, "categories", cats.join(","));
                }
                "notify" => notify = false,
                "move" | "delete" | "junk" => {
                    let dest = if action == "move" {
                        arg.parse::<i64>().ok().and_then(|id| self.db.folder(id))
                    } else {
                        self.db.folder_by_role_or_local(Some(account_id), if action == "delete" { "trash" } else { "junk" })
                    };
                    if let Some(dest) = dest {
                        if dest.id != folder.id {
                            if dest.account_id == Some(account_id) && dest.remote_name.is_some() {
                                let imap = self.imap(account_id)?;
                                imap.select(&src_remote)?;
                                imap.move_to(&[uid], dest.remote_name.as_deref().unwrap())?;
                                self.db.delete_message(m.id);
                            } else {
                                self.db.set_message_field(m.id, "folder_id", dest.id);
                                self.db.set_message_field(m.id, "account_id", dest.account_id);
                                self.db.set_message_field(m.id, "uid", None::<i64>);
                                let imap = self.imap(account_id)?;
                                imap.select(&src_remote)?;
                                imap.delete(&[uid])?;
                            }
                            return Ok(false);
                        }
                    }
                }
                "forward_attachment" if !arg.is_empty() => {
                    if let (Some(acc), Some(full)) = (self.db.account(account_id), self.db.message(m.id)) {
                        let out = mail::build::ComposedMail {
                            to: arg.clone(),
                            subject: mail::reply_subject(&full.subject, &t!("compose.fw_prefix")),
                            text: t!("compose.forwarded"),
                            importance: 1,
                            ..Default::default()
                        };
                        let raw = mail::build::build_raw(Some(&acc), &out, &[("ileti.eml".to_string(), "message/rfc822".to_string(), full.raw.clone())], false);
                        if let Err(e) = self.smtp_send(&acc, &mail::bare_addresses(&arg), &raw) {
                            (self.emit)("mail://error", json!(e.message()));
                        }
                    }
                }
                "forward" if !arg.is_empty() => {
                    if let (Some(acc), Some(full)) = (self.db.account(account_id), self.db.message(m.id)) {
                        let body = format!(
                            "\n\n---------- {} ----------\n{}: {}\n{}: {}\n\n{}",
                            t!("compose.forwarded"),
                            t!("mail.from"),
                            full.sender,
                            t!("mail.subject"),
                            full.subject,
                            full.body_text
                        );
                        let raw = mail::build::simple_raw(&acc.from_header(), &arg, &mail::reply_subject(&full.subject, &t!("compose.fw_prefix")), &body);
                        let rcpts = mail::bare_addresses(&arg);
                        if let Err(e) = self.smtp_send(&acc, &rcpts, &raw) {
                            (self.emit)("mail://error", json!(e.message()));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(notify)
    }

    fn auto_reply(&mut self, acc: &crate::db::Account, m: &Message) {
        if !acc.auto_reply_enabled || acc.auto_reply_text.trim().is_empty() {
            return;
        }
        let Some(sender) = mail::bare_addresses(&m.sender).into_iter().next() else { return };
        if sender.eq_ignore_ascii_case(&acc.email) || sender.to_lowercase().contains("noreply") || sender.to_lowercase().contains("no-reply") {
            return;
        }
        let key = format!("autoreply:{}:{}", acc.id, sender.to_lowercase());
        let last: f64 = self.db.setting(&key).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        if now() - last < 86400.0 {
            return;
        }
        let raw = mail::build::simple_raw(&acc.from_header(), &sender, &mail::reply_subject(&m.subject, &t!("compose.re_prefix")), &acc.auto_reply_text);
        // Auto-Submitted başlığını ekle (döngüleri önler)
        let mut with_hdr = b"Auto-Submitted: auto-replied\r\n".to_vec();
        with_hdr.extend_from_slice(&raw);
        match self.smtp_send(acc, &[sender], &with_hdr) {
            Ok(()) => self.db.set_setting(&key, &now().to_string()),
            Err(e) => (self.emit)("mail://error", json!(t!("error.auto_reply", e))),
        }
    }

    fn append_existing(&mut self, id: i64, dest_id: i64) -> MailResult<()> {
        let (Some(mut m), Some(dest)) = (self.db.message(id), self.db.folder(dest_id)) else { return Ok(()) };
        let (Some(acc), Some(remote)) = (dest.account_id, dest.remote_name.clone()) else { return Ok(()) };
        if m.raw.is_empty() {
            m.raw = crate::ews_sync::ensure_content(&self.db, id).unwrap_or_default();
        }
        if m.raw.is_empty() {
            return Ok(());
        }
        if self.db.account(acc).map(|a| a.is_exchange()).unwrap_or(false) {
            let raw = m.raw.clone();
            let read = m.is_read;
            let rid = self.with_ews(acc, |e| e.save_mime(&remote, &raw, read, false))?;
            self.db.set_message_field(id, "remote_id", rid);
            self.emit_changed(Some(dest_id));
            return Ok(());
        }
        self.imap(acc)?.append(&remote, &m.raw, m.is_read, m.is_draft)?;
        self.db.delete_message(id);
        self.sync_folder(acc, &dest)?;
        self.emit_changed(Some(dest_id));
        Ok(())
    }

    fn send_outbox_item(&mut self, id: i64) -> MailResult<()> {
        let Some(m) = self.db.message(id) else { return Ok(()) };
        let p: Vec<&str> = m.pending.split(':').collect();
        if p.len() != 3 || p[0] != "send" {
            return Ok(());
        }
        let account_id: i64 = p[2].parse().unwrap_or(0);
        let acc = self.db.account(account_id).ok_or_else(|| MailError(t!("error.no_account")))?;
        // İleti sahiplenilir: bu andan sonra "Geri al" başarısız olur (ileti hem gönderilip hem
        // taslağa dönmesin). Başka bir iş zaten gönderiyorsa ya da geri alındıysa atlanır.
        let sending = format!("sending:{}:{}", p[1], p[2]);
        if !self.db.swap_pending(id, &m.pending, &sending) {
            return Ok(());
        }
        let r = self.send_claimed(id, m.clone(), acc, account_id);
        if r.is_err() {
            // Gönderilemedi: Giden Kutusu'nda yeniden denenecek duruma döner
            self.db.swap_pending(id, &sending, &m.pending);
        }
        r
    }

    fn send_claimed(&mut self, id: i64, m: Message, acc: crate::db::Account, account_id: i64) -> MailResult<()> {
        if acc.is_exchange() {
            // Exchange: Gizli alıcılar MIME içinde kalır, sunucu kopyayı Gönderilmiş Öğeler'e koyar
            let raw = m.raw.clone();
            if let Err(e) = self.with_ews(account_id, |c| c.send_mime(&raw)) {
                // Bağlantı yoksa ileti Giden Kutusu'nda bekler (Outlook gibi), bağlanınca gönderilir
                if e.is_net() {
                    return Err(e);
                }
                self.db.set_message_field(id, "snippet", format!("[!] {}", e.message()));
                self.emit_changed(None);
                return Err(MailError(t!("error.send_failed", m.subject, e.message())));
            }
            self.db.delete_message(id);
            if !m.in_reply_to.is_empty() {
                for orig in self.db.messages_by_message_id(m.in_reply_to.trim()) {
                    self.db.set_message_field(orig.id, "is_answered", 1);
                }
            }
            self.emit_changed(None);
            (self.emit)("mail://sent", json!(m.subject));
            // Gönderilmiş Öğeler sonra (gönderim bildirimi beklemez)
            if let Some(sent) = self.db.folder_by_role(Some(account_id), "sent") {
                let sid = sent.id;
                self.defer_fg(format!("f:{sid}"), move |w| {
                    let Some(f) = w.db.folder(sid) else { return Ok(()) };
                    w.ews_sync_folder(account_id, &f)?;
                    w.emit_changed(Some(sid));
                    Ok(())
                });
            }
            return Ok(());
        }
        let mut rcpts = mail::bare_addresses(&m.to_addr);
        rcpts.extend(mail::bare_addresses(&m.cc));
        rcpts.extend(mail::bare_addresses(&m.bcc));
        let wire = strip_bcc(&m.raw);
        if let Err(e) = self.smtp_send(&acc, &rcpts, &wire) {
            if e.is_net() {
                return Err(e);
            }
            self.db.set_message_field(id, "snippet", format!("[!] {}", e.message()));
            self.emit_changed(None);
            return Err(MailError(t!("error.send_failed", m.subject, e.message())));
        }
        self.db.delete_message(id);
        // Gönderilmiş Öğeler'e kaydet
        let sent = self.db.folder_by_role_or_local(Some(account_id), "sent");
        let host = acc.smtp_host.to_lowercase();
        let server_saves = host.contains("gmail") || host.contains("office365") || host.contains("outlook");
        if let Some(sent) = sent {
            match (&sent.remote_name, server_saves) {
                (Some(_), true) => {
                    let _ = self.sync_folder(account_id, &sent);
                }
                (Some(remote), false) => {
                    let remote = remote.clone();
                    if let Err(e) = self.imap(account_id).and_then(|i| i.append(&remote, &m.raw, true, false)) {
                        (self.emit)("mail://error", json!(t!("error.save_sent", e)));
                    }
                    let _ = self.sync_folder(account_id, &sent);
                }
                (None, _) => {
                    let mut copy = parse::parse(&m.raw).into_message();
                    copy.folder_id = sent.id;
                    copy.account_id = sent.account_id;
                    copy.is_read = true;
                    let _ = self.db.insert_message(&copy);
                }
            }
        }
        // Yanıtlanan iletiyi işaretle
        if !m.in_reply_to.is_empty() {
            for orig in self.db.messages_by_message_id(m.in_reply_to.trim()) {
                self.db.set_message_field(orig.id, "is_answered", 1);
                if let (Some(uid), Some(a)) = (orig.uid, orig.account_id) {
                    if let Some(remote) = self.db.folder(orig.folder_id).and_then(|f| f.remote_name) {
                        if let Ok(imap) = self.imap(a) {
                            let _ = imap.select(&remote).and_then(|_| imap.store_flag(&[uid as u32], "\\Answered", true));
                        }
                    }
                }
            }
        }
        self.emit_changed(None);
        (self.emit)("mail://sent", json!(m.subject));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcc_is_stripped() {
        let raw = b"From: a@x.com\r\nTo: b@y.com\r\nBcc: gizli@z.com,\r\n diger@z.com\r\nSubject: s\r\n\r\nBcc: govdede kalir\r\n";
        let out = String::from_utf8(strip_bcc(raw)).unwrap();
        assert!(!out.contains("gizli") && !out.contains("diger"));
        assert!(out.contains("Subject: s") && out.contains("Bcc: govdede kalir"));
    }

    /// Sahte IMAP sunucusu: her komutu kaydeder ve olumlu yanıtlar
    fn fake_imap(host: &str) -> (u16, Arc<Mutex<Vec<String>>>) {
        use std::io::{BufRead, Write};
        let l = std::net::TcpListener::bind((host, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let seen = seen.clone();
                std::thread::spawn(move || {
                    let mut w = s.try_clone().unwrap();
                    let _ = w.write_all(b"* OK hazir\r\n");
                    for line in std::io::BufReader::new(s).lines().map_while(Result::ok) {
                        let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                        seen.lock().unwrap().push(cmd.to_string());
                        let up = cmd.to_uppercase();
                        let data = if up.starts_with("CAPABILITY") {
                            "* CAPABILITY IMAP4rev1\r\n"
                        } else if up.starts_with("SELECT") {
                            "* 3 EXISTS\r\n* OK [UIDVALIDITY 1] u\r\n"
                        } else {
                            ""
                        };
                        let _ = write!(w, "{data}{tag} OK tamam\r\n");
                    }
                });
            }
        });
        (port, log)
    }

    fn test_worker(db: &Db) -> Worker {
        let q = || Arc::new(Queue { items: Mutex::new(VecDeque::new()), cv: Condvar::new(), pending: AtomicUsize::new(0) });
        Worker { db: db.clone(), conns: HashMap::new(), ews: HashMap::new(), emit: Arc::new(|_, _| {}), fg: q(), bg: q(), enum_seen: Default::default(), folder_locks: Default::default() }
    }

    fn imap_account(db: &Db, host: &str, port: u16) -> (i64, i64) {
        let acc = db
            .save_account(&crate::db::Account { name: "t".into(), email: "t@x.test".into(), imap_host: host.into(), imap_port: port as i64, imap_security: "none".into(), username: "t".into(), password: "p".into(), ..Default::default() })
            .unwrap();
        let inbox = db.insert_folder(&Folder { account_id: Some(acc), name: "INBOX".into(), remote_name: Some("INBOX".into()), role: Some("inbox".into()), ..Default::default() }).unwrap();
        (acc, inbox)
    }

    /// Gönderimi geri alma: gönderim ile "Geri al" aynı iletiyi aynı anda sahiplenemez; gönderilemeyen
    /// ileti Giden Kutusu'nda yeniden denenecek duruma döner; kapanışta yarım kalan gönderim geri alınır
    #[test]
    fn outbox_send_and_undo_cannot_both_win() {
        let db = Db::open_memory().unwrap();
        let acc = db
            .save_account(&crate::db::Account { name: "t".into(), email: "t@x.test".into(), smtp_host: "127.0.0.9".into(), smtp_port: 1, smtp_security: "none".into(), username: "t".into(), password: "p".into(), ..Default::default() })
            .unwrap();
        let outbox = db.folder_by_role(None, "outbox").expect("giden kutusu");
        let add = |pending: &str| db.insert_message(&Message { folder_id: outbox.id, to_addr: "a@x.test".into(), raw: b"Subject: x\r\n\r\nx".to_vec(), pending: pending.into(), ..Default::default() }).unwrap();
        let pend = |id: i64| db.message(id).unwrap().pending;
        let send = format!("send:0:{acc}");

        // Koşullu değişim tek kez başarır
        let a = add(&send);
        assert!(db.swap_pending(a, &send, "hold"));
        assert!(!db.swap_pending(a, &send, "sending:0:1"));
        // Geri alınmış (hold) ileti gönderilmez, durumu değişmez
        test_worker(&db).send_outbox_item(a).unwrap();
        assert_eq!(pend(a), "hold");

        // Sunucuya ulaşılamadı: ileti silinmez, yeniden denenecek duruma döner
        let b = add(&send);
        assert!(test_worker(&db).send_outbox_item(b).is_err());
        assert_eq!(pend(b), send);

        // Gönderim sırasında uygulama kapandı: açılışta yeniden gönderilecek duruma döner
        let c = add(&format!("sending:0:{acc}"));
        assert_eq!(db.reset_stuck_sending(), 1);
        assert_eq!(pend(c), send);
    }

    /// Okundu/bayrak değişiklikleri klasör ve değer başına tek STORE ile gider; yazılınca bekleyenlerden düşer
    #[test]
    fn pending_flags_are_pushed_in_groups() {
        let db = Db::open_memory().unwrap();
        let (port, log) = fake_imap("127.0.0.1");
        let (acc, inbox) = imap_account(&db, "127.0.0.1", port);
        let ids: Vec<i64> = (1..=3).map(|uid| db.insert_message(&Message { folder_id: inbox, account_id: Some(acc), uid: Some(uid), ..Default::default() }).unwrap()).collect();
        db.set_flags_local(&ids[..2], "is_read", true);
        db.set_flags_local(&ids[2..], "is_flagged", true);
        test_worker(&db).push_flags(acc).unwrap();
        assert!(db.pending_flags(acc).is_empty());
        let log = log.lock().unwrap();
        assert!(log.iter().any(|c| c == "UID STORE 1,2 +FLAGS.SILENT (\\Seen)"), "{log:?}");
        assert!(log.iter().any(|c| c == "UID STORE 3 +FLAGS.SILENT (\\Flagged)"), "{log:?}");
    }

    /// Sunucuya ulaşılamazken yapılan değişiklik kaybolmaz ve eşitleme onu geri almaz
    #[test]
    fn pending_flags_survive_offline() {
        let db = Db::open_memory().unwrap();
        let closed = std::net::TcpListener::bind(("127.0.0.3", 0)).unwrap().local_addr().unwrap().port();
        let (acc, inbox) = imap_account(&db, "127.0.0.3", closed);
        let id = db.insert_message(&Message { folder_id: inbox, account_id: Some(acc), uid: Some(5), ..Default::default() }).unwrap();
        db.set_flags_local(&[id], "is_read", true);
        let e = test_worker(&db).push_flags(acc).unwrap_err();
        assert!(e.is_net(), "{}", e.message());
        assert_eq!(db.pending_flags(acc).len(), 1);
        // Sunucudan gelen eski "okunmadı" bilgisi yazılmaz
        assert!(!db.set_flag_from_server(id, "is_read", false));
        assert!(db.message_row(id).unwrap().is_read);
    }
}
