//! Yinelenen etkinliklerin belirli bir aralığa açılması.

use crate::db::Event;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct Occurrence {
    pub event: Event,
    pub start: f64,
    pub end: f64,
}

fn to_local(ts: f64) -> NaiveDateTime {
    Local.timestamp_opt(ts as i64, 0).single().map(|d| d.naive_local()).unwrap_or_default()
}

fn to_ts(d: NaiveDateTime) -> f64 {
    Local.from_local_datetime(&d).earliest().map(|x| x.timestamp() as f64).unwrap_or(0.0)
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1).and_then(|d| d.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

fn add_months(d: NaiveDateTime, months: i64) -> NaiveDateTime {
    let total = d.year() as i64 * 12 + d.month0() as i64 + months;
    let (y, m) = ((total / 12) as i32, (total % 12) as u32 + 1);
    NaiveDate::from_ymd_opt(y, m, d.day().min(days_in_month(y, m))).map(|nd| nd.and_time(d.time())).unwrap_or(d)
}

pub fn occurrences(e: &Event, range_start: f64, range_end: f64) -> Vec<(f64, f64)> {
    let duration = e.end - e.start;
    if e.recurrence.is_empty() || e.recurrence == "none" {
        return if e.start < range_end && e.end > range_start { vec![(e.start, e.end)] } else { vec![] };
    }
    let interval = e.recur_interval.max(1);
    let ex: Vec<NaiveDate> = crate::db::split_list(&e.exdates)
        .iter()
        .filter_map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .collect();
    let origin = to_local(e.start);
    let mut out = Vec::new();
    let mut cur = origin;
    if e.recurrence == "weekdays" {
        while cur.weekday().num_days_from_monday() >= 5 {
            cur += Duration::days(1);
        }
    }
    // Uzak geçmişteki günlük/haftalık seriler için hızlı ileri sarma
    let rs = to_local(range_start);
    if matches!(e.recurrence.as_str(), "daily" | "weekly") && rs > cur + Duration::days(2) {
        let step = interval * if e.recurrence == "weekly" { 7 } else { 1 };
        let skip = (rs - cur).num_days() / step - 1;
        if skip > 0 {
            cur += Duration::days(skip * step);
        }
    }
    let mut k: i64 = 0;
    for _ in 0..5000 {
        let cs = to_ts(cur);
        if cs >= range_end {
            break;
        }
        if let Some(u) = e.recur_until {
            if cs > u + 86399.0 {
                break;
            }
        }
        if cs + duration > range_start && !ex.contains(&cur.date()) {
            out.push((cs, cs + duration));
        }
        k += 1;
        let next = match e.recurrence.as_str() {
            "daily" => cur + Duration::days(interval),
            "weekdays" => {
                let mut n = cur + Duration::days(1);
                while n.weekday().num_days_from_monday() >= 5 {
                    n += Duration::days(1);
                }
                n
            }
            "weekly" => cur + Duration::weeks(interval),
            "monthly" => add_months(origin, interval * k),
            "yearly" => add_months(origin, 12 * interval * k),
            _ => break,
        };
        if next <= cur {
            break;
        }
        cur = next;
    }
    out
}

pub fn expand(events: Vec<Event>, hidden_calendars: &[i64], range_start: f64, range_end: f64) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for e in events {
        if hidden_calendars.contains(&e.calendar_id) {
            continue;
        }
        for (s, en) in occurrences(&e, range_start, range_end) {
            out.push(Occurrence { event: e.clone(), start: s, end: en });
        }
    }
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap().then((b.end - b.start).partial_cmp(&(a.end - a.start)).unwrap()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u32, d: u32, h: u32) -> f64 {
        to_ts(NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap())
    }

    #[test]
    fn weekly_and_monthly() {
        let e = Event { start: ts(2024, 1, 1, 9), end: ts(2024, 1, 1, 10), recurrence: "weekly".into(), recur_interval: 1, ..Event::new_default() };
        let occ = occurrences(&e, ts(2024, 1, 1, 0), ts(2024, 2, 1, 0));
        assert_eq!(occ.len(), 5);
        let m = Event { start: ts(2024, 1, 31, 9), end: ts(2024, 1, 31, 10), recurrence: "monthly".into(), recur_interval: 1, ..Event::new_default() };
        let occ = occurrences(&m, ts(2024, 1, 1, 0), ts(2024, 5, 1, 0));
        let days: Vec<u32> = occ.iter().map(|o| to_local(o.0).day()).collect();
        assert_eq!(days, vec![31, 29, 31, 30]);
        let w = Event { start: ts(2024, 1, 5, 9), end: ts(2024, 1, 5, 10), recurrence: "weekdays".into(), recur_interval: 1, exdates: "2024-01-09".into(), ..Event::new_default() };
        let occ = occurrences(&w, ts(2024, 1, 1, 0), ts(2024, 1, 13, 0));
        assert_eq!(occ.len(), 5); // 5,8,10,11,12 (9 hariç)
    }
}
