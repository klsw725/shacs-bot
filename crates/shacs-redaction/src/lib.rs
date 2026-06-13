use serde_json::{Map, Value};

pub const REDACTED: &str = "[REDACTED]";

const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "api_key",
    "apikey",
    "token",
    "bearer",
    "access_token",
    "refresh_token",
    "authorization",
    "password",
    "passwd",
    "secret",
    "credential",
    "credentials",
    "smtp_password",
    "smtp_username",
    "imap_password",
    "imap_username",
    "client_secret",
    "private_key",
    "cookie",
    "set_cookie",
];

const SECRET_PATH_FRAGMENTS: &[&str] = &[
    "/.env",
    ".env",
    "/credentials",
    "credentials",
    "/credential",
    "credential",
    "/secrets",
    "secrets",
    "/secret",
    "secret",
    "/auth",
    "auth",
    "/token",
    "token",
    "/oauth",
    "oauth",
    "/.ssh/",
    ".ssh/",
];

pub fn redact_value(value: &Value) -> Value {
    redact_value_with_key(None, value)
}

pub fn redact_string(value: &str) -> String {
    if looks_like_secret_path(value) || contains_private_key_block(value) {
        return REDACTED.to_owned();
    }
    let value = redact_sensitive_key_values(value);
    let value = redact_inline_env_assignments(&value);
    let value = redact_bearer_tokens(&value);
    redact_token_prefixes(&value)
}

pub fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    SECRET_KEY_FRAGMENTS
        .iter()
        .any(|fragment| normalized.contains(fragment))
}

fn redact_value_with_key(key: Option<&str>, value: &Value) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String(REDACTED.to_owned());
    }
    match value {
        Value::Object(object) => Value::Object(redact_object(object)),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::String(text) => Value::String(redact_string(text)),
        other => other.clone(),
    }
}

fn redact_object(object: &Map<String, Value>) -> Map<String, Value> {
    object
        .iter()
        .map(|(key, value)| (key.clone(), redact_value_with_key(Some(key), value)))
        .collect()
}

fn looks_like_secret_value(value: &str) -> bool {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    contains_sensitive_assignment(&lower)
        || lower.contains("bearer ")
        || lower.contains("basic ")
        || lower.contains("token ")
        || lower.contains("sk-")
        || lower.contains("xoxb-")
        || lower.contains("xoxp-")
        || lower.contains("ghp_")
        || lower.contains("github_pat_")
        || lower.contains("ya29.")
        || lower.contains("authorization: bearer")
        || contains_private_key_block(value)
        || lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || lower.starts_with("token ")
        || lower.starts_with("sk-")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("ya29.")
        || lower.contains("authorization: bearer")
}

fn contains_private_key_block(value: &str) -> bool {
    value
        .to_ascii_lowercase()
        .contains("-----begin private key-----")
}

fn looks_like_secret_path(value: &str) -> bool {
    if !(value.starts_with('/') || value.starts_with('~') || value.starts_with('.')) {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    SECRET_PATH_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(fragment))
}

fn contains_sensitive_assignment(lower: &str) -> bool {
    SECRET_KEY_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(&format!("{fragment}=")))
}

fn redact_sensitive_key_values(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        if let Some(match_) = parse_sensitive_key_value(value, index) {
            redacted.push_str(&value[index..match_.value_start]);
            redacted.push_str(REDACTED);
            index = match_.value_end;
        } else {
            let Some(character) = value[index..].chars().next() else {
                break;
            };
            redacted.push(character);
            index += character.len_utf8();
        }
    }
    redacted
}

struct SensitiveKeyValueMatch {
    value_start: usize,
    value_end: usize,
}

fn parse_sensitive_key_value(value: &str, start: usize) -> Option<SensitiveKeyValueMatch> {
    let mut index = start;
    let key_quote = value[index..]
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''));
    if let Some(quote) = key_quote {
        index += quote.len_utf8();
    }

    let key_start = index;
    while let Some(character) = value[index..].chars().next() {
        if !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-')) {
            break;
        }
        index += character.len_utf8();
    }
    if index == key_start {
        return None;
    }
    let key = &value[key_start..index];
    if let Some(quote) = key_quote {
        if !value[index..].starts_with(quote) {
            return None;
        }
        index += quote.len_utf8();
    }
    if !is_redactable_key_value_key(key) {
        return None;
    }

    index = skip_horizontal_whitespace(value, index);
    let separator = value[index..].chars().next()?;
    if !matches!(separator, ':' | '=') {
        return None;
    }
    index += separator.len_utf8();
    index = skip_horizontal_whitespace(value, index);

    let value_quote = value[index..]
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''));
    if let Some(quote) = value_quote {
        index += quote.len_utf8();
    }
    let value_start = index;
    let value_end = if let Some(quote) = value_quote {
        find_quoted_value_end(value, index, quote)
    } else {
        find_unquoted_value_end(value, index)
    };
    (value_end > value_start).then_some(SensitiveKeyValueMatch {
        value_start,
        value_end,
    })
}

fn is_redactable_key_value_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    is_sensitive_key(key) && !normalized.contains("authorization") && !normalized.contains("bearer")
}

fn skip_horizontal_whitespace(value: &str, mut index: usize) -> usize {
    while let Some(character) = value[index..].chars().next() {
        if !matches!(character, ' ' | '\t') {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn find_quoted_value_end(value: &str, mut index: usize, quote: char) -> usize {
    while let Some(character) = value[index..].chars().next() {
        if character == quote {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn find_unquoted_value_end(value: &str, mut index: usize) -> usize {
    while let Some(character) = value[index..].chars().next() {
        if character.is_whitespace() || matches!(character, ',' | '}' | ']') {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn redact_inline_env_assignments(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut token = String::new();
    for character in value.chars() {
        if character.is_whitespace() {
            redacted.push_str(&redact_assignment_token(&token));
            token.clear();
            redacted.push(character);
        } else {
            token.push(character);
        }
    }
    redacted.push_str(&redact_assignment_token(&token));
    redacted
}

fn redact_assignment_token(token: &str) -> String {
    let Some((key, rest)) = token.split_once('=') else {
        return token.to_owned();
    };
    if rest.contains(REDACTED) {
        return token.to_owned();
    }
    if is_sensitive_key(key) || looks_like_secret_value(rest) || looks_like_secret_path(rest) {
        format!("{key}={REDACTED}")
    } else {
        token.to_owned()
    }
}

fn redact_bearer_tokens(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut remainder = value;
    loop {
        let lower = remainder.to_ascii_lowercase();
        let Some(index) = lower.find("bearer ") else {
            redacted.push_str(remainder);
            break;
        };
        let after_bearer = index + "bearer".len();
        redacted.push_str(&remainder[..after_bearer]);
        let mut token_start = after_bearer;
        for character in remainder[after_bearer..].chars() {
            if !character.is_whitespace() {
                break;
            }
            redacted.push(character);
            token_start += character.len_utf8();
        }
        let token_len = remainder[token_start..]
            .chars()
            .take_while(|character| is_secret_token_character(*character))
            .map(char::len_utf8)
            .sum::<usize>();
        if token_len == 0 {
            remainder = &remainder[token_start..];
        } else {
            redacted.push_str(REDACTED);
            remainder = &remainder[token_start + token_len..];
        }
    }
    redacted
}

fn redact_token_prefixes(value: &str) -> String {
    let prefixes = ["sk-", "xoxb-", "xoxp-", "ghp_", "github_pat_", "ya29."];
    let mut redacted = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        let tail = &value[index..];
        let Some(prefix) = prefixes
            .iter()
            .find(|prefix| tail.to_ascii_lowercase().starts_with(**prefix))
        else {
            let Some(character) = tail.chars().next() else {
                break;
            };
            redacted.push(character);
            index += character.len_utf8();
            continue;
        };
        let token_len = tail
            .chars()
            .take_while(|character| is_secret_token_character(*character))
            .map(char::len_utf8)
            .sum::<usize>();
        if token_len <= prefix.len() {
            redacted.push_str(prefix);
            index += prefix.len();
        } else {
            redacted.push_str(REDACTED);
            index += token_len;
        }
    }
    redacted
}

fn is_secret_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_recursive_secret_keys_values_and_paths() {
        let redacted = redact_value(&json!({
            "api_key": "sk-secret",
            "nested": {
                "headers": { "authorization": "Bearer token" },
                "log": "SMTP_PASSWORD=hunter2 ok=true",
                "path": "/Users/me/.config/shacs/credentials.json",
                "relative_path": "./.env",
                "cookie": "session=secret",
                "message": "provider failed with sk-embedded-secret in body"
            }
        }));

        assert_eq!(redacted["api_key"], REDACTED);
        assert_eq!(redacted["nested"]["headers"]["authorization"], REDACTED);
        assert_eq!(
            redacted["nested"]["log"],
            format!("SMTP_PASSWORD={REDACTED} ok=true")
        );
        assert_eq!(redacted["nested"]["path"], REDACTED);
        assert_eq!(redacted["nested"]["relative_path"], REDACTED);
        assert_eq!(redacted["nested"]["cookie"], REDACTED);
        assert_eq!(
            redacted["nested"]["message"],
            format!("provider failed with {REDACTED} in body")
        );
    }

    #[test]
    fn redact_string_preserves_non_secret_evidence() {
        let assignment = redact_string("OPENAI_API_KEY=sk-secret-token visible text");
        assert_eq!(
            assignment,
            format!("OPENAI_API_KEY={REDACTED} visible text")
        );
        assert!(!assignment.contains("sk-secret-token"));

        let bearer = redact_string("before Authorization: Bearer ghp_secret_token after");
        assert_eq!(
            bearer,
            format!("before Authorization: Bearer {REDACTED} after")
        );
        assert!(!bearer.contains("ghp_secret_token"));

        let multiline = redact_string("first line\nOPENAI_API_KEY=sk-secret-token\nlast line");
        assert_eq!(
            multiline,
            format!("first line\nOPENAI_API_KEY={REDACTED}\nlast line")
        );
        assert!(!multiline.contains("sk-secret-token"));
    }

    #[test]
    fn redact_string_redacts_json_like_sensitive_key_values() {
        let quoted = redact_string(r#"before {"api_key":"plain-secret"} after"#);
        assert_eq!(
            quoted,
            format!(r#"before {{"api_key":"{REDACTED}"}} after"#)
        );
        assert!(!quoted.contains("plain-secret"));

        let unquoted = redact_string(r#"client_secret: "plain-secret" ok=true"#);
        assert_eq!(unquoted, format!(r#"client_secret: "{REDACTED}" ok=true"#));
        assert!(!unquoted.contains("plain-secret"));

        let single_quoted = redact_string("token='plain-secret' ok=true");
        assert_eq!(single_quoted, format!("token='{REDACTED}' ok=true"));
        assert!(!single_quoted.contains("plain-secret"));
    }

    #[test]
    fn redact_string_still_fully_redacts_sensitive_paths_and_private_keys() {
        assert_eq!(redact_string("/Users/me/.env"), REDACTED);
        assert_eq!(
            redact_string("-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----"),
            REDACTED
        );
    }
}
