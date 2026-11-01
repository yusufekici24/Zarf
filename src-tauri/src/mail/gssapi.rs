//! Kerberos tek oturum açma (GSSAPI).
//!
//! Etki alanına (Active Directory / FreeIPA) katılmış Pardus makinelerinde kullanıcı
//! oturum açtığında SSSD bir Kerberos bileti alır. MailNix bu bileti kullanarak
//! IMAP/SMTP (SASL GSSAPI) ve Exchange (HTTP Negotiate) sunucularına parola
//! sormadan ve parola saklamadan bağlanır. AD parolası değiştiğinde bilet oturum
//! açılışında yenilendiği için hesap kilitlenmesi riski de yoktur.
//!
//! `libgssapi_krb5` çalışma anında yüklenir; sistemde yoksa yalnızca bu özellik
//! devre dışı kalır.

use super::{MailError, MailResult};
use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::sync::OnceLock;

type OM = u32;
type Ptr = *mut c_void;

#[repr(C)]
struct Buffer {
    length: usize,
    value: *mut c_void,
}

#[repr(C)]
struct Oid {
    length: OM,
    elements: *const u8,
}

const GSS_C_NT_HOSTBASED_SERVICE: &[u8] = b"\x2a\x86\x48\x86\xf7\x12\x01\x02\x01\x04";
const MECH_KRB5: &[u8] = b"\x2a\x86\x48\x86\xf7\x12\x01\x02\x02";
const MECH_SPNEGO: &[u8] = b"\x2b\x06\x01\x05\x05\x02";

const GSS_C_MUTUAL_FLAG: OM = 2;
const GSS_C_SEQUENCE_FLAG: OM = 8;
const GSS_C_INTEG_FLAG: OM = 32;
const GSS_C_INITIATE: i32 = 1;
const GSS_S_CONTINUE_NEEDED: OM = 1;

fn is_error(major: OM) -> bool {
    major & 0xffff_0000 != 0
}

type ImportName = unsafe extern "C" fn(*mut OM, *const Buffer, *const Oid, *mut Ptr) -> OM;
type InitSec = unsafe extern "C" fn(
    *mut OM, Ptr, *mut Ptr, Ptr, *const Oid, OM, OM, Ptr, *const Buffer, *mut *const Oid, *mut Buffer, *mut OM, *mut OM,
) -> OM;
type ReleaseBuffer = unsafe extern "C" fn(*mut OM, *mut Buffer) -> OM;
type ReleaseName = unsafe extern "C" fn(*mut OM, *mut Ptr) -> OM;
type DeleteSec = unsafe extern "C" fn(*mut OM, *mut Ptr, *mut Buffer) -> OM;
type Wrap = unsafe extern "C" fn(*mut OM, Ptr, i32, OM, *const Buffer, *mut i32, *mut Buffer) -> OM;
type Unwrap = unsafe extern "C" fn(*mut OM, Ptr, *const Buffer, *mut Buffer, *mut i32, *mut OM) -> OM;
type AcquireCred = unsafe extern "C" fn(*mut OM, Ptr, OM, Ptr, i32, *mut Ptr, Ptr, *mut OM) -> OM;
type InquireCred = unsafe extern "C" fn(*mut OM, Ptr, *mut Ptr, *mut OM, *mut i32, Ptr) -> OM;
type DisplayName = unsafe extern "C" fn(*mut OM, Ptr, *mut Buffer, *mut *const Oid) -> OM;
type ReleaseCred = unsafe extern "C" fn(*mut OM, *mut Ptr) -> OM;
type DisplayStatus = unsafe extern "C" fn(*mut OM, OM, i32, *const Oid, *mut OM, *mut Buffer) -> OM;

struct Lib {
    lib: Library,
}

fn lib() -> Option<&'static Lib> {
    static LIB: OnceLock<Option<Lib>> = OnceLock::new();
    LIB.get_or_init(|| {
        for name in ["libgssapi_krb5.so.2", "libgssapi_krb5.so", "libgssapi.so.3"] {
            // SAFETY: sistem GSSAPI kitaplığı; yalnızca belgelenmiş C API kullanılır.
            if let Ok(lib) = unsafe { Library::new(name) } {
                return Some(Lib { lib });
            }
        }
        None
    })
    .as_ref()
}

impl Lib {
    fn sym<T>(&self, name: &[u8]) -> MailResult<Symbol<'_, T>> {
        // SAFETY: imzalar RFC 2744 C bağlamalarıyla birebir aynıdır.
        unsafe { self.lib.get::<T>(name) }.map_err(|e| MailError(format!("GSSAPI: {e}")))
    }

    fn status_text(&self, major: OM, minor: OM) -> String {
        let Ok(display) = self.sym::<DisplayStatus>(b"gss_display_status\0") else {
            return format!("{major:#x}/{minor}");
        };
        let Ok(release) = self.sym::<ReleaseBuffer>(b"gss_release_buffer\0") else {
            return format!("{major:#x}/{minor}");
        };
        let mut parts = Vec::new();
        for (code, kind) in [(major, 1), (minor, 2)] {
            if code == 0 {
                continue;
            }
            let mut ctx: OM = 0;
            loop {
                let mut m: OM = 0;
                let mut buf = Buffer { length: 0, value: std::ptr::null_mut() };
                // SAFETY: çıktı arabelleği kitaplık tarafından ayrılır ve aşağıda serbest bırakılır.
                let r = unsafe { display(&mut m, code, kind, std::ptr::null(), &mut ctx, &mut buf) };
                if is_error(r) {
                    break;
                }
                parts.push(buf_to_string(&buf));
                unsafe { release(&mut m, &mut buf) };
                if ctx == 0 {
                    break;
                }
            }
        }
        parts.join(": ")
    }
}

fn buf_to_vec(b: &Buffer) -> Vec<u8> {
    if b.value.is_null() || b.length == 0 {
        return Vec::new();
    }
    // SAFETY: kitaplık `length` baytlık geçerli bir bellek döndürür.
    unsafe { std::slice::from_raw_parts(b.value as *const u8, b.length) }.to_vec()
}

fn buf_to_string(b: &Buffer) -> String {
    String::from_utf8_lossy(&buf_to_vec(b)).trim_end_matches('\0').to_string()
}

/// Sistemde GSSAPI kitaplığı var mı?
pub fn available() -> bool {
    lib().is_some()
}

/// Geçerli Kerberos biletinin sahibi (ör. "ali@KURUM.GOV.TR"); bilet yoksa None.
pub fn current_principal() -> Option<String> {
    let l = lib()?;
    let acquire = l.sym::<AcquireCred>(b"gss_acquire_cred\0").ok()?;
    let inquire = l.sym::<InquireCred>(b"gss_inquire_cred\0").ok()?;
    let display = l.sym::<DisplayName>(b"gss_display_name\0").ok()?;
    let rel_name = l.sym::<ReleaseName>(b"gss_release_name\0").ok()?;
    let rel_cred = l.sym::<ReleaseCred>(b"gss_release_cred\0").ok()?;
    let rel_buf = l.sym::<ReleaseBuffer>(b"gss_release_buffer\0").ok()?;
    let mut minor: OM = 0;
    let mut cred: Ptr = std::ptr::null_mut();
    // SAFETY: tüm işaretçiler ya null ya da bu işlevde ayrılan yerel değişkenlerdir.
    unsafe {
        let r = acquire(&mut minor, std::ptr::null_mut(), 0, std::ptr::null_mut(), GSS_C_INITIATE, &mut cred, std::ptr::null_mut(), std::ptr::null_mut());
        if is_error(r) {
            return None;
        }
        let mut name: Ptr = std::ptr::null_mut();
        let mut lifetime: OM = 0;
        let r = inquire(&mut minor, cred, &mut name, &mut lifetime, std::ptr::null_mut(), std::ptr::null_mut());
        let mut out = None;
        if !is_error(r) && lifetime > 0 {
            let mut buf = Buffer { length: 0, value: std::ptr::null_mut() };
            if !is_error(display(&mut minor, name, &mut buf, std::ptr::null_mut())) {
                out = Some(buf_to_string(&buf));
                rel_buf(&mut minor, &mut buf);
            }
            rel_name(&mut minor, &mut name);
        }
        rel_cred(&mut minor, &mut cred);
        out
    }
}

#[derive(Clone, Copy)]
pub enum Mech {
    Kerberos,
    Spnego,
}

/// Bir sunucuya karşı GSSAPI güvenlik bağlamı (istemci tarafı).
pub struct Client {
    lib: &'static Lib,
    ctx: Ptr,
    target: Ptr,
    mech: Mech,
    pub complete: bool,
}

// Bağlam yalnızca tek iş parçacığında kullanılır; kitaplık iş parçacığı güvenlidir.
unsafe impl Send for Client {}

impl Client {
    /// `service`: "imap", "smtp" veya "HTTP"; `host`: sunucunun tam adı.
    pub fn new(service: &str, host: &str, mech: Mech) -> MailResult<Client> {
        let lib = lib().ok_or_else(|| MailError(crate::t!("error.gssapi_missing")))?;
        let import = lib.sym::<ImportName>(b"gss_import_name\0")?;
        let name = format!("{service}@{host}");
        let buf = Buffer { length: name.len(), value: name.as_ptr() as *mut c_void };
        let oid = Oid { length: GSS_C_NT_HOSTBASED_SERVICE.len() as OM, elements: GSS_C_NT_HOSTBASED_SERVICE.as_ptr() };
        let mut minor: OM = 0;
        let mut target: Ptr = std::ptr::null_mut();
        // SAFETY: giriş arabelleği `name` yaşadığı sürece geçerlidir.
        let r = unsafe { import(&mut minor, &buf, &oid, &mut target) };
        if is_error(r) {
            return Err(MailError(format!("GSSAPI: {}", lib.status_text(r, minor))));
        }
        Ok(Client { lib, ctx: std::ptr::null_mut(), target, mech, complete: false })
    }

    /// Sunucudan gelen belirteci işler ve gönderilecek belirteci döndürür.
    pub fn step(&mut self, input: &[u8]) -> MailResult<Vec<u8>> {
        let init = self.lib.sym::<InitSec>(b"gss_init_sec_context\0")?;
        let release = self.lib.sym::<ReleaseBuffer>(b"gss_release_buffer\0")?;
        let mech_bytes = match self.mech {
            Mech::Kerberos => MECH_KRB5,
            Mech::Spnego => MECH_SPNEGO,
        };
        let mech = Oid { length: mech_bytes.len() as OM, elements: mech_bytes.as_ptr() };
        let in_buf = Buffer { length: input.len(), value: input.as_ptr() as *mut c_void };
        let mut out = Buffer { length: 0, value: std::ptr::null_mut() };
        let mut minor: OM = 0;
        let mut ret_flags: OM = 0;
        // SAFETY: bağlam ve hedef ad bu yapıya aittir; çıktı arabelleği serbest bırakılır.
        let r = unsafe {
            init(
                &mut minor,
                std::ptr::null_mut(),
                &mut self.ctx,
                self.target,
                &mech,
                GSS_C_MUTUAL_FLAG | GSS_C_SEQUENCE_FLAG | GSS_C_INTEG_FLAG,
                0,
                std::ptr::null_mut(),
                if input.is_empty() { std::ptr::null() } else { &in_buf },
                std::ptr::null_mut(),
                &mut out,
                &mut ret_flags,
                std::ptr::null_mut(),
            )
        };
        let token = buf_to_vec(&out);
        unsafe { release(&mut minor, &mut out) };
        if is_error(r) {
            // Bilet yok/süresi dolmuş: bu bir parola hatası değildir, hesap kilitlenmez.
            return Err(MailError(crate::t!("error.kerberos", self.lib.status_text(r, minor))));
        }
        self.complete = r & GSS_S_CONTINUE_NEEDED == 0;
        Ok(token)
    }

    pub fn wrap(&self, data: &[u8]) -> MailResult<Vec<u8>> {
        let wrap = self.lib.sym::<Wrap>(b"gss_wrap\0")?;
        let release = self.lib.sym::<ReleaseBuffer>(b"gss_release_buffer\0")?;
        let input = Buffer { length: data.len(), value: data.as_ptr() as *mut c_void };
        let mut out = Buffer { length: 0, value: std::ptr::null_mut() };
        let mut minor: OM = 0;
        let mut conf: i32 = 0;
        let r = unsafe { wrap(&mut minor, self.ctx, 0, 0, &input, &mut conf, &mut out) };
        let v = buf_to_vec(&out);
        unsafe { release(&mut minor, &mut out) };
        if is_error(r) {
            return Err(MailError(format!("GSSAPI wrap: {}", self.lib.status_text(r, minor))));
        }
        Ok(v)
    }

    pub fn unwrap(&self, data: &[u8]) -> MailResult<Vec<u8>> {
        let unwrap = self.lib.sym::<Unwrap>(b"gss_unwrap\0")?;
        let release = self.lib.sym::<ReleaseBuffer>(b"gss_release_buffer\0")?;
        let input = Buffer { length: data.len(), value: data.as_ptr() as *mut c_void };
        let mut out = Buffer { length: 0, value: std::ptr::null_mut() };
        let mut minor: OM = 0;
        let r = unsafe { unwrap(&mut minor, self.ctx, &input, &mut out, std::ptr::null_mut(), std::ptr::null_mut()) };
        let v = buf_to_vec(&out);
        unsafe { release(&mut minor, &mut out) };
        if is_error(r) {
            return Err(MailError(format!("GSSAPI unwrap: {}", self.lib.status_text(r, minor))));
        }
        Ok(v)
    }

    /// RFC 4752 SASL GSSAPI'nin son adımı: sunucunun güvenlik katmanı teklifini
    /// açar, "güvenlik katmanı yok" (1) seçimini ve yetki kimliğini sarar.
    pub fn sasl_final(&self, challenge: &[u8], authzid: &str) -> MailResult<Vec<u8>> {
        let offer = self.unwrap(challenge)?;
        if offer.is_empty() || offer[0] & 1 == 0 {
            return Err(MailError("GSSAPI: sunucu güvenlik katmansız oturumu desteklemiyor".into()));
        }
        let mut resp = vec![1u8, 0, 0, 0];
        resp.extend_from_slice(authzid.as_bytes());
        self.wrap(&resp)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let mut minor: OM = 0;
        if let Ok(del) = self.lib.sym::<DeleteSec>(b"gss_delete_sec_context\0") {
            if !self.ctx.is_null() {
                unsafe { del(&mut minor, &mut self.ctx, std::ptr::null_mut()) };
            }
        }
        if let Ok(rel) = self.lib.sym::<ReleaseName>(b"gss_release_name\0") {
            if !self.target.is_null() {
                unsafe { rel(&mut minor, &mut self.target) };
            }
        }
    }
}

/// SASL GSSAPI kimlik doğrulama durum makinesi (IMAP ve SMTP ortak).
pub struct SaslGssapi {
    client: Client,
    started: bool,
    authzid: String,
}

impl SaslGssapi {
    pub fn new(service: &str, host: &str, authzid: &str) -> MailResult<Self> {
        Ok(SaslGssapi { client: Client::new(service, host, Mech::Kerberos)?, started: false, authzid: authzid.to_string() })
    }

    /// Sunucu meydan okumasına (çözülmüş) yanıt üretir.
    pub fn respond(&mut self, challenge: &[u8]) -> MailResult<Vec<u8>> {
        if !self.started {
            self.started = true;
            return self.client.step(&[]);
        }
        if !self.client.complete {
            return self.client.step(challenge);
        }
        if challenge.is_empty() {
            return Ok(Vec::new());
        }
        self.client.sasl_final(challenge, &self.authzid)
    }
}

/// Oturum açmış kullanıcının bilgileri (otomatik hesap kurulumu için).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SystemIdentity {
    pub username: String,
    pub full_name: String,
    pub principal: String,
    pub realm: String,
    pub domain: String,
    pub email_guess: String,
    pub kerberos: bool,
    /// Adres nereden bulundu: "directory" (AD/LDAP), "guess" (tahmin)
    pub email_source: String,
    /// Exchange/AD oturum adı (ör. "ali@KURUM.LOCAL"); kullanıcı adı boş bırakılınca kullanılır
    pub login: String,
    pub log: Vec<String>,
}

pub fn system_identity() -> SystemIdentity {
    let username = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let mut id = SystemIdentity { username: username.clone(), ..Default::default() };
    // Tam ad: /etc/passwd GECOS alanı (SSSD kullanıcıları için getent)
    let gecos = std::process::Command::new("getent")
        .args(["passwd", &username])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    if let Some(g) = gecos.split(':').nth(4) {
        id.full_name = g.split(',').next().unwrap_or("").trim().to_string();
    }
    if let Some(p) = current_principal() {
        id.kerberos = true;
        id.principal = p.clone();
        if let Some((user, realm)) = p.split_once('@') {
            id.realm = realm.to_string();
            id.domain = realm.to_lowercase();
            let local = user.split('/').next().unwrap_or(user);
            id.email_guess = format!("{}@{}", local.to_lowercase(), id.domain);
            id.email_source = "guess".into();
        }
        // Gerçek adres Active Directory'deki "mail" özniteliğindedir
        // (ör. kullanici.adi@kurum.gov.tr; giriş adı ise kullaniciadi@KURUM.LOCAL)
        let mut log = Vec::new();
        if let Some(d) = super::ldap::lookup_principal(&p, &mut log) {
            id.email_guess = d.email;
            id.email_source = "directory".into();
            if !d.display_name.is_empty() {
                id.full_name = d.display_name;
            }
        }
        id.log = log;
    }
    id.login = default_login_from(&id);
    if id.domain.is_empty() {
        // Kerberos bileti yoksa yapılandırmadan etki alanını bul
        id.domain = default_realm().map(|r| r.to_lowercase()).or_else(host_domain).unwrap_or_default();
        id.realm = id.domain.to_uppercase();
        let local = username.split('@').next().unwrap_or(&username).to_lowercase();
        if !id.domain.is_empty() && !local.is_empty() {
            id.email_guess = format!("{local}@{}", id.domain);
        }
    }
    // SSSD kullanıcı adı "ad@etkialani" biçimindeyse
    if username.contains('@') && id.email_guess.is_empty() {
        id.email_guess = username.to_lowercase();
    }
    id
}

fn default_login_from(id: &SystemIdentity) -> String {
    // Outlook'ta olduğu gibi yalnızca kullanıcı adı (ör. "ab001234"); etki alanı
    // NTLM/Kerberos tarafından sunucudan alınır, Autodiscover için ayrıca bulunur.
    if let Some((user, _)) = id.principal.split_once('@') {
        return user.split('/').next().unwrap_or(user).to_lowercase();
    }
    let local = id.username.split('@').next().unwrap_or(&id.username);
    let local = local.rsplit('\\').next().unwrap_or(local).to_lowercase();
    if local.is_empty() || id.domain.is_empty() {
        return String::new();
    }
    local
}

/// Oturum açmış kullanıcının etki alanı oturum adı (yalın kullanıcı adı); bulunamazsa boş.
/// LDAP araması yapmaz (hızlı).
pub fn default_login() -> String {
    let username = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let mut id = SystemIdentity { username, ..Default::default() };
    if let Some(p) = current_principal() {
        id.principal = p;
    } else {
        id.domain = default_realm().map(|r| r.to_lowercase()).or_else(host_domain).unwrap_or_default();
    }
    default_login_from(&id)
}

/// Bilgisayarın bağlı olduğu etki alanı (ör. "kurum.local"); yoksa None.
pub fn machine_domain() -> Option<String> {
    current_principal()
        .and_then(|p| p.split_once('@').map(|(_, r)| r.to_lowercase()))
        .or_else(|| default_realm().map(|r| r.to_lowercase()))
        .or_else(host_domain)
}

fn default_realm() -> Option<String> {
    let text = std::fs::read_to_string("/etc/krb5.conf").ok()?;
    for line in text.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("default_realm") {
            let v = v.trim_start().trim_start_matches('=').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    // SSSD yapılandırması (genellikle yalnızca root okuyabilir)
    let sssd = std::fs::read_to_string("/etc/sssd/sssd.conf").ok()?;
    sssd.lines().find_map(|l| l.trim().strip_prefix("[domain/").map(|d| d.trim_end_matches(']').to_string()))
}

fn host_domain() -> Option<String> {
    let out = std::process::Command::new("hostname").arg("-d").output().ok()?;
    let d = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if d.is_empty() || d == "(none)" || d == "localdomain" {
        None
    } else {
        Some(d)
    }
}

#[cfg(test)]
mod login_tests {
    use super::*;

    #[test]
    fn bare_login_like_outlook() {
        let k = SystemIdentity { principal: "AB001234@KURUM.LOCAL".into(), ..Default::default() };
        assert_eq!(default_login_from(&k), "ab001234");
        let l = SystemIdentity { username: "KURUM\\ab001234".into(), domain: "kurum.local".into(), ..Default::default() };
        assert_eq!(default_login_from(&l), "ab001234");
        let u = SystemIdentity { username: "ab001234@kurum.local".into(), domain: "kurum.local".into(), ..Default::default() };
        assert_eq!(default_login_from(&u), "ab001234");
    }
}
