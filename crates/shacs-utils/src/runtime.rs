use crate::text::stringify_text_blocks;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const EMPTY_FINAL_RESPONSE_MESSAGE: &str = "I completed the tool steps but couldn't produce a final answer. Please try again or narrow the task.";
pub const FINALIZATION_RETRY_PROMPT: &str =
    "Please provide your response to the user based on the conversation above.";
pub const LENGTH_RECOVERY_PROMPT: &str = "Output limit reached. Continue exactly where you left off — no recap, no apology. Break remaining work into smaller steps if needed.";
pub const MAX_REPEAT_EXTERNAL_LOOKUPS: usize = 2;

pub fn empty_tool_result_message(tool_name: &str) -> String {
    format!("({tool_name} completed with no output)")
}

pub fn ensure_nonempty_tool_result(tool_name: &str, content: Value) -> Value {
    match &content {
        Value::Null => Value::String(empty_tool_result_message(tool_name)),
        Value::String(text) if text.trim().is_empty() => {
            Value::String(empty_tool_result_message(tool_name))
        }
        Value::Array(values) if values.is_empty() => {
            Value::String(empty_tool_result_message(tool_name))
        }
        Value::Array(values) => stringify_text_blocks(values)
            .filter(|text| text.trim().is_empty())
            .map(|_| Value::String(empty_tool_result_message(tool_name)))
            .unwrap_or(content),
        _ => content,
    }
}

pub fn is_blank_text(content: Option<&str>) -> bool {
    content.map(str::trim).unwrap_or_default().is_empty()
}

pub fn build_finalization_retry_message() -> Value {
    json!({"role": "user", "content": FINALIZATION_RETRY_PROMPT})
}

pub fn build_length_recovery_message() -> Value {
    json!({"role": "user", "content": LENGTH_RECOVERY_PROMPT})
}

pub fn external_lookup_signature(
    tool_name: &str,
    arguments: &Map<String, Value>,
) -> Option<String> {
    if tool_name == "web_fetch" {
        return arguments
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(|url| format!("web_fetch:{}", url.to_ascii_lowercase()));
    }
    if tool_name == "web_search" {
        return arguments
            .get("query")
            .or_else(|| arguments.get("search_term"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(|query| format!("web_search:{}", query.to_ascii_lowercase()));
    }
    None
}

pub fn repeated_external_lookup_error(
    tool_name: &str,
    arguments: &Map<String, Value>,
    seen_counts: &mut BTreeMap<String, usize>,
) -> Option<String> {
    let signature = external_lookup_signature(tool_name, arguments)?;
    let count = seen_counts.entry(signature).or_default();
    *count += 1;
    (*count > MAX_REPEAT_EXTERNAL_LOOKUPS).then(|| {
        "Error: repeated external lookup blocked. Use the results you already have to answer, or try a meaningfully different source.".to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_helpers_replace_empty_tool_results_and_throttle_repeated_lookups() {
        assert_eq!(
            ensure_nonempty_tool_result("x", Value::Null),
            "(x completed with no output)"
        );
        assert_eq!(
            ensure_nonempty_tool_result("x", json!([{"type":"text","text":"   "}])),
            "(x completed with no output)"
        );
        assert_eq!(
            build_finalization_retry_message()["content"],
            FINALIZATION_RETRY_PROMPT
        );
        let args = Map::from_iter([("url".to_owned(), json!("HTTPS://EXAMPLE.COM/A"))]);
        let mut seen = BTreeMap::new();
        assert!(repeated_external_lookup_error("web_fetch", &args, &mut seen).is_none());
        assert!(repeated_external_lookup_error("web_fetch", &args, &mut seen).is_none());
        assert!(repeated_external_lookup_error("web_fetch", &args, &mut seen).is_some());
    }
}
