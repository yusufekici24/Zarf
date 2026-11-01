//! Gerçek bir IMAP/SMTP sunucusuna karşı uçtan uca posta testi.
//!
//! Varsayılan olarak atlanır. Çalıştırmak için yerel bir test sunucusu
//! (ör. Dovecot + aiosmtpd) kurup şu değişkenleri verin:
//!
//! ```sh
//! MAILNIX_E2E_IMAP=127.0.0.1:1143 MAILNIX_E2E_SMTP=127.0.0.1:2525 \
//! MAILNIX_E2E_USER=ali@test.local MAILNIX_E2E_PASS=parola1 \
//! MAILNIX_E2E_PEER=veli@test.local \
//! cargo test --test e2e_mail -- --ignored --nocapture
//! ```

use mailnix_lib::db::{Account, Db, MsgFilter, Rule};
use mailnix_lib::mail::build::{build_raw, ComposedMail};
use mailnix_lib::sync::MailService;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_idle(svc: &MailService) {
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_millis(150));
    while svc.busy() {
        assert!(t0.elapsed() < Duration::from_secs(60), "zaman aşımı");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn split(addr: &str) -> (String, i64) {
    let (h, p) = addr.rsplit_once(':').unwrap();
    (h.to_string(), p.parse().unwrap())
}

#[test]
#[ignore]
fn full_mail_cycle() {
    let (Ok(imap), Ok(smtp), Ok(user), Ok(pass), Ok(peer)) = (
        std::env::var("MAILNIX_E2E_IMAP"),
        std::env::var("MAILNIX_E2E_SMTP"),
        std::env::var("MAILNIX_E2E_USER"),
        std::env::var("MAILNIX_E2E_PASS"),
        std::env::var("MAILNIX_E2E_PEER"),
    ) else {
        eprintln!("MAILNIX_E2E_* tanımlı değil, atlandı");
        return;
    };
    mailnix_lib::i18n::init(Some("tr"));
    let db = Db::open_memory().unwrap();
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let errs = errors.clone();
    let svc = MailService::new(
        db.clone(),
        Arc::new(move |ev: &str, payload: serde_json::Value| {
            if ev == "mail://error" {
                eprintln!("HATA: {payload}");
                errs.lock().unwrap().push(payload.to_string());
            }
        }),
    );
    let (ih, ip) = split(&imap);
    let (sh, sp) = split(&smtp);
    let acc = Account {
        name: "Test".into(),
        email: user.clone(),
        display_name: "Ali Test".into(),
        imap_host: ih,
        imap_port: ip,
        imap_security: "none".into(),
        smtp_host: sh,
        smtp_port: sp,
        smtp_security: "none".into(),
        password: mailnix_lib::util::obfuscate(&pass),
        sync_limit: 500,
        ..Default::default()
    };
    let (imap_ok, smtp_ok, log) = MailService::test_account(&acc, &pass);
    println!("{log}");
    assert!(imap_ok && smtp_ok);
    let acc_id = db.save_account(&acc).unwrap();

    // 1) Klasör listesi ve ilk eşitleme
    svc.sync_account(acc_id);
    wait_idle(&svc);
    let folders = db.folders(Some(acc_id));
    let roles: Vec<String> = folders.iter().filter_map(|f| f.role.clone()).collect();
    println!("klasörler: {:?}", folders.iter().map(|f| (&f.name, &f.role)).collect::<Vec<_>>());
    for r in ["inbox", "sent", "drafts", "trash", "junk", "archive"] {
        assert!(roles.contains(&r.to_string()), "{r} rolü bulunamadı");
    }
    let inbox = db.folder_by_role(Some(acc_id), "inbox").unwrap();
    let before = db.messages(Some(inbox.id), "", &MsgFilter::All).len();
    println!("gelen kutusunda {before} ileti");

    // 2) Kendine ileti gönder (kural testi için konu işaretli)
    db.save_rule(&Rule {
        name: "Fatura".into(),
        enabled: true,
        field: "subject".into(),
        op: "contains".into(),
        value: "fatura".into(),
        action: "flag".into(),
        ..Default::default()
    })
    .unwrap();
    let mail = ComposedMail {
        account_id: Some(acc_id),
        to: format!("Kendim <{user}>"),
        cc: peer.clone(),
        bcc: format!("gizli <{peer}>"),
        subject: "E2E Fatura testi ğüşıöç".into(),
        text: "Merhaba,\n.satır nokta ile başlıyor\nekte dosya var".into(),
        html: "<p>Merhaba <b>dünya</b></p>".into(),
        importance: 2,
        ..Default::default()
    };
    let raw = build_raw(Some(&db.account(acc_id).unwrap()), &mail, &[("not.txt".into(), "text/plain".into(), b"ek icerik".to_vec())], true);
    svc.send(acc_id, raw, None).unwrap();
    wait_idle(&svc);
    let outbox = db.folder_by_role(None, "outbox").unwrap();
    assert_eq!(db.total_count(outbox.id), 0, "giden kutusu boşalmalı");
    let sent = db.folder_by_role(Some(acc_id), "sent").unwrap();
    let sent_msgs = db.messages(Some(sent.id), "E2E", &MsgFilter::All);
    assert_eq!(sent_msgs.len(), 1, "gönderilmiş öğelerde kopya olmalı");
    let sent_full = db.message(sent_msgs[0].id).unwrap();
    assert!(sent_full.bcc.contains(&peer), "gönderilmiş kopyada Gizli alanı korunmalı");
    assert!(sent_full.uid.is_some(), "kopya sunucuda (UID) olmalı");

    // 3) Gelen kutusunu eşitle → yeni ileti + kural (bayrak)
    svc.sync_folder(inbox.id);
    wait_idle(&svc);
    let got = db.messages(Some(inbox.id), "E2E", &MsgFilter::All);
    assert_eq!(got.len(), 1, "ileti gelen kutusuna ulaşmalı");
    let m = db.message(got[0].id).unwrap();
    assert_eq!(m.subject, "E2E Fatura testi ğüşıöç");
    assert!(m.is_flagged, "kural iletiyi bayraklamalı");
    assert_eq!(m.importance, 2);
    assert!(m.has_attachments);
    assert!(m.body_text.contains(".satır nokta"), "nokta doldurma geri alınmalı: {:?}", m.body_text);
    assert!(m.bcc.is_empty(), "alıcıya Gizli başlığı gitmemeli");
    assert!(!m.is_read);

    // 4) Okundu bayrağı sunucuya yazılmalı
    svc.set_read(&[m.id], true);
    wait_idle(&svc);
    // Yerel kaydı bozup sunucudan geri okut
    db.set_message_field(m.id, "is_read", 0);
    svc.sync_folder(inbox.id);
    wait_idle(&svc);
    assert!(db.message_row(m.id).unwrap().is_read, "okundu bilgisi sunucudan gelmeli");

    // 5) Arşive taşı
    let archive = db.folder_by_role(Some(acc_id), "archive").unwrap();
    svc.move_messages(&[m.id], archive.id);
    wait_idle(&svc);
    assert_eq!(db.messages(Some(inbox.id), "E2E", &MsgFilter::All).len(), 0);
    let arch = db.messages(Some(archive.id), "E2E", &MsgFilter::All);
    assert_eq!(arch.len(), 1, "ileti arşivde olmalı");
    assert!(arch[0].uid.is_some());

    // 6) Sil → Silinmiş Öğeler, tekrar sil → kalıcı
    svc.delete(&[arch[0].id], false);
    wait_idle(&svc);
    let trash = db.folder_by_role(Some(acc_id), "trash").unwrap();
    svc.sync_folder(trash.id);
    wait_idle(&svc);
    let tr = db.messages(Some(trash.id), "E2E", &MsgFilter::All);
    assert_eq!(tr.len(), 1, "ileti çöpte olmalı");
    svc.delete(&[tr[0].id], false);
    wait_idle(&svc);
    svc.sync_folder(trash.id);
    wait_idle(&svc);
    assert_eq!(db.messages(Some(trash.id), "E2E", &MsgFilter::All).len(), 0, "kalıcı silinmeli");

    // 7) Klasör oluştur / yeniden adlandır / sil (Türkçe karakterli)
    svc.create_folder(Some(acc_id), "Müşteriler".into(), None);
    wait_idle(&svc);
    let f = db.folders(Some(acc_id)).into_iter().find(|f| f.name == "Müşteriler").expect("klasör oluşmalı");
    svc.rename_folder(f.id, "Önemli Müşteriler".into());
    wait_idle(&svc);
    assert!(db.folders(Some(acc_id)).iter().any(|f| f.name == "Önemli Müşteriler"));
    let f = db.folders(Some(acc_id)).into_iter().find(|f| f.name == "Önemli Müşteriler").unwrap();
    svc.delete_folder(f.id);
    wait_idle(&svc);
    svc.sync_account(acc_id);
    wait_idle(&svc);
    assert!(!db.folders(Some(acc_id)).iter().any(|f| f.name.contains("Müşteriler")));

    // 8) Taslak kaydet → sunucuya yazılmalı; at → silinmeli
    let draft_raw = build_raw(Some(&db.account(acc_id).unwrap()), &ComposedMail { subject: "Taslak E2E".into(), text: "yarım".into(), ..Default::default() }, &[], true);
    svc.save_draft(Some(acc_id), draft_raw, None);
    wait_idle(&svc);
    let drafts = db.folder_by_role(Some(acc_id), "drafts").unwrap();
    let d = db.messages(Some(drafts.id), "Taslak E2E", &MsgFilter::All);
    assert_eq!(d.len(), 1, "taslak kaydedilmeli");
    assert!(d[0].uid.is_some(), "taslak sunucuda olmalı");
    svc.discard_draft(d[0].id);
    wait_idle(&svc);
    svc.sync_folder(drafts.id);
    wait_idle(&svc);
    assert_eq!(db.messages(Some(drafts.id), "Taslak E2E", &MsgFilter::All).len(), 0);

    // 9) Yerel klasöre içe aktarılan iletiyi sunucu klasörüne taşı (APPEND)
    let local_inbox = db.folder_by_role(None, "inbox").unwrap();
    let raw = mailnix_lib::mail::build::simple_raw("Dış <dis@x.com>", &user, "Yerelden sunucuya", "gövde");
    let lid = svc.import_raw(local_inbox.id, raw, true).unwrap();
    svc.move_messages(&[lid], archive.id);
    wait_idle(&svc);
    let a = db.messages(Some(archive.id), "Yerelden sunucuya", &MsgFilter::All);
    assert_eq!(a.len(), 1);
    assert!(a[0].uid.is_some(), "APPEND sonrası UID olmalı");
    svc.delete(&[a[0].id], true);
    wait_idle(&svc);

    assert!(errors.lock().unwrap().is_empty(), "hata olayları: {:?}", errors.lock().unwrap());
    println!("TÜM ADIMLAR BAŞARILI");
}

/// Kerberos (GSSAPI) ile parolasız IMAP girişi.
/// `MAILNIX_E2E_KRB_IMAP=localhost:1143 cargo test --test e2e_mail kerberos -- --ignored`
/// (önceden `kinit` ile bilet alınmış olmalı)
#[test]
#[ignore]
fn kerberos_imap_login() {
    let Ok(addr) = std::env::var("MAILNIX_E2E_KRB_IMAP") else { return };
    mailnix_lib::i18n::init(Some("tr"));
    let (h, p) = split(&addr);
    let id = mailnix_lib::mail::gssapi::system_identity();
    println!("kimlik: {id:?}");
    assert!(id.kerberos, "Kerberos bileti bulunamadı");
    let acc = Account {
        email: id.email_guess.clone(),
        imap_host: h,
        imap_port: p,
        imap_security: "none".into(),
        auth_method: "kerberos".into(),
        ..Default::default()
    };
    let mut c = mailnix_lib::mail::imap_client::Imap::connect(&acc, "").expect("GSSAPI girişi");
    let folders = c.list_folders().unwrap();
    println!("klasörler: {}", folders.len());
    assert!(folders.iter().any(|f| f.role.as_deref() == Some("inbox")));
    c.logout();
}

/// Sahte/gerçek bir Exchange EWS sunucusuna karşı uçtan uca test.
///
/// ```sh
/// MAILNIX_E2E_EWS=http://localhost:8443/EWS/Exchange.asmx MAILNIX_E2E_EWS_AUTH=kerberos \
/// cargo test --test e2e_mail exchange_cycle -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn exchange_cycle() {
    let Ok(ews) = std::env::var("MAILNIX_E2E_EWS") else {
        eprintln!("MAILNIX_E2E_EWS tanımlı değil, atlandı");
        return;
    };
    let auth = std::env::var("MAILNIX_E2E_EWS_AUTH").unwrap_or_else(|_| "kerberos".into());
    let expect_reject = std::env::var("MAILNIX_E2E_EWS_REJECT").is_ok();
    mailnix_lib::i18n::init(Some("tr"));
    let db = Db::open_memory().unwrap();
    let auth_failed = Arc::new(Mutex::new(0usize));
    let af = auth_failed.clone();
    let svc = MailService::new(
        db.clone(),
        Arc::new(move |ev: &str, payload: serde_json::Value| {
            if ev == "mail://error" {
                eprintln!("HATA: {payload}");
            }
            if ev == "account://auth_failed" {
                *af.lock().unwrap() += 1;
            }
        }),
    );
    let pass = if auth == "kerberos" { String::new() } else { "parola1".to_string() };
    let acc = Account {
        name: "Kurum".into(),
        email: "ali@test.local".into(),
        display_name: "Ali Test".into(),
        username: if auth == "kerberos" { String::new() } else { "TEST\\ali".into() },
        kind: "exchange".into(),
        auth_method: auth.clone(),
        ews_url: ews.clone(),
        password: mailnix_lib::util::obfuscate(&pass),
        sync_limit: 500,
        ..Default::default()
    };

    if let Ok(ad) = std::env::var("MAILNIX_AUTODISCOVER_URL") {
        let mut log = Vec::new();
        let a = if auth == "kerberos" {
            mailnix_lib::mail::http::Auth::Negotiate
        } else {
            mailnix_lib::mail::http::Auth::Ntlm { user: "TEST\\ali".into(), password: pass.clone() }
        };
        let d = mailnix_lib::mail::discover::exchange("ali@test.local", a, &mut log);
        println!("autodiscover ({ad}): {:?}\n{}", d.as_ref().map(|d| d.ews_url.clone()), log.join("\n"));
        if !expect_reject {
            assert_eq!(d.unwrap().ews_url, ews);
        }
    }

    let acc_id = db.save_account(&acc).unwrap();
    if expect_reject {
        // Hatalı parola: tek deneme, ardından hesap duraklatılmalı
        svc.sync_account(acc_id);
        wait_idle(&svc);
        let a = db.account(acc_id).unwrap();
        assert_eq!(a.auth_state, "failed", "hesap duraklatılmalı");
        assert_eq!(*auth_failed.lock().unwrap(), 1);
        for _ in 0..3 {
            svc.sync_account(acc_id);
            wait_idle(&svc);
        }
        assert_eq!(*auth_failed.lock().unwrap(), 1, "duraklatılmış hesap tekrar denememeli");
        return;
    }
    let (ok, _, log) = MailService::test_account(&db.account(acc_id).unwrap(), &pass);
    println!("{log}");
    assert!(ok, "EWS testi başarısız");

    // 1) Eşitleme: klasörler, iletiler, takvim, kişiler
    svc.sync_account(acc_id);
    wait_idle(&svc);
    let folders = db.folders(Some(acc_id));
    println!("klasörler: {:?}", folders.iter().map(|f| (&f.name, &f.role)).collect::<Vec<_>>());
    for r in ["inbox", "sent", "drafts", "trash", "junk", "archive"] {
        assert!(folders.iter().any(|f| f.role.as_deref() == Some(r)), "{r} rolü bulunamadı");
    }
    let inbox = db.folder_by_role(Some(acc_id), "inbox").unwrap();
    let msgs = db.messages(Some(inbox.id), "Exchange ileti", &MsgFilter::All);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs.iter().filter(|m| m.is_read).count(), 1);
    let m0 = db.message(msgs[0].id).unwrap();
    assert!(m0.body_text.contains("ğüş"), "{:?}", m0.body_text);
    let cal = db.calendar_for_account(acc_id).expect("Exchange takvimi");
    let evs = db.events_in_calendar(cal.id);
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].title, "Haftalık toplantı");
    assert_eq!(evs[0].location, "Toplantı Odası");
    let cts = db.contacts_of_account(acc_id);
    assert_eq!(cts.len(), 1);
    assert!(cts[0].email == "mudur@kurum.gov.tr");

    // 2) Okundu + bayrak sunucuya yazılmalı ve geri okunmalı
    let unread: Vec<i64> = msgs.iter().filter(|m| !m.is_read).map(|m| m.id).collect();
    svc.set_read(&unread, true);
    svc.set_flagged(&[unread[0]], true);
    wait_idle(&svc);
    for id in &unread {
        db.set_message_field(*id, "is_read", 0);
    }
    db.set_message_field(unread[0], "is_flagged", 0);
    svc.sync_folder(inbox.id);
    wait_idle(&svc);
    assert!(unread.iter().all(|id| db.message_row(*id).unwrap().is_read), "okundu bilgisi sunucudan gelmeli");
    assert!(db.message_row(unread[0]).unwrap().is_flagged, "bayrak sunucudan gelmeli");

    // 3) Arşive taşı, sil
    let archive = db.folder_by_role(Some(acc_id), "archive").unwrap();
    svc.move_messages(&[msgs[0].id], archive.id);
    wait_idle(&svc);
    svc.sync_account(acc_id);
    wait_idle(&svc);
    assert_eq!(db.messages(Some(inbox.id), "Exchange ileti", &MsgFilter::All).len(), 2);
    let arch = db.messages(Some(archive.id), "Exchange ileti", &MsgFilter::All);
    assert_eq!(arch.len(), 1, "arşivde olmalı");
    svc.delete(&[arch[0].id], false);
    wait_idle(&svc);
    svc.sync_account(acc_id);
    wait_idle(&svc);
    let trash = db.folder_by_role(Some(acc_id), "trash").unwrap();
    assert_eq!(db.messages(Some(trash.id), "Exchange ileti", &MsgFilter::All).len(), 1, "çöpte olmalı");

    // 4) Gönder (giden kutusu → EWS SendAndSaveCopy)
    let mail = ComposedMail {
        account_id: Some(acc_id),
        to: "Ali <ali@test.local>".into(),
        subject: "EWS gönderim ğüş".into(),
        text: "deneme".into(),
        ..Default::default()
    };
    let raw = build_raw(Some(&db.account(acc_id).unwrap()), &mail, &[], true);
    svc.send(acc_id, raw, None).unwrap();
    wait_idle(&svc);
    let outbox = db.folder_by_role(None, "outbox").unwrap();
    assert_eq!(db.total_count(outbox.id), 0, "giden kutusu boşalmalı");
    svc.sync_account(acc_id);
    wait_idle(&svc);
    let sent = db.folder_by_role(Some(acc_id), "sent").unwrap();
    assert_eq!(db.messages(Some(sent.id), "EWS gönderim", &MsgFilter::All).len(), 1, "gönderilmiş kopya");
    assert_eq!(db.messages(Some(inbox.id), "EWS gönderim", &MsgFilter::All).len(), 1, "kendine gelen");

    // 5) Klasör oluştur / yeniden adlandır / sil
    svc.create_folder(Some(acc_id), "Müşteriler".into(), None);
    wait_idle(&svc);
    let f = db.folders(Some(acc_id)).into_iter().find(|f| f.name == "Müşteriler").expect("klasör oluşmalı");
    assert!(f.remote_name.is_some());
    svc.rename_folder(f.id, "Önemli Müşteriler".into());
    wait_idle(&svc);
    svc.sync_account(acc_id);
    wait_idle(&svc);
    let f = db.folders(Some(acc_id)).into_iter().find(|f| f.name == "Önemli Müşteriler").expect("yeniden adlandırılmalı");
    svc.delete_folder(f.id);
    wait_idle(&svc);
    svc.sync_account(acc_id);
    wait_idle(&svc);
    assert!(!db.folders(Some(acc_id)).iter().any(|f| f.name.contains("Müşteriler")));

    // 6) Taslak
    let draft_raw = build_raw(Some(&db.account(acc_id).unwrap()), &ComposedMail { subject: "Taslak EWS".into(), text: "yarım".into(), ..Default::default() }, &[], true);
    svc.save_draft(Some(acc_id), draft_raw, None);
    wait_idle(&svc);
    let drafts = db.folder_by_role(Some(acc_id), "drafts").unwrap();
    let d = db.messages(Some(drafts.id), "Taslak EWS", &MsgFilter::All);
    assert_eq!(d.len(), 1, "taslak kaydedilmeli");
    svc.discard_draft(d[0].id);
    wait_idle(&svc);
    svc.sync_account(acc_id);
    wait_idle(&svc);
    assert_eq!(db.messages(Some(drafts.id), "Taslak EWS", &MsgFilter::All).len(), 0);

    // 7) Takvim: yeni etkinlik sunucuya, toplantı yanıtı
    let mut ev = evs[0].clone();
    ev.id = 0;
    ev.remote_id = None;
    ev.title = "Yeni EWS etkinliği".into();
    let eid = db.save_event(&ev).unwrap();
    svc.push_event(acc_id, eid);
    wait_idle(&svc);
    assert!(db.event(eid).and_then(|e| e.remote_id).is_some(), "etkinlik sunucuya yazılmalı");
    svc.respond_ews(acc_id, evs[0].remote_id.clone().unwrap(), "accept".into());
    wait_idle(&svc);

    // 8) GAL araması ve otomatik yanıt
    let mut e = mailnix_lib::mail::ews::Ews::for_account(&db.account(acc_id).unwrap()).unwrap();
    let names = e.resolve_names("ayşe").unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].1, "ayse.yilmaz@kurum.gov.tr");
    e.set_oof("ali@test.local", true, "İzindeyim").unwrap();
    assert_eq!(*auth_failed.lock().unwrap(), 0);
}

/// Kerberos ile dizinden (AD/LDAP) kullanıcının gerçek e-posta adresini okuma.
/// `MAILNIX_LDAP_SERVER=localhost:3389 cargo test --test e2e_mail directory_lookup -- --ignored`
#[test]
#[ignore]
fn directory_lookup() {
    if std::env::var("MAILNIX_LDAP_SERVER").is_err() && std::env::var("MAILNIX_DNS_SERVER").is_err() {
        return;
    }
    mailnix_lib::i18n::init(Some("tr"));
    let p = mailnix_lib::mail::gssapi::current_principal().expect("kinit gerekli");
    let mut log = Vec::new();
    let u = mailnix_lib::mail::ldap::lookup_principal(&p, &mut log);
    println!("{p} → {u:?}\n{}", log.join("\n"));
    let u = u.expect("dizinde bulunamadı");
    assert_eq!(u.email, "ali.veli@kurum.gov.tr");
    assert_eq!(u.display_name, "Ali Veli Test");
}

/// Outlook gibi: e-posta yazmadan, bilgisayar kullanıcı adı (KURUM\kullanici) ve parola ile kurulum.
/// Sahte EWS (EWS_AUTH=ntlm) ve MAILNIX_AUTODISCOVER_URL gerekir.
#[test]
#[ignore]
fn discover_by_login() {
    if std::env::var("MAILNIX_AUTODISCOVER_URL").is_err() {
        return;
    }
    mailnix_lib::i18n::init(Some("tr"));
    let reject = std::env::var("MAILNIX_E2E_EWS_REJECT").is_ok();
    let d = mailnix_lib::mail::discover::discover("", "TEST\\ali", "parola1");
    println!("{d:#?}");
    if reject {
        assert!(d.auth_failed, "hatalı parola bildirilmeli");
        return;
    }
    assert!(!d.auth_failed && !d.need_email);
    assert_eq!(d.email, "ali.veli@kurum.gov.tr");
    assert_eq!(d.kind, "exchange");
    assert_eq!(d.username, "TEST\\ali");
    assert!(d.ews_url.ends_with("/EWS/Exchange.asmx"));
}

/// Autodiscover 401 dönerse IMAP tahminine geçilmemeli; kullanıcı adı/parola istenmeli.
/// Kerberos bileti olmadan çalıştırın: KRB5CCNAME=FILE:/yok
#[test]
#[ignore]
fn discover_asks_credentials_on_401() {
    if std::env::var("MAILNIX_AUTODISCOVER_URL").is_err() {
        return;
    }
    mailnix_lib::i18n::init(Some("tr"));
    let d = mailnix_lib::mail::discover::discover("ad.soyad@kurum.gov.tr", "", "");
    println!("{:?}", d.log);
    assert!(d.need_credentials, "parola istenmeli");
    assert_eq!(d.kind, "exchange");
    assert!(d.imap_host.is_empty());
    // Ardından kullanıcı adı ve parola ile
    let d = mailnix_lib::mail::discover::discover("ad.soyad@kurum.gov.tr", "KURUM\\ad.soyad", "parola1");
    println!("{:?}", d.log);
    assert!(!d.need_credentials && !d.auth_failed);
    assert!(d.ews_url.ends_with("/EWS/Exchange.asmx"));
    assert_eq!(d.username, "KURUM\\ad.soyad");
}

/// Büyük posta kutusu: önce en yeniler, sonra tüm başlıklar; yeni posta denetimi; sunucuda arama.
/// Sahte EWS: `EWS_BULK=600 EWS_AUTH=negotiate`
#[test]
#[ignore]
fn exchange_large_mailbox() {
    let Ok(ews) = std::env::var("MAILNIX_E2E_EWS") else { return };
    mailnix_lib::i18n::init(Some("tr"));
    let db = Db::open_memory().unwrap();
    let news = Arc::new(Mutex::new(Vec::<String>::new()));
    let n2 = news.clone();
    let svc = MailService::new(
        db.clone(),
        Arc::new(move |ev: &str, payload: serde_json::Value| {
            if ev == "mail://error" {
                eprintln!("HATA: {payload}");
            }
            if ev == "mail://new" {
                n2.lock().unwrap().push(payload.to_string());
            }
        }),
    );
    let acc = Account { name: "Kurum".into(), email: "ali@test.local".into(), kind: "exchange".into(), auth_method: "kerberos".into(), ews_url: ews, sync_limit: 50, ..Default::default() };
    let acc_id = db.save_account(&acc).unwrap();
    let t0 = Instant::now();
    svc.sync_account(acc_id);
    // İlk sayfa (en yeniler) tam tarama bitmeden listede olmalı
    let inbox = loop {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(f) = db.folder_by_role(Some(acc_id), "inbox") {
            if db.total_count(f.id) >= 100 {
                break f;
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(30), "ilk sayfa gelmedi");
    };
    println!("ilk 100 başlık: {:?}", t0.elapsed());
    let newest = db.messages(Some(inbox.id), "", &MsgFilter::All);
    assert!(newest.iter().any(|m| m.subject.starts_with("Exchange ileti")), "en yeniler önce gelmeli");
    wait_idle(&svc);
    let total = db.total_count(inbox.id);
    println!("tümü: {total} ({:?})", t0.elapsed());
    assert!(total >= 603, "tüm başlıklar alınmalı: {total}");
    assert!(db.folder(inbox.id).unwrap().full_sync);
    // En yeni 50 iletinin gövdesi indirilmiş, eskiler yalnızca başlık
    let with_body = db.messages_without_content(inbox.id, 10000).len();
    assert!(with_body >= total as usize - 60, "eskiler başlık olarak kalmalı");
    // Yeni posta: dakikalık denetim yakalamalı
    let ews_base = std::env::var("MAILNIX_E2E_EWS").unwrap().replace("/EWS/Exchange.asmx", "");
    let _ = std::process::Command::new("curl").args(["-s", &format!("{ews_base}/__newmail")]).output();
    let before = db.total_count(inbox.id);
    svc.quick_check();
    wait_idle(&svc);
    println!("yeni denetim: {before} → {}", db.total_count(inbox.id));
    assert_eq!(db.messages(Some(inbox.id), "Az önce gelen", &MsgFilter::All).len(), 1);
    assert!(news.lock().unwrap().iter().any(|n| n.contains("Az önce gelen")), "yeni posta bildirilmeli");
    // Sunucuda arama (AQS)
    let mut e = mailnix_lib::mail::ews::Ews::for_account(&db.account(acc_id).unwrap()).unwrap();
    let (hits, _) = e.find_items("inbox", None, Some(&mailnix_lib::db::search_to_aqs("konu:\"Eski ileti 599\"")), 50).unwrap();
    assert_eq!(hits.len(), 1);
}

/// Exchange sunucu kuralları: okuma, oluşturma, aç/kapat, silme; Outlook istemci kuralı uyarısı.
#[test]
#[ignore]
fn exchange_rules() {
    let Ok(ews) = std::env::var("MAILNIX_E2E_EWS") else { return };
    mailnix_lib::i18n::init(Some("tr"));
    let db = Db::open_memory().unwrap();
    let svc = MailService::new(db.clone(), Arc::new(|ev: &str, p: serde_json::Value| if ev == "mail://error" { eprintln!("HATA: {p}") }));
    let acc = Account { name: "Kurum".into(), email: "ali@test.local".into(), kind: "exchange".into(), auth_method: "kerberos".into(), ews_url: ews, sync_limit: 10, ..Default::default() };
    let acc_id = db.save_account(&acc).unwrap();
    svc.sync_account(acc_id);
    wait_idle(&svc);
    let rules = db.rules(false);
    println!("{:#?}", rules.iter().map(|r| (&r.name, &r.field, &r.op, &r.value, &r.action, &r.action_arg, r.complex)).collect::<Vec<_>>());
    assert_eq!(rules.len(), 2, "sunucu kuralları alınmalı");
    let fat = rules.iter().find(|r| r.name == "Faturalar").unwrap();
    let archive = db.folder_by_role(Some(acc_id), "archive").unwrap();
    assert_eq!((fat.field.as_str(), fat.value.as_str(), fat.action.as_str()), ("subject", "fatura", "move"));
    assert_eq!(fat.action_arg, archive.id.to_string());
    assert!(!fat.complex);
    assert!(rules.iter().find(|r| r.name == "Müdür ve önemli").unwrap().complex);
    // Yeni kural: sunucuda oluşturulur
    let new = Rule { name: "Duyurular okundu".into(), enabled: true, account_id: Some(acc_id), field: "from".into(), op: "contains".into(), value: "duyuru@".into(), action: "mark_read".into(), ..Default::default() };
    let blob = std::env::var("EWS_RULE_BLOB").is_ok();
    let r = mailnix_lib::ews_sync::push_server_rule(&db, &new, false);
    if blob {
        assert_eq!(r, Err("rules.blob_exists".into()));
        mailnix_lib::ews_sync::push_server_rule(&db, &new, true).unwrap();
    } else {
        r.unwrap();
    }
    let rules = db.rules(false);
    let created = rules.iter().find(|r| r.name == "Duyurular okundu").expect("oluşturulmalı");
    assert!(created.remote_id.is_some());
    // Karmaşık kuralı kapat (yapısı korunmalı)
    let mut cx = rules.iter().find(|r| r.complex).unwrap().clone();
    cx.enabled = false;
    mailnix_lib::ews_sync::push_server_rule(&db, &cx, true).unwrap();
    let after = db.rules(false);
    let cx2 = after.iter().find(|r| r.name == "Müdür ve önemli").unwrap();
    assert!(!cx2.enabled && cx2.complex && cx2.remote_xml.contains("Importance"));
    // Sil
    mailnix_lib::ews_sync::delete_server_rule(&db, created, true).unwrap();
    assert!(!db.rules(false).iter().any(|r| r.name == "Duyurular okundu"));
    // Sunucu kuralları yerelde ikinci kez uygulanmaz
    let m = mailnix_lib::db::Message { subject: "Fatura Ekim".into(), ..Default::default() };
    assert!(mailnix_lib::rules::evaluate(&db.rules(true), &m, Some(acc_id)).is_empty());
}
