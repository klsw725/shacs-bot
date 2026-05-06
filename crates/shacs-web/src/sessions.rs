use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::media::augment_media_urls;

pub const WEBUI_SESSION_PREFIX: &str = "websocket:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSummary {
    pub key: String,
    pub channel: String,
    pub chat_id: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub preview: String,
}

pub fn is_webui_session_key(key: &str) -> bool {
    key.starts_with(WEBUI_SESSION_PREFIX)
}

pub fn sanitize_session_list(rows: &[Value]) -> Vec<ChatSummary> {
    rows.iter()
        .filter_map(|row| {
            let object = row.as_object()?;
            let key = object.get("key")?.as_str()?;
            if !is_webui_session_key(key) {
                return None;
            }
            let (channel, chat_id) = split_key(key);
            Some(ChatSummary {
                key: key.to_owned(),
                channel,
                chat_id,
                created_at: optional_string(object, "created_at"),
                updated_at: optional_string(object, "updated_at"),
                preview: optional_string(object, "preview").unwrap_or_default(),
            })
        })
        .collect()
}

pub fn prepare_session_payload(
    mut payload: Value,
    media_root: impl AsRef<std::path::Path>,
    media_secret: &[u8],
) -> Option<Value> {
    let key = payload.get("key").and_then(Value::as_str)?;
    if !is_webui_session_key(key) {
        return None;
    }
    augment_media_urls(&mut payload, media_root, media_secret);
    Some(payload)
}

pub fn decode_api_key(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return None;
                }
                let high = hex_value(bytes[index + 1])?;
                let low = hex_value(bytes[index + 2])?;
                output.push(high << 4 | low);
                index += 3;
            }
            b'+' => {
                output.push(b'+');
                index += 1;
            }
            value => {
                output.push(value);
                index += 1;
            }
        }
    }
    let decoded = String::from_utf8(output).ok()?;
    is_valid_api_session_key(&decoded).then_some(decoded)
}

pub fn is_valid_api_session_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b':' | b'.' | b'-'))
}

fn optional_string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn split_key(key: &str) -> (String, String) {
    key.split_once(':')
        .map(|(channel, chat_id)| (channel.to_owned(), chat_id.to_owned()))
        .unwrap_or_else(|| (String::new(), key.to_owned()))
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn sanitizes_websocket_session_list_and_decodes_keys() {
        let rows = vec![
            json!({"key": "cli:direct", "path": "/secret"}),
            json!({"key": "websocket:abc", "path": "/secret", "preview": "hi"}),
        ];
        let summaries = sanitize_session_list(&rows);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].channel, "websocket");
        assert_eq!(summaries[0].chat_id, "abc");
        assert_eq!(
            decode_api_key("websocket%3Aabc"),
            Some("websocket:abc".to_owned())
        );
        assert_eq!(decode_api_key("bad%ZZ"), None);
        assert_eq!(decode_api_key("websocket%2Fabc"), None);
        assert_eq!(decode_api_key("websocket%0Aabc"), None);
        assert_eq!(decode_api_key(&"a".repeat(129)), None);
        assert!(is_valid_api_session_key("websocket:abc-123.ok"));
    }

    #[test]
    fn prepares_session_payload_with_signed_media_and_filters_non_websocket(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("pic.png"), b"png")?;
        let payload = json!({
            "key": "websocket:abc",
            "messages": [{"role": "user", "media": [root.path().join("pic.png").to_string_lossy()]}]
        });
        let prepared = prepare_session_payload(payload, root.path(), b"secret").expect("payload");
        assert!(prepared["messages"][0].get("media").is_none());
        assert!(prepared["messages"][0]["media_urls"].is_array());
        assert!(
            prepare_session_payload(json!({"key": "cli:direct"}), root.path(), b"secret").is_none()
        );
        Ok(())
    }
}
