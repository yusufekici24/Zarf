//! TLS bağlantıları (rustls; sistemdeki OpenSSL sürümüne bağımlılık yok).

use super::{MailError, MailResult};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

fn config() -> Arc<ClientConfig> {
    static CFG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // Kurumsal sertifikalar için sistem deposunu da ekle
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = roots.add(cert);
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("TLS protokolleri")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(cfg)
    })
    .clone()
}

pub enum MailStream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl MailStream {
    /// Okuma zaman aşımı (uzun süreli akışlar için değiştirilir)
    pub fn set_read_timeout(&self, t: Option<std::time::Duration>) {
        let _ = match self {
            MailStream::Plain(s) => s.set_read_timeout(t),
            MailStream::Tls(s) => s.sock.set_read_timeout(t),
        };
    }
}

impl Read for MailStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            MailStream::Plain(s) => s.read(buf),
            MailStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for MailStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            MailStream::Plain(s) => s.write(buf),
            MailStream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            MailStream::Plain(s) => s.flush(),
            MailStream::Tls(s) => s.flush(),
        }
    }
}

impl imap::extensions::idle::SetReadTimeout for MailStream {
    /// IDLE bitince kitaplık zaman aşımını kaldırır: kopuk bağlantıda DONE yanıtını sonsuza
    /// dek beklememek için süresiz okuma yerine kısa bir süre kullanılır
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> imap::error::Result<()> {
        MailStream::set_read_timeout(self, Some(timeout.unwrap_or(Duration::from_secs(40))));
        Ok(())
    }
}

/// Geçici ağ hatası mı (yeniden denemeye değer)? Uyku/VPN/Wi-Fi değişimi, sunucunun
/// anlık reddi, yük dengeleyicinin SYN'i düşürmesi gibi durumlarda bağlantı birkaç saniye
/// sonra kurulabilir; tek denemede hata göstermek "bağlanılamadı" uyarılarına yol açıyordu.
fn transient(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(e.kind(), TimedOut | ConnectionRefused | ConnectionReset | ConnectionAborted | Interrupted | WouldBlock | AddrNotAvailable)
        || matches!(e.raw_os_error(), Some(101 | 113 | 110 | 111 | 104)) // ENETUNREACH, EHOSTUNREACH, ETIMEDOUT, ECONNREFUSED, ECONNRESET
}

pub fn tcp_connect(host: &str, port: u16, timeout: Duration) -> MailResult<TcpStream> {
    use super::net;
    if net::work_offline() {
        return Err(MailError::net(crate::t!("net.working_offline")));
    }
    // Kurum ağı dışında: kısa süre önce ulaşılamayan sunucu için beklemeden vazgeç
    if net::recently_down(host) {
        return Err(MailError::net(crate::t!("error.connect", format!("{host}:{port}"), crate::t!("net.disconnected"))));
    }
    let r = tcp_connect_inner(host, port, timeout);
    match &r {
        Ok(_) => {
            net::mark_up(host);
            net::set_disconnected(false);
        }
        Err(_) => net::mark_down(host),
    }
    r
}

fn tcp_connect_inner(host: &str, port: u16, timeout: Duration) -> MailResult<TcpStream> {
    // Adres başına bağlantı süresi kısa tutulur: çalışmayan bir IPv6 adresi
    // tüm süreyi tüketip IPv4'e sıra gelmeden hata vermesin
    let per_addr = timeout.min(Duration::from_secs(12));
    let mut last: Option<std::io::Error> = None;
    // Kısa zaman aşımlı denemeler (Autodiscover adres taraması) tek seferdir; yoksa
    // ulaşılamayan her aday adres için yarım dakika beklenirdi
    let attempts = if timeout >= Duration::from_secs(30) { 3u32 } else { 1 };
    for attempt in 0..attempts {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(700 * attempt as u64 * attempt as u64));
        }
        let mut addrs: Vec<std::net::SocketAddr> = match (host, port).to_socket_addrs() {
            Ok(a) => a.collect(),
            Err(e) => {
                // DNS de (VPN açılırken vb.) anlık başarısız olabilir
                let m = e.to_string().to_lowercase();
                if attempt + 1 < attempts && (m.contains("temporary") || m.contains("try again") || e.kind() == std::io::ErrorKind::TimedOut) {
                    last = Some(e);
                    continue;
                }
                return Err(MailError::net(crate::t!("error.resolve", host, e)));
            }
        };
        // Kurum ağlarında IPv6 çoğu zaman yönlendirilmez: önce IPv4
        addrs.sort_by_key(|a| a.is_ipv6());
        addrs.dedup();
        let mut retry = false;
        for a in &addrs {
            match TcpStream::connect_timeout(a, per_addr) {
                Ok(s) => {
                    s.set_read_timeout(Some(timeout)).ok();
                    s.set_write_timeout(Some(timeout)).ok();
                    s.set_nodelay(true).ok();
                    return Ok(s);
                }
                Err(e) => {
                    retry |= transient(&e);
                    last = Some(e);
                }
            }
        }
        if !retry && !addrs.is_empty() {
            break;
        }
    }
    Err(MailError::net(crate::t!(
        "error.connect",
        format!("{host}:{port}"),
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

pub fn wrap_tls(tcp: TcpStream, host: &str) -> MailResult<MailStream> {
    let name = ServerName::try_from(host.to_string()).map_err(|e| MailError(e.to_string()))?;
    let conn = ClientConnection::new(config(), name).map_err(|e| MailError(e.to_string()))?;
    let mut s = StreamOwned::new(conn, tcp);
    // El sıkışmayı hemen tamamla ki sertifika hataları açıkça raporlansın
    while s.conn.is_handshaking() {
        s.conn
            .complete_io(&mut s.sock)
            .map_err(|e| MailError(crate::t!("error.tls", host, e)))?;
    }
    Ok(MailStream::Tls(Box::new(s)))
}

pub fn connect(host: &str, port: u16, tls: bool, timeout: Duration) -> MailResult<MailStream> {
    let tcp = tcp_connect(host, port, timeout)?;
    if tls {
        wrap_tls(tcp, host)
    } else {
        Ok(MailStream::Plain(tcp))
    }
}
