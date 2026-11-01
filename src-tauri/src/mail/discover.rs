//! Otomatik hesap yapılandırması.
//!
//! 1. Exchange Autodiscover (Outlook'un kullandığı POX protokolü): EWS adresi,
//!    görünen ad ve birincil SMTP adresi.
//! 2. Mozilla Autoconfig (alan adındaki config-v1.1.xml ve ISPDB): IMAP/SMTP.
//! 3. Bilinen sağlayıcı tablosu / tahmin.

use super::http::{Auth, Client};
use super::xml;
use super::{guess_servers, MailError, MailResult};
use serde::Serialize;

#[derive(Serialize, Debug, Clone, Default)]
pub struct Discovery {
    /// "exchange" veya "imap"
    pub kind: String,
    pub email: String,
    pub display_name: String,
    pub ews_url: String,
    /// Exchange Online (Microsoft 365) — OAuth gerekir
    pub office365: bool,
    pub imap_host: String,
    pub imap_port: i64,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: i64,
    pub smtp_security: String,
    /// "password", "kerberos", "oauth"
    pub auth_method: String,
    pub username: String,
    pub source: String,
    /// Kullanıcı adı/parola reddedildi (tekrar denenmedi)
    pub auth_failed: bool,
    /// E-posta adresi bulunamadı; kullanıcıdan istenmeli
    pub need_email: bool,
    /// Exchange bulundu ama oturum açmak için kullanıcı adı/parola gerekiyor
    pub need_credentials: bool,
    pub log: Vec<String>,
}

fn domain_of(email: &str) -> String {
    email.rsplit('@').next().unwrap_or("").trim().to_lowercase()
}

const AD_REQUEST: &str = r#"<?xml version="1.0" encoding="utf-8"?><Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/requestschema/2006"><Request><EMailAddress>{EMAIL}</EMailAddress><AcceptableResponseSchema>http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a</AcceptableResponseSchema></Request></Autodiscover>"#;

/// Exchange Autodiscover. `auth`: Negotiate (Kerberos) veya parola tabanlı.
pub fn exchange(email: &str, auth: Auth, log: &mut Vec<String>) -> MailResult<Discovery> {
    let domain = domain_of(email);
    let mut target = email.to_string();
    let mut urls = vec![
        format!("https://autodiscover.{domain}/autodiscover/autodiscover.xml"),
        format!("https://{domain}/autodiscover/autodiscover.xml"),
    ];
    // Kurumsal dağıtımlar için (ör. iç ağda farklı adres) elle verilen Autodiscover adresi
    if let Ok(u) = std::env::var("MAILNIX_AUTODISCOVER_URL") {
        if !u.trim().is_empty() {
            urls.insert(0, u.trim().to_string());
        }
    }
    let mut tried_redirect = 0;
    let mut kerberos_failed = false;
    let mut i = 0;
    while i < urls.len() {
        let url = urls[i].clone();
        i += 1;
        step(crate::t!("discover.step_trying", url.split('/').nth(2).unwrap_or(&url)));
        let mut client = Client::new(auth.clone());
        client.timeout = std::time::Duration::from_secs(15);
        let body = AD_REQUEST.replace("{EMAIL}", &xml::esc(&target));
        match client.request("POST", &url, "text/xml; charset=utf-8", body.as_bytes()) {
            Ok(r) if r.status == 200 => {
                let Ok(doc) = xml::parse(&r.body) else { continue };
                if let Some(action) = doc.find("Action").map(|a| a.text.trim().to_string()) {
                    if action == "redirectAddr" {
                        if let Some(addr) = doc.find("RedirectAddr").map(|x| x.text.trim().to_string()) {
                            if tried_redirect < 5 {
                                tried_redirect += 1;
                                note(log, format!("Autodiscover yönlendirme: {addr}"));
                                target = addr.clone();
                                let d = domain_of(&addr);
                                urls.push(format!("https://autodiscover.{d}/autodiscover/autodiscover.xml"));
                                urls.push(format!("https://{d}/autodiscover/autodiscover.xml"));
                                continue;
                            }
                        }
                    } else if action == "redirectUrl" {
                        if let Some(u) = doc.find("RedirectUrl").map(|x| x.text.trim().to_string()) {
                            if u.starts_with("https://") && tried_redirect < 5 {
                                tried_redirect += 1;
                                urls.push(u);
                                continue;
                            }
                        }
                    }
                }
                let mut d = Discovery { kind: "exchange".into(), email: target.clone(), source: url.clone(), ..Default::default() };
                if let Some(u) = doc.find("User") {
                    d.display_name = u.text_of("DisplayName");
                    let smtp = u.text_of("AutoDiscoverSMTPAddress");
                    if !smtp.is_empty() {
                        d.email = smtp;
                    }
                }
                let mut protos = Vec::new();
                doc.find_all("Protocol", &mut protos);
                // Önce dahili (EXCH), sonra harici (EXPR) EWS adresi
                for wanted in ["EXCH", "EXPR", "WEB"] {
                    for p in &protos {
                        if p.text_of("Type") == wanted && d.ews_url.is_empty() {
                            let ews = p.text_of("EwsUrl");
                            if !ews.is_empty() {
                                d.ews_url = ews;
                            }
                        }
                    }
                }
                for p in &protos {
                    let ty = p.text_of("Type");
                    let server = p.text_of("Server");
                    let port: i64 = p.text_of("Port").parse().unwrap_or(0);
                    let ssl = p.text_of("SSL") != "off";
                    let enc = p.text_of("Encryption").to_lowercase();
                    let sec = if enc == "tls" { "starttls" } else if ssl { "ssl" } else { "none" };
                    if ty == "IMAP" && d.imap_host.is_empty() {
                        d.imap_host = server.clone();
                        d.imap_port = if port > 0 { port } else { 993 };
                        d.imap_security = sec.into();
                    } else if ty == "SMTP" && d.smtp_host.is_empty() {
                        d.smtp_host = server.clone();
                        d.smtp_port = if port > 0 { port } else { 587 };
                        d.smtp_security = sec.into();
                    }
                }
                d.office365 = d.ews_url.contains("outlook.office365.com") || d.ews_url.contains("outlook.office.com");
                if !d.ews_url.is_empty() {
                    note(log, format!("Exchange bulundu: {}", d.ews_url));
                    return Ok(d);
                }
            }
            Ok(r) if r.status == 401 => {
                note(log, format!("{url}: kimlik doğrulama gerekli (401)"));
                return Err(MailError::auth(crate::t!("discover.auth_needed")));
            }
            Ok(r) => note(log, format!("{url}: HTTP {}", r.status)),
            Err(e) => {
                // Sunucu Kerberos istedi ama bilet yok/geçersiz (ör. kurum ağı dışı, başka etki
                // alanı): Exchange bulundu sayılır, Outlook gibi kullanıcı adı ve parola sorulur
                let m = e.to_string();
                if m.contains("Kerberos") || m.contains("GSS") || m.contains("Negotiate") {
                    kerberos_failed = true;
                }
                note(log, format!("{url}: {e}"));
            }
        }
    }
    if kerberos_failed {
        return Err(MailError::auth(crate::t!("discover.auth_needed")));
    }
    // HTTP üzerinden yönlendirme (yalnızca HTTPS hedefe)
    let mut c = Client::new(Auth::None);
    c.timeout = std::time::Duration::from_secs(10);
    if let Ok(r) = c.request("GET", &format!("http://autodiscover.{domain}/autodiscover/autodiscover.xml"), "", b"") {
        note(log, format!("HTTP yönlendirme yanıtı: {}", r.status));
    }
    Err(MailError(crate::t!("discover.no_exchange")))
}

/// Mozilla Autoconfig
pub fn autoconfig(email: &str, log: &mut Vec<String>) -> Option<Discovery> {
    let domain = domain_of(email);
    let urls = [
        format!("https://autoconfig.{domain}/mail/config-v1.1.xml?emailaddress={}", super::http::url_encode(email)),
        format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"),
        format!("https://autoconfig.thunderbird.net/v1.1/{domain}"),
    ];
    for url in urls {
        step(crate::t!("discover.step_autoconfig", url.split('/').nth(2).unwrap_or(&url)));
        let mut c = Client::new(Auth::None);
        c.timeout = std::time::Duration::from_secs(10);
        let Ok(r) = c.request("GET", &url, "", b"") else {
            note(log, format!("{url}: erişilemedi"));
            continue;
        };
        if r.status != 200 {
            note(log, format!("{url}: HTTP {}", r.status));
            continue;
        }
        let Ok(doc) = xml::parse(&r.body) else { continue };
        let local = email.split('@').next().unwrap_or("");
        let subst = |s: &str| s.replace("%EMAILADDRESS%", email).replace("%EMAILLOCALPART%", local).replace("%EMAILDOMAIN%", &domain);
        let mut d = Discovery { kind: "imap".into(), email: email.into(), source: url.clone(), auth_method: "password".into(), ..Default::default() };
        let mut incoming = Vec::new();
        doc.find_all("incomingServer", &mut incoming);
        for s in incoming.iter().filter(|s| s.attr("type") == Some("imap")) {
            d.imap_host = subst(&s.text_of("hostname"));
            d.imap_port = s.text_of("port").parse().unwrap_or(993);
            d.imap_security = match s.text_of("socketType").as_str() {
                "SSL" => "ssl",
                "STARTTLS" => "starttls",
                _ => "none",
            }
            .into();
            d.username = subst(&s.text_of("username"));
            let auths: Vec<String> = s.children_named("authentication").map(|a| a.text.clone()).collect();
            if auths.iter().any(|a| a == "GSSAPI") && super::gssapi::current_principal().is_some() {
                d.auth_method = "kerberos".into();
            } else if auths.iter().any(|a| a == "OAuth2") && !auths.iter().any(|a| a.starts_with("password")) {
                d.auth_method = "oauth".into();
            }
            break;
        }
        let mut outgoing = Vec::new();
        doc.find_all("outgoingServer", &mut outgoing);
        if let Some(s) = outgoing.first() {
            d.smtp_host = subst(&s.text_of("hostname"));
            d.smtp_port = s.text_of("port").parse().unwrap_or(587);
            d.smtp_security = match s.text_of("socketType").as_str() {
                "SSL" => "ssl",
                "STARTTLS" => "starttls",
                _ => "none",
            }
            .into();
        }
        if let Some(n) = doc.find("displayName") {
            note(log, format!("Autoconfig: {} ({url})", n.text));
        }
        if !d.imap_host.is_empty() {
            return Some(d);
        }
    }
    None
}

/// Aday etki alanları: kullanıcı adındaki UPN soneki, bilgisayarın etki alanı, e-posta alan adı.
fn candidate_domains(login: &str, email: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut add = |d: String| {
        let d = d.trim().trim_matches('.').to_lowercase();
        if d.contains('.') && !v.contains(&d) {
            v.push(d);
        }
    };
    if let Some((_, d)) = login.rsplit_once('@') {
        add(d.to_string());
    }
    if let Some(d) = super::gssapi::machine_domain() {
        add(d);
    }
    if email.contains('@') {
        add(domain_of(email));
    }
    v
}

/// E-posta adresi bilinmiyorsa Exchange'e oturum adıyla sorar (ResolveNames).
/// İlk kimlik reddinde durur; böylece hatalı parola tek kez denenmiş olur.
fn mailbox_via_ews(login: &str, auth: &Auth, uses_password: bool, domains: &[String], log: &mut Vec<String>) -> MailResult<Option<(String, String, String)>> {
    let mut urls: Vec<String> = Vec::new();
    if let Ok(u) = std::env::var("MAILNIX_AUTODISCOVER_URL") {
        let lower = u.to_ascii_lowercase();
        if let Some(i) = lower.find("/autodiscover/") {
            urls.push(format!("{}/EWS/Exchange.asmx", &u[..i]));
        }
    }
    for d in domains {
        for h in [format!("autodiscover.{d}"), format!("mail.{d}"), format!("webmail.{d}"), format!("exchange.{d}")] {
            urls.push(format!("https://{h}/EWS/Exchange.asmx"));
        }
    }
    // Aranacak ad: "KURUM\ali" ve "ali@kurum.local" Exchange'de doğrudan çözülür
    let short = login.rsplit('\\').next().unwrap_or(login).split('@').next().unwrap_or(login).to_lowercase();
    for url in urls {
        step(crate::t!("discover.step_mailbox", url.split('/').nth(2).unwrap_or(&url)));
        let mut ews = super::ews::Ews::with_auth(&url, auth.clone(), uses_password);
        match ews.resolve_names(login) {
            Ok(list) => {
                let norm = |s: &str| s.to_lowercase().replace(['.', '_', '-'], "");
                let pick = if list.len() == 1 {
                    list.first().cloned()
                } else {
                    list.iter().find(|(_, e)| norm(e.split('@').next().unwrap_or("")) == norm(&short)).cloned()
                };
                if let Some((name, email)) = pick {
                    note(log, format!("{url}: {login} → {email}"));
                    return Ok(Some((email, name, url)));
                }
                note(log, format!("{url}: {login} için posta kutusu bulunamadı"));
                return Ok(None);
            }
            Err(e) if e.is_auth() => {
                note(log, format!("{url}: {}", e.message()));
                return Err(e);
            }
            Err(e) => note(log, format!("{url}: {}", e.message())),
        }
    }
    Ok(None)
}

/// Tüm yöntemleri sırayla dener. Parola verilmezse ve Kerberos bileti varsa
/// Kerberos kullanılır (tek oturum açma). `user` boşsa bilgisayara giriş yapılan
/// etki alanı kullanıcısı kullanılır; `email` boşsa dizinden/Exchange'den bulunur.
/// Kurulum ilerlemesi (arayüzde "Hesabınız ayarlanıyor" altında adım adım gösterilir)
static PROGRESS: std::sync::Mutex<Option<Box<dyn Fn(&str) + Send>>> = std::sync::Mutex::new(None);

pub fn set_progress(f: Option<Box<dyn Fn(&str) + Send>>) {
    *PROGRESS.lock().unwrap_or_else(|e| e.into_inner()) = f;
}

/// Ekranda gösterilecek adım
pub fn step(msg: impl AsRef<str>) {
    if let Some(f) = PROGRESS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        f(msg.as_ref());
    }
}

/// Günlüğe yazar ve adımı gösterir
fn note(log: &mut Vec<String>, msg: impl Into<String>) {
    let m = msg.into();
    step(&m);
    log.push(m);
}

pub fn discover(email: &str, user: &str, password: &str) -> Discovery {
    let mut log = Vec::new();
    let kerberos = password.is_empty() && super::gssapi::current_principal().is_some();
    let default_login = super::gssapi::default_login();
    let login = if !user.is_empty() {
        user.to_string()
    } else if !default_login.is_empty() {
        default_login
    } else {
        email.to_string()
    };
    let auth = if kerberos {
        Auth::Negotiate
    } else if !password.is_empty() {
        Auth::Auto { user: login.clone(), password: password.into(), kerberos: false }
    } else {
        Auth::None
    };
    let username = if kerberos { String::new() } else { login.clone() };
    let failed = |log: Vec<String>| Discovery { email: email.into(), username: username.clone(), auth_failed: true, log, ..Default::default() };

    // 1) E-posta adresi: verilmediyse dizinden (AD) ya da Exchange'den
    let mut email = email.trim().to_string();
    let mut display_name = String::new();
    let mut ews_hint = String::new();
    if email.is_empty() {
        if kerberos {
            if let Some(p) = super::gssapi::current_principal() {
                step(crate::t!("discover.step_directory"));
                if let Some(u) = super::ldap::lookup_principal(&p, &mut log) {
                    note(&mut log, format!("Dizin: {} → {}", p, u.email));
                    email = u.email;
                    display_name = u.display_name;
                }
            }
        }
        if email.is_empty() && !matches!(auth, Auth::None) {
            match mailbox_via_ews(&login, &auth, !kerberos, &candidate_domains(&login, ""), &mut log) {
                Ok(Some((e, n, url))) => {
                    email = e;
                    display_name = n;
                    ews_hint = url;
                }
                Ok(None) => {}
                Err(_) => return failed(log),
            }
        }
        if email.is_empty() {
            return Discovery { need_email: true, username, log, ..Default::default() };
        }
    }

    // 2) Exchange Autodiscover
    let consumer = guess_servers(&email).note.starts_with("provider.");
    if !consumer {
        match exchange(&email, auth.clone(), &mut log) {
            Ok(mut d) => {
                d.auth_method = if d.office365 { "oauth" } else if kerberos { "kerberos" } else { "password" }.into();
                if d.display_name.is_empty() {
                    d.display_name = display_name.clone();
                }
                d.username = username.clone();
                d.log = log;
                return d;
            }
            Err(e) if e.is_auth() && !matches!(auth, Auth::None) && !kerberos => {
                note(&mut log, e.to_string());
                return failed(log);
            }
            Err(e) if e.is_auth() => {
                // Autodiscover 401 verdi: sunucu Exchange, yalnızca oturum gerekiyor.
                // Kerberos yoksa ya da sunucu kabul etmediyse kullanıcı adı/parola istenir
                // (IMAP tahminine geçilmez).
                note(&mut log, e.to_string());
                return Discovery {
                    kind: "exchange".into(),
                    email,
                    display_name,
                    ews_url: ews_hint,
                    auth_method: "password".into(),
                    username: if user.is_empty() { String::new() } else { user.to_string() },
                    need_credentials: true,
                    source: "autodiscover".into(),
                    log,
                    ..Default::default()
                };
            }
            Err(e) => note(&mut log, e.to_string()),
        }
        if !ews_hint.is_empty() {
            return Discovery {
                kind: "exchange".into(),
                email,
                display_name,
                ews_url: ews_hint,
                auth_method: if kerberos { "kerberos" } else { "password" }.into(),
                username,
                source: "ews".into(),
                log,
                ..Default::default()
            };
        }
    }
    if let Some(mut d) = autoconfig(&email, &mut log) {
        d.username = if user.is_empty() { String::new() } else { user.to_string() };
        d.log = log;
        return d;
    }
    let g = guess_servers(&email);
    Discovery {
        kind: "imap".into(),
        email,
        display_name,
        imap_host: g.imap_host,
        imap_port: g.imap_port,
        imap_security: g.imap_security,
        smtp_host: g.smtp_host,
        smtp_port: g.smtp_port,
        smtp_security: g.smtp_security,
        auth_method: if kerberos { "kerberos" } else { "password" }.into(),
        username: if user.is_empty() { String::new() } else { user.to_string() },
        source: "guess".into(),
        log,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_autodiscover_response() {
        let x = br#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
<Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
<User><DisplayName>Ali Veli</DisplayName><AutoDiscoverSMTPAddress>ali.veli@kurum.gov.tr</AutoDiscoverSMTPAddress></User>
<Account><AccountType>email</AccountType><Action>settings</Action>
<Protocol><Type>EXCH</Type><Server>EX01.kurum.local</Server><EwsUrl>https://mail.kurum.gov.tr/EWS/Exchange.asmx</EwsUrl></Protocol>
<Protocol><Type>EXPR</Type><EwsUrl>https://dis.kurum.gov.tr/EWS/Exchange.asmx</EwsUrl></Protocol>
</Account></Response></Autodiscover>"#;
        let doc = super::xml::parse(x).unwrap();
        let mut p = Vec::new();
        doc.find_all("Protocol", &mut p);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].text_of("EwsUrl"), "https://mail.kurum.gov.tr/EWS/Exchange.asmx");
        assert_eq!(doc.find("User").unwrap().text_of("DisplayName"), "Ali Veli");
    }
}

#[cfg(test)]
mod progress_tests {
    #[test]
    fn note_records_step() {
        let mut log = Vec::new();
        super::note(&mut log, "adım");
        assert_eq!(log, vec!["adım".to_string()]);
    }
}
