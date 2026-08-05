use shacs_redaction::redact_string;
use std::path::Path;

pub(super) fn sanitized_summary(value: &str) -> Result<String, ()> {
    let redacted = redact_string(value);
    if contains_projection_unsafe_text(&redacted) {
        Err(())
    } else {
        Ok(redacted)
    }
}

pub(super) fn validate_opaque_ref(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > 128
        || redact_string(value) != value
        || contains_projection_unsafe_text(value)
        || value.chars().any(is_opaque_ref_forbidden_char)
    {
        Err(())
    } else {
        Ok(())
    }
}

fn is_opaque_ref_forbidden_char(character: char) -> bool {
    character.is_whitespace()
        || character.is_control()
        || matches!(character, '/' | '\\' | '{' | '}' | '[' | ']' | '"' | '\'')
}

fn contains_projection_unsafe_text(value: &str) -> bool {
    value
        .split_whitespace()
        .any(token_contains_absolute_host_path)
        || contains_credential_url(value)
        || contains_raw_projection_material_label(value)
}

fn token_contains_absolute_host_path(token: &str) -> bool {
    let candidate = trim_punctuation(token);
    is_absolute_host_path(candidate)
        || candidate
            .split_once('=')
            .is_some_and(|(_, value)| is_absolute_host_path(trim_punctuation(value)))
        || candidate.split_once(':').is_some_and(|(label, value)| {
            label.len() != 1 && is_absolute_host_path(trim_punctuation(value))
        })
}

fn trim_punctuation(value: &str) -> &str {
    value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    })
}

fn is_absolute_host_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        || (value.len() >= 3
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'\\' | b'/'))
}

fn contains_credential_url(value: &str) -> bool {
    value.split_whitespace().any(|part| {
        part.split_once("://").is_some_and(|(_, rest)| {
            rest.split_once('/')
                .map_or(rest, |(authority, _)| authority)
                .contains('@')
        })
    })
}

fn contains_raw_projection_material_label(value: &str) -> bool {
    value.split_whitespace().any(token_has_raw_material_label)
}

fn token_has_raw_material_label(token: &str) -> bool {
    let token = trim_punctuation(token);
    raw_material_label_matches(token)
        || token
            .split_once('=')
            .is_some_and(|(label, _)| raw_material_label_matches(label))
        || token
            .split_once(':')
            .is_some_and(|(label, _)| raw_material_label_matches(label))
}

fn raw_material_label_matches(label: &str) -> bool {
    let normalized: String = label
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "pid"
            | "processhandle"
            | "processid"
            | "rawproviderpayload"
            | "rawtoolpayload"
            | "providerpayload"
            | "toolpayload"
            | "rawstdout"
            | "rawstderr"
            | "promptbytes"
            | "mediabytes"
    )
}
