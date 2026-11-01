//! S/MIME ve OpenPGP: imzalama, şifreleme, doğrulama, şifre çözme.
//!
//! Ek kitaplık yoktur, sistemdeki araçlar kullanılır:
//! - OpenPGP: `gpg`. Anahtarlar kullanıcının GnuPG anahtarlığındadır, parola gpg-agent ile sorulur.
//! - S/MIME: `openssl`. Kişisel sertifika .p12/.pfx'ten içe aktarılır; özel anahtar diskte rastgele
//!   bir parolayla şifreli durur, o parola anahtarlıkta saklanır. Alıcı sertifikaları imzalı
//!   iletilerden otomatik toplanır ya da elle içe aktarılır.
//!
//! İçerik araçlarla stdin/stdout üzerinden konuşulur: çözülen ileti diske yazılmaz, veritabanına da
//! girmez; yalnızca bellekte tutulur. Parolalar komut satırında değil ortam değişkeninde gider.

use crate::db::{default_data_dir, Account};
use crate::mail::parse;
use crate::t;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

type R<T> = Result<T, String>;

/// Parola sorusu kullanıcıyı bekleyebilir
const TIMEOUT: Duration = Duration::from_secs(120);
const KEY_PASS_ENV: &str = "LYNORA_KEYPASS";
const P12_PASS_ENV: &str = "LYNORA_P12";

/// Okuma bölmesine gösterilecek sonuç
#[derive(Clone, Debug, Default, Serialize)]
pub struct SecurityResult {
    /// "pgp" | "smime"
    pub method: String,
    pub signed: bool,
    pub encrypted: bool,
    /// "good" | "untrusted" | "bad" | "no_key" | "error" | ""
    pub signature: String,
    pub signer: String,
    pub error: String,
    /// Çözülen MIME varlığı (yalnızca bellekte)
    #[serde(skip)]
    pub decrypted: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CertInfo {
    pub subject: String,
    pub emails: Vec<String>,
    pub expires: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PgpKey {
    pub fingerprint: String,
    pub uid: String,
}

fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    let start = text.char_indices().rev().nth(199).map_or(0, |(i, _)| i);
    text[start..].to_string()
}

/// Aracı çalıştırır; girdi ve çıktı boru üzerindendir, zaman aşımında süreç sonlandırılır
fn run(program: &str, args: &[&str], env: &[(&str, &str)], input: &[u8]) -> R<Output> {
    let mut child = Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| t!("error.crypto_tool", program, e))?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        if let Some(mut s) = stdin {
            let _ = s.write_all(&data);
        }
    });
    fn drain(src: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut s) = src {
                let _ = s.read_to_end(&mut buf);
            }
            buf
        })
    }
    let (out, err) = (drain(stdout), drain(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(t!("error.crypto_timeout", program));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let _ = writer.join();
    Ok(Output { status, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
}

fn available(program: &str) -> bool {
    Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

pub fn gpg_available() -> bool {
    available("gpg")
}

pub fn openssl_available() -> bool {
    Command::new("openssl").arg("version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Yalnızca kullanıcının erişebildiği, işi bitince silinen geçici klasör
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> R<TempDir> {
        let p = std::env::temp_dir().join(format!("lynora-sec-{}", uuid::Uuid::new_v4().simple()));
        make_private_dir(&p)?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn make_private_dir(path: &Path) -> R<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path).map_err(|e| e.to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())
}

fn write_private(path: &Path, data: &[u8]) -> R<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|e| e.to_string())?;
    f.write_all(data).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------- MIME yardımcıları
fn crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 32);
    for (i, b) in data.iter().enumerate() {
        if *b == b'\n' && (i == 0 || data[i - 1] != b'\r') {
            out.push(b'\r');
        }
        out.push(*b);
    }
    out
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// (başlıklar, gövde); başlık bölümü son satır sonunu içerir
fn split_headers(raw: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let raw = crlf(raw);
    match find(&raw, b"\r\n\r\n", 0) {
        Some(i) => (raw[..i + 2].to_vec(), raw[i + 4..].to_vec()),
        None => (raw, Vec::new()),
    }
}

/// Katlanmış başlık satırları tek mantıksal satır olur
fn header_lines(head: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    for line in head.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if line[0] == b' ' || line[0] == b'\t' => {
                last.extend_from_slice(b"\r\n");
                last.extend_from_slice(line);
            }
            _ => out.push(line.to_vec()),
        }
    }
    out
}

fn header_name(line: &[u8]) -> String {
    let end = line.iter().position(|b| *b == b':').unwrap_or(line.len());
    String::from_utf8_lossy(&line[..end]).trim().to_lowercase()
}

/// Taşıma başlıkları (From/To/Subject…): içerik başlıkları hariç
fn outer_headers(raw: &[u8]) -> Vec<u8> {
    let (head, _) = split_headers(raw);
    let mut out = Vec::new();
    for l in header_lines(&head) {
        let name = header_name(&l);
        if !name.starts_with("content-") && name != "mime-version" {
            out.extend_from_slice(&l);
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

/// İletinin gövde varlığı: içerik başlıkları + gövde (imzalanacak/şifrelenecek kısım)
fn body_entity(raw: &[u8]) -> Vec<u8> {
    let (head, body) = split_headers(raw);
    let mut out = Vec::new();
    for l in header_lines(&head) {
        if header_name(&l).starts_with("content-") {
            out.extend_from_slice(&l);
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&body);
    out
}

/// Üst başlıktaki değer (katlamalar açılmış, küçük harf)
fn header_value(head: &[u8], name: &str) -> String {
    header_lines(head)
        .iter()
        .find(|l| header_name(l) == name)
        .map(|l| {
            let text = String::from_utf8_lossy(l);
            let v = text.split_once(':').map_or("", |(_, v)| v);
            v.replace("\r\n", " ").trim().to_string()
        })
        .unwrap_or_default()
}

fn param(value: &str, name: &str) -> String {
    value
        .split(';')
        .skip(1)
        .find_map(|p| {
            let (k, v) = p.split_once('=')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

/// Üst başlık bölümü (gövde kopyalanmaz: her ileti açılışında çalışır)
fn top_headers(raw: &[u8]) -> &[u8] {
    let end = find(raw, b"\r\n\r\n", 0).into_iter().chain(find(raw, b"\n\n", 0)).min().unwrap_or(raw.len());
    &raw[..end]
}

/// İletinin güvenlik biçimi: pgp-signed, pgp-encrypted, pgp-inline-signed, pgp-inline-encrypted,
/// smime-signed, smime-opaque, smime-encrypted ya da boş.
pub fn detect(raw: &[u8]) -> &'static str {
    let head = top_headers(raw);
    let ctype = header_value(head, "content-type");
    let main = ctype.split(';').next().unwrap_or("").trim().to_lowercase();
    let protocol = param(&ctype, "protocol").to_lowercase();
    match main.as_str() {
        "multipart/signed" if protocol.contains("pgp") => "pgp-signed",
        "multipart/signed" if protocol.contains("pkcs7") => "smime-signed",
        "multipart/encrypted" if protocol.contains("pgp") => "pgp-encrypted",
        "application/pkcs7-mime" | "application/x-pkcs7-mime" => {
            if param(&ctype, "smime-type").eq_ignore_ascii_case("signed-data") {
                "smime-opaque"
            } else {
                "smime-encrypted"
            }
        }
        "text/plain" | "" => {
            let encoded = header_value(head, "content-transfer-encoding").eq_ignore_ascii_case("base64");
            if !encoded && find(raw, b"-----BEGIN PGP", 0).is_none() {
                return "";
            }
            let text = parse::parse(raw).text;
            if text.contains("-----BEGIN PGP MESSAGE-----") {
                "pgp-inline-encrypted"
            } else if text.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
                "pgp-inline-signed"
            } else {
                ""
            }
        }
        _ => "",
    }
}

/// Üst düzey multipart iletinin parçaları BAYT olarak. İmza ilk parçanın tam baytları üzerinden
/// hesaplanır: kitaplıkla yeniden üretmek (başlık katlama vb.) doğrulamayı bozardı. RFC 2046:
/// sınırdan önceki CRLF sınıra aittir, parçaya değil.
fn multipart_parts(raw: &[u8]) -> R<Vec<Vec<u8>>> {
    let (head, body) = split_headers(raw);
    let boundary = param(&header_value(&head, "content-type"), "boundary");
    if boundary.is_empty() {
        return Err(t!("error.crypto_mime"));
    }
    let marker = format!("--{boundary}").into_bytes();
    let mut marks = Vec::new();
    let mut i = 0;
    while let Some(j) = find(&body, &marker, i) {
        if j == 0 || body[..j].ends_with(b"\r\n") {
            marks.push(j);
        }
        i = j + marker.len();
    }
    let mut parts = Vec::new();
    for w in marks.windows(2) {
        let Some(line_end) = find(&body, b"\r\n", w[0]) else { continue };
        if line_end + 2 <= w[1].saturating_sub(2) {
            parts.push(body[line_end + 2..w[1] - 2].to_vec());
        }
    }
    Ok(parts)
}

/// multipart/signed → (imzalanan varlık, imza)
fn split_signed(raw: &[u8]) -> R<(Vec<u8>, Vec<u8>)> {
    let mut parts = multipart_parts(raw)?;
    if parts.len() < 2 {
        return Err(t!("error.crypto_mime"));
    }
    let sig = split_headers(&parts[1]).1;
    Ok((parts.swap_remove(0), sig))
}

// ---------------------------------------------------------------------- OpenPGP
fn gpg(args: &[&str], input: &[u8]) -> R<Output> {
    let mut all = vec!["--batch", "--no-tty", "--status-fd", "2"];
    all.extend_from_slice(args);
    run("gpg", &all, &[], input)
}

/// "[GNUPG:] ANAHTAR değer" satırları
fn gpg_status(stderr: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter_map(|l| l.strip_prefix("[GNUPG:] "))
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            (k.to_string(), v.to_string())
        })
        .collect()
}

fn gpg_signature(status: &[(String, String)], r: &mut SecurityResult) {
    let has = |k: &str| status.iter().any(|(s, _)| s == k);
    for (k, v) in status {
        if matches!(k.as_str(), "GOODSIG" | "BADSIG" | "EXPKEYSIG" | "REVKEYSIG") {
            r.signer = v.split_once(' ').map_or(v.as_str(), |(_, who)| who).to_string();
        }
    }
    r.signature = if has("BADSIG") {
        "bad"
    } else if has("GOODSIG") {
        if has("TRUST_FULLY") || has("TRUST_ULTIMATE") {
            "good"
        } else {
            "untrusted"
        }
    } else if has("EXPKEYSIG") || has("REVKEYSIG") {
        "untrusted" // bozulmamış ama anahtar süresi dolmuş ya da iptal edilmiş
    } else if has("NO_PUBKEY") || has("ERRSIG") {
        "no_key"
    } else {
        "error"
    }
    .to_string();
}

fn pgp_verify_detached(signed: &[u8], signature: &[u8]) -> R<SecurityResult> {
    let mut r = SecurityResult { method: "pgp".into(), signed: true, ..Default::default() };
    let tmp = TempDir::new()?;
    let sig = tmp.0.join("sig.asc");
    write_private(&sig, signature)?;
    let out = gpg(&["--verify", &sig.to_string_lossy(), "-"], signed)?;
    gpg_signature(&gpg_status(&out.stderr), &mut r);
    Ok(r)
}

fn pgp_decrypt(data: &[u8]) -> R<SecurityResult> {
    let mut r = SecurityResult { method: "pgp".into(), encrypted: true, ..Default::default() };
    let out = gpg(&["--decrypt"], data)?;
    let status = gpg_status(&out.stderr);
    let has = |k: &str| status.iter().any(|(s, _)| s == k);
    if out.status.success() || has("DECRYPTION_OKAY") {
        r.decrypted = Some(out.stdout);
        if has("GOODSIG") || has("BADSIG") || has("ERRSIG") || has("NO_PUBKEY") {
            r.signed = true;
            gpg_signature(&status, &mut r);
        }
    } else {
        r.error = if has("NO_SECKEY") { t!("sec.no_secret_key") } else { tail(&out.stderr) };
    }
    Ok(r)
}

fn pgp_sign(entity: &[u8], key: &str) -> R<Vec<u8>> {
    let mut args = vec!["--armor", "--detach-sign", "--digest-algo", "SHA256"];
    if !key.is_empty() {
        args.extend(["--local-user", key]);
    }
    let out = gpg(&args, &crlf(entity))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(t!("error.pgp_sign", tail(&out.stderr)));
    }
    Ok(out.stdout)
}

fn pgp_missing(recipients: &[String]) -> R<Vec<String>> {
    let mut missing = Vec::new();
    for a in recipients {
        let out = gpg(&["--list-keys", "--with-colons", a], b"")?;
        if !out.status.success() || !String::from_utf8_lossy(&out.stdout).lines().any(|l| l.starts_with("pub:")) {
            missing.push(a.clone());
        }
    }
    Ok(missing)
}

fn pgp_encrypt(entity: &[u8], recipients: &[String], sign_key: Option<&str>) -> R<Vec<u8>> {
    let mut args = vec!["--armor", "--encrypt", "--trust-model", "always"];
    for a in recipients {
        args.extend(["--recipient", a.as_str()]);
    }
    if let Some(k) = sign_key {
        args.push("--sign");
        if !k.is_empty() {
            args.extend(["--local-user", k]);
        }
    }
    let out = gpg(&args, &crlf(entity))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(t!("error.pgp_encrypt", tail(&out.stderr)));
    }
    Ok(out.stdout)
}

/// Gizli anahtarlar (parmak izi, kullanıcı kimliği)
pub fn list_secret_keys() -> Vec<PgpKey> {
    let Ok(out) = run("gpg", &["--batch", "--list-secret-keys", "--with-colons"], &[], b"") else { return Vec::new() };
    let mut keys = Vec::new();
    let mut fpr: Option<String> = None;
    let mut want = false;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = line.split(':').collect();
        match f[0] {
            "sec" => {
                fpr = None;
                want = true;
            }
            "ssb" => want = false,
            "fpr" if want && fpr.is_none() => fpr = f.get(9).map(|s| s.to_string()),
            "uid" => {
                if let Some(fingerprint) = fpr.take() {
                    keys.push(PgpKey { fingerprint, uid: f.get(9).unwrap_or(&"").to_string() });
                    want = false;
                }
            }
            _ => {}
        }
    }
    keys
}

pub fn import_pgp_key(path: &str) -> R<()> {
    let out = run("gpg", &["--batch", "--import", "--", path], &[], b"")?;
    if out.status.success() {
        Ok(())
    } else {
        Err(tail(&out.stderr))
    }
}

// ---------------------------------------------------------------------- S/MIME
fn openssl(args: &[&str], env: &[(&str, &str)], input: &[u8]) -> R<Output> {
    run("openssl", args, env, input)
}

fn ca_file() -> Option<&'static str> {
    ["/etc/ssl/certs/ca-certificates.crt", "/etc/ssl/cert.pem"].into_iter().find(|p| Path::new(p).exists())
}

/// Alıcı sertifikasının dosya adı; yol dışına çıkamaz
fn cert_file_name(address: &str) -> Option<String> {
    let a = address.trim().to_lowercase();
    let safe = a.contains('@') && !a.starts_with('.') && a.chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c));
    safe.then(|| format!("{a}.pem"))
}

/// Sertifikadaki adreslerden biri iletiyi gönderen mi
fn signer_matches(sender: &str, emails: &[String]) -> bool {
    sender.is_empty() || emails.iter().any(|e| e.eq_ignore_ascii_case(sender))
}

pub fn certificate_info(pem: &[u8]) -> CertInfo {
    let mut info = CertInfo::default();
    let Ok(out) = openssl(&["x509", "-noout", "-subject", "-email", "-enddate"], &[], pem) else { return info };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(v) = line.strip_prefix("subject=") {
            info.subject = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("notAfter=") {
            info.expires = v.trim().to_string();
        } else if line.contains('@') {
            info.emails.push(line.trim().to_lowercase());
        }
    }
    info
}

/// Sertifika deposu: `<veri>/smime/<hesap>/` kişisel kimlik, `<veri>/smime/certs/` alıcılar
pub struct Smime {
    root: PathBuf,
}

impl Smime {
    pub fn system() -> Smime {
        Smime { root: default_data_dir().join("smime") }
    }

    fn dir(&self, account: Option<i64>) -> R<PathBuf> {
        let d = self.root.join(account.map_or("certs".to_string(), |a| a.to_string()));
        make_private_dir(&self.root)?;
        make_private_dir(&d)?;
        Ok(d)
    }

    pub fn identity(&self, account: i64) -> Option<CertInfo> {
        let d = self.root.join(account.to_string());
        if !d.join("key.pem").exists() {
            return None;
        }
        std::fs::read(d.join("cert.pem")).ok().map(|pem| certificate_info(&pem))
    }

    pub fn remove_identity(&self, account: i64) {
        let d = self.root.join(account.to_string());
        if let Ok(stored) = std::fs::read_to_string(d.join("key.pass")) {
            crate::secret::remove(stored.trim());
        }
        let _ = std::fs::remove_dir_all(d);
    }

    fn key_passphrase(&self, account: i64) -> String {
        std::fs::read_to_string(self.root.join(account.to_string()).join("key.pass")).map(|s| crate::secret::open(s.trim())).unwrap_or_default()
    }

    /// Kişisel sertifikayı içe aktarır: özel anahtar rastgele parolayla şifrelenip saklanır
    pub fn import_pkcs12(&self, account: i64, path: &str, password: &str) -> R<CertInfo> {
        // Eski .pfx dosyaları (RC2) OpenSSL 3'te yalnızca -legacy ile açılır
        let (cert, key) = read_pkcs12(path, password, false).or_else(|_| read_pkcs12(path, password, true))?;
        let mut raw = [0u8; 32];
        getrandom::getrandom(&mut raw).map_err(|e| e.to_string())?;
        let passphrase = uuid::Uuid::from_slice(&raw[..16]).map_err(|e| e.to_string())?.simple().to_string() + &uuid::Uuid::from_slice(&raw[16..]).map_err(|e| e.to_string())?.simple().to_string();
        let sealed = openssl(&["pkey", "-aes256", "-passout", &format!("env:{KEY_PASS_ENV}")], &[(KEY_PASS_ENV, &passphrase)], &key)?;
        if !sealed.status.success() {
            return Err(t!("error.smime_key", tail(&sealed.stderr)));
        }
        let d = self.dir(Some(account))?;
        write_private(&d.join("cert.pem"), &cert)?;
        write_private(&d.join("key.pem"), &sealed.stdout)?;
        write_private(&d.join("key.pass"), crate::secret::seal(&format!("smime-key-{account}"), &passphrase).as_bytes())?;
        Ok(certificate_info(&cert))
    }

    /// Alıcı sertifikasını e-posta adresleri adına saklar. `only`: yalnızca bu adres (imzalı
    /// iletiden toplarken gönderen). `overwrite` kapalıyken var olan sertifika değişmez: güvenilir
    /// olmayan bir imzayla gelen sertifika, bilinenin yerine geçip şifreli yanıtları saldırgana açamaz.
    pub fn store_recipient(&self, pem: &[u8], only: &str, overwrite: bool) -> R<Vec<String>> {
        let dir = self.dir(None)?;
        let mut stored = Vec::new();
        for address in certificate_info(pem).emails {
            if !only.is_empty() && address != only.trim().to_lowercase() {
                continue;
            }
            let Some(name) = cert_file_name(&address) else { continue };
            let path = dir.join(name);
            if path.exists() && !overwrite {
                continue;
            }
            write_private(&path, pem)?;
            stored.push(address);
        }
        Ok(stored)
    }

    /// Elle içe aktarılan alıcı sertifikası (.pem, .crt ya da DER biçiminde .cer)
    pub fn import_certificate(&self, path: &str) -> R<Vec<String>> {
        let mut pem = std::fs::read(path).map_err(|e| e.to_string())?;
        if find(&pem, b"-----BEGIN CERTIFICATE-----", 0).is_none() {
            let out = openssl(&["x509", "-inform", "DER", "-outform", "PEM"], &[], &pem)?;
            if !out.status.success() {
                return Err(t!("error.smime_cert"));
            }
            pem = out.stdout;
        }
        let stored = self.store_recipient(&pem, "", true)?;
        if stored.is_empty() {
            return Err(t!("error.smime_cert_mail"));
        }
        Ok(stored)
    }

    fn recipient(&self, address: &str) -> Option<PathBuf> {
        let p = self.root.join("certs").join(cert_file_name(address)?);
        p.exists().then_some(p)
    }

    /// Açık ya da opak imzayı doğrular. Zincir güvenilir değilse yalnızca bütünlük denetlenir
    /// ("untrusted"). Geçerli imzanın sertifikası gönderene aitse şifreli yanıtlar için saklanır.
    fn verify(&self, raw: &[u8], sender: &str) -> R<SecurityResult> {
        let mut r = SecurityResult { method: "smime".into(), signed: true, ..Default::default() };
        let tmp = TempDir::new()?;
        let signer = tmp.0.join("signer.pem");
        let signer_path = signer.to_string_lossy().to_string();
        let data = crlf(raw);
        let mut args = vec!["smime", "-verify", "-signer", &signer_path];
        if let Some(ca) = ca_file() {
            args.extend(["-CAfile", ca]);
        }
        let first = openssl(&args, &[], &data)?;
        let out = if first.status.success() {
            r.signature = "good".into();
            first
        } else {
            let second = openssl(&["smime", "-verify", "-signer", &signer_path, "-noverify"], &[], &data)?;
            if second.status.success() {
                r.signature = "untrusted".into();
            } else {
                r.signature = "bad".into();
                r.error = tail(&second.stderr);
            }
            second
        };
        if let Ok(pem) = std::fs::read(&signer) {
            let info = certificate_info(&pem);
            // Geçerli bir sertifikayla başkasının adına gönderilmiş ileti "geçerli" görünmesin
            if r.signature == "good" && !signer_matches(sender, &info.emails) {
                r.signature = "untrusted".into();
            }
            r.signer = if info.emails.is_empty() { info.subject } else { info.emails.join(", ") };
            if !sender.is_empty() && r.signature != "bad" {
                let _ = self.store_recipient(&pem, sender, r.signature == "good");
            }
        }
        if out.status.success() && detect(raw) == "smime-opaque" {
            r.decrypted = Some(out.stdout);
        }
        Ok(r)
    }

    fn decrypt(&self, raw: &[u8], account: Option<i64>) -> R<SecurityResult> {
        let mut r = SecurityResult { method: "smime".into(), encrypted: true, ..Default::default() };
        let Some(account) = account.filter(|a| self.identity(*a).is_some()) else {
            r.error = t!("sec.no_identity");
            return Ok(r);
        };
        let d = self.root.join(account.to_string());
        let (cert, key) = (d.join("cert.pem"), d.join("key.pem"));
        let pass = self.key_passphrase(account);
        let out = openssl(
            &["smime", "-decrypt", "-recip", &cert.to_string_lossy(), "-inkey", &key.to_string_lossy(), "-passin", &format!("env:{KEY_PASS_ENV}")],
            &[(KEY_PASS_ENV, &pass)],
            &crlf(raw),
        )?;
        if out.status.success() {
            r.decrypted = Some(out.stdout);
        } else {
            r.error = tail(&out.stderr);
        }
        Ok(r)
    }

    /// multipart/signed varlığı (açık imza) döndürür
    fn sign(&self, entity: &[u8], account: i64) -> R<Vec<u8>> {
        if self.identity(account).is_none() {
            return Err(t!("sec.no_identity"));
        }
        let d = self.root.join(account.to_string());
        let (cert, key) = (d.join("cert.pem"), d.join("key.pem"));
        let pass = self.key_passphrase(account);
        let out = openssl(
            &["smime", "-sign", "-binary", "-md", "sha256", "-signer", &cert.to_string_lossy(), "-inkey", &key.to_string_lossy(), "-passin", &format!("env:{KEY_PASS_ENV}"), "-outform", "SMIME"],
            &[(KEY_PASS_ENV, &pass)],
            &crlf(entity),
        )?;
        if !out.status.success() || out.stdout.is_empty() {
            return Err(t!("error.smime_sign", tail(&out.stderr)));
        }
        Ok(crlf(&out.stdout))
    }

    fn encrypt(&self, entity: &[u8], certs: &[PathBuf]) -> R<Vec<u8>> {
        let paths: Vec<String> = certs.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let mut args = vec!["smime", "-encrypt", "-aes256", "-binary", "-outform", "SMIME"];
        args.extend(paths.iter().map(String::as_str));
        let out = openssl(&args, &[], &crlf(entity))?;
        if !out.status.success() || out.stdout.is_empty() {
            return Err(t!("error.smime_encrypt", tail(&out.stderr)));
        }
        Ok(crlf(&out.stdout))
    }
}

fn read_pkcs12(path: &str, password: &str, legacy: bool) -> R<(Vec<u8>, Vec<u8>)> {
    let pass = format!("env:{P12_PASS_ENV}");
    let env = [(P12_PASS_ENV, password)];
    let read = |what: &[&str]| {
        let mut args = vec!["pkcs12", "-in", path, "-passin", pass.as_str()];
        args.extend_from_slice(what);
        if legacy {
            args.push("-legacy");
        }
        openssl(&args, &env, b"")
    };
    let (cert, key) = (read(&["-clcerts", "-nokeys"])?, read(&["-nocerts", "-nodes"])?);
    let start = find(&cert.stdout, b"-----BEGIN CERTIFICATE-----", 0);
    match (cert.status.success() && key.status.success(), start) {
        (true, Some(i)) if find(&key.stdout, b"PRIVATE KEY", 0).is_some() => Ok((cert.stdout[i..].to_vec(), key.stdout)),
        _ => Err(t!("error.smime_p12")),
    }
}

// ---------------------------------------------------------------------- iletiye uygulama
fn new_boundary() -> String {
    format!("ly-{}", uuid::Uuid::new_v4().simple())
}

fn pgp_encrypted_body(armored: &[u8]) -> Vec<u8> {
    let b = new_boundary();
    let mut out = format!(
        "Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"{b}\"\r\n\r\n\
         This is an OpenPGP/MIME encrypted message (RFC 4880 and 3156)\r\n--{b}\r\nContent-Type: application/pgp-encrypted\r\n\
         Content-Description: PGP/MIME version identification\r\n\r\nVersion: 1\r\n\r\n--{b}\r\n\
         Content-Type: application/octet-stream; name=\"encrypted.asc\"\r\nContent-Description: OpenPGP encrypted message\r\n\
         Content-Disposition: inline; filename=\"encrypted.asc\"\r\n\r\n"
    )
    .into_bytes();
    out.extend_from_slice(&crlf(armored));
    out.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    out
}

fn pgp_signed_body(entity: &[u8], signature: &[u8]) -> Vec<u8> {
    let b = new_boundary();
    let mut out = format!(
        "Content-Type: multipart/signed; micalg=pgp-sha256; protocol=\"application/pgp-signature\"; boundary=\"{b}\"\r\n\r\n\
         This is an OpenPGP/MIME signed message (RFC 4880 and 3156)\r\n--{b}\r\n"
    )
    .into_bytes();
    out.extend_from_slice(entity);
    out.extend_from_slice(
        format!(
            "\r\n--{b}\r\nContent-Type: application/pgp-signature; name=\"signature.asc\"\r\nContent-Description: OpenPGP digital signature\r\n\
             Content-Disposition: attachment; filename=\"signature.asc\"\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(&crlf(signature));
    out.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    out
}

/// Hazır giden iletiyi imzalar ve/veya şifreler. Şifrelemede gönderen de alıcılara eklenir
/// (Gönderilmiş kopyası okunabilsin).
pub fn secure_message(store: &Smime, raw: &[u8], method: &str, sign: bool, encrypt: bool, account: &Account, recipients: &[String], pgp_key: &str) -> R<Vec<u8>> {
    if !sign && !encrypt {
        return Ok(raw.to_vec());
    }
    let headers = outer_headers(raw);
    let entity = crlf(&body_entity(raw));
    let own = account.email.to_lowercase();
    let mut rcpts: Vec<String> = recipients.iter().map(|r| r.trim().to_lowercase()).filter(|r| !r.is_empty()).collect();
    if !own.is_empty() {
        rcpts.push(own.clone());
    }
    rcpts.sort();
    rcpts.dedup();
    let body = if method == "pgp" {
        if encrypt {
            // Seçili anahtar kendi adresimizi taşımasa da Gönderilmiş kopyası ona şifrelenir
            if !pgp_key.is_empty() {
                rcpts.retain(|r| *r != own);
                rcpts.push(pgp_key.to_string());
            }
            let missing = pgp_missing(&rcpts)?;
            if !missing.is_empty() {
                return Err(t!("error.pgp_missing", missing.join(", ")));
            }
            pgp_encrypted_body(&pgp_encrypt(&entity, &rcpts, sign.then_some(pgp_key))?)
        } else {
            pgp_signed_body(&entity, &pgp_sign(&entity, pgp_key)?)
        }
    } else {
        // Önce imzala, sonra şifrele (Outlook ile aynı sıra)
        let mut current = entity;
        if sign {
            current = body_entity(&store.sign(&current, account.id)?);
        }
        if encrypt {
            let mut certs = Vec::new();
            let mut missing = Vec::new();
            for a in &rcpts {
                let own_cert = (*a == own && store.identity(account.id).is_some()).then(|| store.root.join(account.id.to_string()).join("cert.pem"));
                match store.recipient(a).or(own_cert) {
                    Some(p) => certs.push(p),
                    None => missing.push(a.clone()),
                }
            }
            if !missing.is_empty() {
                return Err(t!("error.smime_missing", missing.join(", ")));
            }
            current = body_entity(&store.encrypt(&current, &certs)?);
        }
        current
    };
    let mut out = headers;
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(&body);
    Ok(out)
}

fn sender_address(raw: &[u8]) -> String {
    crate::mail::bare_addresses(&parse::parse(&top_message(raw)).sender).into_iter().next().unwrap_or_default().to_lowercase()
}

/// Yalnızca başlıklar: gönderen için tüm iletiyi ayrıştırmaya gerek yok
fn top_message(raw: &[u8]) -> Vec<u8> {
    let mut out = top_headers(raw).to_vec();
    out.extend_from_slice(b"\r\n\r\n");
    out
}

fn process_kind(store: &Smime, raw: &[u8], kind: &str, account: Option<i64>) -> R<SecurityResult> {
    match kind {
        "pgp-signed" => {
            let (signed, sig) = split_signed(raw)?;
            pgp_verify_detached(&signed, &sig)
        }
        "pgp-encrypted" => {
            let p = parse::parse(raw);
            let payload = p.attachments.iter().find(|a| a.mime.contains("octet-stream") || a.name.ends_with(".asc")).map(|a| a.data.clone()).unwrap_or_default();
            let mut r = pgp_decrypt(&payload)?;
            if let Some(inner) = r.decrypted.clone().filter(|d| detect(d) == "pgp-signed") {
                let (signed, sig) = split_signed(&inner)?;
                let v = pgp_verify_detached(&signed, &sig)?;
                r.signed = true;
                r.signature = v.signature;
                r.signer = v.signer;
                r.decrypted = Some(signed);
            }
            Ok(r)
        }
        "pgp-inline-encrypted" => {
            let mut r = pgp_decrypt(parse::parse(raw).text.as_bytes())?;
            r.decrypted = r.decrypted.map(|d| [b"Content-Type: text/plain; charset=utf-8\r\n\r\n".as_slice(), &d].concat());
            Ok(r)
        }
        "pgp-inline-signed" => {
            let mut r = SecurityResult { method: "pgp".into(), signed: true, ..Default::default() };
            let out = gpg(&["--decrypt"], parse::parse(raw).text.as_bytes())?;
            gpg_signature(&gpg_status(&out.stderr), &mut r);
            // Yalnızca imzalanan bölüm gösterilir: imza bloğunun dışına eklenen metin "geçerli" görünmesin
            if out.status.success() {
                r.decrypted = Some([b"Content-Type: text/plain; charset=utf-8\r\n\r\n".as_slice(), &out.stdout].concat());
            }
            Ok(r)
        }
        "smime-signed" | "smime-opaque" => store.verify(raw, &sender_address(raw)),
        "smime-encrypted" => {
            let mut r = store.decrypt(raw, account)?;
            let inner = r.decrypted.clone().unwrap_or_default();
            let inner_kind = detect(&inner);
            if matches!(inner_kind, "smime-signed" | "smime-opaque") {
                let v = store.verify(&inner, &sender_address(raw))?;
                r.signed = true;
                r.signature = v.signature;
                r.signer = v.signer;
                if v.decrypted.is_some() {
                    r.decrypted = v.decrypted;
                } else if inner_kind == "smime-signed" {
                    r.decrypted = Some(split_signed(&inner)?.0);
                }
            }
            Ok(r)
        }
        _ => Ok(SecurityResult::default()),
    }
}

/// Gelen iletiyi doğrular ya da çözer; iç içe (imzala + şifrele) durumları işler. Hata okuma
/// bölmesini düşürmez, sonuçta gösterilir.
pub fn process(store: &Smime, raw: &[u8], account: Option<i64>) -> SecurityResult {
    let kind = detect(raw);
    if kind.is_empty() {
        return SecurityResult::default();
    }
    process_kind(store, raw, kind, account).unwrap_or_else(|error| SecurityResult {
        method: if kind.starts_with("pgp") { "pgp" } else { "smime" }.into(),
        signed: kind.contains("signed") || kind.contains("opaque"),
        encrypted: kind.contains("encrypted"),
        signature: "error".into(),
        error,
        ..Default::default()
    })
}

/// Çözülen varlık taşıma başlıklarıyla birleştirilir: okuma bölmesi normal ileti gibi ayrıştırır
pub fn rebuild(raw: &[u8], entity: &[u8]) -> Vec<u8> {
    let mut out = outer_headers(raw);
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(entity);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::build::{build_raw, ComposedMail};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lynora-crypto-{name}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(to: &str) -> (Account, Vec<u8>) {
        let acc = Account { id: 7, email: "ali@test.local".into(), display_name: "Ali Yılmaz".into(), ..Default::default() };
        let mail = ComposedMail { to: to.into(), subject: "Gizli konu ığüşöç".into(), html: "<p>Şifreli gövde ığüşöç</p>".into(), importance: 1, ..Default::default() };
        let raw = build_raw(Some(&acc), &mail, &[("ek.txt".to_string(), "text/plain".to_string(), "ek içeriği".as_bytes().to_vec())], true);
        (acc, raw)
    }

    #[test]
    fn detects_formats() {
        let cases: &[(&str, &str)] = &[
            ("Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=a\r\n\r\n", "pgp-signed"),
            ("Content-Type: multipart/signed;\r\n protocol=\"application/pkcs7-signature\"; boundary=a\r\n\r\n", "smime-signed"),
            ("Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=a\r\n\r\n", "pgp-encrypted"),
            ("Content-Type: application/pkcs7-mime; smime-type=enveloped-data\r\n\r\nAAAA", "smime-encrypted"),
            ("Content-Type: application/x-pkcs7-mime; smime-type=signed-data\r\n\r\nAAAA", "smime-opaque"),
            ("Content-Type: text/plain\r\n\r\n-----BEGIN PGP MESSAGE-----\r\nx\r\n", "pgp-inline-encrypted"),
            ("Content-Type: text/plain\r\n\r\n-----BEGIN PGP SIGNED MESSAGE-----\r\nx\r\n", "pgp-inline-signed"),
            ("Content-Type: text/plain\r\n\r\nmerhaba\r\n", ""),
            ("Content-Type: multipart/mixed; boundary=a\r\n\r\n", ""),
        ];
        for (raw, kind) in cases {
            assert_eq!(detect(raw.as_bytes()), *kind, "{raw}");
        }
    }

    #[test]
    fn splits_headers_and_parts_on_bytes() {
        let raw = b"From: a@b\r\nSubject: uzun\r\n konu\r\nMIME-Version: 1.0\r\nContent-Type: multipart/signed; boundary=\"s\"\r\n\r\npreamble\r\n--s\r\nContent-Type: text/plain\r\n\r\nimzali  \r\n--s\r\nContent-Type: application/x\r\n\r\nIMZA\r\n--s--\r\n";
        assert_eq!(outer_headers(raw), b"From: a@b\r\nSubject: uzun\r\n konu\r\n");
        assert!(body_entity(raw).starts_with(b"Content-Type: multipart/signed"));
        let (signed, sig) = split_signed(raw).unwrap();
        // Sondaki boşluklar korunur: imza bu baytların üzerinden hesaplanır
        assert_eq!(signed, b"Content-Type: text/plain\r\n\r\nimzali  ");
        assert_eq!(sig, b"IMZA");
        assert!(split_signed(b"Content-Type: multipart/signed\r\n\r\nx").is_err());
    }

    #[test]
    fn signer_must_be_the_sender() {
        let certs = vec!["ali@test.local".to_string()];
        assert!(signer_matches("ALI@test.local", &certs));
        assert!(!signer_matches("ceo@kurum.com", &certs));
        assert!(!signer_matches("ali@test.local", &[]));
        // Gönderen bilinmiyorsa (bozuk başlık) karar verilemez, sonuç değişmez
        assert!(signer_matches("", &certs));
    }

    #[test]
    fn recipient_files_stay_inside() {
        assert_eq!(cert_file_name(" Ali@Test.local "), Some("ali@test.local.pem".into()));
        for bad in ["../../etc/passwd@x", "a/b@x.com", ".gizli@x.com", "yok", "a b@x.com"] {
            assert_eq!(cert_file_name(bad), None, "{bad}");
        }
    }

    /// Gerçek gpg ile: imza, şifreleme, yanlış imza, inline biçim
    #[test]
    fn pgp_end_to_end() {
        if !gpg_available() {
            return;
        }
        crate::i18n::init(Some("tr"));
        let home = temp("gnupg");
        std::env::set_var("GNUPGHOME", &home);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
        for who in ["Ali Yılmaz <ali@test.local>", "Veli <veli@test.local>"] {
            let out = run("gpg", &["--batch", "--passphrase", "", "--quick-generate-key", who, "default", "default", "1d"], &[], b"").unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
        let keys = list_secret_keys();
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys.iter().any(|k| k.uid.contains("ali@test.local")));
        let store = Smime { root: temp("pgp-store") };
        let (acc, raw) = sample("Veli <veli@test.local>");
        let rcpts = vec!["veli@test.local".to_string()];

        // Yalnızca imza: içerik düz görünür, imza geçerli (kendi anahtarı "ultimate" değil: güvenilmiyor ya da iyi)
        let signed = secure_message(&store, &raw, "pgp", true, false, &acc, &rcpts, "ali@test.local").unwrap();
        assert_eq!(detect(&signed), "pgp-signed");
        let r = process(&store, &signed, Some(7));
        assert!(matches!(r.signature.as_str(), "good" | "untrusted"), "{r:?}");
        assert!(r.signer.contains("ali@test.local"), "{r:?}");
        assert!(r.decrypted.is_none());
        assert!(parse::parse(&signed).html.contains("Şifreli gövde"));
        // İmzalandıktan sonra değiştirilen ileti "bad"
        let text = String::from_utf8_lossy(&signed).to_string();
        let tampered = text.replacen("Content-Transfer-Encoding", "Content-Transfer-Encodin9", 1);
        assert_eq!(process(&store, tampered.as_bytes(), Some(7)).signature, "bad");

        // İmzala + şifrele: ham ileti düz metni taşımaz, gönderen de çözebilir, ek korunur
        let sealed = secure_message(&store, &raw, "pgp", true, true, &acc, &rcpts, "").unwrap();
        assert_eq!(detect(&sealed), "pgp-encrypted");
        assert!(find(&sealed, "Şifreli gövde".as_bytes(), 0).is_none());
        assert!(find(&sealed, b"Subject:", 0).is_some(), "taşıma başlıkları açık kalır");
        let r = process(&store, &sealed, Some(7));
        assert!(r.encrypted && r.signed, "{r:?}");
        let full = rebuild(&sealed, r.decrypted.as_ref().unwrap());
        let p = parse::parse(&full);
        assert!(p.html.contains("Şifreli gövde ığüşöç"), "{}", p.html);
        assert_eq!(p.attachments[0].name, "ek.txt");
        assert_eq!(p.subject, "Gizli konu ığüşöç");

        // Anahtarı olmayan alıcıya şifreleme durur
        let e = secure_message(&store, &raw, "pgp", false, true, &acc, &["yok@test.local".to_string()], "").unwrap_err();
        assert!(e.contains("yok@test.local"), "{e}");

        // Satır içi şifreli ve imzalı metin
        let armored = pgp_encrypt(b"satir ici gizli", &["ali@test.local".to_string()], None).unwrap();
        let inline = [b"From: a@b.com\r\nContent-Type: text/plain\r\n\r\n".as_slice(), &armored].concat();
        assert_eq!(detect(&inline), "pgp-inline-encrypted");
        let r = process(&store, &inline, None);
        assert!(parse::parse(&rebuild(&inline, r.decrypted.as_ref().unwrap())).text.contains("satir ici gizli"), "{r:?}");
        let clear = gpg(&["--clearsign", "--local-user", "ali@test.local"], b"imzali satir\n").unwrap().stdout;
        let spoof = [b"From: a@b.com\r\nContent-Type: text/plain\r\n\r\nEKLENEN SAHTE METIN\r\n".as_slice(), &clear].concat();
        let r = process(&store, &spoof, None);
        assert!(r.signed && matches!(r.signature.as_str(), "good" | "untrusted"), "{r:?}");
        let shown = parse::parse(&rebuild(&spoof, r.decrypted.as_ref().unwrap())).text;
        assert!(shown.contains("imzali satir") && !shown.contains("SAHTE"), "imza dışı metin gösterilmemeli: {shown}");
        let _ = std::fs::remove_dir_all(home);
    }

    /// Gerçek openssl ile: .p12 içe aktarma, imza, şifreleme, sertifika toplama
    #[test]
    fn smime_end_to_end() {
        if !openssl_available() {
            return;
        }
        crate::i18n::init(Some("tr"));
        std::env::set_var("LYNORA_NO_KEYRING", "1");
        let work = temp("smime");
        let store = Smime { root: work.join("store") };
        let make = |name: &str, mail: &str, pass: &str| {
            let (k, c, p) = (work.join(format!("{name}.key")), work.join(format!("{name}.crt")), work.join(format!("{name}.p12")));
            let subj = format!("/CN={name}/emailAddress={mail}");
            let out = openssl(&["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", k.to_str().unwrap(), "-out", c.to_str().unwrap(), "-subj", &subj, "-days", "2"], &[], b"").unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            let out = openssl(&["pkcs12", "-export", "-inkey", k.to_str().unwrap(), "-in", c.to_str().unwrap(), "-out", p.to_str().unwrap(), "-passout", &format!("pass:{pass}")], &[], b"").unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            (p, c)
        };
        let (ali_p12, _) = make("ali", "ali@test.local", "ali parola");
        let (veli_p12, veli_crt) = make("veli", "veli@test.local", "veli");
        let (acc, raw) = sample("veli@test.local");

        assert!(store.import_pkcs12(7, ali_p12.to_str().unwrap(), "yanlis").is_err());
        let info = store.import_pkcs12(7, ali_p12.to_str().unwrap(), "ali parola").unwrap();
        assert_eq!(info.emails, ["ali@test.local"]);
        assert!(!info.expires.is_empty());
        assert!(store.identity(7).is_some());
        // Özel anahtar diskte şifreli, izinleri kapalı
        use std::os::unix::fs::PermissionsExt;
        let key = store.root.join("7/key.pem");
        assert!(std::fs::read_to_string(&key).unwrap().contains("ENCRYPTED"));
        assert_eq!(std::fs::metadata(&key).unwrap().permissions().mode() & 0o777, 0o600);

        // İmza: otosertifika güvenilmez ama bütünlük geçerli; değiştirilen ileti "bad"
        let rcpts = vec!["veli@test.local".to_string()];
        let signed = secure_message(&store, &raw, "smime", true, false, &acc, &rcpts, "").unwrap();
        assert_eq!(detect(&signed), "smime-signed");
        let other = Smime { root: work.join("other") };
        let r = process(&other, &signed, None);
        assert_eq!((r.signature.as_str(), r.signer.as_str()), ("untrusted", "ali@test.local"), "{r:?}");
        assert!(parse::parse(&signed).html.contains("Şifreli gövde"));
        // Gönderenin sertifikası yanıtta şifreleme için toplanır
        assert!(other.recipient("ali@test.local").is_some());
        let tampered = String::from_utf8_lossy(&signed).replacen("Content-Transfer-Encoding", "Content-Transfer-Encodin9", 1);
        assert_eq!(process(&other, tampered.as_bytes(), None).signature, "bad");

        // Sertifikası olmayan alıcıya şifreleme durur
        let e = secure_message(&store, &raw, "smime", false, true, &acc, &rcpts, "").unwrap_err();
        assert!(e.contains("veli@test.local"), "{e}");
        assert_eq!(store.store_recipient(&std::fs::read(&veli_crt).unwrap(), "", true).unwrap(), ["veli@test.local"]);

        // Güvenilmeyen imzadan gelen sertifika bilineni ezmez
        let known = other.recipient("ali@test.local").unwrap();
        let before = std::fs::read(&known).unwrap();
        other.store_recipient(&std::fs::read(&veli_crt).unwrap(), "veli@test.local", false).unwrap();
        assert_eq!(other.store_recipient(&before, "ali@test.local", false).unwrap(), Vec::<String>::new());

        // İmzala + şifrele: hem gönderen hem alıcı çözer, imza içeriden okunur
        let sealed = secure_message(&store, &raw, "smime", true, true, &acc, &rcpts, "").unwrap();
        assert_eq!(detect(&sealed), "smime-encrypted");
        assert!(find(&sealed, "Şifreli gövde".as_bytes(), 0).is_none());
        let r = process(&store, &sealed, Some(7));
        assert!(r.encrypted && r.signed, "{r:?}");
        assert_eq!(r.signature, "untrusted");
        let p = parse::parse(&rebuild(&sealed, r.decrypted.as_ref().unwrap()));
        assert!(p.html.contains("Şifreli gövde ığüşöç"), "{}", p.html);
        assert_eq!(p.attachments[0].name, "ek.txt");
        let veli = Smime { root: work.join("veli") };
        veli.import_pkcs12(9, veli_p12.to_str().unwrap(), "veli").unwrap();
        let r = process(&veli, &sealed, Some(9));
        assert!(r.decrypted.is_some() && r.signed, "{r:?}");
        // Kimliği olmayan hesap çözemez, hata gösterilir
        let r = process(&veli, &sealed, Some(1));
        assert!(r.decrypted.is_none() && !r.error.is_empty(), "{r:?}");

        store.remove_identity(7);
        assert!(store.identity(7).is_none() && !store.root.join("7").exists());
        let _ = std::fs::remove_dir_all(work);
    }
}
