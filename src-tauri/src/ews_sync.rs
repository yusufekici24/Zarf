//! Exchange (EWS) hesaplarının eşitlenmesi: posta, takvim, kişiler.

use crate::db::{Calendar, Folder, Message};
use crate::mail::ews::{Ews, EwsHeader};
use crate::mail::{parse, MailError, MailResult};
use crate::sync::Worker;
use crate::t;
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// Klasör başına son sabitleme denetimi (10 dakikada bir)
static PIN_CHECK: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<i64, std::time::Instant>>> = std::sync::LazyLock::new(Default::default);

/// İletinin tam içeriği; yalnızca başlığı inmiş Exchange iletisiyse sunucudan
/// hemen indirilip kaydedilir (posta açılınca; eşitleme kuyruğunu beklemez).
pub fn ensure_content(db: &crate::db::Db, id: i64) -> Option<Vec<u8>> {
    let m = db.message(id)?;
    if !m.raw.is_empty() {
        return Some(m.raw);
    }
    let rid = m.remote_id.clone()?;
    let acc = db.account(m.account_id?)?;
    if !acc.is_exchange() || acc.auth_state == "failed" {
        return None;
    }
    let r = crate::mail::ews::pooled(&acc, |e| e.get_messages(&[rid]));
    match r {
        Ok(items) => {
            let it = items.into_iter().next()?;
            let parsed = parse::parse(&it.mime).into_message();
            db.update_message_content(id, &parsed);
            Some(it.mime)
        }
        Err(e) => {
            // Kimlik reddi: hesap duraklatılır (parola tekrar denenmez)
            if e.is_auth() {
                db.set_account_auth(acc.id, "failed", e.message());
            }
            None
        }
    }
}

/// Sunucu kurallarını yerel listeye yansıtır (sunucu esas alınır).
pub fn store_server_rules(db: &crate::db::Db, account_id: i64, list: Vec<crate::mail::ews::ServerRule>) {
    let folders = db.folders(Some(account_id));
    let existing: Vec<crate::db::Rule> = db.rules(false).into_iter().filter(|r| r.account_id == Some(account_id) && r.remote_id.is_some()).collect();
    let mut seen = HashSet::new();
    for sr in list {
        seen.insert(sr.id.clone());
        // Taşıma hedefi: sunucu klasör kimliği → yerel klasör
        let action_arg = if sr.action == "move" {
            let dist_role = match sr.action_arg.as_str() {
                "inbox" => Some("inbox"),
                "deleteditems" => Some("trash"),
                "junkemail" => Some("junk"),
                "sentitems" => Some("sent"),
                "drafts" => Some("drafts"),
                _ => None,
            };
            folders
                .iter()
                .find(|f| f.remote_name.as_deref() == Some(sr.action_arg.as_str()) || (dist_role.is_some() && f.role.as_deref() == dist_role))
                .map(|f| f.id.to_string())
                .unwrap_or_default()
        } else {
            sr.action_arg.clone()
        };
        let action = if sr.action == "move" && folders.iter().any(|f| f.role.as_deref() == Some("junk") && f.id.to_string() == action_arg) { "junk".to_string() } else { sr.action.clone() };
        let old = existing.iter().find(|r| r.remote_id.as_deref() == Some(sr.id.as_str()));
        let r = crate::db::Rule {
            id: old.map(|o| o.id).unwrap_or(0),
            name: sr.name,
            enabled: sr.enabled,
            account_id: Some(account_id),
            field: if sr.field.is_empty() { "any".into() } else { sr.field },
            op: sr.op,
            value: sr.value,
            action: if action.is_empty() { "notify".into() } else { action },
            action_arg,
            stop: sr.stop,
            remote_id: Some(sr.id),
            complex: sr.complex,
            remote_xml: sr.body_xml,
            spec: String::new(),
        };
        let _ = db.save_rule(&r);
    }
    for r in existing {
        if !r.remote_id.as_ref().map(|id| seen.contains(id)).unwrap_or(false) {
            db.delete_rule(r.id);
        }
    }
}

/// Kuralı Exchange'e yazar (yeni ya da değişen). `force`: Outlook masaüstü istemci
/// kuralları varsa onların silinmesine izin verilir.
pub fn push_server_rule(db: &crate::db::Db, rule: &crate::db::Rule, force: bool) -> Result<(), String> {
    let acc = rule.account_id.and_then(|a| db.account(a)).ok_or_else(|| t!("error.no_account"))?;
    let body = if rule.complex && rule.remote_id.is_some() {
        rule.remote_xml.clone()
    } else {
        let arg = if rule.action == "move" {
            db.folder(rule.action_arg.parse().unwrap_or(0)).and_then(|f| f.remote_name).ok_or_else(|| t!("rules.server_folder"))?
        } else {
            rule.action_arg.clone()
        };
        if rule.action == "move" {
            let f = db.folder(rule.action_arg.parse().unwrap_or(0));
            if f.and_then(|f| f.account_id) != Some(acc.id) {
                return Err(t!("rules.server_folder"));
            }
        }
        crate::mail::ews::ServerRule::build_body(&rule.field, &rule.op, &rule.value, &rule.action, &arg, rule.stop).map_err(|_| t!("rules.server_action"))?
    };
    let core = format!(
        "<t:DisplayName>{}</t:DisplayName><t:Priority>1</t:Priority><t:IsEnabled>{}</t:IsEnabled>{body}",
        crate::mail::xml::esc(&rule.name),
        rule.enabled
    );
    let op = match &rule.remote_id {
        Some(id) => format!("<t:SetRuleOperation><t:Rule><t:RuleId>{}</t:RuleId>{core}</t:Rule></t:SetRuleOperation>", crate::mail::xml::esc(id)),
        None => format!("<t:CreateRuleOperation><t:Rule>{core}</t:Rule></t:CreateRuleOperation>"),
    };
    server_rules_op(db, &acc, &op, force)
}

/// Tam modelle (çok koşullu/eylemli) düzenlenmiş kuralı Exchange'e yazar. Var olan
/// kuralın önceliği korunur; yeni kural Outlook'taki gibi en üste eklenir.
pub fn push_server_rule_body(db: &crate::db::Db, account_id: i64, remote_id: Option<&str>, name: &str, enabled: bool, body: &str, force: bool) -> Result<(), String> {
    let acc = db.account(account_id).ok_or_else(|| t!("error.no_account"))?;
    let priority = match remote_id {
        Some(id) => crate::mail::ews::pooled(&acc, |e| e.get_rules(&acc.email))
            .ok()
            .and_then(|l| l.into_iter().find(|r| r.id == id).map(|r| r.priority))
            .unwrap_or(1),
        None => 1,
    };
    let core = format!(
        "<t:DisplayName>{}</t:DisplayName><t:Priority>{priority}</t:Priority><t:IsEnabled>{enabled}</t:IsEnabled>{body}",
        crate::mail::xml::esc(name)
    );
    let op = match remote_id {
        Some(id) => format!("<t:SetRuleOperation><t:Rule><t:RuleId>{}</t:RuleId>{core}</t:Rule></t:SetRuleOperation>", crate::mail::xml::esc(id)),
        None => format!("<t:CreateRuleOperation><t:Rule>{core}</t:Rule></t:CreateRuleOperation>"),
    };
    server_rules_op(db, &acc, &op, force)
}

pub fn delete_server_rule(db: &crate::db::Db, rule: &crate::db::Rule, force: bool) -> Result<(), String> {
    let acc = rule.account_id.and_then(|a| db.account(a)).ok_or_else(|| t!("error.no_account"))?;
    let Some(id) = &rule.remote_id else { return Ok(()) };
    server_rules_op(db, &acc, &format!("<t:DeleteRuleOperation><t:RuleId>{}</t:RuleId></t:DeleteRuleOperation>", crate::mail::xml::esc(id)), force)
}

fn server_rules_op(db: &crate::db::Db, acc: &crate::db::Account, op: &str, force: bool) -> Result<(), String> {
    if acc.auth_state == "failed" {
        return Err(t!("error.auth_paused", acc.email));
    }
    if let Err(err) = crate::mail::ews::pooled(acc, |e| e.update_rules(&acc.email, op, force)) {
        if err.is_auth() {
            db.set_account_auth(acc.id, "failed", err.message());
        }
        if err.message().contains("OutlookRuleBlobExists") {
            // Arayüz kullanıcıya sorup force ile yeniden dener
            return Err("rules.blob_exists".into());
        }
        return Err(err.to_string());
    }
    // Sunucudaki son durumu (yeni kural kimliği dahil) al
    if let Ok(list) = crate::mail::ews::pooled(acc, |e| e.get_rules(&acc.email)) {
        store_server_rules(db, acc.id, list);
    }
    Ok(())
}

static ENUM_PAGES: std::sync::LazyLock<std::sync::Mutex<HashMap<i64, u64>>> = std::sync::LazyLock::new(Default::default);
static PREFETCH_BATCHES: std::sync::LazyLock<std::sync::Mutex<HashMap<i64, u32>>> = std::sync::LazyLock::new(Default::default);

/// Başlıktan liste kaydı (gövde, ileti açılınca indirilir).
pub(crate) fn header_to_message(h: &EwsHeader, folder: &Folder, account_id: i64) -> Message {
    Message {
        folder_id: folder.id,
        account_id: Some(account_id),
        remote_id: Some(h.id.clone()),
        message_id: h.message_id.clone(),
        subject: h.subject.clone(),
        sender: h.sender.clone(),
        to_addr: h.to.clone(),
        cc: h.cc.clone(),
        date: h.date,
        snippet: crate::mail::make_snippet(&h.preview, 200),
        body_text: h.preview.clone(),
        is_read: h.is_read,
        is_flagged: h.flagged,
        // Gönderilmiş/Silinmiş Öğeler'de "taslak" gösterilmez (EWS ile gönderilen kopyada
        // gönderilmedi bayrağı kalabiliyor; ileti gerçekte gönderildi)
        is_draft: (h.is_draft && !matches!(folder.role.as_deref(), Some("sent") | Some("trash") | Some("outbox"))) || folder.role.as_deref() == Some("drafts"),
        importance: h.importance,
        categories: h.categories.join(","),
        has_attachments: h.has_attachments,
        is_pinned: h.pinned,
        ..Default::default()
    }
}

impl Worker {
    /// EWS istemcisini (önbellekten) alıp işlemi çalıştırır; kimlik hatalarında
    /// hesabı duraklatır.
    pub fn with_ews<T>(&mut self, account_id: i64, f: impl FnOnce(&mut Ews) -> MailResult<T>) -> MailResult<T> {
        let acc = self.db.account(account_id).ok_or_else(|| MailError(t!("error.no_account")))?;
        self.check_paused(&acc)?;
        crate::mail::ews::pooled(&acc, f).map_err(|e| self.note_auth(&acc, e))
    }

    /// Hesap eşitleme: klasör listesi, sonra her klasör ayrı iş olarak (Gelen Kutusu önce),
    /// ardından takvim ve kişiler. Böylece kullanıcının tıkladığı klasör araya girebilir.
    pub fn ews_sync_account(&mut self, account_id: i64) -> MailResult<()> {
        // Arayüz (klasör ağacı ve liste) yalnızca gerçekten bir şey değiştiyse yenilenir
        let before = self.db.total_changes();
        self.ews_sync_folder_list(account_id)?;
        if self.db.total_changes() != before {
            self.emit_changed(None);
        }
        let mut folders: Vec<Folder> = self.db.folders(Some(account_id)).into_iter().filter(|f| f.remote_name.is_some()).collect();
        let rank = |f: &Folder| match f.role.as_deref() {
            Some("inbox") => 0,
            Some("sent") => 1,
            Some("drafts") => 2,
            _ => 3,
        };
        folders.sort_by_key(rank);
        for f in folders {
            let fid = f.id;
            let job = move |w: &mut Worker| {
                let Some(f) = w.db.folder(fid) else { return Ok(()) };
                let before = w.db.total_changes();
                let new = w.ews_sync_folder(account_id, &f)?;
                if !new.is_empty() || w.db.total_changes() != before {
                    w.emit_changed(Some(fid));
                }
                w.emit_new(new);
                Ok(())
            };
            // Gelen Kutusu/Gönderilmiş/Taslaklar ön kuyrukta (hemen), diğerleri arka planda
            if rank(&f) < 3 {
                self.defer_fg(format!("f:{fid}"), job);
            } else {
                self.defer(format!("f:{fid}"), job);
            }
        }
        self.defer(format!("cal:{account_id}"), move |w| w.ews_sync_calendar(account_id));
        self.defer(format!("con:{account_id}"), move |w| w.ews_sync_contacts(account_id));
        if self.db.account(account_id).is_some_and(|a| a.shared_of.is_none()) {
            self.defer(format!("pim:{account_id}"), move |w| w.ews_sync_pim(account_id));
        }
        self.defer(format!("rules:{account_id}"), move |w| {
            let acc = w.db.account(account_id).ok_or_else(|| MailError(t!("error.no_account")))?;
            let email = acc.email.clone();
            let list = w.with_ews(account_id, move |e| e.get_rules(&email))?;
            store_server_rules(&w.db, account_id, list);
            (w.emit)("rules://changed", json!({}));
            Ok(())
        });
        Ok(())
    }

    pub fn ews_sync_folder_list(&mut self, account_id: i64) -> MailResult<()> {
        let remote = self.with_ews(account_id, |e| e.list_folders())?;
        let existing: HashMap<String, Folder> =
            self.db.folders(Some(account_id)).into_iter().filter_map(|f| f.remote_name.clone().map(|r| (r, f))).collect();
        let mut ids: HashMap<String, i64> = HashMap::new();
        let mut seen = HashSet::new();
        for rf in &remote {
            seen.insert(rf.id.clone());
            let id = if let Some(f) = existing.get(&rf.id) {
                let mut f = f.clone();
                f.name = rf.name.clone();
                f.role = rf.role.clone();
                let _ = self.db.update_folder(&f);
                f.id
            } else {
                self.db
                    .insert_folder(&Folder {
                        account_id: Some(account_id),
                        name: rf.name.clone(),
                        remote_name: Some(rf.id.clone()),
                        role: rf.role.clone(),
                        position: 100,
                        ..Default::default()
                    })
                    .unwrap_or(0)
            };
            ids.insert(rf.id.clone(), id);
        }
        for rf in &remote {
            if let Some(mut f) = ids.get(&rf.id).and_then(|id| self.db.folder(*id)) {
                f.parent_id = if f.role.is_some() { None } else { ids.get(&rf.parent_id).copied() };
                let _ = self.db.update_folder(&f);
            }
        }
        for (rid, f) in existing {
            if !seen.contains(&rid) {
                let _ = self.db.delete_folder(f.id);
            }
        }
        Ok(())
    }

    /// Klasör eşitleme.
    /// - Tam tarama bitmemişse: en yeni iletiler hemen listelenir (FindItem), tüm
    ///   başlıkları getiren tarama arka planda sayfa sayfa sürer.
    /// - Tarama bittiyse: yalnızca değişiklikler (SyncFolderItems) alınır.
    /// İleti gövdeleri açılınca ya da arka planda en yeniler için indirilir.
    pub fn ews_sync_folder(&mut self, account_id: i64, folder: &Folder) -> MailResult<Vec<Message>> {
        let Some(rid) = folder.remote_name.clone() else { return Ok(vec![]) };
        let lock = self.folder_lock(folder.id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        // Kilidi beklerken diğer iş parçacığı durumu değiştirmiş olabilir
        let folder = &self.db.folder(folder.id).unwrap_or_else(|| folder.clone());
        let mut new_msgs = Vec::new();
        if !folder.full_sync {
            let first = self.db.total_count(folder.id) == 0;
            let newest = self.db.newest_date(folder.id).unwrap_or(0.0);
            let r = rid.clone();
            let (headers, _) = self.with_ews(account_id, move |e| e.find_items(&r, None, None, if first { 100 } else { 50 }))?;
            // Yalnızca listede bilinen en yeniden daha yeni olanlar "yeni posta" sayılır
            let notify_after = if first { f64::MAX } else { newest - 60.0 };
            new_msgs = self.ews_insert_headers(account_id, folder, &headers, notify_after)?;
            let fid = folder.id;
            if folder.sync_state.is_empty() {
                self.enum_seen.lock().unwrap_or_else(|e| e.into_inner()).entry(fid).or_default();
            }
            self.defer(format!("enum:{fid}"), move |w| w.ews_enumerate(account_id, fid));
        } else {
            let mut state = folder.sync_state.clone();
            for _ in 0..50 {
                let (st, r) = (state.clone(), rid.clone());
                let ch = self.with_ews(account_id, move |e| e.sync_items(&r, &st))?;
                new_msgs.extend(self.ews_apply_changes(account_id, folder, &ch, 0.0)?);
                state = ch.state.clone();
                self.store_state(folder.id, &state, true);
                if ch.done {
                    break;
                }
            }
        }
        let fid = folder.id;
        self.defer(format!("pre:{fid}"), move |w| w.ews_prefetch(account_id, fid));
        // Sabitlenmiş öğeler sunucuyla eşitlenir (OWA'da daha önce sabitlenenler dahil)
        let due = {
            let mut m = PIN_CHECK.lock().unwrap_or_else(|e| e.into_inner());
            let due = m.get(&fid).map_or(true, |t| t.elapsed() > std::time::Duration::from_secs(600));
            if due {
                m.insert(fid, std::time::Instant::now());
            }
            due
        };
        if due {
            self.defer(format!("pins:{fid}"), move |w| w.ews_sync_pins(account_id, fid));
        }
        Ok(new_msgs)
    }

    /// Sunucudaki sabitlenmiş öğeleri yerel listeye yansıtır (sunucu esas alınır)
    pub fn ews_sync_pins(&mut self, account_id: i64, folder_id: i64) -> MailResult<()> {
        let Some(folder) = self.db.folder(folder_id) else { return Ok(()) };
        let Some(rid) = folder.remote_name.clone() else { return Ok(()) };
        let pinned: std::collections::HashSet<String> = self.with_ews(account_id, move |e| e.pinned_ids(&rid))?.into_iter().collect();
        let mut changed = false;
        for (id, remote) in self.db.remote_ids_in_folder(folder_id) {
            let want = pinned.contains(&remote);
            if self.db.message_pinned(id) != want {
                self.db.set_message_field(id, "is_pinned", want as i64);
                changed = true;
            }
        }
        if changed {
            self.emit_changed(Some(folder_id));
        }
        Ok(())
    }

    /// Tam taramanın bir sayfası; bitmediyse kendini kuyruğun sonuna yeniden ekler.
    pub fn ews_enumerate(&mut self, account_id: i64, folder_id: i64) -> MailResult<()> {
        let lock = self.folder_lock(folder_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let Some(folder) = self.db.folder(folder_id) else { return Ok(()) };
        if folder.full_sync {
            return Ok(());
        }
        let Some(rid) = folder.remote_name.clone() else { return Ok(()) };
        let st = folder.sync_state.clone();
        let ch = self.with_ews(account_id, move |e| e.sync_items(&rid, &st))?;
        if let Some(seen) = self.enum_seen.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&folder_id) {
            seen.extend(ch.creates.iter().map(|h| h.id.clone()));
            seen.extend(ch.updates.iter().map(|u| u.0.clone()));
        }
        // Tarama sırasında gelenler zaten hızlı denetimle bildiriliyor
        self.ews_apply_changes(account_id, &folder, &ch, f64::MAX)?;
        if ch.done {
            // Taramada görülmeyen yerel öğeler sunucuda silinmiştir
            let seen = self.enum_seen.lock().unwrap_or_else(|e| e.into_inner()).remove(&folder_id);
            if let Some(seen) = seen {
                for (id, remote) in self.db.remote_ids_in_folder(folder_id) {
                    if !seen.contains(&remote) {
                        self.db.delete_message(id);
                    }
                }
            }
        }
        self.store_state(folder_id, &ch.state, ch.done);
        // Arayüz listesi her sayfada değil, birkaç sayfada bir yenilenir (titreme ve
        // gereksiz yeniden yükleme olmaz)
        let pages = {
            let mut m = ENUM_PAGES.lock().unwrap_or_else(|e| e.into_inner());
            let n = m.entry(folder_id).or_insert(0);
            *n += 1;
            *n
        };
        if ch.done || pages % 4 == 1 {
            self.emit_changed(Some(folder_id));
        }
        if !ch.done {
            self.defer(format!("enum:{folder_id}"), move |w| w.ews_enumerate(account_id, folder_id));
        }
        Ok(())
    }

    /// Değişiklikleri uygular; `notify_after` zamanından yeni okunmamış Gelen Kutusu
    /// iletileri bildirim için döndürülür.
    fn ews_apply_changes(&mut self, account_id: i64, folder: &Folder, ch: &crate::mail::ews::SyncChanges, notify_after: f64) -> MailResult<Vec<Message>> {
        for (id, read, flag, pinned) in &ch.updates {
            if let Some(mid) = self.db.message_by_remote(folder.id, id) {
                if let Some(r) = read {
                    self.db.set_message_field(mid, "is_read", *r as i64);
                }
                if let Some(f) = flag {
                    self.db.set_message_field(mid, "is_flagged", *f as i64);
                }
                if let Some(p) = pinned {
                    self.db.set_message_field(mid, "is_pinned", *p as i64);
                }
            }
        }
        for id in &ch.deletes {
            if let Some(mid) = self.db.message_by_remote(folder.id, id) {
                self.db.delete_message(mid);
            }
        }
        self.ews_insert_headers(account_id, folder, &ch.creates, notify_after)
    }

    /// Başlıkları (yoksa) ekler.
    pub(crate) fn ews_insert_headers(&mut self, account_id: i64, folder: &Folder, headers: &[EwsHeader], notify_after: f64) -> MailResult<Vec<Message>> {
        let is_inbox = folder.role.as_deref() == Some("inbox");
        let rules = if is_inbox { self.db.rules(true) } else { vec![] };
        let mut new_msgs = Vec::new();
        for h in headers {
            if self.db.message_by_remote(folder.id, &h.id).is_some() {
                continue;
            }
            for c in &h.categories {
                self.db.ensure_category(c);
            }
            let mut m = header_to_message(h, folder, account_id);
            let Ok(id) = self.db.insert_message(&m) else { continue };
            m.id = id;
            if is_inbox && !m.is_read && m.date > notify_after {
                let notify = if rules.is_empty() { true } else { self.ews_apply_rules(&rules, &m, account_id)? };
                if notify {
                    new_msgs.push(m);
                }
            }
        }
        Ok(new_msgs)
    }

    /// En yeni iletilerin gövdelerini arka planda indirir (çevrimdışı okuma ve gövdede arama için).
    pub fn ews_prefetch(&mut self, account_id: i64, folder_id: i64) -> MailResult<()> {
        let limit = self.db.account(account_id).map(|a| a.sync_limit.max(0) as usize).unwrap_or(500);
        let min_date = if limit == 0 { 0.0 } else { self.db.nth_newest_date(folder_id, limit).unwrap_or(0.0) };
        let batch: Vec<(i64, String)> = self.db.messages_without_content(folder_id, 20).into_iter().filter(|(id, _)| self.db.message_row(*id).map(|m| m.date >= min_date).unwrap_or(false)).collect();
        if batch.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = batch.iter().map(|b| b.1.clone()).collect();
        let items = self.with_ews(account_id, move |e| e.get_messages(&ids))?;
        for it in items {
            if let Some((mid, _)) = batch.iter().find(|b| b.1 == it.id) {
                let m = parse::parse(&it.mime).into_message();
                self.db.update_message_content(*mid, &m);
            }
        }
        // Arayüz her 20 gövdede değil, zincir bitince ve arada beş partide bir yenilenir
        // (önizleme metni ve ek simgesi güncellensin, ama liste sürekli yeniden yüklenmesin)
        let n = {
            let mut m = PREFETCH_BATCHES.lock().unwrap_or_else(|e| e.into_inner());
            let n = m.entry(folder_id).or_insert(0);
            *n += 1;
            *n
        };
        if batch.len() < 20 || n % 5 == 1 {
            self.emit_changed(Some(folder_id));
        }
        if batch.len() == 20 {
            self.defer(format!("pre:{folder_id}"), move |w| w.ews_prefetch(account_id, folder_id));
        } else {
            PREFETCH_BATCHES.lock().unwrap_or_else(|e| e.into_inner()).remove(&folder_id);
        }
        Ok(())
    }

    /// Liste sonuna gelindiğinde: en eski yerel iletiden daha eski olanları getirir.
    pub fn ews_load_older(&mut self, account_id: i64, folder_id: i64) -> MailResult<()> {
        let Some(folder) = self.db.folder(folder_id) else { return Ok(()) };
        let Some(rid) = folder.remote_name.clone() else { return Ok(()) };
        let before = self.db.oldest_date(folder_id).unwrap_or(crate::db::now() + 86400.0);
        let (headers, more) = self.with_ews(account_id, move |e| e.find_items(&rid, Some(before), None, 100))?;
        self.ews_insert_headers(account_id, &folder, &headers, f64::MAX)?;
        self.emit_changed(Some(folder_id));
        (self.emit)("mail://older", json!({"folder": folder_id, "more": more && !headers.is_empty()}));
        Ok(())
    }

    fn store_state(&self, folder_id: i64, state: &str, full: bool) {
        if let Some(mut f) = self.db.folder(folder_id) {
            f.sync_state = state.to_string();
            f.full_sync = full;
            let _ = self.db.update_folder(&f);
        }
    }

    fn ews_apply_rules(&mut self, rules: &[crate::db::Rule], m: &Message, account_id: i64) -> MailResult<bool> {
        let mut notify = true;
        let rid = m.remote_id.clone().unwrap_or_default();
        let me = self.db.my_addresses();
        for (action, arg) in crate::rules::evaluate(rules, m, Some(account_id), &me) {
            match action.as_str() {
                "mark_read" => {
                    self.db.set_message_field(m.id, "is_read", 1);
                    let ids = vec![rid.clone()];
                    self.with_ews(account_id, move |e| e.set_read(&ids, true))?;
                    notify = false;
                }
                "mark_unread" => {
                    self.db.set_message_field(m.id, "is_read", 0);
                    let ids = vec![rid.clone()];
                    self.with_ews(account_id, move |e| e.set_read(&ids, false))?;
                }
                "flag" => {
                    self.db.set_message_field(m.id, "is_flagged", 1);
                    let ids = vec![rid.clone()];
                    self.with_ews(account_id, move |e| e.set_flag(&ids, true))?;
                }
                "unflag" => {
                    self.db.set_message_field(m.id, "is_flagged", 0);
                    let ids = vec![rid.clone()];
                    self.with_ews(account_id, move |e| e.set_flag(&ids, false))?;
                }
                "important" => self.db.set_message_field(m.id, "importance", 2),
                "importance" => self.db.set_message_field(m.id, "importance", crate::rules::importance_level(&arg)),
                "permanent_delete" => {
                    let ids = vec![rid.clone()];
                    self.with_ews(account_id, move |e| e.delete_items(&ids, true))?;
                    self.db.delete_message(m.id);
                    return Ok(false);
                }
                "desktop_notify" => (self.emit)("mail://rule-notify", json!({ "title": arg, "body": m.subject })),
                "category" => {
                    let mut cats = crate::db::split_list(&m.categories);
                    if !cats.contains(&arg) {
                        cats.push(arg.clone());
                    }
                    self.db.set_message_field(m.id, "categories", cats.join(","));
                    let ids = vec![rid.clone()];
                    self.with_ews(account_id, move |e| e.set_categories(&ids, &cats))?;
                }
                "notify" => notify = false,
                "move" | "delete" | "junk" => {
                    let dest = if action == "move" {
                        arg.parse::<i64>().ok().and_then(|id| self.db.folder(id))
                    } else {
                        self.db.folder_by_role_or_local(Some(account_id), if action == "delete" { "trash" } else { "junk" })
                    };
                    if let Some(dest) = dest.filter(|d| d.account_id == Some(account_id) && d.remote_name.is_some()) {
                        let ids = vec![rid.clone()];
                        let to = dest.remote_name.clone().unwrap();
                        self.with_ews(account_id, move |e| e.move_items(&ids, &to))?;
                        self.db.delete_message(m.id);
                        return Ok(false);
                    }
                }
                _ => {}
            }
        }
        Ok(notify)
    }

    /// Exchange takvimini yerel takvime yansıtır (sunucu esas alınır).
    pub fn ews_sync_calendar(&mut self, account_id: i64) -> MailResult<()> {
        let acc = self.db.account(account_id).ok_or_else(|| MailError(t!("error.no_account")))?;
        let cal = match self.db.calendar_for_account(account_id) {
            Some(c) => c,
            None => {
                let id = self.db.save_calendar(&Calendar { id: 0, name: acc.email.clone(), color: "#0f6cbd".into(), visible: true, account_id: Some(account_id), ..Default::default() }).map_err(|e| MailError(e.to_string()))?;
                self.db.calendars().into_iter().find(|c| c.id == id).unwrap()
            }
        };
        let now = crate::db::now();
        let events = self.with_ews(account_id, move |e| e.calendar_view(now - 60.0 * 86400.0, now + 400.0 * 86400.0))?;
        let local: HashMap<String, crate::db::Event> =
            self.db.events_in_calendar(cal.id).into_iter().filter_map(|e| e.remote_id.clone().map(|r| (r, e))).collect();
        let mut seen = HashSet::new();
        let mut changed = false;
        for mut e in events {
            let rid = e.remote_id.clone().unwrap_or_default();
            seen.insert(rid.clone());
            e.calendar_id = cal.id;
            if let Some(old) = local.get(&rid) {
                e.id = old.id;
                // Değişmeyen etkinlik yeniden yazılmaz
                if *old == e {
                    continue;
                }
            }
            let _ = self.db.save_event(&e);
            changed = true;
        }
        for (rid, e) in local {
            if !seen.contains(&rid) {
                self.db.delete_event(e.id);
                changed = true;
            }
        }
        // Takvim yalnızca bir şey değiştiyse yeniden yüklenir
        if changed {
            (self.emit)("calendar://changed", json!({}));
        }
        Ok(())
    }

    pub fn ews_sync_contacts(&mut self, account_id: i64) -> MailResult<()> {
        let list = self.with_ews(account_id, |e| e.contacts())?;
        let local: HashMap<String, crate::db::Contact> =
            self.db.contacts_of_account(account_id).into_iter().filter_map(|c| c.remote_id.clone().map(|r| (r, c))).collect();
        let mut seen = HashSet::new();
        for mut c in list {
            let rid = c.remote_id.clone().unwrap_or_default();
            seen.insert(rid.clone());
            c.account_id = Some(account_id);
            if let Some(old) = local.get(&rid) {
                c.id = old.id;
                c.is_favorite = old.is_favorite;
                c.notes = old.notes.clone();
                c.categories = old.categories.clone();
            }
            let _ = self.db.save_contact(&c);
        }
        for (rid, c) in local {
            if !seen.contains(&rid) {
                self.db.delete_contact(c.id);
            }
        }
        (self.emit)("contacts://changed", json!({}));
        Ok(())
    }
}
