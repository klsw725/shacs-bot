use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RESTART_NOTIFY_CHANNEL_ENV: &str = "NANOBOT_RESTART_NOTIFY_CHANNEL";
pub const RESTART_NOTIFY_CHAT_ID_ENV: &str = "NANOBOT_RESTART_NOTIFY_CHAT_ID";
pub const RESTART_NOTIFY_METADATA_ENV: &str = "NANOBOT_RESTART_NOTIFY_METADATA";
pub const RESTART_STARTED_AT_ENV: &str = "NANOBOT_RESTART_STARTED_AT";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestartNotice {
    pub channel: String,
    pub chat_id: String,
    pub started_at_raw: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

pub fn format_restart_completed_message(started_at_raw: &str, now_seconds: f64) -> String {
    let elapsed_suffix = started_at_raw
        .parse::<f64>()
        .ok()
        .map(|started| format!(" in {:.1}s", (now_seconds - started).max(0.0)))
        .unwrap_or_default();
    format!("Restart completed{elapsed_suffix}.")
}

pub fn set_restart_notice_to_env(
    channel: &str,
    chat_id: &str,
    metadata: Option<&Map<String, Value>>,
) {
    std::env::set_var(RESTART_NOTIFY_CHANNEL_ENV, channel);
    std::env::set_var(RESTART_NOTIFY_CHAT_ID_ENV, chat_id);
    std::env::set_var(RESTART_STARTED_AT_ENV, now_seconds().to_string());
    if let Some(metadata) = metadata {
        match serde_json::to_string(metadata) {
            Ok(raw) => std::env::set_var(RESTART_NOTIFY_METADATA_ENV, raw),
            Err(_) => std::env::remove_var(RESTART_NOTIFY_METADATA_ENV),
        }
    } else {
        std::env::remove_var(RESTART_NOTIFY_METADATA_ENV);
    }
}

pub fn consume_restart_notice_from_env() -> Option<RestartNotice> {
    let channel = take_env(RESTART_NOTIFY_CHANNEL_ENV).trim().to_owned();
    let chat_id = take_env(RESTART_NOTIFY_CHAT_ID_ENV).trim().to_owned();
    let started_at_raw = take_env(RESTART_STARTED_AT_ENV).trim().to_owned();
    let metadata_raw = take_env(RESTART_NOTIFY_METADATA_ENV).trim().to_owned();
    if channel.is_empty() || chat_id.is_empty() {
        return None;
    }
    let metadata = serde_json::from_str::<Value>(&metadata_raw)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    Some(RestartNotice {
        channel,
        chat_id,
        started_at_raw,
        metadata,
    })
}

pub fn should_show_cli_restart_notice(notice: &RestartNotice, session_id: &str) -> bool {
    if notice.channel != "cli" {
        return false;
    }
    let cli_chat_id = session_id
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(session_id);
    notice.chat_id.is_empty() || notice.chat_id == cli_chat_id
}

fn take_env(name: &str) -> String {
    let value = std::env::var(name).unwrap_or_default();
    std::env::remove_var(name);
    value
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restart_notice_roundtrips_env_and_clears_values() {
        let mut metadata = Map::new();
        metadata.insert("session_key".to_owned(), json!("cli:abc"));
        set_restart_notice_to_env("cli", "abc", Some(&metadata));
        let notice = consume_restart_notice_from_env().expect("notice");
        assert_eq!(notice.channel, "cli");
        assert_eq!(notice.chat_id, "abc");
        assert_eq!(notice.metadata["session_key"], "cli:abc");
        assert!(consume_restart_notice_from_env().is_none());
        assert!(should_show_cli_restart_notice(&notice, "cli:abc"));
        assert!(!should_show_cli_restart_notice(&notice, "cli:other"));
    }

    #[test]
    fn restart_completed_message_includes_elapsed_when_parseable() {
        assert_eq!(
            format_restart_completed_message("10", 12.34),
            "Restart completed in 2.3s."
        );
        assert_eq!(
            format_restart_completed_message("bad", 12.34),
            "Restart completed."
        );
    }
}
