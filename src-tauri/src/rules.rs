//! Gelen ileti kuralları (Outlook "Kurallar ve Uyarılar").

use crate::db::{Message, Rule};
use serde::{Deserialize, Serialize};

/// Yerel kuralın bir koşulu ya da eylemi
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpecPart {
    pub kind: String,
    /// Metin koşullarında işlem: contains, not_contains, equals, starts, ends, regex
    pub op: String,
    pub values: Vec<String>,
}

/// Yerel kuralın tam modeli (`rules.spec` sütununda JSON): birden çok koşul, "ve/veya",
/// özel durumlar ve eylemler. Uygulama açıkken bu bilgisayarda çalışır.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalSpec {
    pub conditions: Vec<SpecPart>,
    pub exceptions: Vec<SpecPart>,
    pub actions: Vec<SpecPart>,
    /// true: koşullardan biri yeterli, false: hepsi sağlanmalı
    pub match_any: bool,
}

/// "High" / "Normal" / "Low" → iletideki önem düzeyi
pub fn importance_level(v: &str) -> i64 {
    match v {
        "High" => 2,
        "Low" => 0,
        _ => 1,
    }
}

fn text_match(text: &str, p: &SpecPart) -> bool {
    if p.op == "regex" {
        return p.values.iter().any(|v| {
            regex::RegexBuilder::new(v).case_insensitive(true).size_limit(1 << 20).build().is_ok_and(|r| r.is_match(text))
        });
    }
    let text = text.to_lowercase();
    let mut words = p.values.iter().map(|v| v.to_lowercase());
    match p.op.as_str() {
        "not_contains" => !words.any(|v| text.contains(&v)),
        "equals" => words.any(|v| text.trim() == v.trim()),
        "starts" => words.any(|v| text.trim_start().starts_with(&v)),
        "ends" => words.any(|v| text.trim_end().ends_with(&v)),
        _ => words.any(|v| text.contains(&v)),
    }
}

/// "YYYY-MM-DD" → yerel saatle günün başlangıcı (saniye)
fn day_start(v: &str) -> Option<f64> {
    use chrono::TimeZone;
    let d = chrono::NaiveDate::parse_from_str(v.trim(), "%Y-%m-%d").ok()?;
    chrono::Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest().map(|t| t.timestamp() as f64)
}

fn condition_match(p: &SpecPart, m: &Message, me: &[String]) -> bool {
    let has_me = |list: &str| crate::mail::bare_addresses(list).iter().any(|a| me.contains(&a.to_lowercase()));
    let first = || p.values.first().map(String::as_str).unwrap_or("");
    match p.kind.as_str() {
        "from" => text_match(&m.sender, p),
        "to" => text_match(&m.to_addr, p),
        "cc" => text_match(&m.cc, p),
        "sent_to" => text_match(&format!("{} {}", m.to_addr, m.cc), p),
        "subject" => text_match(&m.subject, p),
        "body" => text_match(&m.body_text, p),
        "subject_or_body" => text_match(&format!("{} {}", m.subject, m.body_text), p),
        "categories" => text_match(&m.categories, p),
        "sent_to_me" => has_me(&m.to_addr),
        "sent_cc_me" => has_me(&m.cc),
        "sent_to_or_cc_me" => has_me(&m.to_addr) || has_me(&m.cc),
        "not_sent_to_me" => !has_me(&m.to_addr),
        "sent_only_to_me" => {
            let to = crate::mail::bare_addresses(&m.to_addr);
            to.len() == 1 && has_me(&m.to_addr) && m.cc.trim().is_empty()
        }
        "has_attachments" => m.has_attachments,
        "flagged" => m.is_flagged,
        "is_meeting_request" => m.att_names.to_lowercase().contains(".ics") || m.raw.windows(13).any(|w| w.eq_ignore_ascii_case(b"text/calendar")),
        "importance" => m.importance == importance_level(first()),
        "size_range" => {
            let kb = (m.raw.len() / 1024) as f64;
            let bound = |i: usize| p.values.get(i).and_then(|v| v.trim().parse::<f64>().ok());
            bound(0).map_or(true, |min| kb >= min) && bound(1).map_or(true, |max| kb <= max)
        }
        "date_range" => {
            let bound = |i: usize| p.values.get(i).and_then(|v| day_start(v));
            bound(0).map_or(true, |after| m.date >= after) && bound(1).map_or(true, |before| m.date < before + 86400.0)
        }
        _ => false,
    }
}

fn spec_matches(s: &LocalSpec, m: &Message, me: &[String]) -> bool {
    let hit = |p: &SpecPart| condition_match(p, m, me);
    let matched = if s.conditions.is_empty() {
        true
    } else if s.match_any {
        s.conditions.iter().any(hit)
    } else {
        s.conditions.iter().all(hit)
    };
    matched && !s.exceptions.iter().any(hit)
}

fn field_text(m: &Message, field: &str) -> String {
    match field {
        "from" => m.sender.clone(),
        "to" => format!("{} {}", m.to_addr, m.cc),
        "subject" => m.subject.clone(),
        "body" => m.body_text.clone(),
        _ => format!("{} {} {} {} {}", m.sender, m.to_addr, m.cc, m.subject, m.body_text),
    }
}

pub fn matches(rule: &Rule, m: &Message) -> bool {
    let text = field_text(m, &rule.field).to_lowercase();
    let val = rule.value.to_lowercase();
    match rule.op.as_str() {
        "contains" => text.contains(&val),
        "not_contains" => !text.contains(&val),
        "equals" => text.trim() == val.trim(),
        "starts" => text.trim_start().starts_with(&val),
        "ends" => text.trim_end().ends_with(&val),
        _ => false,
    }
}

/// Kuralların değerlendirilmesi için iletinin tam içeriği (gövde, boyut) gerekiyor mu
pub fn needs_content(rules: &[Rule]) -> bool {
    rules.iter().any(|r| match serde_json::from_str::<LocalSpec>(&r.spec) {
        Ok(s) => s.conditions.iter().chain(&s.exceptions).any(|p| matches!(p.kind.as_str(), "body" | "subject_or_body" | "size_range" | "is_meeting_request")),
        Err(_) => !matches!(r.field.as_str(), "from" | "to" | "subject"),
    })
}

/// Eşleşen kuralların (eylem, argüman) listesi. `me`: kullanıcının adresleri (küçük harf).
pub fn evaluate(rules: &[Rule], m: &Message, account_id: Option<i64>, me: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for r in rules {
        // Exchange sunucu kuralları sunucuda çalışır; yerelde tekrar uygulanmaz
        if !r.enabled || r.remote_id.is_some() {
            continue;
        }
        if r.account_id.is_some() && r.account_id != account_id {
            continue;
        }
        if !r.spec.is_empty() {
            let Ok(s) = serde_json::from_str::<LocalSpec>(&r.spec) else { continue };
            if !spec_matches(&s, m, me) {
                continue;
            }
            let mut stop = r.stop;
            for a in &s.actions {
                match a.kind.as_str() {
                    "stop" => stop = true,
                    "categories" => out.extend(a.values.iter().map(|v| ("category".to_string(), v.clone()))),
                    _ => out.push((a.kind.clone(), a.values.join(", "))),
                }
            }
            if stop {
                break;
            }
        } else if matches(r, m) {
            out.push((r.action.clone(), r.action_arg.clone()));
            if r.stop {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_matching() {
        let m = Message { sender: "Bülten <news@site.com>".into(), subject: "Kampanya".into(), ..Default::default() };
        let rules = vec![
            Rule { enabled: true, field: "from".into(), op: "contains".into(), value: "NEWS@".into(), action: "move".into(), action_arg: "5".into(), stop: true, ..Default::default() },
            Rule { enabled: true, field: "subject".into(), op: "contains".into(), value: "kampanya".into(), action: "flag".into(), ..Default::default() },
        ];
        assert_eq!(evaluate(&rules, &m, None, &[]), vec![("move".to_string(), "5".to_string())]);
    }

    fn rule(spec: serde_json::Value) -> Rule {
        Rule { enabled: true, spec: spec.to_string(), ..Default::default() }
    }

    #[test]
    fn local_conditions() {
        let me = vec!["ben@x.com".to_string()];
        let m = Message { sender: "Ali <ali@site.com>".into(), to_addr: "ben@x.com".into(), subject: "Aylık Rapor 2026".into(), ..Default::default() };
        // Hepsi sağlanmalı: biri tutmuyorsa eylem yok
        let all = rule(serde_json::json!({
            "conditions": [{"kind": "from", "values": ["site.com"]}, {"kind": "subject", "values": ["fatura"]}],
            "actions": [{"kind": "mark_read"}],
        }));
        assert!(evaluate(&[all], &m, None, &me).is_empty());
        // Biri yeterli; kategoriler ayrı eylemlere açılır, "stop" eylem olarak dönmez
        let any = rule(serde_json::json!({
            "match_any": true,
            "conditions": [{"kind": "subject", "op": "regex", "values": ["rapor \\d+"]}, {"kind": "subject", "values": ["fatura"]}],
            "actions": [{"kind": "categories", "values": ["İş", "Rapor"]}, {"kind": "stop"}],
        }));
        let second = rule(serde_json::json!({"actions": [{"kind": "flag"}]}));
        let got = evaluate(&[any, second], &m, None, &me);
        assert_eq!(got, vec![("category".to_string(), "İş".to_string()), ("category".to_string(), "Rapor".to_string())]);
        // Yalnızca bana; özel durum kuralı devre dışı bırakır
        let only = rule(serde_json::json!({"conditions": [{"kind": "sent_only_to_me"}], "actions": [{"kind": "flag"}]}));
        assert_eq!(evaluate(std::slice::from_ref(&only), &m, None, &me).len(), 1);
        let except = rule(serde_json::json!({
            "conditions": [{"kind": "sent_only_to_me"}],
            "exceptions": [{"kind": "from", "op": "not_contains", "values": ["başka"]}],
            "actions": [{"kind": "flag"}],
        }));
        assert!(evaluate(&[except], &m, None, &me).is_empty());
    }

    #[test]
    fn content_needed_only_for_body_conditions() {
        let simple = |field: &str| Rule { enabled: true, field: field.into(), ..Default::default() };
        assert!(!needs_content(&[simple("from"), simple("subject")]));
        assert!(needs_content(&[simple("body")]));
        assert!(needs_content(&[simple("any")]));
        assert!(!needs_content(&[rule(serde_json::json!({"conditions": [{"kind": "from", "values": ["x"]}]}))]));
        assert!(needs_content(&[rule(serde_json::json!({"exceptions": [{"kind": "subject_or_body", "values": ["x"]}]}))]));
        assert!(needs_content(&[rule(serde_json::json!({"conditions": [{"kind": "size_range", "values": ["1", ""]}]}))]));
    }

    #[test]
    fn local_ranges() {
        let m = Message { raw: vec![0; 3 * 1024], importance: 2, date: day_start("2026-03-15").unwrap() + 3600.0, ..Default::default() };
        let check = |kind: &str, values: Vec<&str>| {
            let p = SpecPart { kind: kind.into(), values: values.into_iter().map(String::from).collect(), ..Default::default() };
            condition_match(&p, &m, &[])
        };
        assert!(check("size_range", vec!["1", "5"]));
        assert!(!check("size_range", vec!["4", ""]));
        assert!(check("date_range", vec!["2026-03-15", "2026-03-15"]));
        assert!(!check("date_range", vec!["2026-03-16", ""]));
        assert!(check("importance", vec!["High"]));
    }
}
