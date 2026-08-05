mod actual;
mod evidence;

use serde_json::Value;
use std::error::Error;

const CHAT_ID: &str = "parity-chat-347";
const REPLY_ID: &str = "reply-829";
const FINAL_TEXT: &str = "parity owner final 613";
const OBSERVED_AT_UNIX_MS: u64 = 31_031;
const FIELDS: [&str; 9] = [
    "kind",
    "state",
    "severity",
    "reason.code",
    "lineage.subject_ref",
    "lineage.parent_ref",
    "lineage.action_ref",
    "capability.kind",
    "capability.delivery",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CanonicalFields {
    kind: String,
    state: String,
    severity: String,
    reason: String,
    subject: String,
    parent: String,
    action: String,
    capability: String,
    delivery: String,
}

#[test]
fn canonical_parity_surfaces_match_literal_owner_oracle() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let expected = expected_oracle();
    let rows = [
        ("cli", "internal formatter", actual::cli()?),
        (
            "api",
            "ApiHttpRequest /v1/diagnostics JSON seam",
            actual::api()?,
        ),
        (
            "websocket",
            "real WebSocket server/client",
            actual::websocket()?,
        ),
        ("channel", "channel event projector", actual::channel()?),
    ];
    for (surface, _, actual) in &rows {
        compare(surface, &expected, actual)?;
    }
    for (surface, _, actual) in &rows {
        for field in FIELDS {
            let mutated = mutate(actual, field);
            assert_eq!(
                compare(surface, &expected, &mutated).unwrap_err(),
                expected_error(surface, field, &expected, &mutated)
            );
        }
    }
    evidence::write(root.path(), &expected, &rows)
}

fn expected_oracle() -> CanonicalFields {
    CanonicalFields {
        kind: "progress".to_owned(),
        state: "ready".to_owned(),
        severity: "info".to_owned(),
        reason: "included".to_owned(),
        subject: "subject:channel:websocket:message".to_owned(),
        parent: format!("parent:channel:websocket:chat:{CHAT_ID}"),
        action: format!("action:channel:websocket:reply:{REPLY_ID}"),
        capability: "progress".to_owned(),
        delivery: "final_delivered".to_owned(),
    }
}

pub(super) fn parse_json(value: &Value) -> CanonicalFields {
    CanonicalFields {
        kind: string_at(value, &["kind"]),
        state: string_at(value, &["state"]),
        severity: string_at(value, &["severity"]),
        reason: string_at(value, &["reason", "code"]),
        subject: string_at(value, &["lineage", "subject_ref"]),
        parent: string_at(value, &["lineage", "parent_ref"]),
        action: string_at(value, &["lineage", "action_ref"]),
        capability: string_at(value, &["capability", "kind"]),
        delivery: string_at(value, &["capability", "details", "delivery"]),
    }
}

pub(super) fn parse_line(line: &str) -> Result<CanonicalFields, Box<dyn Error>> {
    Ok(CanonicalFields {
        kind: token(line, "kind=")?,
        state: token(line, "state=")?,
        severity: token(line, "severity=")?,
        reason: token(line, "reason=")?,
        subject: token(line, "lineage=")?,
        parent: token(line, "parent=")?,
        action: token(line, "action=")?,
        capability: token(line, "capability=")?,
        delivery: token(line, "delivery=")?,
    })
}

fn compare(
    surface: &str,
    expected: &CanonicalFields,
    actual: &CanonicalFields,
) -> Result<(), String> {
    for field in FIELDS {
        if value(expected, field) != value(actual, field) {
            return Err(expected_error(surface, field, expected, actual));
        }
    }
    Ok(())
}

fn mutate(actual: &CanonicalFields, field: &str) -> CanonicalFields {
    let mut mutated = actual.clone();
    *slot(&mut mutated, field) = format!("mutated-{}", field.replace('.', "-"));
    mutated
}

fn expected_error(
    surface: &str,
    field: &str,
    expected: &CanonicalFields,
    actual: &CanonicalFields,
) -> String {
    format!(
        "surface={surface} field={field} expected={} actual={}",
        value(expected, field),
        value(actual, field)
    )
}

pub(super) fn value<'a>(fields: &'a CanonicalFields, field: &str) -> &'a str {
    match field {
        "kind" => &fields.kind,
        "state" => &fields.state,
        "severity" => &fields.severity,
        "reason.code" => &fields.reason,
        "lineage.subject_ref" => &fields.subject,
        "lineage.parent_ref" => &fields.parent,
        "lineage.action_ref" => &fields.action,
        "capability.kind" => &fields.capability,
        "capability.delivery" => &fields.delivery,
        _ => "",
    }
}

fn slot<'a>(fields: &'a mut CanonicalFields, field: &str) -> &'a mut String {
    match field {
        "kind" => &mut fields.kind,
        "state" => &mut fields.state,
        "severity" => &mut fields.severity,
        "reason.code" => &mut fields.reason,
        "lineage.subject_ref" => &mut fields.subject,
        "lineage.parent_ref" => &mut fields.parent,
        "lineage.action_ref" => &mut fields.action,
        "capability.kind" => &mut fields.capability,
        "capability.delivery" => &mut fields.delivery,
        _ => &mut fields.kind,
    }
}

fn string_at(value: &Value, path: &[&str]) -> String {
    path.iter()
        .fold(value, |value, key| &value[*key])
        .as_str()
        .unwrap_or("")
        .to_owned()
}

fn token(line: &str, prefix: &str) -> Result<String, Box<dyn Error>> {
    let Some(rest) = line.split_once(prefix).map(|(_, rest)| rest) else {
        return Err(format!("missing token {prefix} in {line}").into());
    };
    Ok(rest.split_whitespace().next().unwrap_or("").to_owned())
}
