//! Gerçek bir CalDAV/CardDAV sunucusuna karşı uçtan uca eşitleme testi.
//!
//! Varsayılan olarak atlanır. Çalıştırmak için yerel bir sunucu (ör. Radicale, htpasswd kimlik
//! doğrulamasıyla) başlatıp şu değişkenleri verin:
//!
//! ```sh
//! LYNORA_E2E_DAV=http://127.0.0.1:5232/ LYNORA_E2E_USER=ali LYNORA_E2E_PASS=parola1 \
//! LYNORA_NO_KEYRING=1 LYNORA_DATA_DIR=/tmp/lynora-dav cargo test --test e2e_dav -- --ignored --nocapture
//! ```
//!
//! "Başka cihaz" düz HTTP istekleriyle, uygulama ise gerçek eşitleme yoluyla çalışır.

use lynora_lib::db::{Account, Contact, Db, Event};
use lynora_lib::mail::dav::{Dav, Kind};
use lynora_lib::mail::http::{Auth, Client};
use lynora_lib::sync::MailService;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SERIES: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Baska//TR\r\nBEGIN:VEVENT\r\nUID:seri-1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20260105T070000Z\r\nDTEND:20260105T080000Z\r\nSUMMARY:Haftalık\r\nRRULE:FREQ=MONTHLY;BYDAY=2MO;COUNT=6\r\nX-OZEL:korunmali\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:seri-1\r\nDTSTAMP:20260101T000000Z\r\nRECURRENCE-ID:20260209T070000Z\r\nDTSTART:20260210T090000Z\r\nDTEND:20260210T100000Z\r\nSUMMARY:Taşınan\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const CARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:kisi-1\r\nN:Yılmaz;Ali;;;\r\nFN:Ali Yılmaz\r\nEMAIL;TYPE=INTERNET:ali@x.com\r\nTEL;TYPE=FAX:333\r\nEND:VCARD\r\n";
const MK_BOOK: &str = r#"<?xml version="1.0"?><d:mkcol xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav"><d:set><d:prop><d:resourcetype><d:collection/><c:addressbook/></d:resourcetype><d:displayname>Rehber</d:displayname></d:prop></d:set></d:mkcol>"#;
const MK_CAL: &str = r#"<?xml version="1.0"?><c:mkcalendar xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:set><d:prop><d:displayname>İş Takvimi</d:displayname></d:prop></d:set></c:mkcalendar>"#;

struct Other {
    http: Client,
    base: String,
}

impl Other {
    fn req(&mut self, method: &str, path: &str, ctype: &str, body: &str) -> (u16, String) {
        let r = self.http.request(method, &format!("{}{path}", self.base), ctype, body.as_bytes()).expect("istek");
        (r.status, r.text())
    }
    fn get(&mut self, path: &str) -> String {
        self.req("GET", path, "", "").1
    }
    fn status(&mut self, path: &str) -> u16 {
        self.req("GET", path, "", "").0
    }
    fn put(&mut self, path: &str, ctype: &str, body: &str) {
        let (s, t) = self.req("PUT", path, ctype, body);
        assert!((200..300).contains(&s), "PUT {path}: {s} {t}");
    }
}

fn wait_idle(svc: &MailService) {
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_millis(200));
    while svc.busy() {
        assert!(t0.elapsed() < Duration::from_secs(30), "zaman aşımı");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore]
fn two_way_sync_against_real_server() {
    let (Ok(base), Ok(user), Ok(pass)) = (std::env::var("LYNORA_E2E_DAV"), std::env::var("LYNORA_E2E_USER"), std::env::var("LYNORA_E2E_PASS")) else {
        eprintln!("LYNORA_E2E_DAV tanımlı değil, atlandı");
        return;
    };
    lynora_lib::i18n::init(Some("tr"));
    let run = uuid::Uuid::new_v4().simple().to_string();
    let (cal_path, book_path) = (format!("/{user}/is-{run}/"), format!("/{user}/rehber-{run}/"));
    let base = base.trim_end_matches('/').to_string();
    let mut other = Other { http: Client::new(Auth::Basic { user: user.clone(), password: pass.clone() }), base: base.clone() };
    other.req("PROPFIND", &format!("/{user}/"), "application/xml", ""); // Radicale kullanıcı klasörünü ilk istekte açar
    assert_eq!(other.req("MKCALENDAR", &cal_path, "application/xml", MK_CAL).0, 201);
    assert_eq!(other.req("MKCOL", &book_path, "application/xml", MK_BOOK).0, 201);
    other.put(&format!("{cal_path}seri.ics"), "text/calendar", SERIES);
    other.put(&format!("{book_path}kisi.vcf"), "text/vcard", CARD);

    let db = Db::open_memory().unwrap();
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let errs = errors.clone();
    let svc = MailService::new(
        db.clone(),
        Arc::new(move |ev: &str, p: serde_json::Value| {
            if ev == "mail://error" {
                eprintln!("HATA: {p}");
                errs.lock().unwrap().push(p.to_string());
            }
        }),
    );
    let good_url = format!("{base}/{user}/");
    let mut acc = Account {
        name: "dav".into(),
        email: "ali@test.local".into(),
        username: user.clone(),
        password: lynora_lib::secret::encrypt_local(&pass),
        caldav_url: good_url.clone(),
        carddav_url: good_url.clone(),
        sync_limit: 10,
        ..Default::default()
    };
    acc.id = db.save_account(&acc).unwrap();

    // Yanlış parola ve keşif
    let mut bad = acc.clone();
    bad.password = lynora_lib::secret::encrypt_local("yanlis");
    let e = Dav::for_account(&bad, &good_url, None).unwrap().discover(Kind::Calendar).unwrap_err();
    assert!(e.message().contains("parolayı"), "{e}");
    assert!(Dav::for_account(&acc, &good_url, None).unwrap().discover(Kind::Contacts).unwrap().iter().any(|b| b.href == book_path));

    // 1. Sunucudakiler iner
    svc.sync_dav(acc.id);
    wait_idle(&svc);
    let cal = db.calendars().into_iter().find(|c| c.remote_id.as_deref() == Some(cal_path.as_str())).expect("takvim");
    assert_eq!((cal.kind.as_str(), cal.name.as_str(), cal.account_id), ("dav", "İş Takvimi", Some(acc.id)));
    let rows = db.events_in_calendar(cal.id);
    assert_eq!(rows.len(), 2, "seri ve taşınan oluşum: {rows:?}");
    let master = rows.iter().find(|e| e.remote_id.is_some()).unwrap().clone();
    assert_eq!(master.title, "Haftalık");
    assert!(!master.exdates.is_empty(), "taşınan gün seride atlanmalı");
    let moved = rows.iter().find(|e| e.remote_id.is_none()).unwrap();
    assert_eq!(moved.title, "Taşınan");
    let contact = db.contacts_of_account(acc.id).into_iter().find(|c| c.email == "ali@x.com").expect("kişi");
    assert_eq!(contact.first_name, "Ali");

    // 2. Yerel düzenleme sunucuya gider, modelde olmayanlar korunur
    let mut e = master.clone();
    e.title = "Yeni ad".into();
    db.save_event(&e).unwrap();
    svc.push_event(acc.id, e.id);
    wait_idle(&svc);
    let text = other.get(&format!("{cal_path}seri.ics"));
    for keep in ["SUMMARY:Yeni ad", "X-OZEL:korunmali", "BYDAY=2MO", "SUMMARY:Taşınan", "UID:seri-1"] {
        assert!(text.contains(keep), "{keep} yok:\n{text}");
    }
    assert!(db.dirty_events(cal.id).is_empty());
    let mut c = contact.clone();
    c.job_title = "Müdür".into();
    db.save_contact(&c).unwrap();
    svc.dav_push_contact(acc.id, c.id);
    wait_idle(&svc);
    let card = other.get(&format!("{book_path}kisi.vcf"));
    assert!(card.contains("TITLE:Müdür") && card.contains("TEL;TYPE=FAX:333") && card.contains("UID:kisi-1"), "{card}");

    // 3. Yeni etkinlik ve kişi oluşturulur
    let new_id = db.save_event(&Event { calendar_id: cal.id, title: "Yeni etkinlik".into(), start: 1_800_000_000.0, end: 1_800_003_600.0, ..Event::new_default() }).unwrap();
    svc.push_event(acc.id, new_id);
    let new_contact = db.save_contact(&Contact { first_name: "Ayşe".into(), email: "ayse@x.com".into(), account_id: Some(acc.id), ..Default::default() }).unwrap();
    svc.dav_push_contact(acc.id, new_contact);
    wait_idle(&svc);
    let href = db.event(new_id).unwrap().remote_id.expect("yeni etkinliğin adresi");
    let new_path = href.clone();
    assert!(other.get(&new_path).contains("SUMMARY:Yeni etkinlik"));
    let card_href = db.contact(new_contact).unwrap().remote_id.expect("yeni kişinin adresi");
    assert!(other.get(&card_href).contains("FN:Ayşe"));
    assert!(!db.event_dav(new_id).etag.is_empty() || !db.event_dav(new_id).dirty);

    // 4. Başka cihazdaki değişiklik ve silme yerele gelir
    other.put(&format!("{cal_path}seri.ics"), "text/calendar", &SERIES.replace("Haftalık", "Başka cihazdan"));
    assert_eq!(other.req("DELETE", &new_path, "", "").0, 200);
    assert_eq!(other.req("DELETE", &card_href, "", "").0, 200);
    svc.sync_dav(acc.id);
    wait_idle(&svc);
    assert_eq!(db.event(master.id).unwrap().title, "Başka cihazdan");
    assert!(db.event(new_id).is_none(), "sunucudan silinen yerelden de silinmeli");
    assert!(db.contact(new_contact).is_none());

    // 5. Çakışma: sunucu değişmişken eski ETag ile yazılır, son yazan kazanır
    other.put(&format!("{cal_path}seri.ics"), "text/calendar", &SERIES.replace("Haftalık", "Araya giren"));
    let mut e = db.event(master.id).unwrap();
    e.location = "Salon".into();
    db.save_event(&e).unwrap();
    svc.push_event(acc.id, e.id);
    wait_idle(&svc);
    assert!(other.get(&format!("{cal_path}seri.ics")).contains("LOCATION:Salon"));

    // 6. Sunucuya ulaşılamazken yapılan değişiklik kaybolmaz, bağlantı gelince gider
    let mut down = db.account(acc.id).unwrap();
    // (başarısız adres kısa süre hızlı-hata listesinde kalır: sunucudan farklı bir ad kullanılır)
    down.caldav_url = "http://localhost:9/".into();
    db.save_account(&down).unwrap();
    let mut e = db.event(master.id).unwrap();
    e.title = "Çevrimdışı".into();
    db.save_event(&e).unwrap();
    svc.push_event(acc.id, e.id);
    wait_idle(&svc);
    assert_eq!(db.dirty_events(cal.id).len(), 1, "gönderilemeyen değişiklik kirli kalmalı");
    db.save_account(&acc).unwrap();
    svc.sync_dav(acc.id);
    wait_idle(&svc);
    assert!(db.dirty_events(cal.id).is_empty());
    assert!(other.get(&format!("{cal_path}seri.ics")).contains("SUMMARY:Çevrimdışı"));
    assert_eq!(db.event(master.id).unwrap().title, "Çevrimdışı", "sunucu sürümü yerel değişikliği ezmemeli");

    // 7. Yerel silme: mezar taşı sunucuya gider, eşitleme diriltmez
    db.add_tombstone(lynora_lib::dav_sync::EVENT_TOMBS, acc.id, &format!("{cal_path}seri.ics"));
    db.delete_event(master.id);
    svc.dav_flush(lynora_lib::dav_sync::EVENT_TOMBS, acc.id);
    wait_idle(&svc);
    assert_eq!(other.status(&format!("{cal_path}seri.ics")), 404);
    svc.sync_dav(acc.id);
    wait_idle(&svc);
    assert!(db.events_in_calendar(cal.id).iter().all(|e| e.title != "Çevrimdışı"));
    assert!(db.tombstones(lynora_lib::dav_sync::EVENT_TOMBS, acc.id).is_empty());
    assert!(errors.lock().unwrap().iter().all(|e| !e.contains("HTTP")), "beklenmeyen hata: {:?}", errors.lock().unwrap());
    eprintln!("tamam");
}
