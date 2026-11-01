//! En küçük DNS istemcisi: SRV kayıtları (etki alanı denetleyicisi ve
//! Autodiscover sunucusu bulmak için). /etc/resolv.conf'taki sunucular kullanılır.

use std::net::{ToSocketAddrs, UdpSocket};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Srv {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    pub target: String,
}

fn nameservers() -> Vec<String> {
    if let Ok(s) = std::env::var("MAILNIX_DNS_SERVER") {
        return vec![s];
    }
    let text = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    let mut v: Vec<String> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("nameserver"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| if s.contains(':') && !s.starts_with('[') { format!("[{s}]:53") } else { format!("{s}:53") })
        .collect();
    if v.is_empty() {
        v.push("127.0.0.53:53".into());
    }
    v
}

pub(crate) fn build_query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut q = Vec::with_capacity(64);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    q
}

/// Sıkıştırılmış adı okur; adın kapladığı baytlardan sonraki konumu döndürür.
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut end = None;
    for _ in 0..64 {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            let ptr = ((len & 0x3f) << 8) | *msg.get(pos + 1)? as usize;
            end.get_or_insert(pos + 2);
            pos = ptr;
            continue;
        }
        labels.push(String::from_utf8_lossy(msg.get(pos + 1..pos + 1 + len)?).to_string());
        pos += 1 + len;
    }
    Some((labels.join("."), end.unwrap_or(pos)))
}

pub(crate) fn parse_srv(msg: &[u8], id: u16) -> Option<Vec<Srv>> {
    if msg.len() < 12 || u16::from_be_bytes([msg[0], msg[1]]) != id {
        return None;
    }
    let rcode = msg[3] & 0x0f;
    if rcode != 0 {
        return Some(vec![]);
    }
    let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qd {
        pos = read_name(msg, pos)?.1 + 4;
    }
    let mut out = Vec::new();
    for _ in 0..an {
        pos = read_name(msg, pos)?.1;
        let rtype = u16::from_be_bytes([*msg.get(pos)?, *msg.get(pos + 1)?]);
        let rdlen = u16::from_be_bytes([*msg.get(pos + 8)?, *msg.get(pos + 9)?]) as usize;
        let rd = pos + 10;
        if rtype == 33 && rdlen >= 7 {
            let p = |i: usize| u16::from_be_bytes([msg[rd + i], msg[rd + i + 1]]);
            let (target, _) = read_name(msg, rd + 6)?;
            out.push(Srv { priority: p(0), weight: p(2), port: p(4), target });
        }
        pos = rd + rdlen;
    }
    // Önce düşük öncelik, sonra yüksek ağırlık
    out.sort_by(|a, b| a.priority.cmp(&b.priority).then(b.weight.cmp(&a.weight)));
    Some(out)
}

/// `_ldap._tcp.kurum.local` gibi bir SRV kaydını sorgular.
pub fn srv(name: &str) -> Vec<Srv> {
    let id = (std::process::id() as u16) ^ (name.len() as u16).wrapping_mul(7919);
    let query = build_query(id, name, 33);
    for ns in nameservers() {
        let Ok(Some(addr)) = ns.to_socket_addrs().map(|mut a| a.next()) else { continue };
        let bind = if addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
        let Ok(sock) = UdpSocket::bind(bind) else { continue };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
        if sock.send_to(&query, addr).is_err() {
            continue;
        }
        let mut buf = [0u8; 4096];
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if let Some(v) = parse_srv(&buf[..n], id) {
                return v;
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srv_answer_with_compression() {
        let id = 0x1234;
        let mut m = build_query(id, "_ldap._tcp.kurum.local", 33);
        m[2] = 0x81;
        m[3] = 0x80;
        m[7] = 1; // bir yanıt
        // ad: soruya işaretçi (0xc00c), tür SRV, sınıf IN, TTL, uzunluk
        let target = b"\x03dc1\x05kurum\x05local\x00";
        m.extend_from_slice(&[0xc0, 0x0c, 0, 33, 0, 1, 0, 0, 0x0e, 0x10]);
        m.extend_from_slice(&((6 + target.len()) as u16).to_be_bytes());
        m.extend_from_slice(&[0, 0, 0, 100, 1, 0x85]);
        m.extend_from_slice(target);
        let v = parse_srv(&m, id).unwrap();
        assert_eq!(v, vec![Srv { priority: 0, weight: 100, port: 389, target: "dc1.kurum.local".into() }]);
    }
}
