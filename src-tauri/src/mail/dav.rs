//! WebDAV istemcisi: CalDAV (RFC 4791) ve CardDAV (RFC 6352). Yalnızca iletişimi yapar;
//! eşitleme `dav_sync.rs` içindedir. Değişiklikler ETag karşılaştırmasıyla bulunur, bu yüzden
//! `sync-collection` desteklemeyen sunucularda da aynı yol çalışır.

use super::http::{Auth, Client, Response, Url};
use super::xml::{self, Element};
use super::{MailError, MailResult};
use crate::db::Account;
use crate::t;

const XML_TYPE: &str = "application/xml; charset=utf-8";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Calendar,
    Contacts,
}

impl Kind {
    fn ns(self) -> &'static str {
        match self {
            Kind::Calendar => "urn:ietf:params:xml:ns:caldav",
            Kind::Contacts => "urn:ietf:params:xml:ns:carddav",
        }
    }

    /// Koleksiyonu tanıtan özellikler
    fn props(self) -> &'static str {
        match self {
            Kind::Calendar => "<d:resourcetype/><d:displayname/><d:current-user-principal/><c:calendar-home-set/><c:supported-calendar-component-set/><a:calendar-color/><cs:getctag/><d:sync-token/>",
            Kind::Contacts => "<d:resourcetype/><d:displayname/><d:current-user-principal/><c:addressbook-home-set/><cs:getctag/><d:sync-token/>",
        }
    }

    fn well_known(self) -> &'static str {
        match self {
            Kind::Calendar => "/.well-known/caldav",
            Kind::Contacts => "/.well-known/carddav",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Kind::Calendar => "text/calendar; charset=utf-8",
            Kind::Contacts => "text/vcard; charset=utf-8",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Kind::Calendar => "ics",
            Kind::Contacts => "vcf",
        }
    }
}

/// Çoklu durum yanıtındaki bir kaynak
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub href: String,
    pub etag: String,
    pub name: String,
    pub color: String,
    /// Koleksiyon değişti mi anlamak için (getctag ya da sync-token)
    pub ctag: String,
    pub is_collection: bool,
    /// Takvim ya da adres defteri koleksiyonu
    pub is_target: bool,
    /// Takvimin desteklediği bileşenler (VEVENT, VTODO…); boşsa bilinmiyor
    pub components: Vec<String>,
    pub principal: String,
    pub home: String,
    /// calendar-data / address-data
    pub data: String,
}

fn entry(r: &Element, kind: Kind) -> Entry {
    let mut e = Entry { href: r.text_of("href").trim().to_string(), ..Default::default() };
    for stat in r.children_named("propstat") {
        // 404 propstat'ı özelliğin bu kaynakta olmadığını söyler
        if !stat.text_of("status").contains(" 200") {
            continue;
        }
        let Some(p) = stat.child("prop") else { continue };
        let href_in = |name: &str| p.child(name).map(|c| c.text_of("href").trim().to_string()).unwrap_or_default();
        e.etag = p.text_of("getetag").trim().to_string();
        e.name = p.text_of("displayname").trim().to_string();
        e.color = p.text_of("calendar-color").trim().chars().take(7).collect();
        e.ctag = Some(p.text_of("getctag")).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| p.text_of("sync-token")).trim().to_string();
        e.principal = href_in("current-user-principal");
        e.home = href_in(if kind == Kind::Calendar { "calendar-home-set" } else { "addressbook-home-set" });
        if let Some(t) = p.child("resourcetype") {
            e.is_collection = t.child("collection").is_some();
            e.is_target = t.child(if kind == Kind::Calendar { "calendar" } else { "addressbook" }).is_some();
        }
        if let Some(set) = p.child("supported-calendar-component-set") {
            e.components = set.children_named("comp").filter_map(|c| c.attr("name").map(|n| n.to_uppercase())).collect();
        }
        e.data = p.text_of(if kind == Kind::Calendar { "calendar-data" } else { "address-data" });
    }
    e
}

pub fn parse_multistatus(body: &[u8], kind: Kind) -> MailResult<Vec<Entry>> {
    let root = xml::parse(body).map_err(MailError)?;
    let mut responses = Vec::new();
    root.find_all("response", &mut responses);
    Ok(responses.into_iter().map(|r| entry(r, kind)).collect())
}

/// Yazma koşulu
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Precond<'a> {
    /// Yeni kaynak: aynı adreste bir şey varsa yazılmaz
    New,
    /// Yalnızca sunucudaki sürüm hâlâ buysa yazılır
    Match(&'a str),
    /// Koşulsuz (son yazan kazanır)
    Any,
}

#[derive(Debug, PartialEq)]
pub enum Put {
    /// Yazıldı; yeni ETag (sunucu bildirmediyse boş)
    Saved(String),
    /// Sunucudaki sürüm değişmiş
    Conflict,
}

pub struct Dav {
    http: Client,
    base: Url,
}

impl Dav {
    pub fn new(url: &str, auth: Auth) -> MailResult<Dav> {
        let url = url.trim();
        let full = if url.contains("://") { url.to_string() } else { format!("https://{url}") };
        Ok(Dav { http: Client::new(auth), base: Url::parse(&full)? })
    }

    /// Parola verilirse o kullanılır (hesap kaydedilmeden denemek için)
    pub fn for_account(acc: &Account, url: &str, password: Option<&str>) -> MailResult<Dav> {
        let auth = match acc.auth_method.as_str() {
            "oauth" => Auth::Bearer(crate::oauth::access_token(acc)?),
            "kerberos" => Auth::Negotiate,
            _ => Auth::Auto {
                user: acc.login().to_string(),
                password: password.map(String::from).unwrap_or_else(|| acc.password_plain()),
                kerberos: false,
            },
        };
        Dav::new(url, auth)
    }

    /// Sunucunun verdiği adres (yol ya da tam adres) → tam adres
    pub fn abs(&self, href: &str) -> String {
        if href.starts_with("http://") || href.starts_with("https://") {
            return href.to_string();
        }
        Url { path: href.to_string(), ..self.base.clone() }.to_string()
    }

    fn call(&mut self, method: &str, url: &str, headers: &[(&str, &str)], content_type: &str, body: &[u8]) -> MailResult<Response> {
        self.http.extra_headers = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let res = self.http.request(method, url, content_type, body);
        self.http.extra_headers.clear();
        let res = res?;
        if res.status == 401 {
            return Err(MailError(t!("error.dav_auth")));
        }
        Ok(res)
    }

    fn multistatus(&mut self, method: &str, url: &str, depth: &str, body: &str, kind: Kind) -> MailResult<Vec<Entry>> {
        let r = self.call(method, url, &[("Depth", depth)], XML_TYPE, body.as_bytes())?;
        if r.status != 207 {
            return Err(MailError(t!("error.dav_status", r.status)));
        }
        parse_multistatus(&r.body, kind)
    }

    fn propfind(&mut self, url: &str, depth: &str, kind: Kind, props: &str) -> MailResult<Vec<Entry>> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="{}" xmlns:a="http://apple.com/ns/ical/" xmlns:cs="http://calendarserver.org/ns/"><d:prop>{props}</d:prop></d:propfind>"#,
            kind.ns()
        );
        self.multistatus("PROPFIND", url, depth, &body, kind)
    }

    /// Verilen adres koleksiyonun kendisi olabilir; değilse kullanıcının ana klasörü bulunup
    /// içindeki takvim ya da adres defterleri listelenir.
    pub fn discover(&mut self, kind: Kind) -> MailResult<Vec<Entry>> {
        let start = self.base.to_string();
        let here = match self.propfind(&start, "0", kind, kind.props()) {
            Ok(v) if !v.is_empty() => v.into_iter().next().unwrap_or_default(),
            // Yalnızca sunucu adı yazılmışsa: RFC 6764 bilinen adresi
            first => {
                let known = self.abs(kind.well_known());
                match self.propfind(&known, "0", kind, kind.props()) {
                    Ok(v) if !v.is_empty() => v.into_iter().next().unwrap_or_default(),
                    _ => return first.and_then(|_| Err(MailError(t!("error.dav_none")))),
                }
            }
        };
        if here.is_target {
            return Ok(vec![Entry { href: if here.href.is_empty() { self.base.path.clone() } else { here.href.clone() }, ..here }]);
        }
        let mut home = here.home.clone();
        if home.is_empty() && !here.principal.is_empty() {
            let principal = self.abs(&here.principal);
            home = self.propfind(&principal, "0", kind, kind.props())?.into_iter().next().map(|e| e.home).unwrap_or_default();
        }
        if home.is_empty() {
            home = if here.href.is_empty() { self.base.path.clone() } else { here.href };
        }
        let url = self.abs(&home);
        let found: Vec<Entry> = self
            .propfind(&url, "1", kind, kind.props())?
            .into_iter()
            .filter(|e| e.is_target && (e.components.is_empty() || e.components.iter().any(|c| c == "VEVENT")))
            .collect();
        if found.is_empty() {
            return Err(MailError(t!("error.dav_none")));
        }
        Ok(found)
    }

    pub fn ctag(&mut self, collection: &str, kind: Kind) -> MailResult<String> {
        let url = self.abs(collection);
        Ok(self.propfind(&url, "0", kind, "<cs:getctag/><d:sync-token/>")?.into_iter().next().map(|e| e.ctag).unwrap_or_default())
    }

    /// Koleksiyondaki kaynaklar (adres ve ETag). Takvimde yalnızca etkinlikler (görevler değil).
    pub fn list(&mut self, collection: &str, kind: Kind) -> MailResult<Vec<Entry>> {
        let url = self.abs(collection);
        let items = match kind {
            Kind::Calendar => {
                let body = format!(
                    r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="{}"><d:prop><d:getetag/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"/></c:comp-filter></c:filter></c:calendar-query>"#,
                    kind.ns()
                );
                self.multistatus("REPORT", &url, "1", &body, kind)?
            }
            Kind::Contacts => self.propfind(&url, "1", kind, "<d:getetag/><d:resourcetype/>")?,
        };
        Ok(items.into_iter().filter(|e| !e.is_collection && !e.href.is_empty() && !e.etag.is_empty()).collect())
    }

    /// Kaynakların içeriği: tek istekte (multiget), sunucu vermezse tek tek GET
    pub fn fetch(&mut self, collection: &str, kind: Kind, hrefs: &[String]) -> MailResult<Vec<Entry>> {
        let url = self.abs(collection);
        let list: String = hrefs.iter().map(|h| format!("<d:href>{}</d:href>", quick_xml::escape::escape(h.as_str()))).collect();
        let (report, data) = match kind {
            Kind::Calendar => ("calendar-multiget", "calendar-data"),
            Kind::Contacts => ("addressbook-multiget", "address-data"),
        };
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><c:{report} xmlns:d="DAV:" xmlns:c="{}"><d:prop><d:getetag/><c:{data}/></d:prop>{list}</c:{report}>"#,
            kind.ns()
        );
        if let Ok(v) = self.multistatus("REPORT", &url, "1", &body, kind) {
            let got: Vec<Entry> = v.into_iter().filter(|e| !e.data.trim().is_empty()).collect();
            if !got.is_empty() {
                return Ok(got);
            }
        }
        let mut out = Vec::new();
        for h in hrefs {
            let u = self.abs(h);
            let r = self.call("GET", &u, &[], "", &[])?;
            if r.status == 200 {
                out.push(Entry { href: h.clone(), etag: r.header("ETag").unwrap_or("").to_string(), data: r.text(), ..Default::default() });
            }
        }
        Ok(out)
    }

    /// Koleksiyonda yeni bir kaynak adresi
    pub fn new_href(collection: &str, kind: Kind) -> String {
        format!("{}/{}.{}", collection.trim_end_matches('/'), uuid::Uuid::new_v4().simple(), kind.extension())
    }

    pub fn put(&mut self, href: &str, kind: Kind, body: &str, cond: Precond) -> MailResult<Put> {
        let url = self.abs(href);
        let headers: Vec<(&str, &str)> = match cond {
            Precond::New => vec![("If-None-Match", "*")],
            Precond::Match(etag) => vec![("If-Match", etag)],
            Precond::Any => vec![],
        };
        let r = self.call("PUT", &url, &headers, kind.content_type(), body.as_bytes())?;
        match r.status {
            200..=299 => Ok(Put::Saved(r.header("ETag").unwrap_or("").to_string())),
            412 => Ok(Put::Conflict),
            s => Err(MailError(t!("error.dav_status", s))),
        }
    }

    /// Zaten silinmiş kaynak hata sayılmaz
    pub fn delete(&mut self, href: &str) -> MailResult<()> {
        let url = self.abs(href);
        let r = self.call("DELETE", &url, &[], "", &[])?;
        match r.status {
            200..=299 | 404 | 410 => Ok(()),
            s => Err(MailError(t!("error.dav_status", s))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALENDARS: &str = r##"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:x="http://apple.com/ns/ical/">
 <d:response><d:href>/dav/ali/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/dav/ali/is/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>İş</d:displayname><x:calendar-color>#1f6fd1ff</x:calendar-color>
   <c:supported-calendar-component-set><c:comp name="VEVENT"/><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:getetag/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
 </d:response>
</d:multistatus>"##;

    #[test]
    fn parses_collections() {
        let v = parse_multistatus(CALENDARS.as_bytes(), Kind::Calendar).unwrap();
        assert_eq!(v.len(), 2);
        assert!(v[0].is_collection && !v[0].is_target);
        let c = &v[1];
        assert!(c.is_target);
        assert_eq!((c.href.as_str(), c.name.as_str(), c.color.as_str()), ("/dav/ali/is/", "İş", "#1f6fd1"));
        assert_eq!(c.components, ["VEVENT", "VTODO"]);
    }

    #[test]
    fn parses_data_and_principal() {
        let xml = r#"<multistatus xmlns="DAV:"><response><href>/a/1.ics</href><propstat><prop><getetag>"e1"</getetag>
          <calendar-data xmlns="urn:ietf:params:xml:ns:caldav"><![CDATA[BEGIN:VCALENDAR
SUMMARY:A &amp; B
END:VCALENDAR]]></calendar-data></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
        let v = parse_multistatus(xml.as_bytes(), Kind::Calendar).unwrap();
        assert_eq!(v[0].etag, "\"e1\"");
        assert!(v[0].data.contains("SUMMARY:A &amp; B"), "{:?}", v[0].data);
        let xml = r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav"><d:response><d:href>/</d:href><d:propstat><d:prop>
          <d:current-user-principal><d:href>/principals/ali/</d:href></d:current-user-principal><c:addressbook-home-set><d:href>/books/ali/</d:href></c:addressbook-home-set>
          </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
        let v = parse_multistatus(xml.as_bytes(), Kind::Contacts).unwrap();
        assert_eq!((v[0].principal.as_str(), v[0].home.as_str()), ("/principals/ali/", "/books/ali/"));
    }

    #[test]
    fn urls() {
        let d = Dav::new("cloud.ornek.com/dav/", Auth::None).unwrap();
        assert_eq!(d.abs("/dav/a/1.ics"), "https://cloud.ornek.com/dav/a/1.ics");
        assert_eq!(d.abs("https://b.com/x"), "https://b.com/x");
        let h = Dav::new_href("/dav/a/", Kind::Contacts);
        assert!(h.starts_with("/dav/a/") && h.ends_with(".vcf") && !h.contains("//"));
    }
}
