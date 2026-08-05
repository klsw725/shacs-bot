use serde_json::{Map, Value};
use shacs_redaction::REDACTED;

pub fn sanitize_diagnostics_value(value: &Value) -> Value {
    sanitize_value(None, value)
}

fn sanitize_value(key: Option<&str>, value: &Value) -> Value {
    if key.is_some_and(|field| redacts_full_value(field, value)) {
        return Value::String(REDACTED.to_owned());
    }
    match value {
        Value::Object(object) => Value::Object(sanitize_object(object)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| sanitize_value(None, item))
                .collect(),
        ),
        Value::String(text) if redacts_diagnostic_text(text) => Value::String(REDACTED.to_owned()),
        other => other.clone(),
    }
}

fn sanitize_object(object: &Map<String, Value>) -> Map<String, Value> {
    object
        .iter()
        .map(|(key, value)| (key.clone(), sanitize_value(Some(key), value)))
        .collect()
}

fn redacts_full_value(key: &str, value: &Value) -> bool {
    let normalized = normalize_key(key);
    if is_raw_diagnostic_material_key(&normalized)
        || (is_path_key(&normalized) && value_contains_absolute_host_path(value))
        || is_process_identity_key(&normalized)
    {
        return true;
    }
    safe_projection_kind(&normalized).is_some_and(|kind| !is_safe_projection_value(kind, value))
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone, Copy)]
enum SafeProjectionKind {
    Ref,
    State,
    Count,
    MalformedPluralRefs,
}

fn safe_projection_kind(normalized: &str) -> Option<SafeProjectionKind> {
    if normalized.ends_with("refs") {
        Some(SafeProjectionKind::MalformedPluralRefs)
    } else if normalized.ends_with("ref") {
        Some(SafeProjectionKind::Ref)
    } else if normalized.ends_with("state") {
        Some(SafeProjectionKind::State)
    } else if normalized.ends_with("count") {
        Some(SafeProjectionKind::Count)
    } else {
        None
    }
}

fn is_raw_diagnostic_material_key(normalized: &str) -> bool {
    normalized.contains("stdout")
        || normalized.contains("stderr")
        || normalized.contains("standardoutput")
        || normalized.contains("processhandle")
        || normalized.ends_with("payload")
        || matches!(
            normalized,
            "args" | "argv" | "arguments" | "env" | "environment"
        )
}

fn is_path_key(normalized: &str) -> bool {
    matches!(
        normalized,
        "path" | "absolutepath" | "hostpath" | "absolutehostpath"
    ) || normalized.ends_with("path")
}

fn is_process_identity_key(normalized: &str) -> bool {
    matches!(normalized, "pid" | "processid" | "ownerid" | "rawownerid")
        || normalized.ends_with("pid")
        || normalized.ends_with("ownerid")
}

fn value_contains_absolute_host_path(value: &Value) -> bool {
    match value {
        Value::String(text) => contains_absolute_host_path(text),
        Value::Array(items) => items.iter().any(value_contains_absolute_host_path),
        Value::Object(object) => object.values().any(value_contains_absolute_host_path),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn redacts_diagnostic_text(text: &str) -> bool {
    contains_control(text)
        || contains_absolute_host_path(text)
        || contains_raw_diagnostic_text(text)
        || contains_token_like_raw_text(text)
}

fn contains_absolute_host_path(text: &str) -> bool {
    starts_with_unix_absolute_path(text)
        || contains_windows_drive_path(text)
        || text.starts_with("\\\\")
        || text.contains("/Users/")
        || text.contains("/home/")
        || text.contains("/private/var/")
}

fn is_safe_projection_value(kind: SafeProjectionKind, value: &Value) -> bool {
    match kind {
        SafeProjectionKind::Ref | SafeProjectionKind::State => {
            value.as_str().is_some_and(is_safe_projection_token)
        }
        SafeProjectionKind::Count => value.as_u64().is_some_and(|count| count <= 1_000_000),
        SafeProjectionKind::MalformedPluralRefs => false,
    }
}

fn is_safe_projection_token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && !redacts_diagnostic_text(text)
        && !text
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '{' | '}' | '[' | ']' | '"' | '\''))
        && !text.chars().any(char::is_whitespace)
}

fn contains_raw_diagnostic_text(text: &str) -> bool {
    let normalized = normalize_key(text);
    text.to_ascii_lowercase().contains("process_handle")
        || normalized.contains("processhandle")
        || normalized.contains("rawstdout")
        || normalized.contains("rawstderr")
        || normalized.contains("standardoutputraw")
        || normalized.contains("rawproviderpayload")
}

fn contains_token_like_raw_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("bearer ")
        || lower.contains("private key")
        || lower.contains("-----begin private key-----")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("github_pat_")
        || lower.contains("xoxb-")
        || lower.contains("xoxp-")
        || lower.contains("ya29.")
}

fn contains_control(text: &str) -> bool {
    text.chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn starts_with_unix_absolute_path(text: &str) -> bool {
    text.starts_with('/') && text.len() > 1
}

fn contains_windows_drive_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    })
}
