//! Exchange gelen kutusu kurallarının tam modeli (Outlook "Kural Sihirbazı" gibi):
//! birden çok koşul, özel durum ve eylem. EWS XML'i ile iki yönlü eşlenir; modelde
//! karşılığı olmayan öğeler değiştirilmeden korunur.

use super::xml::{self, esc, Element};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RulePart {
    /// Koşul/eylem türü (ör. "from", "subject", "move", "raw")
    pub kind: String,
    /// Değerler (adresler, sözcükler, klasör kimliği, önem düzeyi...)
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RuleSpec {
    pub conditions: Vec<RulePart>,
    pub exceptions: Vec<RulePart>,
    pub actions: Vec<RulePart>,
}

/// Değer türü
#[derive(Clone, Copy, PartialEq)]
enum Val {
    Strings,
    Addresses,
    Bool,
    Text,
    Size,
    Date,
}

/// (tür, EWS öğesi, değer türü) — sıra EWS şemasındaki RulePredicates sırasıdır.
const PREDICATES: &[(&str, &str, Val)] = &[
    ("categories", "Categories", Val::Strings),
    ("body", "ContainsBodyStrings", Val::Strings),
    ("header", "ContainsHeaderStrings", Val::Strings),
    ("recipient_contains", "ContainsRecipientStrings", Val::Strings),
    ("sender_contains", "ContainsSenderStrings", Val::Strings),
    ("subject_or_body", "ContainsSubjectOrBodyStrings", Val::Strings),
    ("subject", "ContainsSubjectStrings", Val::Strings),
    ("flagged", "FlaggedForAction", Val::Text),
    ("from", "FromAddresses", Val::Addresses),
    ("from_account", "FromConnectedAccounts", Val::Strings),
    ("has_attachments", "HasAttachments", Val::Bool),
    ("importance", "Importance", Val::Text),
    ("is_approval_request", "IsApprovalRequest", Val::Bool),
    ("is_automatic_forward", "IsAutomaticForward", Val::Bool),
    ("is_automatic_reply", "IsAutomaticReply", Val::Bool),
    ("is_encrypted", "IsEncrypted", Val::Bool),
    ("is_meeting_request", "IsMeetingRequest", Val::Bool),
    ("is_meeting_response", "IsMeetingResponse", Val::Bool),
    ("is_ndr", "IsNDR", Val::Bool),
    ("is_permission_controlled", "IsPermissionControlled", Val::Bool),
    ("is_read_receipt", "IsReadReceipt", Val::Bool),
    ("is_signed", "IsSigned", Val::Bool),
    ("is_voicemail", "IsVoicemail", Val::Bool),
    ("item_classes", "ItemClasses", Val::Strings),
    ("message_classifications", "MessageClassifications", Val::Strings),
    ("not_sent_to_me", "NotSentToMe", Val::Bool),
    ("sent_cc_me", "SentCcMe", Val::Bool),
    ("sent_only_to_me", "SentOnlyToMe", Val::Bool),
    ("sent_to", "SentToAddresses", Val::Addresses),
    ("sent_to_me", "SentToMe", Val::Bool),
    ("sent_to_or_cc_me", "SentToOrCcMe", Val::Bool),
    ("sensitivity", "Sensitivity", Val::Text),
    ("date_range", "WithinDateRange", Val::Date),
    ("size_range", "WithinSizeRange", Val::Size),
];

/// Eylemler — sıra EWS şemasındaki RuleActions sırasıdır.
const ACTIONS: &[(&str, &str, Val)] = &[
    ("categories", "AssignCategories", Val::Strings),
    ("copy", "CopyToFolder", Val::Text),
    ("delete", "Delete", Val::Bool),
    ("forward_attachment", "ForwardAsAttachmentToRecipients", Val::Addresses),
    ("forward", "ForwardToRecipients", Val::Addresses),
    ("importance", "MarkImportance", Val::Text),
    ("mark_read", "MarkAsRead", Val::Bool),
    ("move", "MoveToFolder", Val::Text),
    ("permanent_delete", "PermanentDelete", Val::Bool),
    ("redirect", "RedirectToRecipients", Val::Addresses),
    ("sms", "SendSMSAlertToRecipients", Val::Addresses),
    ("reply_template", "ServerReplyWithMessage", Val::Text),
    ("stop", "StopProcessingRules", Val::Bool),
];

fn parse_parts(parent: Option<&Element>, table: &[(&str, &str, Val)]) -> Vec<RulePart> {
    let Some(p) = parent else { return vec![] };
    let mut out = Vec::new();
    for c in &p.children {
        let Some((kind, _, val)) = table.iter().find(|(_, n, _)| *n == c.name) else {
            // Modelde olmayan öğe: olduğu gibi saklanır
            out.push(RulePart { kind: "raw".into(), values: vec![c.to_xml("t")] });
            continue;
        };
        let values = match val {
            Val::Strings => c.children.iter().map(|s| s.text.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            Val::Addresses => c
                .children
                .iter()
                .map(|a| {
                    let email = a.find("EmailAddress").map(|x| x.text.trim().to_string()).unwrap_or_default();
                    let name = a.find("Name").map(|x| x.text.trim().to_string()).unwrap_or_default();
                    if !name.is_empty() && name != email && !email.is_empty() { format!("{name} <{email}>") } else if email.is_empty() { name } else { email }
                })
                .filter(|s| !s.is_empty())
                .collect(),
            Val::Bool => {
                if c.text.trim() != "true" {
                    continue;
                }
                vec![]
            }
            Val::Text => {
                if *kind == "move" || *kind == "copy" {
                    let id = c.find("FolderId").or(c.find("DistinguishedFolderId")).and_then(|f| f.attr("Id")).unwrap_or("").to_string();
                    vec![id]
                } else if *kind == "reply_template" {
                    vec![c.find("ItemId").and_then(|f| f.attr("Id")).unwrap_or("").to_string()]
                } else {
                    vec![c.text.trim().to_string()]
                }
            }
            Val::Size => vec![c.text_of("MinimumSize"), c.text_of("MaximumSize")],
            Val::Date => vec![c.text_of("StartDateTime"), c.text_of("EndDateTime")],
        };
        out.push(RulePart { kind: kind.to_string(), values });
    }
    out
}

/// Kural gövdesi XML'inden (Conditions/Exceptions/Actions) model
pub fn from_xml(body_xml: &str) -> RuleSpec {
    let wrapped = format!(r#"<r xmlns:t="t">{body_xml}</r>"#);
    let Ok(doc) = xml::parse(wrapped.as_bytes()) else { return RuleSpec::default() };
    let Some(r) = doc.child("r") else { return RuleSpec::default() };
    RuleSpec {
        conditions: parse_parts(r.child("Conditions"), PREDICATES),
        exceptions: parse_parts(r.child("Exceptions"), PREDICATES),
        actions: parse_parts(r.child("Actions"), ACTIONS),
    }
}

fn addr_xml(v: &str) -> String {
    let (name, email) = match (v.find('<'), v.rfind('>')) {
        (Some(a), Some(b)) if b > a => (v[..a].trim().trim_matches('"').to_string(), v[a + 1..b].trim().to_string()),
        _ => (String::new(), v.trim().to_string()),
    };
    let name = if name.is_empty() { String::new() } else { format!("<t:Name>{}</t:Name>", esc(&name)) };
    format!("<t:Address>{name}<t:EmailAddress>{}</t:EmailAddress></t:Address>", esc(&email))
}

fn folder_xml(id: &str) -> String {
    const DIST: [&str; 8] = ["inbox", "deleteditems", "junkemail", "sentitems", "drafts", "outbox", "archive", "msgfolderroot"];
    if DIST.contains(&id) {
        format!(r#"<t:DistinguishedFolderId Id="{}"/>"#, esc(id))
    } else {
        format!(r#"<t:FolderId Id="{}"/>"#, esc(id))
    }
}

fn parts_xml(parts: &[RulePart], table: &[(&str, &str, Val)]) -> Result<String, String> {
    let mut out = String::new();
    for (kind, el, val) in table {
        for p in parts.iter().filter(|p| p.kind == *kind) {
            let vals: Vec<&String> = p.values.iter().filter(|v| !v.trim().is_empty()).collect();
            let inner = match val {
                Val::Strings => {
                    if vals.is_empty() {
                        return Err(format!("rules.need_value:{kind}"));
                    }
                    vals.iter().map(|v| format!("<t:String>{}</t:String>", esc(v.trim()))).collect()
                }
                Val::Addresses => {
                    if vals.is_empty() {
                        return Err(format!("rules.need_value:{kind}"));
                    }
                    vals.iter().map(|v| addr_xml(v)).collect()
                }
                Val::Bool => "true".to_string(),
                Val::Text => {
                    let v = vals.first().map(|s| s.trim().to_string()).ok_or_else(|| format!("rules.need_value:{kind}"))?;
                    match *kind {
                        "move" | "copy" => folder_xml(&v),
                        "reply_template" => format!(r#"<t:ItemId Id="{}"/>"#, esc(&v)),
                        _ => esc(&v),
                    }
                }
                Val::Size => {
                    let min = p.values.first().map(|s| s.trim()).unwrap_or("");
                    let max = p.values.get(1).map(|s| s.trim()).unwrap_or("");
                    let mut x = String::new();
                    if !min.is_empty() {
                        x.push_str(&format!("<t:MinimumSize>{}</t:MinimumSize>", esc(min)));
                    }
                    if !max.is_empty() {
                        x.push_str(&format!("<t:MaximumSize>{}</t:MaximumSize>", esc(max)));
                    }
                    x
                }
                Val::Date => {
                    let a = p.values.first().map(|s| s.trim()).unwrap_or("");
                    let b = p.values.get(1).map(|s| s.trim()).unwrap_or("");
                    let mut x = String::new();
                    if !a.is_empty() {
                        x.push_str(&format!("<t:StartDateTime>{}</t:StartDateTime>", esc(a)));
                    }
                    if !b.is_empty() {
                        x.push_str(&format!("<t:EndDateTime>{}</t:EndDateTime>", esc(b)));
                    }
                    x
                }
            };
            out.push_str(&format!("<t:{el}>{inner}</t:{el}>"));
        }
    }
    // Modelde olmayan (korunmuş) öğeler sona eklenir
    for p in parts.iter().filter(|p| p.kind == "raw") {
        if let Some(x) = p.values.first() {
            out.push_str(x);
        }
    }
    Ok(out)
}

/// Modelden kural gövdesi XML'i
pub fn to_xml(spec: &RuleSpec) -> Result<String, String> {
    let acts: Vec<&RulePart> = spec.actions.iter().filter(|a| a.kind != "stop").collect();
    if acts.is_empty() {
        return Err("rules.need_action".into());
    }
    let cond = parts_xml(&spec.conditions, PREDICATES)?;
    let exc = parts_xml(&spec.exceptions, PREDICATES)?;
    let act = parts_xml(&spec.actions, ACTIONS)?;
    let mut x = if cond.is_empty() { "<t:Conditions/>".to_string() } else { format!("<t:Conditions>{cond}</t:Conditions>") };
    if !exc.is_empty() {
        x.push_str(&format!("<t:Exceptions>{exc}</t:Exceptions>"));
    }
    x.push_str(&format!("<t:Actions>{act}</t:Actions>"));
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_complex_rule() {
        let body = r#"<t:Conditions><t:ContainsSubjectStrings><t:String>rapor</t:String><t:String>fatura</t:String></t:ContainsSubjectStrings><t:FromAddresses><t:Address><t:Name>Ali Veli</t:Name><t:EmailAddress>ali@x.com</t:EmailAddress></t:Address><t:Address><t:EmailAddress>b@x.com</t:EmailAddress></t:Address></t:FromAddresses><t:HasAttachments>true</t:HasAttachments></t:Conditions><t:Exceptions><t:SentOnlyToMe>true</t:SentOnlyToMe></t:Exceptions><t:Actions><t:AssignCategories><t:String>Kırmızı</t:String></t:AssignCategories><t:MoveToFolder><t:FolderId Id="AAMk=" ChangeKey="x"/></t:MoveToFolder><t:StopProcessingRules>true</t:StopProcessingRules></t:Actions>"#;
        let spec = from_xml(body);
        assert_eq!(spec.conditions.len(), 3);
        assert_eq!(spec.conditions[0].values, vec!["rapor", "fatura"]);
        assert_eq!(spec.conditions[1].values, vec!["Ali Veli <ali@x.com>", "b@x.com"]);
        assert_eq!(spec.exceptions[0].kind, "sent_only_to_me");
        assert_eq!(spec.actions.iter().map(|a| a.kind.as_str()).collect::<Vec<_>>(), vec!["categories", "move", "stop"]);
        let x = to_xml(&spec).unwrap();
        assert!(x.contains("<t:Name>Ali Veli</t:Name><t:EmailAddress>ali@x.com</t:EmailAddress>"));
        assert!(x.contains(r#"<t:FolderId Id="AAMk="/>"#));
        // Yeniden ayrıştırınca aynı model
        assert_eq!(from_xml(&x), spec);
    }

    #[test]
    fn unknown_parts_are_preserved() {
        let body = r#"<t:Conditions><t:Foo><t:Bar>1</t:Bar></t:Foo></t:Conditions><t:Actions><t:Delete>true</t:Delete></t:Actions>"#;
        let spec = from_xml(body);
        assert_eq!(spec.conditions[0].kind, "raw");
        assert!(to_xml(&spec).unwrap().contains("<t:Foo><t:Bar>1</t:Bar></t:Foo>"));
    }
}
