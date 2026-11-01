//! Yedekle / geri yükle.
//!
//! Yedek tek dosyadır (.mnbak). İçinde sırayla kayıtlar bulunur: manifest.json, veritabanının
//! tutarlı anlık görüntüsü (VACUUM INTO: uygulama çalışırken alınabilir; hesap ayarları, iletiler,
//! kişiler, takvim, görevler, notlar, kurallar, imzalar, şablonlar, ayarlar) ve sunucuda karşılığı
//! olmayan dosyalar. Parolalar ve belirteçler anahtarlıkta durur, yedeğe girmez.
//!
//! İsteğe bağlı parola ile içerik AES-256-GCM ile şifrelenir (PBKDF2-HMAC-SHA256, 1 MB'lık
//! parçalar; parça sırası ve son parça doğrulanır, düz metin diske hiç yazılmaz).
//!
//! Geri yükleme iki adımlıdır: dosya doğrulanıp hazırlık klasörüne açılır, uygulama yeniden
//! açılırken (veritabanı bağlantısı yokken) yerine konur. Eski veri "pre-restore-<tarih>"
//! klasöründe saklanır.

use crate::db::{default_data_dir, Db};
use crate::t;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

const MAGIC_PLAIN: &[u8; 6] = b"MNBK1\n";
const MAGIC_SEALED: &[u8; 6] = b"MNBE1\n";
const CHUNK: usize = 1 << 20;
const ITERATIONS: u32 = 600_000;
const DB_NAME: &str = "mailnix.db";
const MANIFEST: &str = "manifest.json";
const PENDING: &str = "restore-pending";
/// Kullanıcı onaylayınca yazılır: yalnızca bu dosya varsa açılışta geri yüklenir
const READY: &str = "ready";
/// Yedeğe giren klasörler (veri klasörüne göre). Kişisel S/MIME anahtarları bilerek dışarıda.
const FILE_DIRS: &[&str] = &["smime/certs"];

fn pbkdf2(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mac = <Hmac<Sha256> as Mac>::new_from_slice(password).expect("HMAC her uzunlukta anahtar alır");
    let mut block = [0u8; 32];
    let mut first = mac.clone();
    first.update(salt);
    first.update(&1u32.to_be_bytes());
    block.copy_from_slice(&first.finalize().into_bytes());
    let mut out = block;
    for _ in 1..rounds {
        let mut next = mac.clone();
        next.update(&block);
        block.copy_from_slice(&next.finalize().into_bytes());
        for (a, b) in out.iter_mut().zip(block.iter()) {
            *a ^= *b;
        }
    }
    out
}

fn cipher_for(password: &str, salt: &[u8], rounds: u32) -> Aes256Gcm {
    Aes256Gcm::new_from_slice(&pbkdf2(password.as_bytes(), salt, rounds)).expect("anahtar 32 bayt")
}

/// Parça kimliği: rastgele önek + sıra + son parça işareti
fn nonce(prefix: &[u8; 7], counter: u32, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..7].copy_from_slice(prefix);
    n[7..11].copy_from_slice(&counter.to_be_bytes());
    n[11] = last as u8;
    n
}

struct SealWriter<W: Write> {
    out: W,
    cipher: Aes256Gcm,
    prefix: [u8; 7],
    counter: u32,
    buf: Vec<u8>,
}

impl<W: Write> SealWriter<W> {
    fn new(out: W, cipher: Aes256Gcm, prefix: [u8; 7]) -> Self {
        Self { out, cipher, prefix, counter: 0, buf: Vec::with_capacity(CHUNK * 2) }
    }

    fn seal(&mut self, len: usize, last: bool) -> io::Result<()> {
        let n = nonce(&self.prefix, self.counter, last);
        let sealed = self
            .cipher
            .encrypt(Nonce::from_slice(&n), Payload { msg: &self.buf[..len], aad: MAGIC_SEALED })
            .map_err(|_| io::Error::other("AES-GCM"))?;
        self.out.write_all(&(sealed.len() as u32).to_be_bytes())?;
        self.out.write_all(&sealed)?;
        self.buf.drain(..len);
        self.counter += 1;
        Ok(())
    }

    /// Kalan veriyi son parça olarak yazar; çağrılmazsa yedek eksik sayılır
    fn finish(mut self) -> io::Result<W> {
        let len = self.buf.len();
        self.seal(len, true)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Write for SealWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        // Eşitlikte yazılmaz: tam parça sınırında biten verinin son parçası işaretli kalmalı
        while self.buf.len() > CHUNK {
            self.seal(CHUNK, false)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_len(r: &mut impl Read) -> io::Result<Option<u32>> {
    let mut b = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut b[got..])? {
            0 if got == 0 => return Ok(None),
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => got += n,
        }
    }
    Ok(Some(u32::from_be_bytes(b)))
}

struct OpenReader<R: Read> {
    input: R,
    cipher: Aes256Gcm,
    prefix: [u8; 7],
    counter: u32,
    next_len: Option<u32>,
    buf: Vec<u8>,
    pos: usize,
    done: bool,
}

impl<R: Read> OpenReader<R> {
    fn new(mut input: R, cipher: Aes256Gcm, prefix: [u8; 7]) -> io::Result<Self> {
        let next_len = read_len(&mut input)?;
        Ok(Self { input, cipher, prefix, counter: 0, next_len, buf: Vec::new(), pos: 0, done: false })
    }

    fn fill(&mut self) -> io::Result<()> {
        let bad = || io::Error::new(io::ErrorKind::InvalidData, t!("backup.bad_password"));
        let len = self.next_len.ok_or_else(bad)? as usize;
        if len > CHUNK + 16 {
            return Err(bad());
        }
        let mut sealed = vec![0u8; len];
        self.input.read_exact(&mut sealed)?;
        self.next_len = read_len(&mut self.input)?;
        let last = self.next_len.is_none();
        let n = nonce(&self.prefix, self.counter, last);
        self.buf = self.cipher.decrypt(Nonce::from_slice(&n), Payload { msg: &sealed, aad: MAGIC_SEALED }).map_err(|_| bad())?;
        self.pos = 0;
        self.counter += 1;
        self.done = last;
        Ok(())
    }
}

impl<R: Read> Read for OpenReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.buf.len() {
            if self.done {
                return Ok(0);
            }
            self.fill()?;
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

// ---------------------------------------------------------------------- kayıt biçimi
fn put_entry(w: &mut impl Write, name: &str, size: u64, data: &mut impl Read) -> io::Result<()> {
    w.write_all(&(name.len() as u32).to_be_bytes())?;
    w.write_all(name.as_bytes())?;
    w.write_all(&size.to_be_bytes())?;
    if io::copy(&mut data.by_ref().take(size), w)? != size {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(())
}

fn write_container(w: &mut impl Write, manifest: &Value, snapshot: &Path, files: &[(String, PathBuf)]) -> io::Result<()> {
    let text = manifest.to_string();
    put_entry(w, MANIFEST, text.len() as u64, &mut text.as_bytes())?;
    let mut db = File::open(snapshot)?;
    put_entry(w, DB_NAME, db.metadata()?.len(), &mut db)?;
    for (name, path) in files {
        let mut f = File::open(path)?;
        put_entry(w, name, f.metadata()?.len(), &mut f)?;
    }
    // Ad uzunluğu 0: yedeğin sonu (eksik kalan şifresiz yedek böyle anlaşılır)
    w.write_all(&0u32.to_be_bytes())
}

/// Kayıt adı hazırlık klasörünün dışına çıkamaz
fn safe_target(root: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name);
    let known = name == MANIFEST || name == DB_NAME || name.starts_with("files/");
    (known && rel.components().all(|c| matches!(c, Component::Normal(_)))).then(|| root.join(rel))
}

fn extract(r: &mut impl Read, root: &Path) -> io::Result<()> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, t!("backup.corrupt"));
    loop {
        let len = read_len(r)?.ok_or_else(bad)? as usize;
        if len == 0 {
            return Ok(());
        }
        if len > 4096 {
            return Err(bad());
        }
        let mut name = vec![0u8; len];
        r.read_exact(&mut name)?;
        let name = String::from_utf8(name).map_err(|_| bad())?;
        let mut size = [0u8; 8];
        r.read_exact(&mut size)?;
        let size = u64::from_be_bytes(size);
        let path = safe_target(root, &name).ok_or_else(bad)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = File::create(&path)?;
        if io::copy(&mut r.by_ref().take(size), &mut out)? != size {
            return Err(bad());
        }
    }
}

// ---------------------------------------------------------------------- yedekleme
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            walk(&e.path(), out);
        } else if kind.is_file() {
            out.push(e.path());
        }
    }
}

fn backup_files(data: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for rel in FILE_DIRS {
        let mut found = Vec::new();
        walk(&data.join(rel), &mut found);
        for path in found {
            if let Ok(tail) = path.strip_prefix(data) {
                out.push((format!("files/{}", tail.to_string_lossy()), path));
            }
        }
    }
    out
}

fn manifest(db: &Db, version: &str) -> Value {
    let tables = ["messages", "contacts", "events", "tasks", "notes", "rules"];
    let counts: serde_json::Map<String, Value> = tables.iter().map(|t| (t.to_string(), json!(db.table_count(t)))).collect();
    let accounts: Vec<String> = db.accounts().into_iter().map(|a| a.email).collect();
    json!({
        "format": 1,
        "app": "mailnix",
        "app_version": version,
        "created": chrono::Local::now().to_rfc3339(),
        "accounts": accounts,
        "counts": counts,
    })
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

/// Yedeği `dest` dosyasına yazar. Yarım yedek kalmasın diye önce yanına, bitince yerine konur.
pub fn create(db: &Db, dest: &Path, password: &str, version: &str) -> Result<Value, String> {
    let data = default_data_dir();
    let snapshot = data.join(format!("backup-{}.db", uuid::Uuid::new_v4().simple()));
    let partial = dest.with_extension("part");
    let run = || -> Result<Value, String> {
        db.snapshot_to(&snapshot).map_err(|e| e.to_string())?;
        let info = manifest(db, version);
        let files = backup_files(&data);
        let mut out = BufWriter::new(File::create(&partial).map_err(|e| e.to_string())?);
        #[cfg(unix)]
        {
            // Yedek tüm postayı içerir
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600));
        }
        if password.is_empty() {
            out.write_all(MAGIC_PLAIN).map_err(|e| e.to_string())?;
            write_container(&mut out, &info, &snapshot, &files).map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        } else {
            let salt: [u8; 16] = random_bytes()?;
            let prefix: [u8; 7] = random_bytes()?;
            out.write_all(MAGIC_SEALED).map_err(|e| e.to_string())?;
            out.write_all(&salt).map_err(|e| e.to_string())?;
            out.write_all(&prefix).map_err(|e| e.to_string())?;
            let mut sealed = SealWriter::new(out, cipher_for(password, &salt, ITERATIONS), prefix);
            write_container(&mut sealed, &info, &snapshot, &files).map_err(|e| e.to_string())?;
            sealed.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())?;
        }
        std::fs::rename(&partial, dest).map_err(|e| e.to_string())?;
        Ok(info)
    };
    let result = run();
    let _ = std::fs::remove_file(&snapshot);
    let _ = std::fs::remove_file(&partial);
    result
}

// ---------------------------------------------------------------------- geri yükleme
pub fn is_encrypted(path: &Path) -> Result<bool, String> {
    let mut magic = [0u8; 6];
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    if f.read_exact(&mut magic).is_err() || (&magic != MAGIC_SEALED && &magic != MAGIC_PLAIN) {
        return Err(t!("backup.not_backup"));
    }
    Ok(&magic == MAGIC_SEALED)
}

/// Veritabanının gerçekten bu uygulamaya ait olduğu denetlenir
fn verify(root: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(root.join(MANIFEST)).map_err(|_| t!("backup.corrupt"))?;
    let info: Value = serde_json::from_str(&text).map_err(|_| t!("backup.corrupt"))?;
    if info.get("app").and_then(Value::as_str) != Some("mailnix") {
        return Err(t!("backup.not_backup"));
    }
    let conn = rusqlite::Connection::open_with_flags(root.join(DB_NAME), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| t!("backup.corrupt"))?;
    let ok: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|_| t!("backup.corrupt"))?;
    conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get::<_, i64>(0)).map_err(|_| t!("backup.corrupt"))?;
    if ok != "ok" {
        return Err(t!("backup.corrupt"));
    }
    Ok(info)
}

fn unpack(path: &Path, password: &str, root: &Path) -> Result<Value, String> {
    let encrypted = is_encrypted(path)?;
    if encrypted && password.is_empty() {
        return Err(t!("backup.need_password"));
    }
    let mut input = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let mut magic = [0u8; 6];
    input.read_exact(&mut magic).map_err(|e| e.to_string())?;
    let result = if encrypted {
        let mut salt = [0u8; 16];
        let mut prefix = [0u8; 7];
        input.read_exact(&mut salt).map_err(|_| t!("backup.corrupt"))?;
        input.read_exact(&mut prefix).map_err(|_| t!("backup.corrupt"))?;
        let mut sealed = OpenReader::new(input, cipher_for(password, &salt, ITERATIONS), prefix).map_err(|e| e.to_string())?;
        extract(&mut sealed, root)
    } else {
        extract(&mut input, root)
    };
    result.map_err(|e| if e.kind() == io::ErrorKind::UnexpectedEof { t!("backup.corrupt") } else { e.to_string() })?;
    verify(root)
}

/// Yedeği açıp doğrular; yerine konması için `confirm_restore` gerekir. Döner: manifest.
pub fn prepare(path: &Path, password: &str) -> Result<Value, String> {
    let root = default_data_dir().join(PENDING);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let result = unpack(path, password, &root);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&root);
    }
    result
}

pub fn cancel() {
    let _ = std::fs::remove_dir_all(default_data_dir().join(PENDING));
}

pub fn confirm() -> Result<(), String> {
    std::fs::write(default_data_dir().join(PENDING).join(READY), b"").map_err(|e| e.to_string())
}

/// Açılışta, veritabanı açılmadan önce: onaylanmış geri yüklemeyi yerine koyar
pub fn apply_pending(data: &Path) {
    let pending = data.join(PENDING);
    if !pending.join(READY).exists() {
        return;
    }
    let keep = data.join(format!("pre-restore-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    if std::fs::create_dir_all(&keep).is_err() {
        return;
    }
    for name in [DB_NAME.to_string(), format!("{DB_NAME}-wal"), format!("{DB_NAME}-shm")] {
        let _ = std::fs::rename(data.join(&name), keep.join(&name));
    }
    let _ = std::fs::rename(pending.join(DB_NAME), data.join(DB_NAME));
    for rel in FILE_DIRS {
        let incoming = pending.join("files").join(rel);
        if incoming.is_dir() {
            let target = data.join(rel);
            if let Some(dir) = target.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::remove_dir_all(&target);
            let _ = std::fs::rename(&incoming, &target);
        }
    }
    let _ = std::fs::remove_dir_all(&pending);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("mailnix-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn pbkdf2_vectors() {
        let hex = |b: [u8; 32]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(pbkdf2(b"password", b"salt", 1)), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
        assert_eq!(hex(pbkdf2(b"password", b"salt", 4096)), "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a");
    }

    fn seal(data: &[u8], password: &str) -> Vec<u8> {
        let mut w = SealWriter::new(Vec::new(), cipher_for(password, b"tuz", 1), [7; 7]);
        // Parça sınırları yazma boyutundan bağımsız olmalı
        for part in data.chunks(70_001) {
            w.write_all(part).unwrap();
        }
        w.finish().unwrap()
    }

    fn open(sealed: &[u8], password: &str) -> io::Result<Vec<u8>> {
        let mut r = OpenReader::new(sealed, cipher_for(password, b"tuz", 1), [7; 7])?;
        let mut out = Vec::new();
        r.read_to_end(&mut out)?;
        Ok(out)
    }

    #[test]
    fn sealed_roundtrip() {
        for size in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, CHUNK * 2, CHUNK * 3 + 5] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            assert_eq!(open(&seal(&data, "parola"), "parola").unwrap(), data, "boyut {size}");
        }
    }

    #[test]
    fn sealed_rejects_tampering() {
        let data = vec![5u8; CHUNK * 2 + 10];
        let good = seal(&data, "parola");
        assert!(open(&good, "yanlış").is_err());
        let mut flipped = good.clone();
        flipped[100] ^= 1;
        assert!(open(&flipped, "parola").is_err());
        // Yalnızca ilk parça bırakılırsa "son parça" işareti tutmaz: kesilme anlaşılır
        let first = 4 + u32::from_be_bytes([good[0], good[1], good[2], good[3]]) as usize;
        assert!(open(&good[..first], "parola").is_err());
        assert!(open(&good[..good.len() - 1], "parola").is_err());
    }

    #[test]
    fn container_paths_stay_inside() {
        let root = Path::new("/veri/restore");
        assert_eq!(safe_target(root, DB_NAME), Some(root.join(DB_NAME)));
        assert_eq!(safe_target(root, "files/smime/certs/a.pem"), Some(root.join("files/smime/certs/a.pem")));
        for bad in ["../mailnix.db", "/etc/passwd", "files/../../x", "baska.txt"] {
            assert_eq!(safe_target(root, bad), None, "{bad}");
        }
    }

    #[test]
    fn extract_rejects_truncated_container() {
        let dir = temp_dir();
        let snapshot = dir.join("snap.db");
        std::fs::write(&snapshot, b"veritabani").unwrap();
        let mut bytes = Vec::new();
        write_container(&mut bytes, &json!({"app": "mailnix"}), &snapshot, &[]).unwrap();
        let root = dir.join("out");
        std::fs::create_dir_all(&root).unwrap();
        extract(&mut bytes.as_slice(), &root).unwrap();
        assert_eq!(std::fs::read(root.join(DB_NAME)).unwrap(), b"veritabani");
        assert!(extract(&mut &bytes[..bytes.len() - 4], &root).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_pending_swaps_database() {
        let data = temp_dir();
        let pending = data.join(PENDING);
        std::fs::create_dir_all(&pending).unwrap();
        std::fs::write(data.join(DB_NAME), b"eski").unwrap();
        std::fs::write(pending.join(DB_NAME), b"yeni").unwrap();
        // Onaylanmamış geri yükleme uygulanmaz
        apply_pending(&data);
        assert_eq!(std::fs::read(data.join(DB_NAME)).unwrap(), b"eski");
        std::fs::write(pending.join(READY), b"").unwrap();
        apply_pending(&data);
        assert_eq!(std::fs::read(data.join(DB_NAME)).unwrap(), b"yeni");
        assert!(!pending.exists());
        let kept: Vec<_> = std::fs::read_dir(&data).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("pre-restore-")).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path().join(DB_NAME)).unwrap(), b"eski");
        let _ = std::fs::remove_dir_all(data);
    }
}
