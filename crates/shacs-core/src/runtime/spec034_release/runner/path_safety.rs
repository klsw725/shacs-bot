use super::super::model::Spec034ReleaseArtifactError;
use super::super::artifacts::ArtifactSnapshot;
use serde_json::Value;

mod grammar;
mod encoding;

#[cfg(test)]
pub(super) fn redact_host_paths(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(start) = next_host_path(text, cursor) {
        output.push_str(&text[cursor..start]);
        output.push_str("[REDACTED_PATH]");
        cursor = path_end(text, start);
    }
    output.push_str(&text[cursor..]);
    output
}

pub(super) fn validate_snapshot(
    snapshot: &ArtifactSnapshot,
) -> Result<(), Spec034ReleaseArtifactError> {
    for (name, bytes) in snapshot.files() {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
        if checked_contains_host_path(text)?
            || contains_secret(text)
            || decoded_json_contains_host_path(name, text)?
        {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
    }
    Ok(())
}

fn decoded_json_contains_host_path(
    locator: &str,
    text: &str,
) -> Result<bool, Spec034ReleaseArtifactError> {
    if !locator.ends_with(".json") {
        return Ok(false);
    }
    let value: Value = serde_json::from_str(text).map_err(Spec034ReleaseArtifactError::Json)?;
    value_contains_host_path(&value)
}

fn value_contains_host_path(value: &Value) -> Result<bool, Spec034ReleaseArtifactError> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(false),
        Value::String(text) => checked_contains_host_path(text),
        Value::Array(values) => {
            for value in values {
                if value_contains_host_path(value)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Object(values) => {
            for (key, value) in values {
                if checked_contains_host_path(key)? || value_contains_host_path(value)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

#[cfg(test)]
fn contains_host_path(text: &str) -> bool {
    checked_contains_host_path(text).unwrap_or(true)
}

fn checked_contains_host_path(text: &str) -> Result<bool, Spec034ReleaseArtifactError> {
    if next_host_path(text, 0).is_some() {
        return Ok(true);
    }
    let layers = encoding::decoded_layers(text)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
    Ok(layers
        .iter()
        .any(|decoded| next_host_path(decoded, 0).is_some()))
}

fn contains_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if [
        "authorization: basic ",
        "authorization: bearer ",
        "cookie:",
        "set-cookie:",
        "password=",
        "password:",
        "secret=",
        "secret:",
        "token=",
        "token:",
        "session=",
        "session:",
        "api_key=",
        "api-key:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return true;
    }
    lower.split_whitespace().any(|word| {
        word.split_once("://")
            .and_then(|(_, authority)| authority.split('/').next())
            .is_some_and(|authority| authority.contains('@') && authority.contains(':'))
    })
}

fn next_host_path(text: &str, cursor: usize) -> Option<usize> {
    let remaining = &text[cursor..];
    let mut control_boundary = false;
    for (offset, character) in remaining.char_indices() {
        let start = cursor + offset;
        let boundary = control_boundary || immediate_path_boundary(text, start);
        if is_host_path_at(text, start, character, boundary)
            && !grammar::web_uri_syntax_slash(text, start)
        {
            return Some(start);
        }
        if matches!(character, '\n' | '\r') {
            control_boundary = false;
        } else if ansi_control(character) {
            control_boundary = true;
        }
    }
    None
}

fn is_host_path_at(text: &str, start: usize, character: char, boundary: bool) -> bool {
    let remaining = &text[start..];
    let file_uri = remaining
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"));
    let drive = character.is_ascii_alphabetic() && remaining.as_bytes().get(1) == Some(&b':');
    if !matches!(character, '/' | '\\') && !file_uri && !drive {
        return false;
    }
    if !boundary {
        return false;
    }
    if file_uri {
        let locator = &remaining[7..];
        return locator.starts_with('/')
            || locator.starts_with('\\')
            || locator.contains('/')
            || locator.contains('\\');
    }
    if character == '/' {
        let drive_separator = start >= 2
            && text.as_bytes()[start - 1] == b':'
            && text.as_bytes()[start - 2].is_ascii_alphabetic()
            && path_boundary(text, start - 2);
        if drive_separator {
            return false;
        }
        let run = remaining.bytes().take_while(|byte| *byte == b'/').count();
        return remaining[run..]
            .chars()
            .next()
            .is_some_and(|next| !terminator(next));
    }
    if character == '\\' {
        return remaining[1..]
            .chars()
            .next()
            .is_some_and(|next| !terminator(next));
    }
    drive
        && remaining
            .as_bytes()
            .get(2)
            .is_some_and(|next| matches!(next, b'/' | b'\\'))
}

#[cfg(test)]
fn path_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find_map(|(offset, character)| (offset > 0 && terminator(character)).then_some(start + offset))
        .unwrap_or(text.len())
}

fn path_boundary(text: &str, position: usize) -> bool {
    line_has_ansi_control(&text[..position]) || immediate_path_boundary(text, position)
}

fn immediate_path_boundary(text: &str, position: usize) -> bool {
    text[..position]
        .chars()
        .next_back()
        .map_or(true, |character| !path_token_character(character))
}

fn line_has_ansi_control(prefix: &str) -> bool {
    prefix
        .chars()
        .rev()
        .take_while(|character| !matches!(character, '\n' | '\r'))
        .any(ansi_control)
}

fn ansi_control(character: char) -> bool {
    character == '\u{1b}' || ('\u{80}'..='\u{9f}').contains(&character)
}

fn path_token_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '.' | '~' | '%' | '/' | '\\')
}

fn terminator(character: char) -> bool {
    character.is_whitespace() || character.is_control() || matches!(character, ')' | ']' | '}' | '"' | '\'' | '`')
}

#[cfg(test)]
#[path = "path_safety_test.rs"]
mod tests;
