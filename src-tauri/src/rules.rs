//! Gelen ileti kuralları (Outlook "Kurallar ve Uyarılar").

use crate::db::{Message, Rule};

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

/// Eşleşen kuralların (eylem, argüman) listesi.
pub fn evaluate(rules: &[Rule], m: &Message, account_id: Option<i64>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for r in rules {
        // Exchange sunucu kuralları sunucuda çalışır; yerelde tekrar uygulanmaz
        if !r.enabled || r.remote_id.is_some() {
            continue;
        }
        if r.account_id.is_some() && r.account_id != account_id {
            continue;
        }
        if matches(r, m) {
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
        assert_eq!(evaluate(&rules, &m, None), vec![("move".to_string(), "5".to_string())]);
    }
}
