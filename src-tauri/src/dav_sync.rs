//! CalDAV takvimleri ve CardDAV adres defterlerinin çift yönlü eşitlenmesi.
//!
//! `pim_sync.rs` ile aynı düzen: yerel değişiklik önce "kirli" olarak yazılır, sonra sunucuya
//! gönderilir; gönderilemezse sonraki eşitlemede yeniden denenir. Silinenler mezar taşıyla tutulur.
//! Çakışmada son yazan kazanır. Sunucudaki nesnenin yalnızca değişen özellikleri güncellenir, bu
//! yüzden uygulamanın modelinde karşılığı olmayan alanlar kaybolmaz (bkz. `ical::merge_changes`).

use crate::db::{Account, Calendar, Contact, Event};
use crate::ical;
use crate::mail::dav::{Dav, Entry, Kind, Precond, Put};
use crate::mail::{MailError, MailResult};
use crate::pim_sync::quiet;
use crate::sync::{MailService, Worker};
use serde_json::json;
use std::collections::HashSet;

pub const EVENT_TOMBS: &str = "dav_events";
pub const CONTACT_TOMBS: &str = "dav_contacts";
/// Tek istekte istenen kaynak sayısı
const BATCH: usize = 50;
const DEFAULT_COLOR: &str = "#3a7bd5";

fn ctag_key(href: &str) -> String {
    format!("dav_ctag:{href}")
}

/// Yazar; sunucudaki sürüm değişmişse son yazan kazanır
fn put(dav: &mut Dav, href: &str, kind: Kind, body: &str, etag: &str, is_new: bool) -> MailResult<String> {
    let cond = if is_new {
        Precond::New
    } else if etag.is_empty() {
        Precond::Any
    } else {
        Precond::Match(etag)
    };
    match dav.put(href, kind, body, cond)? {
        Put::Saved(e) => Ok(e),
        Put::Conflict => match dav.put(href, kind, body, Precond::Any)? {
            Put::Saved(e) => Ok(e),
            Put::Conflict => Err(MailError(crate::t!("error.dav_status", 412))),
        },
    }
}

impl MailService {
    /// Etkinlik CalDAV sunucusuna yazılır; gönderilemezse sonraki eşitlemede denenir
    pub fn dav_push_event(&self, account_id: i64, event_id: i64) {
        self.db.mark_event_dirty(event_id);
        self.submit_keyed(format!("dav-event:{event_id}"), false, String::new(), move |w| {
            quiet(w.dav_put_event(account_id, event_id))?;
            (w.emit)("calendar://changed", json!({}));
            Ok(())
        });
    }

    pub fn dav_push_contact(&self, account_id: i64, contact_id: i64) {
        self.db.mark_contact_dirty(contact_id);
        self.submit_keyed(format!("dav-contact:{contact_id}"), false, String::new(), move |w| {
            quiet(w.dav_put_contact(account_id, contact_id))?;
            (w.emit)("contacts://changed", json!({}));
            Ok(())
        });
    }

    /// Mezar taşına yazılan kaynakları sunucudan siler
    pub fn dav_flush(&self, kind: &'static str, account_id: i64) {
        self.submit_keyed(format!("dav-tomb:{kind}:{account_id}"), false, String::new(), move |w| {
            let Some(acc) = w.db.account(account_id) else { return Ok(()) };
            let url = if kind == EVENT_TOMBS { acc.caldav_url.clone() } else { acc.carddav_url.clone() };
            let mut dav = Dav::for_account(&acc, &url, None)?;
            w.dav_flush_tombstones(&mut dav, kind, account_id);
            Ok(())
        });
    }
}

impl Worker {
    pub fn dav_sync_account(&mut self, account_id: i64) -> MailResult<()> {
        let Some(acc) = self.db.account(account_id) else { return Ok(()) };
        if !acc.caldav_url.trim().is_empty() {
            self.dav_sync_calendars(&acc)?;
        }
        if !acc.carddav_url.trim().is_empty() {
            self.dav_sync_contacts(&acc)?;
        }
        Ok(())
    }

    fn dav_flush_tombstones(&mut self, dav: &mut Dav, kind: &str, account_id: i64) -> bool {
        let tombs = self.db.tombstones(kind, account_id);
        let left: Vec<String> = tombs.iter().filter(|h| dav.delete(h).is_err()).cloned().collect();
        if left.len() != tombs.len() {
            self.db.set_tombstones(kind, account_id, &left);
        }
        left.len() != tombs.len()
    }

    // ------------------------------------------------------------------ takvimler
    fn dav_sync_calendars(&mut self, acc: &Account) -> MailResult<()> {
        let mut dav = Dav::for_account(acc, &acc.caldav_url, None)?;
        let remote = dav.discover(Kind::Calendar)?;
        let local: Vec<Calendar> = self.db.calendars().into_iter().filter(|c| c.account_id == Some(acc.id) && c.kind == "dav").collect();
        // Sunucudan silinen takvim yerelden de kalkar
        for c in &local {
            if !remote.iter().any(|r| c.remote_id.as_deref() == Some(r.href.as_str())) {
                self.db.delete_calendar(c.id);
            }
        }
        let mut changed = self.dav_flush_tombstones(&mut dav, EVENT_TOMBS, acc.id);
        for r in &remote {
            let id = match local.iter().find(|c| c.remote_id.as_deref() == Some(r.href.as_str())) {
                Some(c) => c.id,
                None => {
                    changed = true;
                    self.db
                        .save_calendar(&Calendar {
                            name: if r.name.is_empty() { acc.email.clone() } else { r.name.clone() },
                            color: if r.color.is_empty() { DEFAULT_COLOR.into() } else { r.color.clone() },
                            visible: true,
                            account_id: Some(acc.id),
                            kind: "dav".into(),
                            remote_id: Some(r.href.clone()),
                            ..Default::default()
                        })
                        .map_err(|e| MailError(e.to_string()))?
                }
            };
            changed |= self.dav_sync_calendar(&mut dav, acc.id, id, &r.href)?;
        }
        if changed {
            (self.emit)("calendar://changed", json!({}));
        }
        Ok(())
    }

    fn dav_sync_calendar(&mut self, dav: &mut Dav, account_id: i64, calendar_id: i64, href: &str) -> MailResult<bool> {
        for e in self.db.dirty_events(calendar_id) {
            self.dav_put_event_with(dav, href, &e)?;
        }
        let ctag = dav.ctag(href, Kind::Calendar)?;
        if !ctag.is_empty() && self.db.setting(&ctag_key(href)).as_deref() == Some(ctag.as_str()) {
            return Ok(false);
        }
        let remote = dav.list(href, Kind::Calendar)?;
        let index = self.db.dav_events_index(calendar_id);
        let dead: HashSet<String> = self.db.tombstones(EVENT_TOMBS, account_id).into_iter().collect();
        let seen: HashSet<&str> = remote.iter().map(|r| r.href.as_str()).collect();
        let mut changed = false;
        for (h, row) in &index {
            if !seen.contains(h.as_str()) && !row.dirty {
                self.db.delete_event(row.id);
                self.db.delete_event_overrides(calendar_id, h);
                changed = true;
            }
        }
        let stale: Vec<String> = remote
            .iter()
            .filter(|r| !dead.contains(&r.href) && index.get(&r.href).map_or(true, |x| !x.dirty && x.etag != r.etag))
            .map(|r| r.href.clone())
            .collect();
        for chunk in stale.chunks(BATCH) {
            for item in dav.fetch(href, Kind::Calendar, chunk)? {
                changed |= self.dav_store_event(calendar_id, &item);
            }
        }
        if !ctag.is_empty() {
            self.db.set_setting(&ctag_key(href), &ctag);
        }
        Ok(changed)
    }

    /// Sunucudaki kaynağı yerele yazar. Taşınmış tek oluşumlar ayrı satır olur (salt okunur:
    /// `remote_id` taşımaz, `master_id` serinin adresidir) ve seride o gün atlanır.
    fn dav_store_event(&mut self, calendar_id: i64, item: &Entry) -> bool {
        let Some((mut e, overrides)) = ical::dav_event(&item.data) else { return false };
        e.id = self.db.event_by_remote(calendar_id, &item.href).unwrap_or(0);
        e.calendar_id = calendar_id;
        e.remote_id = Some(item.href.clone());
        let Ok(id) = self.db.save_event(&e) else { return false };
        self.db.set_event_dav(id, &item.etag, &item.data);
        self.db.delete_event_overrides(calendar_id, &item.href);
        for mut o in overrides {
            o.id = 0;
            o.calendar_id = calendar_id;
            o.master_id = Some(item.href.clone());
            let _ = self.db.save_event(&o);
        }
        true
    }

    pub fn dav_put_event(&mut self, account_id: i64, event_id: i64) -> MailResult<()> {
        let (Some(acc), Some(e)) = (self.db.account(account_id), self.db.event(event_id)) else { return Ok(()) };
        let collection = self.db.calendars().into_iter().find(|c| c.id == e.calendar_id).and_then(|c| c.remote_id);
        let Some(collection) = collection else { return Ok(()) };
        let mut dav = Dav::for_account(&acc, &acc.caldav_url, None)?;
        self.dav_put_event_with(&mut dav, &collection, &e)
    }

    fn dav_put_event_with(&mut self, dav: &mut Dav, collection: &str, e: &Event) -> MailResult<()> {
        let state = self.db.event_dav(e.id);
        let body = ical::dav_event_body(e, &state.raw);
        let href = e.remote_id.clone().unwrap_or_else(|| Dav::new_href(collection, Kind::Calendar));
        let etag = put(dav, &href, Kind::Calendar, &body, &state.etag, e.remote_id.is_none())?;
        self.db.set_event_remote(e.id, &href);
        self.db.set_event_dav(e.id, &etag, &body);
        Ok(())
    }

    // ------------------------------------------------------------------ kişiler
    fn dav_sync_contacts(&mut self, acc: &Account) -> MailResult<()> {
        let mut dav = Dav::for_account(acc, &acc.carddav_url, None)?;
        let books = dav.discover(Kind::Contacts)?;
        let mut changed = self.dav_flush_tombstones(&mut dav, CONTACT_TOMBS, acc.id);
        // Yeni kişiler ilk adres defterine yazılır
        let first = books.first().map(|b| b.href.clone()).unwrap_or_default();
        for c in self.db.dirty_contacts(acc.id) {
            self.dav_put_contact_with(&mut dav, &first, &c)?;
        }
        for b in &books {
            changed |= self.dav_sync_book(&mut dav, acc.id, &b.href)?;
        }
        if changed {
            (self.emit)("contacts://changed", json!({}));
        }
        Ok(())
    }

    fn dav_sync_book(&mut self, dav: &mut Dav, account_id: i64, href: &str) -> MailResult<bool> {
        let ctag = dav.ctag(href, Kind::Contacts)?;
        if !ctag.is_empty() && self.db.setting(&ctag_key(href)).as_deref() == Some(ctag.as_str()) {
            return Ok(false);
        }
        let remote = dav.list(href, Kind::Contacts)?;
        let index = self.db.dav_contacts_index(account_id);
        let dead: HashSet<String> = self.db.tombstones(CONTACT_TOMBS, account_id).into_iter().collect();
        let seen: HashSet<&str> = remote.iter().map(|r| r.href.as_str()).collect();
        let mut changed = false;
        // Adres defteri birden çok olabilir: yalnızca bu defterin kişileri silinir
        for (h, row) in index.iter().filter(|(h, _)| h.starts_with(href)) {
            if !seen.contains(h.as_str()) && !row.dirty {
                self.db.delete_contact(row.id);
                changed = true;
            }
        }
        let stale: Vec<String> = remote
            .iter()
            .filter(|r| !dead.contains(&r.href) && index.get(&r.href).map_or(true, |x| !x.dirty && x.etag != r.etag))
            .map(|r| r.href.clone())
            .collect();
        for chunk in stale.chunks(BATCH) {
            for item in dav.fetch(href, Kind::Contacts, chunk)? {
                let Some(mut c) = ical::parse_vcf(&item.data).into_iter().next() else { continue };
                c.id = index.get(&item.href).map_or(0, |x| x.id);
                c.account_id = Some(account_id);
                c.remote_id = Some(item.href.clone());
                if let Ok(id) = self.db.save_contact(&c) {
                    self.db.set_contact_dav(id, &item.etag, &item.data);
                    changed = true;
                }
            }
        }
        if !ctag.is_empty() {
            self.db.set_setting(&ctag_key(href), &ctag);
        }
        Ok(changed)
    }

    pub fn dav_put_contact(&mut self, account_id: i64, contact_id: i64) -> MailResult<()> {
        let (Some(acc), Some(c)) = (self.db.account(account_id), self.db.contact(contact_id)) else { return Ok(()) };
        let mut dav = Dav::for_account(&acc, &acc.carddav_url, None)?;
        let book = if c.remote_id.is_some() { String::new() } else { dav.discover(Kind::Contacts)?.first().map(|b| b.href.clone()).unwrap_or_default() };
        self.dav_put_contact_with(&mut dav, &book, &c)
    }

    fn dav_put_contact_with(&mut self, dav: &mut Dav, book: &str, c: &Contact) -> MailResult<()> {
        let state = self.db.contact_dav(c.id);
        let body = ical::dav_contact_body(c, &state.raw);
        let href = c.remote_id.clone().unwrap_or_else(|| Dav::new_href(book, Kind::Contacts));
        let etag = put(dav, &href, Kind::Contacts, &body, &state.etag, c.remote_id.is_none())?;
        self.db.set_contact_remote(c.id, &href);
        self.db.set_contact_dav(c.id, &etag, &body);
        Ok(())
    }
}
