use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Local, SecondsFormat};
use chrono_tz::Tz;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn strip_think(text: &str) -> String {
    let mut stripped = text.to_owned();
    for pattern in [
        r"(?s)<think>.*?</think>",
        r"(?s)^\s*<think>.*$",
        r"(?s)<thought>.*?</thought>",
        r"(?s)^\s*<thought>.*$",
        r"^\s*</think>\s*",
        r"\s*</think>\s*$",
        r"^\s*</thought>\s*",
        r"\s*</thought>\s*$",
        r"^\s*<\|?channel\|?>\s*",
    ] {
        if let Ok(regex) = Regex::new(pattern) {
            stripped = regex.replace_all(&stripped, "").into_owned();
        }
    }
    stripped = strip_malformed_opening_tag(&stripped, "<think");
    stripped = strip_malformed_opening_tag(&stripped, "<thought");
    stripped.trim().to_owned()
}

pub fn detect_image_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub fn build_image_content_blocks(raw: &[u8], mime: &str, path: &str, label: &str) -> Vec<Value> {
    vec![
        json!({
            "type": "image_url",
            "image_url": {"url": format!("data:{mime};base64,{}", STANDARD.encode(raw))},
            "_meta": {"path": path},
        }),
        json!({"type": "text", "text": label}),
    ]
}

pub fn ensure_dir(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let path = path.as_ref().to_path_buf();
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub fn timestamp_iso() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

pub fn current_time_str(timezone: Option<&str>) -> String {
    if let Some(timezone) = timezone.filter(|value| !value.trim().is_empty()) {
        if let Ok(tz) = timezone.parse::<Tz>() {
            let now = chrono::Utc::now().with_timezone(&tz);
            return format!(
                "{} ({timezone}, UTC{})",
                now.format("%Y-%m-%d %H:%M (%A)"),
                now.format("%:z")
            );
        }
    }
    let now: DateTime<Local> = Local::now();
    let tz_name = timezone
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("local");
    format!(
        "{} ({tz_name}, UTC{})",
        now.format("%Y-%m-%d %H:%M (%A)"),
        now.format("%:z")
    )
}

pub fn safe_filename(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

pub fn image_placeholder_text(path: Option<&str>, empty: &str) -> String {
    path.filter(|value| !value.is_empty())
        .map(|path| format!("[image: {path}]"))
        .unwrap_or_else(|| empty.to_owned())
}

pub fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n... (truncated)");
    truncated
}

pub fn stringify_text_blocks(content: &[Value]) -> Option<String> {
    let mut parts = Vec::new();
    for block in content {
        let object = block.as_object()?;
        if object.get("type").and_then(Value::as_str) != Some("text") {
            return None;
        }
        parts.push(object.get("text")?.as_str()?.to_owned());
    }
    Some(parts.join("\n"))
}

pub fn find_legal_message_start(messages: &[Value]) -> usize {
    let mut declared = std::collections::BTreeSet::new();
    let mut start = 0;
    for (index, message) in messages.iter().enumerate() {
        let Some(object) = message.as_object() else {
            continue;
        };
        match object.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(tool_calls) = object.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                            declared.insert(id.to_owned());
                        }
                    }
                }
            }
            Some("tool") => {
                let tool_call_id = object.get("tool_call_id").and_then(Value::as_str);
                if tool_call_id.is_some_and(|id| !declared.contains(id)) {
                    start = index + 1;
                    declared.clear();
                    for previous in &messages[start..=index] {
                        if previous.get("role").and_then(Value::as_str) == Some("assistant") {
                            if let Some(tool_calls) =
                                previous.get("tool_calls").and_then(Value::as_array)
                            {
                                for call in tool_calls {
                                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                                        declared.insert(id.to_owned());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    start
}

pub fn estimate_message_tokens(message: &Value) -> usize {
    let mut parts = Vec::new();
    collect_message_token_parts(message, &mut parts);
    let payload = parts.join("\n");
    if payload.is_empty() {
        4
    } else {
        (payload.chars().count() / 4 + 4).max(4)
    }
}

pub fn estimate_prompt_tokens(messages: &[Value], tools: Option<&[Value]>) -> usize {
    let mut parts = Vec::new();
    for message in messages {
        collect_message_token_parts(message, &mut parts);
    }
    if let Some(tools) = tools {
        parts.push(Value::Array(tools.to_vec()).to_string());
    }
    parts.join("\n").chars().count() / 4 + messages.len() * 4
}

pub fn split_message(content: &str, max_len: usize) -> Vec<String> {
    if content.is_empty() || max_len == 0 {
        return Vec::new();
    }
    if content.chars().count() <= max_len {
        return vec![content.to_owned()];
    }
    let mut chunks = Vec::new();
    let mut remaining = content.trim_start().to_owned();
    while !remaining.is_empty() {
        if remaining.chars().count() <= max_len {
            chunks.push(remaining);
            break;
        }
        let cut_byte = nth_char_boundary(&remaining, max_len);
        let cut = &remaining[..cut_byte];
        let split_at = cut
            .rfind('\n')
            .filter(|position| *position > 0)
            .or_else(|| cut.rfind(' ').filter(|position| *position > 0))
            .unwrap_or(cut_byte);
        chunks.push(remaining[..split_at].to_owned());
        remaining = remaining[split_at..].trim_start().to_owned();
    }
    chunks
}

pub fn build_assistant_message(
    content: Option<&str>,
    tool_calls: Option<Vec<Value>>,
    reasoning_content: Option<&str>,
    thinking_blocks: Option<Vec<Value>>,
) -> Value {
    let mut message = Map::from_iter([
        ("role".to_owned(), Value::String("assistant".to_owned())),
        (
            "content".to_owned(),
            Value::String(content.unwrap_or_default().to_owned()),
        ),
    ]);
    if let Some(tool_calls) = tool_calls.filter(|calls| !calls.is_empty()) {
        message.insert("tool_calls".to_owned(), Value::Array(tool_calls));
    }
    if reasoning_content.is_some()
        || thinking_blocks
            .as_ref()
            .is_some_and(|blocks| !blocks.is_empty())
    {
        message.insert(
            "reasoning_content".to_owned(),
            Value::String(reasoning_content.unwrap_or_default().to_owned()),
        );
    }
    if let Some(thinking_blocks) = thinking_blocks.filter(|blocks| !blocks.is_empty()) {
        message.insert("thinking_blocks".to_owned(), Value::Array(thinking_blocks));
    }
    Value::Object(message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub version: String,
    pub model: String,
    pub start_time_unix_s: u64,
    pub last_usage: Map<String, Value>,
    pub context_window_tokens: usize,
    pub session_msg_count: usize,
    pub context_tokens_estimate: usize,
    pub search_usage_text: Option<String>,
    pub active_task_count: usize,
    pub max_completion_tokens: usize,
}

pub fn build_status_content(snapshot: &StatusSnapshot) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let uptime_s = now.saturating_sub(snapshot.start_time_unix_s);
    let uptime = if uptime_s >= 3600 {
        format!("{}h {}m", uptime_s / 3600, (uptime_s % 3600) / 60)
    } else {
        format!("{}m {}s", uptime_s / 60, uptime_s % 60)
    };
    let last_in = usage_u64(&snapshot.last_usage, "prompt_tokens");
    let last_out = usage_u64(&snapshot.last_usage, "completion_tokens");
    let cached = usage_u64(&snapshot.last_usage, "cached_tokens");
    let ctx_total = snapshot.context_window_tokens;
    let ctx_budget = ctx_total
        .saturating_sub(snapshot.max_completion_tokens)
        .saturating_sub(1024)
        .max(1);
    let ctx_pct = ((snapshot.context_tokens_estimate as f64 / ctx_budget as f64) * 100.0)
        .floor()
        .min(999.0) as usize;
    let ctx_used = compact_count(snapshot.context_tokens_estimate);
    let ctx_total = if ctx_total > 0 {
        compact_count(ctx_total)
    } else {
        "n/a".to_owned()
    };
    let mut token_line = format!("📊 Tokens: {last_in} in / {last_out} out");
    if cached > 0 && last_in > 0 {
        token_line.push_str(&format!(" ({}% cached)", cached * 100 / last_in));
    }
    let mut lines = vec![
        format!("🐈 shacs-bot v{}", snapshot.version),
        format!("🧠 Model: {}", snapshot.model),
        token_line,
        format!("📚 Context: {ctx_used}/{ctx_total} ({ctx_pct}% of input budget)"),
        format!("💬 Session: {} messages", snapshot.session_msg_count),
        format!("⏱ Uptime: {uptime}"),
        format!("⚡ Tasks: {} active", snapshot.active_task_count),
    ];
    if let Some(search_usage_text) = &snapshot.search_usage_text {
        if !search_usage_text.trim().is_empty() {
            lines.push(search_usage_text.clone());
        }
    }
    lines.join("\n")
}

pub trait PromptTokenEstimator {
    fn estimate_prompt_tokens(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        model: Option<&str>,
    ) -> Result<Option<(usize, String)>, String>;
}

pub fn estimate_prompt_tokens_chain(
    provider: Option<&impl PromptTokenEstimator>,
    model: Option<&str>,
    messages: &[Value],
    tools: Option<&[Value]>,
) -> (usize, String) {
    if let Some(provider) = provider {
        if let Ok(Some((tokens, source))) = provider.estimate_prompt_tokens(messages, tools, model)
        {
            if tokens > 0 {
                return (tokens, non_empty_source(&source));
            }
        }
    }
    let estimated = estimate_prompt_tokens(messages, tools);
    if estimated > 0 {
        (estimated, "heuristic".to_owned())
    } else {
        (0, "none".to_owned())
    }
}

fn usage_u64(usage: &Map<String, Value>, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn compact_count(value: usize) -> String {
    if value >= 1000 {
        format!("{}k", value / 1000)
    } else {
        value.to_string()
    }
}

fn non_empty_source(source: &str) -> String {
    if source.trim().is_empty() {
        "provider_counter".to_owned()
    } else {
        source.to_owned()
    }
}

fn strip_malformed_opening_tag(text: &str, tag: &str) -> String {
    let mut remaining = text;
    let mut output = String::new();
    while let Some(index) = remaining.find(tag) {
        output.push_str(&remaining[..index]);
        let after_tag = &remaining[index + tag.len()..];
        let should_strip = after_tag
            .chars()
            .next()
            .map_or(true, |next| !is_valid_tag_continuation(next));
        if should_strip {
            remaining = after_tag;
        } else {
            output.push_str(tag);
            remaining = after_tag;
        }
    }
    output.push_str(remaining);
    output
}

fn collect_message_token_parts(message: &Value, parts: &mut Vec<String>) {
    let Some(object) = message.as_object() else {
        return;
    };
    match object.get("content") {
        Some(Value::String(text)) if !text.is_empty() => parts.push(text.clone()),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            parts.push(text.to_owned());
                        }
                    }
                } else {
                    parts.push(block.to_string());
                }
            }
        }
        Some(value) if !value.is_null() => parts.push(value.to_string()),
        _ => {}
    }
    for key in ["tool_calls", "reasoning_content", "name", "tool_call_id"] {
        if let Some(value) = object.get(key) {
            match value {
                Value::String(text) if !text.is_empty() => parts.push(text.clone()),
                Value::Array(items) if !items.is_empty() => parts.push(value.to_string()),
                Value::Object(map) if !map.is_empty() => parts.push(value.to_string()),
                _ => {}
            }
        }
    }
}

fn is_valid_tag_continuation(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | ':' | '>' | '/')
}

fn nth_char_boundary(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_think_removes_blocks_and_malformed_leaks_but_preserves_prose() {
        assert_eq!(strip_think("<think>secret</think> visible"), "visible");
        assert_eq!(strip_think("<think광장 visible"), "광장 visible");
        assert_eq!(
            strip_think("A literal </think> token"),
            "A literal </think> token"
        );
        assert_eq!(strip_think("<|channel|> final"), "final");
        assert_eq!(
            strip_think("mention <think-tag> safely"),
            "mention <think-tag> safely"
        );
    }

    #[test]
    fn text_helpers_detect_images_split_and_build_messages() {
        assert_eq!(
            detect_image_mime(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(detect_image_mime(b"GIF89a..."), Some("image/gif"));
        assert_eq!(safe_filename("a:b/c?d"), "a_b_c_d");
        assert_eq!(
            image_placeholder_text(Some("pic.png"), "[image]"),
            "[image: pic.png]"
        );
        assert_eq!(truncate_text("abcdef", 3), "abc\n... (truncated)");
        assert_eq!(split_message("aa bb cc", 5), vec!["aa", "bb cc"]);
        let message = build_assistant_message(Some("hi"), None, Some("why"), None);
        assert_eq!(message["reasoning_content"], "why");
        assert!(estimate_message_tokens(&message) >= 4);
        assert!(estimate_prompt_tokens(&[message], None) >= 4);
    }

    #[test]
    fn legal_message_start_skips_orphan_tool_results() {
        let messages = vec![
            json!({"role": "tool", "tool_call_id": "orphan", "content": "bad"}),
            json!({"role": "assistant", "tool_calls": [{"id": "known"}]}),
            json!({"role": "tool", "tool_call_id": "known", "content": "ok"}),
        ];
        assert_eq!(find_legal_message_start(&messages), 1);
    }

    #[test]
    fn time_status_and_provider_counter_helpers_match_nanobot_contract() {
        assert!(timestamp_iso().contains('T'));
        assert!(current_time_str(Some("Asia/Seoul")).contains("Asia/Seoul"));

        let snapshot = StatusSnapshot {
            version: "0.1.0".to_owned(),
            model: "test-model".to_owned(),
            start_time_unix_s: 0,
            last_usage: Map::from_iter([
                ("prompt_tokens".to_owned(), Value::from(100_u64)),
                ("completion_tokens".to_owned(), Value::from(25_u64)),
                ("cached_tokens".to_owned(), Value::from(50_u64)),
            ]),
            context_window_tokens: 10_000,
            session_msg_count: 7,
            context_tokens_estimate: 2_000,
            search_usage_text: Some("search usage".to_owned()),
            active_task_count: 2,
            max_completion_tokens: 1_000,
        };
        let status = build_status_content(&snapshot);
        assert!(status.contains("🐈 shacs-bot v0.1.0"));
        assert!(status.contains("50% cached"));
        assert!(status.contains("search usage"));

        struct Counter;
        impl PromptTokenEstimator for Counter {
            fn estimate_prompt_tokens(
                &self,
                _messages: &[Value],
                _tools: Option<&[Value]>,
                _model: Option<&str>,
            ) -> Result<Option<(usize, String)>, String> {
                Ok(Some((42, "custom".to_owned())))
            }
        }
        assert_eq!(
            estimate_prompt_tokens_chain(Some(&Counter), Some("m"), &[], None),
            (42, "custom".to_owned())
        );
    }
}
