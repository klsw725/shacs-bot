use serde_json::Value;
use shacs_redaction::redact_string;

use super::{WebExtraction, WebValueKind};

/// Longest phrase a text condition can hold.
pub(super) const MAX_PHRASE_CHARS: usize = 64;
pub(super) const MAX_ANCHOR_CHARS: usize = 160;
const WINDOW_CHARS: usize = 1_024;
const MAX_WORD_CHARS: usize = 256;

/// An extraction as shown outside the session file: its phrase and anchors made safe like
/// other user and page text. The stored extraction keeps them as matched on the page.
pub(super) fn safe_extraction(extraction: &WebExtraction) -> WebExtraction {
    let mut shown = extraction.clone();
    shown.prefix = safe_text(&extraction.prefix, MAX_ANCHOR_CHARS);
    shown.suffix = safe_text(&extraction.suffix, MAX_ANCHOR_CHARS);
    if let WebValueKind::Text { phrase } = &mut shown.value {
        *phrase = safe_text(phrase, MAX_PHRASE_CHARS);
    }
    shown
}

/// User text made safe to keep in notification facts and to mail: secrets redacted, every URL
/// reduced to scheme, host and path, on one line, cut to `max_chars`. Only whole words of the
/// first `WINDOW_CHARS` characters are read and an oversized word is replaced, so the work is
/// bounded however long the text is.
pub(super) fn safe_text(text: &str, max_chars: usize) -> String {
    let mut characters = text.chars();
    let head: String = characters.by_ref().take(WINDOW_CHARS).collect();
    let cut = characters.next().is_some();
    let mut words: Vec<&str> = head.split_whitespace().collect();
    if cut && !head.ends_with(char::is_whitespace) {
        words.pop();
    }
    let kept = words
        .into_iter()
        .map(|word| {
            if word.chars().count() > MAX_WORD_CHARS {
                "[REDACTED]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let safe = redact_string(&kept)
        .split_whitespace()
        .map(safe_word)
        .collect::<Vec<_>>()
        .join(" ");
    if !cut && safe.chars().count() <= max_chars {
        return safe;
    }
    let mut bounded: String = safe.chars().take(max_chars.saturating_sub(1)).collect();
    bounded.push('…');
    bounded
}

/// Text for diagnostics and exports: every address, with or without a scheme, keeps only its
/// scheme, host and path (the word rule of [`safe_text`]); other words, mail addresses and the
/// spacing stay as they are.
pub fn without_url_details(text: &str) -> String {
    text.split_inclusive(char::is_whitespace)
        .map(|piece| {
            let word = piece.trim_end();
            format!("{}{}", safe_word(word), &piece[word.len()..])
        })
        .collect()
}

/// Every text in `value` through [`without_url_details`], object keys included (the model can
/// write an argument name, too); a text that holds a JSON object or array (stored tool call
/// arguments, tool outputs) is changed inside and kept as JSON text. Keys that become the same
/// text are told apart by a count, so no entry is lost.
pub(super) fn without_url_details_in(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = match serde_json::from_str::<Value>(text) {
                Ok(mut parsed) if parsed.is_object() || parsed.is_array() => {
                    without_url_details_in(&mut parsed);
                    parsed.to_string()
                }
                _ => without_url_details(text),
            };
        }
        Value::Array(items) => items.iter_mut().for_each(without_url_details_in),
        Value::Object(object) => {
            for (key, mut item) in std::mem::take(object) {
                without_url_details_in(&mut item);
                let shown = without_url_details(&key);
                let mut unique = shown.clone();
                let mut count = 1;
                while object.contains_key(&unique) {
                    count += 1;
                    unique = format!("{shown} ({count})");
                }
                object.insert(unique, item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// A word with a URL keeps the text before it and the URL's scheme, host and path, without
/// userinfo, query or fragment; a URL that cannot be reduced that way is replaced whole.
fn safe_word(word: &str) -> String {
    let Some(separator) = word.find("://") else {
        return schemeless_word(word);
    };
    let scheme_start = word[..separator]
        .char_indices()
        .rev()
        .find(|(_, character)| !(character.is_ascii_alphanumeric() || "+-.".contains(*character)))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let rest = &word[separator + 3..];
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if scheme_start == separator || host.is_empty() || path.contains("://") || path.contains('@') {
        return format!("{}[REDACTED]", &word[..scheme_start]);
    }
    format!("{}://{host}{path}", &word[..separator])
}

/// A word without a scheme that still reads as an address: one with userinfo (`user:pw@host`,
/// `//user@host`, `user@host/path`) is replaced whole, and a query or fragment after a host or
/// path is cut. A bare mail address stays.
fn schemeless_word(word: &str) -> String {
    if word.rsplit_once('@').is_some_and(|(before, after)| {
        before.contains([':', '/']) || after.contains(['/', '?', '#'])
    }) {
        return "[REDACTED]".to_owned();
    }
    match word.find(['?', '#']) {
        Some(cut) if word[..cut].contains(['.', '/']) => word[..cut].to_owned(),
        _ => word.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_text, without_url_details_in};
    use serde_json::Value;

    #[test]
    fn kept_json_keys_lose_their_url_details_and_no_entry_is_lost() -> Result<(), serde_json::Error>
    {
        let mut value = serde_json::json!({
            "arguments": "{\"https://shop.example/item?coupon=priv4te#frag\":\"x\",\
                          \"https://shop.example/item?coupon=other\":\"y\"}",
            "https://a.test/p?k=v": {"name": "https://b.test/q?r=s"}
        });

        without_url_details_in(&mut value);

        let shown = value.to_string();
        for secret in ["priv4te", "frag", "coupon", "k=v", "r=s"] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        let arguments: Value =
            serde_json::from_str(value["arguments"].as_str().unwrap_or_default())?;
        let mut names = arguments
            .as_object()
            .map(|arguments| arguments.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        names.sort();
        assert_eq!(
            names,
            ["https://shop.example/item", "https://shop.example/item (2)"]
        );
        assert_eq!(value["https://a.test/p"]["name"], "https://b.test/q");
        Ok(())
    }

    #[test]
    fn kept_text_has_no_credentials_url_query_or_fragment() {
        let text = safe_text(
            "watch (https://alice:s3cr3t@example.test/path?coupon=priv4te#checkout-frag)\nand \
             ftp://files.test/a?b, Authorization: Bearer abc.def.ghi, ://odd and \
             https://a.test/p/https://bob:pw9@c.test Authorization: Basic dXNlcjpwYXNz",
            240,
        );

        for secret in [
            "alice",
            "s3cr3t",
            "coupon",
            "priv4te",
            "checkout",
            "abc.def.ghi",
            "bob",
            "pw9",
            "dXNlcjpwYXNz",
        ] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        assert!(text.contains("(https://example.test/path"), "{text}");
        assert!(text.contains("ftp://files.test/a"), "{text}");
        assert!(!text.contains('\n'));
    }

    #[test]
    fn a_url_after_multibyte_text_is_reduced_or_replaced_without_panic() {
        assert_eq!(
            safe_text("목표https://a.test/p/https://b.test", 240),
            "목표[REDACTED]"
        );
        assert_eq!(
            safe_text("목표:https://user:pw@a.test/p?q=1", 240),
            "목표:https://a.test/p"
        );
    }

    #[test]
    fn an_address_without_a_scheme_loses_its_userinfo_query_and_fragment() {
        assert_eq!(
            safe_text(
                "shop.example/item?coupon=priv4te#frag u:pw9@host.test/p //v:pw8@h.test \
                 //alice@shop.example/item?c=x#f bob@shop.example/item why? C# #tag a@b.test",
                240
            ),
            "shop.example/item [REDACTED] [REDACTED] [REDACTED] [REDACTED] why? C# #tag a@b.test"
        );
    }

    #[test]
    fn only_the_first_window_is_read() {
        assert_eq!(safe_text(&"x".repeat(100_000), 240), "…");
        assert_eq!(safe_text(&format!("{}goal", " ".repeat(100_000)), 240), "…");
        let words = safe_text(&"word ".repeat(100_000), 240);
        assert_eq!(words.chars().count(), 240);
        assert!(words.ends_with('…'));
    }
}
