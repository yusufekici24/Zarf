//! Microsoft Outlook .msg dosyaları (OLE / Compound File Binary).
//!
//! * Okuma: iletiler, randevular (IPM.Appointment), kişiler (IPM.Contact) ve
//!   görevler (IPM.Task). Sıkıştırılmış RTF gövdeler (LZFu) açılır ve RTF içine
//!   gömülü HTML çıkarılır. Gömülü .msg ekleri .eml olarak sunulur.
//! * Yazma: herhangi bir iletiyi Outlook'un açabileceği .msg dosyasına çevirir.

use crate::db::{Contact, Event, Task};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

type Cfb = cfb::CompoundFile<Cursor<Vec<u8>>>;

pub const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

pub fn is_msg(data: &[u8]) -> bool {
    data.len() > 8 && data[..8] == CFB_SIGNATURE
}

#[derive(Clone, Debug)]
pub enum Prop {
    Str(String),
    Bin(Vec<u8>),
    Int(i64),
    Bool(bool),
    Float(f64),
    Time(f64),
}

const PT_I2: u16 = 0x0002;
const PT_LONG: u16 = 0x0003;
const PT_DOUBLE: u16 = 0x0005;
const PT_BOOLEAN: u16 = 0x000B;
const PT_I8: u16 = 0x0014;
const PT_STRING8: u16 = 0x001E;
const PT_UNICODE: u16 = 0x001F;
const PT_SYSTIME: u16 = 0x0040;
const PT_BINARY: u16 = 0x0102;

fn filetime_to_ts(v: u64) -> Option<f64> {
    if v == 0 || v == u64::MAX {
        return None;
    }
    let secs = v as f64 / 10_000_000.0 - 11_644_473_600.0;
    if (0.0..4_102_444_800.0).contains(&secs) {
        Some(secs)
    } else {
        None
    }
}

fn ts_to_filetime(ts: f64) -> u64 {
    ((ts + 11_644_473_600.0) * 10_000_000.0) as u64
}

/// Windows kod sayfası → kodlama. Bilinmeyen/eksik kod sayfasında windows-1254 kullanılır:
/// 1252'den yalnızca Ğ İ Ş ğ ı ş (Ð Ý Þ ð ý þ yerine) ile ayrılır, Türkçe iletiler bozulmaz.
pub(crate) fn codepage_encoding(codepage: u32) -> &'static encoding_rs::Encoding {
    match codepage {
        65001 => encoding_rs::UTF_8,
        1252 | 28591 | 20127 => encoding_rs::WINDOWS_1252,
        1254 | 28599 => encoding_rs::WINDOWS_1254,
        874 => encoding_rs::WINDOWS_874,
        932 | 50220 | 50221 | 50222 => encoding_rs::SHIFT_JIS,
        936 | 54936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        1200 => encoding_rs::UTF_16LE,
        1201 => encoding_rs::UTF_16BE,
        20866 => encoding_rs::KOI8_R,
        21866 => encoding_rs::KOI8_U,
        51932 => encoding_rs::EUC_JP,
        28592..=28598 | 28603 | 28605 => {
            encoding_rs::Encoding::for_label(format!("iso-8859-{}", codepage - 28590).as_bytes()).unwrap_or(encoding_rs::WINDOWS_1254)
        }
        1250..=1258 => encoding_rs::Encoding::for_label(format!("windows-{codepage}").as_bytes()).unwrap_or(encoding_rs::WINDOWS_1254),
        _ => encoding_rs::WINDOWS_1254,
    }
}

fn decode_8bit(data: &[u8], codepage: u32) -> String {
    codepage_encoding(codepage).decode(data).0.trim_end_matches('\0').to_string()
}

/// RTF yazı tipi karakter kümesi (\fcharsetN) → kod sayfası; 0/1 belgenin kod sayfası demektir.
fn rtf_charset_codepage(charset: u32) -> Option<u32> {
    Some(match charset {
        77 => 10000,
        128 => 932,
        129 => 949,
        134 => 936,
        136 => 950,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        _ => return None,
    })
}

/// Yazı tipi tablosundan yazı tipi numarası → kod sayfası (\fcharset162 Türkçe vb.)
fn rtf_font_codepages(rtf: &[u8]) -> HashMap<i32, u32> {
    let mut map = HashMap::new();
    let Some(start) = rtf.windows(8).position(|w| w == b"\\fonttbl") else { return map };
    // Tablonun sonu: açılan gruba denk gelen kapanış
    let mut depth = 1i32;
    let mut end = start;
    for (k, &b) in rtf[start..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + k;
                    break;
                }
            }
            _ => {}
        }
    }
    let table = &rtf[start..end.max(start)];
    let re = regex::bytes::Regex::new(r"\\f(\d+)[^;{}]*?\\fcharset(\d+)").unwrap();
    for c in re.captures_iter(table) {
        let f = std::str::from_utf8(&c[1]).ok().and_then(|x| x.parse().ok());
        let cs = std::str::from_utf8(&c[2]).ok().and_then(|x| x.parse().ok()).and_then(rtf_charset_codepage);
        if let (Some(f), Some(cp)) = (f, cs) {
            map.insert(f, cp);
        }
    }
    map
}

/// HTML gövde baytları: geçerli UTF-8 ise UTF-8 (MailNix/yeni Outlook), değilse
/// <meta charset>, sonra iletinin kod sayfası (PR_INTERNET_CPID), en son windows-1254.
pub(crate) fn decode_html_bytes(b: &[u8], codepage: Option<u32>) -> String {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    if let Ok(s) = std::str::from_utf8(b) {
        return s.trim_end_matches('\0').to_string();
    }
    let head = String::from_utf8_lossy(&b[..b.len().min(4000)]).to_ascii_lowercase();
    let label = head.find("charset=").map(|p| {
        head[p + 8..].trim_start_matches(['"', '\'', ' ']).chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect::<String>()
    });
    let enc = label
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .filter(|e| *e != encoding_rs::UTF_8)
        .unwrap_or_else(|| codepage_encoding(codepage.filter(|c| *c != 65001).unwrap_or(1254)));
    // iso-8859-1/windows-1252 olarak işaretli Türkçe iletiler de vardır: Outlook bunları 1254 yazar
    let enc = if enc == encoding_rs::WINDOWS_1252 && codepage == Some(1254) { encoding_rs::WINDOWS_1254 } else { enc };
    enc.decode(b).0.trim_end_matches('\0').to_string()
}

fn utf16le(data: &[u8]) -> String {
    let u: Vec<u16> = data.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&u).trim_end_matches('\0').to_string()
}

#[derive(Default, Clone)]
struct PropStore {
    props: HashMap<u16, Prop>,
    objects: HashMap<u16, PathBuf>,
}

impl PropStore {
    fn read(cfb: &mut Cfb, storage: &Path, header: usize, codepage: u32) -> PropStore {
        let mut ps = PropStore::default();
        let entries: Vec<(String, PathBuf, bool)> = match cfb.read_storage(storage) {
            Ok(it) => it.map(|e| (e.name().to_string(), e.path().to_path_buf(), e.is_storage())).collect(),
            Err(_) => return ps,
        };
        for (name, path, is_storage) in &entries {
            let lname = name.to_lowercase();
            if !lname.starts_with("__substg1.0_") || lname.len() < 20 {
                continue;
            }
            if lname.len() > 20 {
                continue; // çok değerli özellik parçaları
            }
            let tag = &lname[12..20];
            let (Ok(pid), Ok(ptype)) = (u16::from_str_radix(&tag[..4], 16), u16::from_str_radix(&tag[4..], 16)) else {
                continue;
            };
            if *is_storage {
                ps.objects.insert(pid, path.clone());
                continue;
            }
            let mut data = Vec::new();
            if let Ok(mut s) = cfb.open_stream(path) {
                let _ = s.read_to_end(&mut data);
            }
            let v = match ptype {
                PT_UNICODE => Prop::Str(utf16le(&data)),
                PT_STRING8 => Prop::Str(decode_8bit(&data, codepage)),
                _ => Prop::Bin(data),
            };
            ps.props.insert(pid, v);
        }
        let pv = storage.join("__properties_version1.0");
        let mut data = Vec::new();
        if let Ok(mut s) = cfb.open_stream(&pv) {
            let _ = s.read_to_end(&mut data);
        }
        let mut off = header;
        while off + 16 <= data.len() {
            let ptype = u16::from_le_bytes([data[off], data[off + 1]]);
            let pid = u16::from_le_bytes([data[off + 2], data[off + 3]]);
            let v = &data[off + 8..off + 16];
            off += 16;
            if ps.props.contains_key(&pid) {
                continue;
            }
            let val = match ptype {
                PT_I2 => Prop::Int(i16::from_le_bytes([v[0], v[1]]) as i64),
                PT_LONG => Prop::Int(i32::from_le_bytes([v[0], v[1], v[2], v[3]]) as i64),
                PT_BOOLEAN => Prop::Bool(v[0] != 0),
                PT_I8 => Prop::Int(i64::from_le_bytes(v.try_into().unwrap())),
                PT_DOUBLE => Prop::Float(f64::from_le_bytes(v.try_into().unwrap())),
                PT_SYSTIME => match filetime_to_ts(u64::from_le_bytes(v.try_into().unwrap())) {
                    Some(t) => Prop::Time(t),
                    None => continue,
                },
                _ => continue,
            };
            ps.props.insert(pid, val);
        }
        ps
    }

    fn str(&self, pids: &[u16]) -> String {
        for p in pids {
            match self.props.get(p) {
                Some(Prop::Str(s)) if !s.trim().is_empty() => return s.trim().to_string(),
                Some(Prop::Bin(b)) if !b.is_empty() => {
                    let s = String::from_utf8_lossy(b).trim_end_matches('\0').trim().to_string();
                    if !s.is_empty() {
                        return s;
                    }
                }
                _ => {}
            }
        }
        String::new()
    }

    fn int(&self, pid: u16) -> Option<i64> {
        match self.props.get(&pid) {
            Some(Prop::Int(i)) => Some(*i),
            Some(Prop::Bool(b)) => Some(*b as i64),
            _ => None,
        }
    }

    fn time(&self, pid: u16) -> Option<f64> {
        match self.props.get(&pid) {
            Some(Prop::Time(t)) => Some(*t),
            _ => None,
        }
    }

    fn bin(&self, pid: u16) -> Option<&Vec<u8>> {
        match self.props.get(&pid) {
            Some(Prop::Bin(b)) => Some(b),
            _ => None,
        }
    }
}

// ============================================================ adlandırılmış özellikler
const PSETID_ADDRESS: &str = "00062004-0000-0000-c000-000000000046";
const PSETID_APPOINTMENT: &str = "00062002-0000-0000-c000-000000000046";
const PSETID_COMMON: &str = "00062008-0000-0000-c000-000000000046";
const PSETID_TASK: &str = "00062003-0000-0000-c000-000000000046";

fn guid_str(b: &[u8]) -> String {
    let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let d2 = u16::from_le_bytes([b[4], b[5]]);
    let d3 = u16::from_le_bytes([b[6], b[7]]);
    let rest: String = b[8..16].iter().map(|x| format!("{x:02x}")).collect();
    format!("{d1:08x}-{d2:04x}-{d3:04x}-{}-{}", &rest[..4], &rest[4..])
}

fn read_named_map(cfb: &mut Cfb) -> HashMap<(String, u32), u16> {
    let mut out = HashMap::new();
    let base = Path::new("/__nameid_version1.0");
    let read = |cfb: &mut Cfb, name: &str| -> Vec<u8> {
        let mut d = Vec::new();
        if let Ok(mut s) = cfb.open_stream(base.join(name)) {
            let _ = s.read_to_end(&mut d);
        }
        d
    };
    let guids_raw = read(cfb, "__substg1.0_00020102");
    let entries = read(cfb, "__substg1.0_00030102");
    let guids: Vec<String> = guids_raw.chunks(16).filter(|c| c.len() == 16).map(guid_str).collect();
    for e in entries.chunks(8).filter(|c| c.len() == 8) {
        let lid = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
        let info = u32::from_le_bytes([e[4], e[5], e[6], e[7]]);
        if info & 1 != 0 {
            continue; // dize adlı özellik
        }
        let guid_index = ((info >> 1) & 0x7FFF) as usize;
        let prop_index = (info >> 16) as u16;
        let guid = match guid_index {
            1 => "00020328-0000-0000-c000-000000000046".to_string(),
            2 => "00020329-0000-0000-c000-000000000046".to_string(),
            n if n >= 3 && n - 3 < guids.len() => guids[n - 3].clone(),
            _ => continue,
        };
        out.insert((guid, lid), 0x8000 + prop_index);
    }
    out
}

// ============================================================ RTF
pub const RTF_PREBUF: &[u8] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

/// MS-OXRTFCP sıkıştırılmış RTF'yi açar.
pub fn decompress_rtf(data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let rd = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let comp_size = rd(0) as usize;
    let raw_size = rd(4) as usize;
    let magic = rd(8);
    if magic == 0x414C454D {
        return data[16..(16 + raw_size).min(data.len())].to_vec();
    }
    if magic != 0x75465A4C {
        return Vec::new();
    }
    let mut dict = [0u8; 4096];
    dict[..RTF_PREBUF.len()].copy_from_slice(RTF_PREBUF);
    let mut wpos = RTF_PREBUF.len();
    let mut out = Vec::with_capacity(raw_size);
    let mut pos = 16;
    let end = data.len().min(comp_size + 4);
    while pos < end {
        let control = data[pos];
        pos += 1;
        for bit in 0..8 {
            if pos >= end {
                break;
            }
            if control & (1 << bit) != 0 {
                if pos + 1 >= data.len() {
                    return out;
                }
                let r = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
                pos += 2;
                let mut offset = r >> 4;
                let len = (r & 0xF) + 2;
                if offset == wpos {
                    return out;
                }
                for _ in 0..len {
                    let c = dict[offset];
                    out.push(c);
                    dict[wpos] = c;
                    wpos = (wpos + 1) & 0xFFF;
                    offset = (offset + 1) & 0xFFF;
                }
            } else {
                let c = data[pos];
                pos += 1;
                out.push(c);
                dict[wpos] = c;
                wpos = (wpos + 1) & 0xFFF;
            }
        }
    }
    out
}

const SKIP_DESTS: &[&[u8]] = &[
    b"fonttbl", b"colortbl", b"stylesheet", b"info", b"pict", b"header", b"footer",
    b"listtable", b"listoverridetable", b"rsidtbl", b"generator", b"xmlnstbl", b"themedata",
    b"colorschememapping", b"latentstyles", b"datastore", b"filetbl", b"revtbl",
];

enum Piece {
    /// Bayt ve çözüleceği kod sayfası (yazı tipine göre)
    Bytes(Vec<u8>, u32),
    Char(char),
}

/// RTF'den (html_mi, metin) çıkarır.
pub fn rtf_extract(rtf: &[u8]) -> (bool, String) {
    let is_html = rtf.windows(9).take(1200).any(|w| w == b"\\fromhtml");
    let mut codepage = 1252u32;
    let fonts = rtf_font_codepages(rtf);
    // Geçerli yazı tipinin kod sayfası (0 = belgenin \ansicpg değeri)
    let mut font_cp = 0u32;
    let mut out: Vec<Piece> = Vec::new();
    let mut stack: Vec<(bool, bool, bool, usize, u32)> = Vec::new();
    let (mut suppress, mut in_tag, mut skip, mut uc) = (false, false, false, 1usize);
    let mut pending_skip = 0usize;
    let mut group_start = false;
    let n = rtf.len();
    let mut i = 0;
    macro_rules! emit {
        ($b:expr) => {
            if !skip && !(is_html && suppress && !in_tag) {
                out.push($b);
            }
        };
    }
    while i < n {
        let c = rtf[i];
        match c {
            b'{' => {
                stack.push((suppress, in_tag, skip, uc, font_cp));
                group_start = true;
                i += 1;
            }
            b'}' => {
                if let Some(s) = stack.pop() {
                    (suppress, in_tag, skip, uc, font_cp) = s;
                }
                group_start = false;
                i += 1;
            }
            b'\\' => {
                if i + 1 >= n {
                    break;
                }
                let nc = rtf[i + 1];
                if nc == b'\'' {
                    let b = std::str::from_utf8(&rtf[(i + 2).min(n)..(i + 4).min(n)])
                        .ok()
                        .and_then(|h| u8::from_str_radix(h, 16).ok());
                    i += 4;
                    group_start = false;
                    if pending_skip > 0 {
                        pending_skip -= 1;
                    } else if let Some(b) = b {
                        emit!(Piece::Bytes(vec![b], font_cp));
                    }
                    continue;
                }
                if nc == b'\\' || nc == b'{' || nc == b'}' {
                    emit!(Piece::Bytes(vec![nc], font_cp));
                    i += 2;
                    group_start = false;
                    continue;
                }
                if nc == b'*' {
                    let mut j = i + 2;
                    while j < n && matches!(rtf[j], b' ' | b'\r' | b'\n') {
                        j += 1;
                    }
                    if rtf[j..].starts_with(b"\\htmltag") {
                        in_tag = true;
                        suppress = false;
                    } else {
                        skip = true;
                    }
                    i = j;
                    group_start = false;
                    continue;
                }
                if nc == b'~' {
                    emit!(Piece::Char('\u{a0}'));
                    i += 2;
                    continue;
                }
                if nc == b'-' || nc == b'_' {
                    i += 2;
                    continue;
                }
                if nc == b'\r' || nc == b'\n' {
                    emit!(Piece::Char('\n'));
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                while j < n && rtf[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let word = &rtf[i + 1..j];
                let mut k = j;
                if k < n && (rtf[k] == b'-' || rtf[k].is_ascii_digit()) {
                    k += 1;
                    while k < n && rtf[k].is_ascii_digit() {
                        k += 1;
                    }
                }
                let param = std::str::from_utf8(&rtf[j..k]).unwrap_or("");
                if k < n && rtf[k] == b' ' {
                    k += 1;
                }
                i = k;
                if group_start && SKIP_DESTS.contains(&word) {
                    skip = true;
                }
                group_start = false;
                match word {
                    b"htmlrtf" => suppress = param != "0",
                    b"htmltag" => in_tag = true,
                    b"ansicpg" => codepage = param.parse().unwrap_or(1252),
                    b"f" if !skip => font_cp = param.parse::<i32>().ok().and_then(|f| fonts.get(&f).copied()).unwrap_or(0),
                    b"par" | b"line" => emit!(Piece::Char('\n')),
                    b"tab" => emit!(Piece::Char('\t')),
                    b"uc" => uc = param.parse().unwrap_or(1),
                    b"u" => {
                        if let Ok(mut v) = param.parse::<i32>() {
                            if v < 0 {
                                v += 65536;
                            }
                            if let Some(ch) = char::from_u32(v as u32) {
                                emit!(Piece::Char(ch));
                            }
                            pending_skip = uc;
                        }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                i += 1;
                if pending_skip > 0 {
                    pending_skip -= 1;
                    continue;
                }
                group_start = false;
                emit!(Piece::Bytes(vec![c], font_cp));
            }
        }
    }
    let mut text = String::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut buf_cp = 0u32;
    let doc_cp = |cp: u32| if cp == 0 { codepage } else { cp };
    for p in out {
        match p {
            Piece::Bytes(b, cp) => {
                if !buf.is_empty() && cp != buf_cp {
                    text.push_str(&decode_8bit(&buf, doc_cp(buf_cp)));
                    buf.clear();
                }
                buf_cp = cp;
                buf.extend(b);
            }
            Piece::Char(ch) => {
                if !buf.is_empty() {
                    text.push_str(&decode_8bit(&buf, doc_cp(buf_cp)));
                    buf.clear();
                }
                text.push(ch);
            }
        }
    }
    if !buf.is_empty() {
        text.push_str(&decode_8bit(&buf, doc_cp(buf_cp)));
    }
    (is_html, text)
}

// ============================================================ MSG
#[derive(Clone, Debug, Default)]
pub struct MsgRecipient {
    pub name: String,
    pub email: String,
    pub kind: i64,
}

#[derive(Clone, Debug, Default)]
pub struct MsgAttachment {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    pub cid: String,
}

pub struct MsgFile {
    props: PropStore,
    named: HashMap<(String, u32), u16>,
    pub message_class: String,
    pub recipients: Vec<MsgRecipient>,
    pub attachments: Vec<MsgAttachment>,
}

const CODEPAGE_PROP: u16 = 0x3FDE;
const MESSAGE_CODEPAGE_PROP: u16 = 0x3FFD;

impl MsgFile {
    pub fn parse(data: &[u8]) -> Result<MsgFile, String> {
        if !is_msg(data) {
            return Err(crate::t!("msg.invalid"));
        }
        let mut cfb = cfb::CompoundFile::open(Cursor::new(data.to_vec())).map_err(|e| e.to_string())?;
        let named = read_named_map(&mut cfb);
        Self::from_storage(&mut cfb, Path::new("/"), 32, named, 0)
    }

    fn from_storage(
        cfb: &mut Cfb,
        root: &Path,
        header: usize,
        named: HashMap<(String, u32), u16>,
        depth: usize,
    ) -> Result<MsgFile, String> {
        // 8 bit (ANSI) dizeler PR_MESSAGE_CODEPAGE, yoksa PR_INTERNET_CPID ile çözülür;
        // ikisi de yoksa windows-1254 (Türkçe Windows'ta kaydedilen ANSI .msg)
        let mut props = PropStore::read(cfb, root, header, 1254);
        let cp = props.int(MESSAGE_CODEPAGE_PROP).or_else(|| props.int(CODEPAGE_PROP)).filter(|c| *c > 0).unwrap_or(1254) as u32;
        if cp != 1254 {
            props = PropStore::read(cfb, root, header, cp);
        }
        let message_class = {
            let s = props.str(&[0x001A]);
            if s.is_empty() { "IPM.Note".into() } else { s }
        };
        let mut children: Vec<(String, PathBuf)> = cfb
            .read_storage(root)
            .map(|it| it.filter(|e| e.is_storage()).map(|e| (e.name().to_lowercase(), e.path().to_path_buf())).collect())
            .unwrap_or_default();
        children.sort();
        let mut recipients = Vec::new();
        let mut attachments = Vec::new();
        for (name, path) in children {
            if name.starts_with("__recip_version1.0_") {
                let r = PropStore::read(cfb, &path, 8, cp);
                let addr = r.str(&[0x39FE, 0x3003]);
                recipients.push(MsgRecipient {
                    name: r.str(&[0x3001]),
                    email: if addr.contains('@') { addr } else { String::new() },
                    kind: r.int(0x0C15).unwrap_or(1),
                });
            } else if name.starts_with("__attach_version1.0_") {
                let a = PropStore::read(cfb, &path, 8, cp);
                let mut att = MsgAttachment {
                    name: {
                        let n = a.str(&[0x3707, 0x3704, 0x3001]);
                        if n.is_empty() { "ek".into() } else { n }
                    },
                    mime: a.str(&[0x370E]),
                    cid: a.str(&[0x3712]),
                    data: a.bin(0x3701).cloned().unwrap_or_default(),
                };
                if let Some(sub) = a.objects.get(&0x3701) {
                    if depth < 5 {
                        if let Ok(inner) = MsgFile::from_storage(cfb, sub, 24, named.clone(), depth + 1) {
                            att.data = inner.to_eml();
                            att.mime = "message/rfc822".into();
                            let subj = inner.subject();
                            att.name = format!("{}.eml", crate::util::safe_filename(if subj.is_empty() { "ileti" } else { &subj }));
                        }
                    }
                }
                if att.mime.is_empty() {
                    att.mime = crate::mail::build::guess_mime(&att.name);
                }
                attachments.push(att);
            }
        }
        Ok(MsgFile { props, named, message_class, recipients, attachments })
    }

    fn named_pid(&self, set: &str, lid: u32) -> Option<u16> {
        self.named.get(&(set.to_string(), lid)).copied()
    }

    fn named_time(&self, set: &str, lid: u32) -> Option<f64> {
        self.named_pid(set, lid).and_then(|p| self.props.time(p))
    }

    fn named_str(&self, set: &str, lid: u32) -> String {
        self.named_pid(set, lid).map(|p| self.props.str(&[p])).unwrap_or_default()
    }

    fn named_int(&self, set: &str, lid: u32) -> Option<i64> {
        self.named_pid(set, lid).and_then(|p| self.props.int(p))
    }

    fn named_float(&self, set: &str, lid: u32) -> Option<f64> {
        self.named_pid(set, lid).and_then(|p| match self.props.props.get(&p) {
            Some(Prop::Float(f)) => Some(*f),
            _ => None,
        })
    }

    pub fn subject(&self) -> String {
        self.props.str(&[0x0037])
    }

    fn headers(&self) -> HashMap<String, String> {
        let raw = self.props.str(&[0x007D]);
        let mut map = HashMap::new();
        let mut last = String::new();
        for line in raw.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(v) = map.get_mut(&last) {
                    let v: &mut String = v;
                    v.push(' ');
                    v.push_str(line.trim());
                }
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                last = k.trim().to_lowercase();
                map.insert(last.clone(), v.trim().to_string());
            }
        }
        map
    }

    pub fn sender(&self) -> String {
        let name = self.props.str(&[0x0C1A, 0x0042]);
        let addr = self.props.str(&[0x5D01, 0x0C1F, 0x0065, 0x5D02]);
        if !addr.contains('@') {
            if let Some(h) = self.headers().get("from") {
                return h.clone();
            }
            return name;
        }
        if name.is_empty() || name == addr {
            addr
        } else {
            format!("{name} <{addr}>")
        }
    }

    fn recips(&self, kind: i64, display_pid: u16) -> String {
        let v: Vec<String> = self
            .recipients
            .iter()
            .filter(|r| r.kind == kind)
            .filter_map(|r| {
                if !r.email.is_empty() {
                    Some(if r.name.is_empty() || r.name == r.email {
                        r.email.clone()
                    } else if r.name.contains(',') {
                        format!("\"{}\" <{}>", r.name, r.email)
                    } else {
                        format!("{} <{}>", r.name, r.email)
                    })
                } else if !r.name.is_empty() {
                    Some(r.name.clone())
                } else {
                    None
                }
            })
            .collect();
        if v.is_empty() {
            self.props.str(&[display_pid]).replace(';', ",")
        } else {
            v.join(", ")
        }
    }

    pub fn to(&self) -> String {
        self.recips(1, 0x0E04)
    }
    pub fn cc(&self) -> String {
        self.recips(2, 0x0E03)
    }
    pub fn bcc(&self) -> String {
        self.recips(3, 0x0E02)
    }

    pub fn date(&self) -> f64 {
        self.props
            .time(0x0E06)
            .or_else(|| self.props.time(0x0039))
            .or_else(|| self.props.time(0x3007))
            .unwrap_or_else(crate::db::now)
    }

    pub fn importance(&self) -> i64 {
        self.props.int(0x0017).filter(|v| (0..=2).contains(v)).unwrap_or(1)
    }

    /// (düz metin, html)
    pub fn bodies(&self) -> (String, String) {
        let mut text = self.props.str(&[0x1000]);
        let mut html = String::new();
        match self.props.props.get(&0x1013) {
            Some(Prop::Bin(b)) if !b.is_empty() => {
                html = decode_html_bytes(b, self.props.int(CODEPAGE_PROP).map(|c| c as u32));
            }
            Some(Prop::Str(s)) => html = s.clone(),
            _ => {}
        }
        if html.is_empty() {
            if let Some(rtf) = self.props.bin(0x1009) {
                let (is_html, content) = rtf_extract(&decompress_rtf(rtf));
                if is_html {
                    html = content;
                } else if text.is_empty() {
                    text = content.trim().to_string();
                }
            }
        }
        if text.is_empty() && !html.is_empty() {
            text = crate::mail::html::html_to_text(&html);
        }
        (text, html)
    }

    pub fn to_eml(&self) -> Vec<u8> {
        use mail_builder::headers::raw::Raw;
        use mail_builder::headers::text::Text;
        use mail_builder::MessageBuilder;
        let (text, html) = self.bodies();
        let mut b = MessageBuilder::new()
            .subject(self.subject())
            .date(self.date() as i64)
            .header("X-MailNix-Source", Raw::new("msg"))
            .header("X-MS-Message-Class", Raw::new(self.message_class.clone()));
        let sender = self.sender();
        if !sender.is_empty() {
            b = b.header("From", Text::new(sender));
        }
        for (h, v) in [("To", self.to()), ("Cc", self.cc()), ("Bcc", self.bcc())] {
            if !v.is_empty() {
                b = b.header(h, Text::new(v));
            }
        }
        let mid = self.props.str(&[0x1035]);
        if !mid.is_empty() {
            b = b.message_id(mid.trim_matches(|c| c == '<' || c == '>').to_string());
        }
        let irt = self.props.str(&[0x1042]);
        if !irt.is_empty() {
            b = b.in_reply_to(irt.trim_matches(|c| c == '<' || c == '>').to_string());
        }
        match self.importance() {
            2 => b = b.header("Importance", Raw::new("High")).header("X-Priority", Raw::new("1")),
            0 => b = b.header("Importance", Raw::new("Low")).header("X-Priority", Raw::new("5")),
            _ => {}
        }
        b = b.text_body(text);
        if !html.is_empty() {
            b = b.html_body(html.clone());
        }
        for a in &self.attachments {
            if !a.cid.is_empty() && html.contains(&format!("cid:{}", a.cid)) {
                b = b.inline(a.mime.clone(), a.cid.clone(), a.data.clone());
            } else {
                b = b.attachment(a.mime.clone(), a.name.clone(), a.data.clone());
            }
        }
        if let Some(ev) = self.to_event() {
            b = b.attachment("text/calendar", "randevu.ics", crate::ical::export_ics(&[ev], None, None).into_bytes());
        }
        b.write_to_vec().unwrap_or_default()
    }

    pub fn kind(&self) -> &'static str {
        let c = self.message_class.to_uppercase();
        if c.starts_with("IPM.APPOINTMENT") || c.starts_with("IPM.SCHEDULE.MEETING") {
            "appointment"
        } else if c.starts_with("IPM.CONTACT") {
            "contact"
        } else if c.starts_with("IPM.TASK") {
            "task"
        } else if c.starts_with("IPM.STICKYNOTE") {
            "note"
        } else {
            "message"
        }
    }

    pub fn to_event(&self) -> Option<Event> {
        if self.kind() != "appointment" {
            return None;
        }
        let start = self.named_time(PSETID_APPOINTMENT, 0x820D).or_else(|| self.props.time(0x0060))?;
        let end = self.named_time(PSETID_APPOINTMENT, 0x820E).or_else(|| self.props.time(0x0061)).unwrap_or(start + 3600.0);
        let (text, _) = self.bodies();
        let reminder_set = self.named_int(PSETID_COMMON, 0x8503).unwrap_or(0) != 0;
        let reminder = self.named_int(PSETID_COMMON, 0x8501).unwrap_or(15);
        let busy = self.named_int(PSETID_APPOINTMENT, 0x8205).unwrap_or(2);
        Some(Event {
            title: self.subject(),
            location: self.named_str(PSETID_APPOINTMENT, 0x8208),
            description: text,
            start,
            end,
            all_day: self.named_int(PSETID_APPOINTMENT, 0x8215).unwrap_or(0) != 0,
            reminder_minutes: if reminder_set { reminder } else { -1 },
            attendees: self.recipients.iter().filter(|r| !r.email.is_empty()).map(|r| r.email.clone()).collect::<Vec<_>>().join(","),
            show_as: match busy {
                0 => "free",
                1 => "tentative",
                3 => "oof",
                _ => "busy",
            }
            .into(),
            ..Event::new_default()
        })
    }

    pub fn to_contact(&self) -> Option<Contact> {
        if self.kind() != "contact" {
            return None;
        }
        let p = &self.props;
        let address = [p.str(&[0x3A29]), p.str(&[0x3A2A]), p.str(&[0x3A27]), p.str(&[0x3A26])]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        let birthday = p
            .time(0x3A42)
            .and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let (text, _) = self.bodies();
        Some(Contact {
            first_name: p.str(&[0x3A06]),
            last_name: p.str(&[0x3A11]),
            display_name: {
                let d = p.str(&[0x3001]);
                if d.is_empty() { self.subject() } else { d }
            },
            email: self.named_str(PSETID_ADDRESS, 0x8083),
            email2: self.named_str(PSETID_ADDRESS, 0x8093),
            phone_mobile: p.str(&[0x3A1C]),
            phone_work: p.str(&[0x3A08]),
            phone_home: p.str(&[0x3A09]),
            company: p.str(&[0x3A16]),
            job_title: p.str(&[0x3A17]),
            department: p.str(&[0x3A18]),
            address,
            website: p.str(&[0x3A50]),
            birthday,
            notes: text,
            ..Default::default()
        })
    }

    pub fn to_task(&self) -> Option<Task> {
        if self.kind() != "task" {
            return None;
        }
        let (text, _) = self.bodies();
        let done = self.named_int(PSETID_TASK, 0x811C).unwrap_or(0) != 0;
        Some(Task {
            title: self.subject(),
            notes: text,
            due_date: self.named_time(PSETID_TASK, 0x8105),
            start_date: self.named_time(PSETID_TASK, 0x8104),
            status: if done { "completed".into() } else { "not_started".into() },
            percent: self.named_float(PSETID_TASK, 0x8102).map(|f| (f * 100.0) as i64).unwrap_or(0),
            priority: self.importance(),
            ..Task::new_default()
        })
    }
}

// ============================================================ yazma
fn w_stream(cfb: &mut Cfb, path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut s = cfb.create_stream(path)?;
    s.write_all(data)
}

fn utf16_bytes(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    v.extend_from_slice(&[0, 0]);
    v
}

struct FixedProps(Vec<u8>);

impl FixedProps {
    fn new(header: Vec<u8>) -> Self {
        FixedProps(header)
    }
    fn entry(&mut self, ptype: u16, pid: u16, value: [u8; 8]) {
        self.0.extend_from_slice(&ptype.to_le_bytes());
        self.0.extend_from_slice(&pid.to_le_bytes());
        self.0.extend_from_slice(&6u32.to_le_bytes()); // okunabilir | yazılabilir
        self.0.extend_from_slice(&value);
    }
    fn long(&mut self, pid: u16, v: i32) {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&v.to_le_bytes());
        self.entry(PT_LONG, pid, b);
    }
    fn boolean(&mut self, pid: u16, v: bool) {
        let mut b = [0u8; 8];
        b[0] = v as u8;
        self.entry(PT_BOOLEAN, pid, b);
    }
    fn time(&mut self, pid: u16, ts: f64) {
        self.entry(PT_SYSTIME, pid, ts_to_filetime(ts).to_le_bytes());
    }
    /// Değişken uzunluklu özellik: değer alanında boyut tutulur.
    fn var(&mut self, ptype: u16, pid: u16, len: usize) {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&(len as u32).to_le_bytes());
        self.entry(ptype, pid, b);
    }
}

fn put_str(cfb: &mut Cfb, dir: &Path, fp: &mut FixedProps, pid: u16, s: &str) -> std::io::Result<()> {
    let data = utf16_bytes(s);
    w_stream(cfb, &dir.join(format!("__substg1.0_{pid:04X}001F")), &data)?;
    fp.var(PT_UNICODE, pid, data.len());
    Ok(())
}

fn put_bin(cfb: &mut Cfb, dir: &Path, fp: &mut FixedProps, pid: u16, d: &[u8]) -> std::io::Result<()> {
    w_stream(cfb, &dir.join(format!("__substg1.0_{pid:04X}0102")), d)?;
    fp.var(PT_BINARY, pid, d.len());
    Ok(())
}

/// Ham RFC 5322 iletisini Outlook .msg biçimine çevirir.
pub fn eml_to_msg(raw: &[u8]) -> std::io::Result<Vec<u8>> {
    let p = crate::mail::parse::parse(raw);
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new()))?;
    let root = Path::new("/");
    // Adlandırılmış özellik eşlemesi (boş da olsa Outlook bu depolamayı bekler)
    let nid = root.join("__nameid_version1.0");
    cfb.create_storage(&nid)?;
    w_stream(&mut cfb, &nid.join("__substg1.0_00020102"), &[])?;
    w_stream(&mut cfb, &nid.join("__substg1.0_00030102"), &[])?;
    w_stream(&mut cfb, &nid.join("__substg1.0_00040102"), &[])?;

    let recips: Vec<(String, String, i32)> = [(&p.to, 1), (&p.cc, 2), (&p.bcc, 3)]
        .iter()
        .flat_map(|(list, kind)| {
            crate::mail::split_addresses(list).into_iter().map(move |a| {
                let (n, e) = crate::mail::split_name_addr(&a);
                (if n.is_empty() { e.clone() } else { n }, e, *kind)
            })
        })
        .collect();
    let atts: Vec<&crate::mail::parse::Attachment> = p.attachments.iter().collect();

    let mut header = vec![0u8; 8];
    header.extend_from_slice(&(recips.len() as u32).to_le_bytes()); // sonraki alıcı kimliği
    header.extend_from_slice(&(atts.len() as u32).to_le_bytes()); // sonraki ek kimliği
    header.extend_from_slice(&(recips.len() as u32).to_le_bytes());
    header.extend_from_slice(&(atts.len() as u32).to_le_bytes());
    header.extend_from_slice(&[0u8; 8]);
    let mut fp = FixedProps::new(header);

    put_str(&mut cfb, root, &mut fp, 0x001A, "IPM.Note")?;
    put_str(&mut cfb, root, &mut fp, 0x0037, &p.subject)?;
    put_str(&mut cfb, root, &mut fp, 0x0E1D, &crate::mail::normalize_subject(&p.subject))?;
    let (sname, semail) = crate::mail::split_name_addr(&p.sender);
    let sname = if sname.is_empty() { semail.clone() } else { sname };
    for pid in [0x0C1A, 0x0042] {
        put_str(&mut cfb, root, &mut fp, pid, &sname)?;
    }
    for pid in [0x0C1F, 0x0065, 0x5D01, 0x5D02] {
        put_str(&mut cfb, root, &mut fp, pid, &semail)?;
    }
    for pid in [0x0C1E, 0x0064] {
        put_str(&mut cfb, root, &mut fp, pid, "SMTP")?;
    }
    let disp = |k: i32| recips.iter().filter(|r| r.2 == k).map(|r| r.0.clone()).collect::<Vec<_>>().join("; ");
    put_str(&mut cfb, root, &mut fp, 0x0E04, &disp(1))?;
    put_str(&mut cfb, root, &mut fp, 0x0E03, &disp(2))?;
    put_str(&mut cfb, root, &mut fp, 0x0E02, &disp(3))?;
    put_str(&mut cfb, root, &mut fp, 0x1000, &p.text)?;
    if !p.html.is_empty() {
        // Gövde UTF-8 yazılır; içindeki eski <meta charset=iso-8859-9> vb. de buna uydurulur
        // (yoksa dosya başka uygulamada/yeniden açılınca Türkçe karakterler bozulur)
        let cs = regex::Regex::new(r#"(?i)(<meta[^>]*charset\s*=\s*["']?)[a-z0-9_\-]+"#).unwrap();
        let html = cs.replace_all(&p.html, "${1}utf-8");
        put_bin(&mut cfb, root, &mut fp, 0x1013, html.as_bytes())?;
    }
    if !p.message_id.is_empty() {
        put_str(&mut cfb, root, &mut fp, 0x1035, &p.message_id)?;
    }
    if !p.in_reply_to.is_empty() {
        put_str(&mut cfb, root, &mut fp, 0x1042, &p.in_reply_to)?;
    }
    // İnternet başlıkları
    let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").or_else(|| raw.windows(2).position(|w| w == b"\n\n"));
    if let Some(e) = head_end {
        put_str(&mut cfb, root, &mut fp, 0x007D, &String::from_utf8_lossy(&raw[..e]))?;
    }
    fp.long(0x0017, p.importance as i32);
    fp.long(0x0E07, 1); // MSGFLAG_READ
    fp.long(0x3FDE, 65001);
    fp.long(0x3FFD, 65001);
    fp.time(0x0E06, p.date);
    fp.time(0x0039, p.date);
    fp.time(0x3007, p.date);
    fp.time(0x3008, p.date);
    fp.boolean(0x0E1B, !atts.is_empty());
    w_stream(&mut cfb, &root.join("__properties_version1.0"), &fp.0)?;

    for (i, (name, email, kind)) in recips.iter().enumerate() {
        let dir = root.join(format!("__recip_version1.0_#{i:08X}"));
        cfb.create_storage(&dir)?;
        let mut rp = FixedProps::new(vec![0u8; 8]);
        put_str(&mut cfb, &dir, &mut rp, 0x3001, name)?;
        put_str(&mut cfb, &dir, &mut rp, 0x3003, email)?;
        put_str(&mut cfb, &dir, &mut rp, 0x39FE, email)?;
        put_str(&mut cfb, &dir, &mut rp, 0x3002, "SMTP")?;
        rp.long(0x0C15, *kind);
        rp.long(0x3000, i as i32);
        rp.long(0x0FFE, 6); // MAPI_MAILUSER
        w_stream(&mut cfb, &dir.join("__properties_version1.0"), &rp.0)?;
    }
    for (i, a) in atts.iter().enumerate() {
        let dir = root.join(format!("__attach_version1.0_#{i:08X}"));
        cfb.create_storage(&dir)?;
        let mut ap = FixedProps::new(vec![0u8; 8]);
        put_str(&mut cfb, &dir, &mut ap, 0x3707, &a.name)?;
        put_str(&mut cfb, &dir, &mut ap, 0x3704, &a.name)?;
        put_str(&mut cfb, &dir, &mut ap, 0x3001, &a.name)?;
        let ext = a.name.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
        put_str(&mut cfb, &dir, &mut ap, 0x3703, &ext)?;
        put_str(&mut cfb, &dir, &mut ap, 0x370E, &a.mime)?;
        if !a.cid.is_empty() {
            put_str(&mut cfb, &dir, &mut ap, 0x3712, &a.cid)?;
        }
        put_bin(&mut cfb, &dir, &mut ap, 0x3701, &a.data)?;
        ap.long(0x3705, 1); // ATTACH_BY_VALUE
        ap.long(0x0E21, i as i32);
        ap.long(0x0FFE, 7); // MAPI_ATTACH
        ap.long(0x0E20, a.data.len() as i32);
        w_stream(&mut cfb, &dir.join("__properties_version1.0"), &ap.0)?;
    }
    cfb.flush()?;
    Ok(cfb.into_inner().into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prebuf_length() {
        assert_eq!(RTF_PREBUF.len(), 207);
    }

    #[test]
    fn rtf_html_extraction() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1254\\fromhtml1 {\\fonttbl{\\f0 Arial;}}{\\*\\htmltag64 <html>}{\\*\\htmltag <body>}\\htmlrtf {\\htmlrtf0 Merhaba \\'fe\\'fd\\htmlrtf }\\htmlrtf0 {\\*\\htmltag </body>}{\\*\\htmltag </html>}}";
        let (is_html, s) = rtf_extract(rtf);
        assert!(is_html);
        assert_eq!(s, "<html><body>Merhaba şı</body></html>");
    }

    #[test]
    fn rtf_font_charset_turkish() {
        // Belge 1252 ama yazı tipi Türkçe (\fcharset162): Outlook'ta sık görülür
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252\\fromhtml1 \\deff0{\\fonttbl{\\f0\\fswiss\\fcharset162 Arial;}{\\f1\\fswiss Arial;}}{\\*\\htmltag64 <html>}\\htmlrtf {\\htmlrtf0 \\f0 \\'de\\'fe\\'fd\\'f0 \\f1 \\'fe\\htmlrtf }\\htmlrtf0 {\\*\\htmltag </html>}}";
        let (_, s) = rtf_extract(rtf);
        assert_eq!(s, "<html>Şşığ þ</html>");
    }

    #[test]
    fn html_body_charset() {
        // MailNix'in yazdığı UTF-8 gövde, içinde eski iso-8859-9 etiketi olsa da bozulmaz
        let utf = "<meta charset=iso-8859-9><p>Şığ</p>".as_bytes();
        assert_eq!(decode_html_bytes(utf, Some(65001)), "<meta charset=iso-8859-9><p>Şığ</p>");
        // Gerçek 8 bit gövde
        assert_eq!(decode_html_bytes(b"<meta charset=windows-1254><p>\xDE\xFD</p>", None), "<meta charset=windows-1254><p>Şı</p>");
        assert_eq!(decode_html_bytes(b"<p>\xDE\xFD</p>", Some(1254)), "<p>Şı</p>");
        assert_eq!(decode_html_bytes(b"<p>\xDE\xFD</p>", None), "<p>Şı</p>");
    }

    #[test]
    fn msg_roundtrip() {
        let raw = crate::mail::build::build_raw(
            Some(&crate::db::Account { email: "ben@ornek.com".into(), display_name: "Ben".into(), ..Default::default() }),
            &crate::mail::build::ComposedMail {
                to: "Ali Veli <ali@x.com>".into(),
                cc: "c@y.com".into(),
                subject: "Çeyrek raporu".into(),
                text: "Gövde metni ğüşiöç".into(),
                html: "<p>Gövde <b>metni</b></p>".into(),
                importance: 2,
                ..Default::default()
            },
            &[("tablo.csv".into(), "text/csv".into(), b"a,b\n1,2".to_vec())],
            false,
        );
        let msg = eml_to_msg(&raw).unwrap();
        assert!(is_msg(&msg));
        let m = MsgFile::parse(&msg).unwrap();
        assert_eq!(m.subject(), "Çeyrek raporu");
        assert_eq!(m.sender(), "Ben <ben@ornek.com>");
        assert_eq!(m.to(), "Ali Veli <ali@x.com>");
        assert_eq!(m.cc(), "c@y.com");
        assert_eq!(m.importance(), 2);
        let (t, h) = m.bodies();
        assert!(t.contains("ğüşiöç"));
        assert!(h.contains("<b>metni</b>"));
        assert_eq!(m.attachments.len(), 1);
        assert_eq!(m.attachments[0].name, "tablo.csv");
        assert_eq!(m.attachments[0].data, b"a,b\n1,2");
        // .msg → .eml dönüşümü
        let p = crate::mail::parse::parse(&m.to_eml());
        assert_eq!(p.subject, "Çeyrek raporu");
        assert_eq!(p.attachments.len(), 1);
    }

    /// Gerçek Outlook dosyalarıyla sınama:
    /// `MAILNIX_MSG_SAMPLES=/klasör cargo test real_samples -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_samples() {
        let Ok(dir) = std::env::var("MAILNIX_MSG_SAMPLES") else { return };
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let data = std::fs::read(e.path()).unwrap();
            let m = MsgFile::parse(&data).unwrap();
            let (t, h) = m.bodies();
            println!("{:?}: {} | {} | {} | text={} html={} ek={}", e.file_name(), m.subject(), m.sender(), m.to(), t.len(), h.len(), m.attachments.len());
            assert!(!m.subject().is_empty());
            let p = crate::mail::parse::parse(&m.to_eml());
            assert_eq!(p.subject, m.subject());
            assert_eq!(p.real_attachments().len(), m.attachments.len());
        }
    }
}
