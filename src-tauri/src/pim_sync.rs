//! Görev, not ve kişi gruplarının Exchange ile çift yönlü eşitlenmesi.
//!
//! Yerel değişiklik önce yerele "kirli" olarak yazılır, sonra sunucuya gönderilir; gönderilemezse
//! sonraki eşitleme turunda yeniden denenir. Silinen öğeler mezar taşıyla tutulur (sunucudan
//! kalkana kadar yerelde diriltilmez). Çakışmada son yazan kazanır: kirli yerel kayıt, sunucudaki
//! sürümün üzerine yazılır. Takip bayraklı iletilerin görevleri iletiye bağlıdır, eşitlenmez.

use crate::db::{ContactGroup, Note, Task};
use crate::mail::MailResult;
use crate::sync::{MailService, Worker};
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// Arka plan gönderimi: bağlantı yoksa kayıt kirli kalır, sonraki eşitlemede denenir
pub(crate) fn quiet(r: MailResult<()>) -> MailResult<()> {
    match r {
        Err(e) if e.is_net() => Ok(()),
        other => other,
    }
}

impl MailService {
    pub fn push_task(&self, id: i64) {
        self.submit_keyed(format!("task:{id}"), false, String::new(), move |w| quiet(w.ews_push_task(id)));
    }

    pub fn push_note(&self, id: i64) {
        self.submit_keyed(format!("note:{id}"), false, String::new(), move |w| quiet(w.ews_push_note(id)));
    }

    pub fn push_group(&self, id: i64) {
        self.submit_keyed(format!("group:{id}"), false, String::new(), move |w| quiet(w.ews_push_group(id)));
    }

    /// Sunucudan silinecekler mezar taşına yazıldıktan sonra çağrılır
    pub fn flush_deleted(&self, kind: &'static str, account_id: i64) {
        self.submit_keyed(format!("tomb:{kind}:{account_id}"), false, String::new(), move |w| {
            w.flush_tombstones(kind, account_id);
            Ok(())
        });
    }
}

fn same_task(a: &Task, b: &Task) -> bool {
    a.title == b.title
        && a.notes == b.notes
        && a.status == b.status
        && a.percent == b.percent
        && a.priority == b.priority
        && a.category == b.category
        && a.start_date == b.start_date
        && a.due_date == b.due_date
        && a.reminder == b.reminder
}

/// Sunucuda artık bulunmayan öğe: güncelleme yerine yeniden oluşturulur
fn is_gone(message: &str) -> bool {
    message.contains("ErrorItemNotFound")
}

impl Worker {
    /// Hesabın görev, not ve kişi gruplarını eşitler
    pub fn ews_sync_pim(&mut self, account_id: i64) -> MailResult<()> {
        self.ews_sync_tasks(account_id)?;
        self.ews_sync_notes(account_id)?;
        self.ews_sync_groups(account_id)
    }

    /// Silinemeyenler bırakılır, kalanların kimlikleri döner (eşitlemede diriltilmezler)
    pub fn flush_tombstones(&mut self, kind: &str, account_id: i64) -> Vec<String> {
        let tombs = self.db.tombstones(kind, account_id);
        let mut left = Vec::new();
        for rid in &tombs {
            let ids = vec![rid.clone()];
            match self.with_ews(account_id, move |e| e.delete_items(&ids, false)) {
                Ok(()) => {}
                Err(e) if is_gone(e.message()) => {}
                Err(_) => left.push(rid.clone()),
            }
        }
        if left.len() != tombs.len() {
            self.db.set_tombstones(kind, account_id, &left);
        }
        left
    }

    // ------------------------------------------------------------------ görevler
    pub fn ews_push_task(&mut self, id: i64) -> MailResult<()> {
        let Some(t) = self.db.task(id).filter(|t| t.dirty) else { return Ok(()) };
        let Some(account_id) = t.account_id else { return Ok(()) };
        let sent = t.clone();
        let rid = self.with_ews(account_id, move |e| match &sent.remote_id {
            Some(rid) => match e.update_task(rid, &sent) {
                Ok(()) => Ok(rid.clone()),
                Err(x) if is_gone(x.message()) => e.create_task(&sent),
                Err(x) => Err(x),
            },
            None => e.create_task(&sent),
        })?;
        self.db.mark_task_synced(&t, &rid);
        Ok(())
    }

    fn ews_sync_tasks(&mut self, account_id: i64) -> MailResult<()> {
        let dead = self.flush_tombstones("tasks", account_id);
        let mine = |w: &Worker| -> Vec<Task> { w.db.tasks().into_iter().filter(|t| t.account_id == Some(account_id)).collect() };
        for t in mine(self).iter().filter(|t| t.dirty) {
            if let Err(e) = self.ews_push_task(t.id) {
                if e.is_net() {
                    return Err(e);
                }
            }
        }
        let remote = self.with_ews(account_id, |e| e.tasks())?;
        let local: HashMap<String, Task> = mine(self).into_iter().filter_map(|t| t.remote_id.clone().map(|r| (r, t))).collect();
        let mut seen = HashSet::new();
        let mut changed = false;
        for mut t in remote {
            let Some(rid) = t.remote_id.clone() else { continue };
            if dead.contains(&rid) {
                continue;
            }
            seen.insert(rid.clone());
            t.account_id = Some(account_id);
            if let Some(old) = local.get(&rid) {
                if old.dirty || same_task(old, &t) {
                    continue;
                }
                t.id = old.id;
                t.reminder_done = old.reminder_done;
            }
            let _ = self.db.save_task(&t);
            changed = true;
        }
        for (rid, t) in &local {
            if !t.dirty && !seen.contains(rid) {
                self.db.delete_task(t.id);
                changed = true;
            }
        }
        if changed {
            (self.emit)("pim://changed", json!("tasks"));
        }
        Ok(())
    }

    // ------------------------------------------------------------------ notlar
    pub fn ews_push_note(&mut self, id: i64) -> MailResult<()> {
        let Some(n) = self.db.note(id).filter(|n| n.dirty) else { return Ok(()) };
        let Some(account_id) = n.account_id else { return Ok(()) };
        let sent = n.clone();
        let rid = self.with_ews(account_id, move |e| match &sent.remote_id {
            Some(rid) => match e.update_note(rid, &sent.body) {
                Ok(()) => Ok(rid.clone()),
                Err(x) if is_gone(x.message()) => e.create_note(&sent.body),
                Err(x) => Err(x),
            },
            None => e.create_note(&sent.body),
        })?;
        // Gönderim sürerken kullanıcı yazmaya devam ettiyse yeni metin kirli kalır
        self.db.mark_note_synced(id, &rid, &n.body);
        Ok(())
    }

    fn ews_sync_notes(&mut self, account_id: i64) -> MailResult<()> {
        let dead = self.flush_tombstones("notes", account_id);
        let mine = |w: &Worker| -> Vec<Note> { w.db.notes().into_iter().filter(|n| n.account_id == Some(account_id)).collect() };
        for n in mine(self).iter().filter(|n| n.dirty) {
            if let Err(e) = self.ews_push_note(n.id) {
                if e.is_net() {
                    return Err(e);
                }
            }
        }
        let remote = self.with_ews(account_id, |e| e.notes())?;
        let local: HashMap<String, Note> = mine(self).into_iter().filter_map(|n| n.remote_id.clone().map(|r| (r, n))).collect();
        let default_color = self.db.setting_or("note_color", "yellow");
        let mut seen = HashSet::new();
        let mut changed = false;
        for mut n in remote {
            let Some(rid) = n.remote_id.clone() else { continue };
            if dead.contains(&rid) {
                continue;
            }
            seen.insert(rid.clone());
            n.account_id = Some(account_id);
            match local.get(&rid) {
                Some(old) if old.dirty || old.body == n.body => continue,
                // Not rengi sunucuda tutulmaz: yalnızca bu bilgisayarda kalır
                Some(old) => {
                    n.id = old.id;
                    n.color = old.color.clone();
                    n.category = old.category.clone();
                }
                None => n.color = default_color.clone(),
            }
            let _ = self.db.save_note(&n);
            changed = true;
        }
        for (rid, n) in &local {
            if !n.dirty && !seen.contains(rid) {
                self.db.delete_note(n.id);
                changed = true;
            }
        }
        if changed {
            (self.emit)("pim://changed", json!("notes"));
        }
        Ok(())
    }

    // ------------------------------------------------------------------ kişi grupları
    pub fn ews_push_group(&mut self, id: i64) -> MailResult<()> {
        let Some(g) = self.db.group(id).filter(|g| g.dirty) else { return Ok(()) };
        let Some(account_id) = g.account_id else { return Ok(()) };
        let sent = g.clone();
        let rid = self.with_ews(account_id, move |e| match &sent.remote_id {
            Some(rid) => match e.update_group(rid, &sent) {
                Ok(()) => Ok(rid.clone()),
                Err(x) if is_gone(x.message()) => e.create_group(&sent),
                Err(x) => Err(x),
            },
            None => e.create_group(&sent),
        })?;
        self.db.mark_group_synced(id, &rid);
        Ok(())
    }

    fn ews_sync_groups(&mut self, account_id: i64) -> MailResult<()> {
        let dead = self.flush_tombstones("groups", account_id);
        let mine = |w: &Worker| -> Vec<ContactGroup> { w.db.groups().into_iter().filter(|g| g.account_id == Some(account_id)).collect() };
        for g in mine(self).iter().filter(|g| g.dirty) {
            if let Err(e) = self.ews_push_group(g.id) {
                if e.is_net() {
                    return Err(e);
                }
            }
        }
        let remote = self.with_ews(account_id, |e| e.contact_groups())?;
        let local: HashMap<String, ContactGroup> = mine(self).into_iter().filter_map(|g| g.remote_id.clone().map(|r| (r, g))).collect();
        let mut seen = HashSet::new();
        let mut changed = false;
        for mut g in remote {
            let Some(rid) = g.remote_id.clone() else { continue };
            if dead.contains(&rid) {
                continue;
            }
            seen.insert(rid.clone());
            g.account_id = Some(account_id);
            if let Some(old) = local.get(&rid) {
                if old.dirty || (old.name == g.name && old.members == g.members) {
                    continue;
                }
                g.id = old.id;
            }
            let _ = self.db.save_group(&g);
            changed = true;
        }
        for (rid, g) in &local {
            if !g.dirty && !seen.contains(rid) {
                self.db.delete_group(g.id);
                changed = true;
            }
        }
        if changed {
            (self.emit)("pim://changed", json!("contacts"));
        }
        Ok(())
    }
}
