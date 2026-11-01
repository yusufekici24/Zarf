//! iCalendar (.ics), vCard (.vcf) ve CSV kişi içe/dışa aktarma.

use crate::db::{Contact, Event, Task};
use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\r', "").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Ters eğik çizgiyle kaçışlanmış ayraçları dikkate alarak böler.
fn split_escaped(s: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push('\\');
            cur.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == delim {
            out.push(unescape(&std::mem::take(&mut cur)));
        } else {
            cur.push(c);
        }
    }
    out.push(unescape(&cur));
    out
}

fn fold(line: &str) -> String {
    if line.len() <= 75 {
        return line.to_string();
    }
    let mut out = String::new();
    let mut cur = 0;
    for ch in line.chars() {
        let l = ch.len_utf8();
        if cur + l > 74 {
            out.push_str("\r\n ");
            cur = 1;
        }
        out.push(ch);
        cur += l;
    }
    out
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        if (line.starts_with(' ') || line.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    lines
}

/// "NAME;PARAM=x:value" → (NAME, [(PARAM, x)], value)
fn parse_line(line: &str) -> (String, Vec<(String, String)>, String) {
    let mut in_q = false;
    let mut split_at = None;
    for (i, c) in line.char_indices() {
        if c == '"' {
            in_q = !in_q;
        } else if c == ':' && !in_q {
            split_at = Some(i);
            break;
        }
    }
    let Some(i) = split_at else { return (line.to_uppercase(), vec![], String::new()) };
    let (head, value) = (&line[..i], &line[i + 1..]);
    let mut parts = head.split(';');
    let name = parts.next().unwrap_or("").to_uppercase();
    let params = parts
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_uppercase(), v.trim_matches('"').to_string()),
            None => (p.to_uppercase(), String::new()),
        })
        .collect();
    (name, params, value.to_string())
}

fn param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

// ------------------------------------------------------------------ iCalendar
fn fmt_dt(ts: f64, all_day: bool) -> String {
    if all_day {
        chrono::Local
            .timestamp_opt(ts as i64, 0)
            .single()
            .map(|d| d.format("%Y%m%d").to_string())
            .unwrap_or_default()
    } else {
        Utc.timestamp_opt(ts as i64, 0).single().map(|d| d.format("%Y%m%dT%H%M%SZ").to_string()).unwrap_or_default()
    }
}

fn parse_dt(value: &str, params: &[(String, String)]) -> Option<(f64, bool)> {
    let v = value.trim();
    if param(params, "VALUE") == Some("DATE") || (v.len() == 8 && v.chars().all(|c| c.is_ascii_digit())) {
        let d = NaiveDate::parse_from_str(&v[..8], "%Y%m%d").ok()?;
        let dt = chrono::Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()?;
        return Some((dt.timestamp() as f64, true));
    }
    let base = v.get(..15)?;
    let nd = NaiveDateTime::parse_from_str(base, "%Y%m%dT%H%M%S").ok()?;
    if v.ends_with('Z') {
        return Some((Utc.from_utc_datetime(&nd).timestamp() as f64, false));
    }
    if let Some(tz) = param(params, "TZID") {
        if let Some(off) = tz_offset_hours(tz) {
            let secs = Utc.from_utc_datetime(&nd).timestamp() - (off * 3600.0) as i64;
            return Some((secs as f64, false));
        }
    }
    let dt = chrono::Local.from_local_datetime(&nd).earliest()?;
    Some((dt.timestamp() as f64, false))
}

/// Yaygın saat dilimleri için sabit sapma (yaz saati uygulamasız bölgeler öncelikli).
fn tz_offset_hours(tz: &str) -> Option<f64> {
    let t = tz.to_lowercase();
    let local_names = ["europe/istanbul", "turkey", "turkey standard time", "asia/istanbul"];
    if local_names.iter().any(|n| t.contains(n)) {
        return Some(3.0);
    }
    if t == "utc" || t == "gmt" || t.contains("coordinated universal") {
        return Some(0.0);
    }
    None
}

fn parse_duration_minutes(v: &str) -> i64 {
    let v = v.trim();
    let neg = v.starts_with('-');
    let v = v.trim_start_matches(['+', '-']);
    if !v.starts_with('P') {
        return 15;
    }
    let mut minutes = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in v[1..].chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().unwrap_or(0);
                num.clear();
                match c {
                    'W' => minutes += n * 10080,
                    'D' => minutes += n * 1440,
                    'H' => minutes += n * 60,
                    'M' if in_time => minutes += n,
                    _ => {}
                }
            }
        }
    }
    if neg || minutes == 0 { minutes } else { 0 }
}

pub fn event_to_vevent(e: &Event, organizer: Option<&str>) -> Vec<String> {
    let mut l = vec![
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", if e.uid.is_empty() { format!("{}@lynora", uuid::Uuid::new_v4()) } else { e.uid.clone() }),
        format!("DTSTAMP:{}", fmt_dt(crate::db::now(), false)),
    ];
    if e.all_day {
        l.push(format!("DTSTART;VALUE=DATE:{}", fmt_dt(e.start, true)));
        l.push(format!("DTEND;VALUE=DATE:{}", fmt_dt(e.end, true)));
    } else {
        l.push(format!("DTSTART:{}", fmt_dt(e.start, false)));
        l.push(format!("DTEND:{}", fmt_dt(e.end, false)));
    }
    l.push(format!("SUMMARY:{}", escape(&e.title)));
    if !e.location.is_empty() {
        l.push(format!("LOCATION:{}", escape(&e.location)));
    }
    if !e.description.is_empty() {
        l.push(format!("DESCRIPTION:{}", escape(&e.description)));
    }
    if !e.category.is_empty() {
        l.push(format!("CATEGORIES:{}", escape(&e.category)));
    }
    if e.recurrence != "none" && !e.recurrence.is_empty() {
        let mut rr = match e.recurrence.as_str() {
            "weekdays" => "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".to_string(),
            "daily" => "FREQ=DAILY".into(),
            "weekly" => "FREQ=WEEKLY".into(),
            "monthly" => "FREQ=MONTHLY".into(),
            "yearly" => "FREQ=YEARLY".into(),
            _ => String::new(),
        };
        if !rr.is_empty() {
            if e.recur_interval > 1 && e.recurrence != "weekdays" {
                rr.push_str(&format!(";INTERVAL={}", e.recur_interval));
            }
            if let Some(u) = e.recur_until {
                rr.push_str(&format!(";UNTIL={}", fmt_dt(u, false)));
            }
            l.push(format!("RRULE:{rr}"));
        }
        for x in crate::db::split_list(&e.exdates) {
            l.push(format!("EXDATE;VALUE=DATE:{}", x.replace('-', "")));
        }
    }
    match e.show_as.as_str() {
        "free" => l.push("TRANSP:TRANSPARENT".into()),
        "tentative" => l.push("STATUS:TENTATIVE".into()),
        _ => {}
    }
    if e.is_private {
        l.push("CLASS:PRIVATE".into());
    }
    if let Some(o) = organizer {
        l.push(format!("ORGANIZER:mailto:{o}"));
    }
    for a in crate::db::split_list(&e.attendees) {
        l.push(format!("ATTENDEE;RSVP=TRUE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION:mailto:{a}"));
    }
    for a in crate::db::split_list(&e.optional_attendees) {
        l.push(format!("ATTENDEE;RSVP=TRUE;ROLE=OPT-PARTICIPANT;PARTSTAT=NEEDS-ACTION:mailto:{a}"));
    }
    for a in crate::db::split_list(&e.resources) {
        l.push(format!("ATTENDEE;CUTYPE=ROOM;RSVP=TRUE;ROLE=NON-PARTICIPANT;PARTSTAT=NEEDS-ACTION:mailto:{a}"));
    }
    if e.reminder_minutes >= 0 {
        l.extend([
            "BEGIN:VALARM".into(),
            "ACTION:DISPLAY".into(),
            "DESCRIPTION:Reminder".into(),
            format!("TRIGGER:-PT{}M", e.reminder_minutes),
            "END:VALARM".into(),
        ]);
    }
    l.push("END:VEVENT".into());
    l
}

pub fn task_to_vtodo(t: &Task) -> Vec<String> {
    let mut l = vec!["BEGIN:VTODO".to_string(), format!("UID:task-{}@lynora", t.id), format!("SUMMARY:{}", escape(&t.title))];
    if !t.notes.is_empty() {
        l.push(format!("DESCRIPTION:{}", escape(&t.notes)));
    }
    if let Some(d) = t.due_date {
        l.push(format!("DUE:{}", fmt_dt(d, false)));
    }
    if let Some(d) = t.start_date {
        l.push(format!("DTSTART:{}", fmt_dt(d, false)));
    }
    l.push(format!("PRIORITY:{}", match t.priority { 2 => 1, 0 => 9, _ => 5 }));
    l.push(format!("PERCENT-COMPLETE:{}", t.percent));
    l.push(format!("STATUS:{}", match t.status.as_str() {
        "completed" => "COMPLETED",
        "in_progress" => "IN-PROCESS",
        _ => "NEEDS-ACTION",
    }));
    l.push("END:VTODO".into());
    l
}

pub fn export_ics(events: &[Event], method: Option<&str>, organizer: Option<&str>) -> String {
    let mut l = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".into(), "PRODID:-//Lynora//Lynora//TR".into(), "CALSCALE:GREGORIAN".into()];
    if let Some(m) = method {
        l.push(format!("METHOD:{m}"));
    }
    for e in events {
        l.extend(event_to_vevent(e, organizer));
    }
    l.push("END:VCALENDAR".into());
    l.iter().map(|x| fold(x)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

pub fn export_tasks_ics(tasks: &[Task]) -> String {
    let mut l = vec!["BEGIN:VCALENDAR".to_string(), "VERSION:2.0".into(), "PRODID:-//Lynora//Lynora//TR".into()];
    for t in tasks {
        l.extend(task_to_vtodo(t));
    }
    l.push("END:VCALENDAR".into());
    l.iter().map(|x| fold(x)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

#[derive(Default, Debug)]
pub struct IcsImport {
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
    pub method: String,
}

pub fn parse_ics(text: &str) -> IcsImport {
    let mut out = IcsImport::default();
    let mut cur: Option<Event> = None;
    let mut todo: Option<Task> = None;
    let mut in_alarm = false;
    let mut has_end = false;
    for line in unfold(text) {
        let (name, params, value) = parse_line(&line);
        let upper = value.to_uppercase();
        match (name.as_str(), upper.as_str()) {
            ("METHOD", _) if cur.is_none() => out.method = upper.clone(),
            ("BEGIN", "VEVENT") => {
                cur = Some(Event { reminder_minutes: -1, ..Event::new_default() });
                has_end = false;
            }
            ("END", "VEVENT") => {
                if let Some(mut e) = cur.take() {
                    if e.start > 0.0 {
                        if !has_end || e.end <= e.start {
                            e.end = e.start + if e.all_day { 86400.0 } else { 3600.0 };
                        }
                        out.events.push(e);
                    }
                }
            }
            ("BEGIN", "VTODO") => todo = Some(Task::new_default()),
            ("END", "VTODO") => {
                if let Some(t) = todo.take() {
                    out.tasks.push(t);
                }
            }
            ("BEGIN", "VALARM") => in_alarm = true,
            ("END", "VALARM") => in_alarm = false,
            _ => {
                if let Some(t) = todo.as_mut() {
                    match name.as_str() {
                        "SUMMARY" => t.title = unescape(&value),
                        "DESCRIPTION" => t.notes = unescape(&value),
                        "DUE" => t.due_date = parse_dt(&value, &params).map(|x| x.0),
                        "DTSTART" => t.start_date = parse_dt(&value, &params).map(|x| x.0),
                        "PERCENT-COMPLETE" => t.percent = value.trim().parse().unwrap_or(0),
                        "PRIORITY" => {
                            let p: i64 = value.trim().parse().unwrap_or(5);
                            t.priority = if p == 0 { 1 } else if p <= 4 { 2 } else if p >= 6 { 0 } else { 1 };
                        }
                        "STATUS" => {
                            t.status = match upper.as_str() {
                                "COMPLETED" => "completed",
                                "IN-PROCESS" => "in_progress",
                                _ => "not_started",
                            }
                            .into()
                        }
                        _ => {}
                    }
                    continue;
                }
                let Some(e) = cur.as_mut() else { continue };
                if in_alarm {
                    if name == "TRIGGER" {
                        e.reminder_minutes = parse_duration_minutes(&value);
                    }
                    continue;
                }
                match name.as_str() {
                    "UID" => e.uid = value.clone(),
                    "SUMMARY" => e.title = unescape(&value),
                    "LOCATION" => e.location = unescape(&value),
                    "DESCRIPTION" => e.description = unescape(&value),
                    "CATEGORIES" => e.category = unescape(&value).split(',').next().unwrap_or("").to_string(),
                    "DTSTART" => {
                        if let Some((t, ad)) = parse_dt(&value, &params) {
                            e.start = t;
                            e.all_day = ad;
                        }
                    }
                    "DTEND" => {
                        if let Some((t, _)) = parse_dt(&value, &params) {
                            e.end = t;
                            has_end = true;
                        }
                    }
                    "DURATION" => {
                        e.end = e.start + (parse_duration_minutes(&format!("-{value}")) * 60) as f64;
                        has_end = true;
                    }
                    "TRANSP" => e.show_as = if upper == "TRANSPARENT" { "free".into() } else { "busy".into() },
                    "STATUS" if upper == "TENTATIVE" => e.show_as = "tentative".into(),
                    "CLASS" => e.is_private = upper == "PRIVATE" || upper == "CONFIDENTIAL",
                    "ATTENDEE" => {
                        let addr = value.strip_prefix("mailto:").or_else(|| value.strip_prefix("MAILTO:")).unwrap_or(&value);
                        let p = |k: &str| params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.to_uppercase()).unwrap_or_default();
                        let target = if p("CUTYPE") == "ROOM" || p("CUTYPE") == "RESOURCE" {
                            &mut e.resources
                        } else if p("ROLE") == "OPT-PARTICIPANT" {
                            &mut e.optional_attendees
                        } else {
                            &mut e.attendees
                        };
                        let mut list = crate::db::split_list(target);
                        list.push(addr.to_string());
                        *target = list.join(",");
                    }
                    "EXDATE" => {
                        for x in value.split(',') {
                            if let Some((t, _)) = parse_dt(x, &params) {
                                let d = chrono::Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
                                let mut list = crate::db::split_list(&e.exdates);
                                list.push(d);
                                e.exdates = list.join(",");
                            }
                        }
                    }
                    "RRULE" => {
                        let rr: Vec<(String, String)> = value
                            .split(';')
                            .filter_map(|p| p.split_once('=').map(|(a, b)| (a.to_uppercase(), b.to_string())))
                            .collect();
                        let get = |k: &str| rr.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
                        let freq = get("FREQ").unwrap_or_default().to_uppercase();
                        e.recurrence = if freq == "WEEKLY" && get("BYDAY").map(|b| b.to_uppercase()) == Some("MO,TU,WE,TH,FR".into()) {
                            "weekdays".into()
                        } else {
                            match freq.as_str() {
                                "DAILY" => "daily",
                                "WEEKLY" => "weekly",
                                "MONTHLY" => "monthly",
                                "YEARLY" => "yearly",
                                _ => "none",
                            }
                            .into()
                        };
                        e.recur_interval = get("INTERVAL").and_then(|v| v.parse().ok()).unwrap_or(1);
                        e.recur_until = get("UNTIL").and_then(|u| parse_dt(&u, &[]).map(|x| x.0));
                        if let Some(c) = get("COUNT").and_then(|c| c.parse::<i64>().ok()) {
                            let step = match e.recurrence.as_str() {
                                "daily" => 86400.0,
                                "weekdays" => 86400.0 * 7.0 / 5.0,
                                "weekly" => 7.0 * 86400.0,
                                "monthly" => 31.0 * 86400.0,
                                "yearly" => 366.0 * 86400.0,
                                _ => 0.0,
                            };
                            e.recur_until = Some(e.start + step * e.recur_interval as f64 * (c - 1).max(0) as f64);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Toplantı davetine iMIP yanıtı (METHOD:REPLY) üretir.
/// Dönüş: (düzenleyici adresi, yanıt iCalendar metni, konu)
pub fn make_reply(ics: &str, my_email: &str, partstat: &str) -> Option<(String, String, String)> {
    let lines = unfold(ics);
    let mut in_ev = false;
    let mut keep: Vec<String> = Vec::new();
    let mut organizer = String::new();
    let mut summary = String::new();
    for l in &lines {
        let (name, _params, value) = parse_line(l);
        if name == "BEGIN" && value.eq_ignore_ascii_case("VEVENT") {
            if in_ev || !keep.is_empty() {
                break;
            }
            in_ev = true;
            continue;
        }
        if name == "END" && value.eq_ignore_ascii_case("VEVENT") {
            break;
        }
        if !in_ev {
            continue;
        }
        match name.as_str() {
            "UID" | "DTSTART" | "DTEND" | "DURATION" | "SEQUENCE" | "RECURRENCE-ID" => keep.push(l.clone()),
            "SUMMARY" => {
                summary = unescape(&value);
                keep.push(l.clone());
            }
            "ORGANIZER" => {
                organizer = value.trim_start_matches("mailto:").trim_start_matches("MAILTO:").to_string();
                keep.push(l.clone());
            }
            _ => {}
        }
    }
    if organizer.is_empty() || !keep.iter().any(|l| l.starts_with("UID")) {
        return None;
    }
    let mut out = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//Lynora//Lynora//TR".into(),
        "METHOD:REPLY".into(),
        "BEGIN:VEVENT".into(),
        format!("DTSTAMP:{}", fmt_dt(crate::db::now(), false)),
    ];
    out.extend(keep);
    out.push(format!("ATTENDEE;PARTSTAT={partstat}:mailto:{my_email}"));
    out.push("END:VEVENT".into());
    out.push("END:VCALENDAR".into());
    Some((organizer, out.iter().map(|x| fold(x)).collect::<Vec<_>>().join("\r\n") + "\r\n", summary))
}

// ------------------------------------------------------------------ vCard
pub fn contact_to_vcard(c: &Contact) -> Vec<String> {
    let mut l = vec![
        "BEGIN:VCARD".to_string(),
        "VERSION:3.0".into(),
        format!("N:{};{};;;", escape(&c.last_name), escape(&c.first_name)),
        format!("FN:{}", escape(&c.computed_display())),
    ];
    if !c.email.is_empty() {
        l.push(format!("EMAIL;TYPE=INTERNET,PREF:{}", c.email));
    }
    if !c.email2.is_empty() {
        l.push(format!("EMAIL;TYPE=INTERNET:{}", c.email2));
    }
    for (v, t) in [(&c.phone_mobile, "CELL"), (&c.phone_work, "WORK"), (&c.phone_home, "HOME")] {
        if !v.is_empty() {
            l.push(format!("TEL;TYPE={t}:{v}"));
        }
    }
    if !c.company.is_empty() || !c.department.is_empty() {
        l.push(format!("ORG:{};{}", escape(&c.company), escape(&c.department)));
    }
    if !c.job_title.is_empty() {
        l.push(format!("TITLE:{}", escape(&c.job_title)));
    }
    if !c.address.is_empty() {
        l.push(format!("ADR;TYPE=HOME:;;{};;;;", escape(&c.address)));
    }
    if !c.website.is_empty() {
        l.push(format!("URL:{}", c.website));
    }
    if !c.birthday.is_empty() {
        l.push(format!("BDAY:{}", c.birthday));
    }
    if !c.notes.is_empty() {
        l.push(format!("NOTE:{}", escape(&c.notes)));
    }
    if !c.categories.is_empty() {
        l.push(format!("CATEGORIES:{}", c.categories));
    }
    l.push("END:VCARD".into());
    l
}

pub fn export_vcf(contacts: &[Contact]) -> String {
    contacts.iter().flat_map(contact_to_vcard).map(|x| fold(&x)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

pub fn parse_vcf(text: &str) -> Vec<Contact> {
    let mut out = Vec::new();
    let mut cur: Option<Contact> = None;
    for line in unfold(text) {
        let (mut name, params, value) = parse_line(&line);
        if let Some((_, n)) = name.split_once('.') {
            name = n.to_string();
        }
        let types: String = params
            .iter()
            .flat_map(|(k, v)| [k.clone(), v.to_uppercase()])
            .collect::<Vec<_>>()
            .join(",");
        match name.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VCARD") => cur = Some(Contact::default()),
            "END" if value.eq_ignore_ascii_case("VCARD") => {
                if let Some(c) = cur.take() {
                    out.push(c);
                }
            }
            _ => {
                let Some(c) = cur.as_mut() else { continue };
                let v = unescape(&value);
                match name.as_str() {
                    "N" => {
                        let p = split_escaped(&value, ';');
                        c.last_name = p.first().cloned().unwrap_or_default();
                        c.first_name = p.get(1).cloned().unwrap_or_default();
                    }
                    "FN" => c.display_name = v,
                    "EMAIL" => {
                        if c.email.is_empty() {
                            c.email = v.trim().into();
                        } else if c.email2.is_empty() {
                            c.email2 = v.trim().into();
                        }
                    }
                    "TEL" => {
                        if types.contains("CELL") && c.phone_mobile.is_empty() {
                            c.phone_mobile = v;
                        } else if types.contains("WORK") && c.phone_work.is_empty() {
                            c.phone_work = v;
                        } else if types.contains("HOME") && c.phone_home.is_empty() {
                            c.phone_home = v;
                        } else if c.phone_mobile.is_empty() {
                            c.phone_mobile = v;
                        }
                    }
                    "ORG" => {
                        let p = split_escaped(&value, ';');
                        c.company = p.first().cloned().unwrap_or_default();
                        c.department = p.get(1).cloned().unwrap_or_default();
                    }
                    "TITLE" => c.job_title = v,
                    "ADR" => {
                        c.address = split_escaped(&value, ';').into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join(", ")
                    }
                    "URL" => c.website = v,
                    "BDAY" => c.birthday = v,
                    "NOTE" => c.notes = v,
                    "CATEGORIES" => c.categories = v,
                    _ => {}
                }
            }
        }
    }
    out
}

/// Outlook / Google / Thunderbird CSV dışa aktarımlarından kişi okur.
pub fn parse_csv_contacts(text: &str) -> Vec<Contact> {
    fn split_csv_line(line: &str, delim: char) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_q = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                if in_q && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_q = !in_q;
                }
            } else if c == delim && !in_q {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
        }
        out.push(cur);
        out
    }
    // Tırnak içindeki satır sonlarını koruyarak kayıtları ayır
    let mut records = Vec::new();
    let mut cur = String::new();
    let mut quotes = 0usize;
    for line in text.trim_start_matches('\u{feff}').lines() {
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
        quotes += line.matches('"').count();
        if quotes % 2 == 0 {
            records.push(std::mem::take(&mut cur));
            quotes = 0;
        }
    }
    if records.is_empty() {
        return vec![];
    }
    let delim = if records[0].matches(';').count() > records[0].matches(',').count() { ';' } else { ',' };
    let header: Vec<String> = split_csv_line(&records[0], delim).into_iter().map(|h| h.trim().to_string()).collect();
    let find = |names: &[&str]| -> Option<usize> {
        names.iter().find_map(|n| header.iter().position(|h| h.eq_ignore_ascii_case(n)))
    };
    let map = [
        ("first_name", find(&["First Name", "Given Name", "Ad", "Adı"])),
        ("last_name", find(&["Last Name", "Family Name", "Soyad", "Soyadı"])),
        ("display_name", find(&["Name", "Display Name", "Görünen Ad", "Ad Soyad"])),
        ("email", find(&["E-mail Address", "E-mail 1 - Value", "Email", "Primary Email", "E-posta Adresi", "E-posta"])),
        ("email2", find(&["E-mail 2 Address", "E-mail 2 - Value", "Secondary Email"])),
        ("phone_mobile", find(&["Mobile Phone", "Phone 1 - Value", "Mobile Number", "Cep Telefonu"])),
        ("phone_work", find(&["Business Phone", "Work Phone", "İş Telefonu"])),
        ("phone_home", find(&["Home Phone", "Ev Telefonu"])),
        ("company", find(&["Company", "Organization 1 - Name", "Organization", "Şirket"])),
        ("job_title", find(&["Job Title", "Organization 1 - Title", "İş Unvanı", "Unvan"])),
        ("notes", find(&["Notes", "Notlar"])),
        ("birthday", find(&["Birthday", "Doğum Günü"])),
        ("website", find(&["Web Page", "Website 1 - Value", "Web Sayfası"])),
    ];
    let mut out = Vec::new();
    for rec in &records[1..] {
        let cols = split_csv_line(rec, delim);
        let get = |i: Option<usize>| i.and_then(|i| cols.get(i)).map(|s| s.trim().to_string()).unwrap_or_default();
        let mut c = Contact::default();
        for (field, idx) in &map {
            let v = get(*idx);
            match *field {
                "first_name" => c.first_name = v,
                "last_name" => c.last_name = v,
                "display_name" => c.display_name = v,
                "email" => c.email = v,
                "email2" => c.email2 = v,
                "phone_mobile" => c.phone_mobile = v,
                "phone_work" => c.phone_work = v,
                "phone_home" => c.phone_home = v,
                "company" => c.company = v,
                "job_title" => c.job_title = v,
                "notes" => c.notes = v,
                "birthday" => c.birthday = v,
                "website" => c.website = v,
                _ => {}
            }
        }
        if !(c.first_name.is_empty() && c.last_name.is_empty() && c.display_name.is_empty() && c.email.is_empty()) {
            out.push(c);
        }
    }
    out
}

pub fn export_csv_contacts(contacts: &[Contact]) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out = String::from("First Name,Last Name,Display Name,E-mail Address,E-mail 2 Address,Mobile Phone,Business Phone,Home Phone,Company,Job Title,Department,Home Address,Web Page,Birthday,Notes,Categories\r\n");
    for c in contacts {
        let row = [
            &c.first_name, &c.last_name, &c.display_name, &c.email, &c.email2, &c.phone_mobile, &c.phone_work,
            &c.phone_home, &c.company, &c.job_title, &c.department, &c.address, &c.website, &c.birthday, &c.notes,
            &c.categories,
        ];
        out.push_str(&row.iter().map(|s| q(s)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ics_roundtrip() {
        let e = Event {
            title: "Haftalık, toplantı".into(),
            location: "Oda 3".into(),
            description: "Satır1\nSatır2".into(),
            start: 1_727_769_600.0,
            end: 1_727_773_200.0,
            recurrence: "weekly".into(),
            recur_interval: 2,
            reminder_minutes: 30,
            attendees: "a@x.com,b@y.com".into(),
            ..Event::new_default()
        };
        let text = export_ics(&[e.clone()], Some("REQUEST"), Some("ben@x.com"));
        let back = parse_ics(&text);
        assert_eq!(back.method, "REQUEST");
        let b = &back.events[0];
        assert_eq!(b.title, e.title);
        assert_eq!(b.description, e.description);
        assert_eq!(b.start, e.start);
        assert_eq!(b.end, e.end);
        assert_eq!(b.recurrence, "weekly");
        assert_eq!(b.recur_interval, 2);
        assert_eq!(b.reminder_minutes, 30);
        assert_eq!(b.attendees, "a@x.com,b@y.com");
    }

    #[test]
    fn meeting_reply() {
        let inv = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:abc-1\r\nDTSTART:20261020T070000Z\r\nDTEND:20261020T080000Z\r\nSUMMARY:Bütçe\r\nORGANIZER;CN=Ayşe:mailto:ayse@x.com\r\nATTENDEE:mailto:ben@x.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (org, reply, sum) = make_reply(inv, "ben@x.com", "ACCEPTED").unwrap();
        assert_eq!(org, "ayse@x.com");
        assert_eq!(sum, "Bütçe");
        assert!(reply.contains("METHOD:REPLY") && reply.contains("UID:abc-1") && reply.contains("PARTSTAT=ACCEPTED:mailto:ben@x.com"));
    }

    #[test]
    fn vcf_and_csv() {
        let c = Contact {
            first_name: "Ayşe".into(),
            last_name: "Yılmaz".into(),
            email: "ayse@x.com".into(),
            phone_mobile: "+90 555".into(),
            company: "Pardus; A.Ş.".into(),
            ..Default::default()
        };
        let back = parse_vcf(&export_vcf(&[c.clone()]));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].first_name, "Ayşe");
        assert_eq!(back[0].company, "Pardus; A.Ş.");
        assert_eq!(back[0].phone_mobile, "+90 555");
        let csv = parse_csv_contacts(&export_csv_contacts(&[c]));
        assert_eq!(csv[0].email, "ayse@x.com");
        assert_eq!(csv[0].company, "Pardus; A.Ş.");
    }
}
