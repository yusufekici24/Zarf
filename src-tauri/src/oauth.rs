//! OAuth 2.0 (Microsoft 365 / Exchange Online ve Gmail) — yetkilendirme kodu + PKCE.
//!
//! Microsoft 365 ve Gmail parola ile girişi kapattığı için modern kimlik
//! doğrulaması gerekir. Kurumlar kendi Azure AD (Entra ID) uygulama kaydının
//! istemci kimliğini Ayarlar'dan girer ya da paketleyen kişi derleme sırasında
//! `LYNORA_MS_CLIENT_ID` / `LYNORA_GOOGLE_CLIENT_ID` ortam değişkenlerini verir.
//! Yönlendirme adresi: `http://localhost` (yerel geri dönüş).

use crate::db::{Account, Db};
use crate::mail::http::{self, Auth, Client};
use crate::mail::{MailError, MailResult};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static DB: OnceLock<Db> = OnceLock::new();
static CACHE: OnceLock<Mutex<HashMap<i64, (String, f64)>>> = OnceLock::new();

pub fn init(db: Db) {
    let _ = DB.set(db);
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct TokenSet {
    pub provider: String,
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: f64,
    #[serde(default)]
    pub scope: String,
}

pub struct Provider {
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: String,
}

fn setting(key: &str) -> String {
    DB.get().and_then(|d| d.setting(key)).unwrap_or_default()
}

pub fn provider(name: &str, exchange: bool) -> MailResult<Provider> {
    match name {
        "microsoft" => {
            let client_id = Some(setting("oauth_ms_client_id")).filter(|s| !s.is_empty()).or_else(|| option_env!("LYNORA_MS_CLIENT_ID").map(String::from)).unwrap_or_default();
            if client_id.is_empty() {
                return Err(MailError(crate::t!("oauth.no_client_id", "Microsoft")));
            }
            let tenant = Some(setting("oauth_ms_tenant")).filter(|s| !s.is_empty()).unwrap_or_else(|| "organizations".into());
            Ok(Provider {
                auth_url: format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize"),
                token_url: format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token"),
                client_id,
                client_secret: String::new(),
                scopes: if exchange {
                    "https://outlook.office365.com/EWS.AccessAsUser.All offline_access openid email profile".into()
                } else {
                    "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email profile".into()
                },
            })
        }
        "google" => {
            let client_id = Some(setting("oauth_google_client_id")).filter(|s| !s.is_empty()).or_else(|| option_env!("LYNORA_GOOGLE_CLIENT_ID").map(String::from)).unwrap_or_default();
            let client_secret = Some(setting("oauth_google_client_secret")).filter(|s| !s.is_empty()).or_else(|| option_env!("LYNORA_GOOGLE_CLIENT_SECRET").map(String::from)).unwrap_or_default();
            if client_id.is_empty() {
                return Err(MailError(crate::t!("oauth.no_client_id", "Google")));
            }
            Ok(Provider {
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
                token_url: "https://oauth2.googleapis.com/token".into(),
                client_id,
                client_secret,
                scopes: "https://mail.google.com/ openid email profile".into(),
            })
        }
        _ => Err(MailError(format!("OAuth: bilinmeyen sağlayıcı {name}"))),
    }
}

/// E-posta adresine/sunucuya göre sağlayıcı
pub fn provider_for(email: &str, host: &str) -> Option<&'static str> {
    let d = email.rsplit('@').next().unwrap_or("").to_lowercase();
    let h = host.to_lowercase();
    if d == "gmail.com" || d == "googlemail.com" || h.contains("gmail") || h.contains("google") {
        Some("google")
    } else if h.contains("office365") || h.contains("outlook.com") || h.contains("microsoft") {
        Some("microsoft")
    } else {
        None
    }
}

fn token_request(p: &Provider, params: &[(&str, &str)]) -> MailResult<TokenSet> {
    let mut all: Vec<(&str, &str)> = vec![("client_id", p.client_id.as_str())];
    if !p.client_secret.is_empty() {
        all.push(("client_secret", p.client_secret.as_str()));
    }
    all.extend_from_slice(params);
    let mut c = Client::new(Auth::None);
    let r = c.request("POST", &p.token_url, "application/x-www-form-urlencoded", http::form_encode(&all).as_bytes())?;
    let v: serde_json::Value = serde_json::from_slice(&r.body).map_err(|e| MailError(format!("OAuth: {e}")))?;
    if r.status != 200 {
        let msg = v.get("error_description").or_else(|| v.get("error")).and_then(|x| x.as_str()).unwrap_or("?").to_string();
        // Yenileme belirteci geçersiz → kullanıcı yeniden oturum açmalı (parola denemesi yok)
        return Err(MailError::auth(format!("OAuth: {msg}")));
    }
    Ok(TokenSet {
        provider: String::new(),
        refresh_token: v.get("refresh_token").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        access_token: v.get("access_token").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        expires_at: crate::db::now() + v.get("expires_in").and_then(|x| x.as_f64()).unwrap_or(3600.0) - 60.0,
        scope: v.get("scope").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    })
}

/// Tarayıcıda oturum açtırır; belirteç kümesini döndürür (JSON olarak saklanır).
pub fn authorize(provider_name: &str, exchange: bool, login_hint: &str, open_url: impl Fn(&str)) -> MailResult<TokenSet> {
    let p = provider(provider_name, exchange)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://localhost:{port}");
    let verifier: String = (0..3).map(|_| uuid::Uuid::new_v4().simple().to_string()).collect();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().simple().to_string();
    let url = format!(
        "{}?{}",
        p.auth_url,
        http::form_encode(&[
            ("client_id", &p.client_id),
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            ("scope", &p.scopes),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("login_hint", login_hint),
            ("access_type", "offline"),
            ("prompt", "select_account"),
        ])
    );
    open_url(&url);
    listener.set_nonblocking(false)?;
    // En fazla 5 dakika bekle
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    let code = loop {
        if std::time::Instant::now() > deadline {
            return Err(MailError(crate::t!("oauth.timeout")));
        }
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        let mut line = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut line)?;
        let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let params: HashMap<String, String> =
            query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), http::url_decode(v))).collect();
        let page = |msg: &str| format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><meta charset=utf-8><title>Zarf</title><body style=\"font-family:sans-serif;text-align:center;padding-top:15vh\"><h2>Zarf</h2><p>{msg}</p></body>");
        if let Some(code) = params.get("code") {
            if params.get("state").map(|s| s == &state).unwrap_or(false) {
                let _ = stream.write_all(page(&crate::t!("oauth.done")).as_bytes());
                break code.clone();
            }
        }
        if let Some(err) = params.get("error") {
            let _ = stream.write_all(page(&crate::t!("oauth.failed")).as_bytes());
            return Err(MailError(format!("OAuth: {err} {}", params.get("error_description").cloned().unwrap_or_default())));
        }
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
    };
    let mut ts = token_request(&p, &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", &redirect), ("code_verifier", &verifier)])?;
    ts.provider = provider_name.to_string();
    if ts.refresh_token.is_empty() {
        return Err(MailError(crate::t!("oauth.no_refresh")));
    }
    Ok(ts)
}

pub fn encode(ts: &TokenSet) -> String {
    crate::secret::encrypt_local(&serde_json::to_string(ts).unwrap_or_default())
}

pub fn decode(s: &str) -> Option<TokenSet> {
    serde_json::from_str(&crate::secret::open(s)).ok()
}

/// Geçerli erişim belirteci (gerekirse yenilenir ve yeni yenileme belirteci saklanır).
pub fn access_token(account: &Account) -> MailResult<String> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((tok, exp)) = cache.lock().unwrap().get(&account.id) {
        if *exp > crate::db::now() {
            return Ok(tok.clone());
        }
    }
    let mut ts = decode(&account.oauth_token).ok_or_else(|| MailError::auth(crate::t!("oauth.signin_required")))?;
    if !ts.access_token.is_empty() && ts.expires_at > crate::db::now() {
        cache.lock().unwrap().insert(account.id, (ts.access_token.clone(), ts.expires_at));
        return Ok(ts.access_token);
    }
    let p = provider(&ts.provider, account.is_exchange())?;
    let fresh = token_request(&p, &[("grant_type", "refresh_token"), ("refresh_token", &ts.refresh_token), ("scope", &p.scopes)])?;
    if !fresh.refresh_token.is_empty() {
        ts.refresh_token = fresh.refresh_token;
    }
    ts.access_token = fresh.access_token.clone();
    ts.expires_at = fresh.expires_at;
    if let Some(db) = DB.get() {
        let _ = db.conn().execute("UPDATE accounts SET oauth_token = ?1 WHERE id = ?2", rusqlite::params![encode(&ts), account.id]);
    }
    cache.lock().unwrap().insert(account.id, (fresh.access_token.clone(), fresh.expires_at));
    Ok(fresh.access_token)
}
