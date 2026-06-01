use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::redaction::{redact_string, redact_value};

const TOOL_PROGRESS_QUERY_MAX_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressEventStatus {
    Started,
    Ok,
    Error,
    Waiting,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolProgressEvent {
    pub name: String,
    pub status: ProgressEventStatus,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolProgressPayload {
    pub version: u8,
    pub phase: String,
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
    #[serde(default)]
    pub files: Vec<Value>,
    #[serde(default)]
    pub embeds: Vec<Value>,
}

pub fn build_tool_event_start_payload(
    name: impl Into<String>,
    detail: impl Into<String>,
) -> ToolProgressEvent {
    ToolProgressEvent {
        name: name.into(),
        status: ProgressEventStatus::Started,
        detail: detail.into(),
        metadata: None,
    }
}

pub fn build_tool_progress_start_payload(
    call_id: impl Into<String>,
    name: impl Into<String>,
    arguments: Value,
) -> ToolProgressPayload {
    ToolProgressPayload {
        version: 1,
        phase: "start".to_owned(),
        call_id: call_id.into(),
        name: name.into(),
        arguments,
        result: None,
        error: None,
        files: Vec::new(),
        embeds: Vec::new(),
    }
}

pub fn project_tool_progress_arguments(name: &str, arguments: &Value) -> Value {
    match name {
        "tool_search" => project_tool_search_arguments(arguments),
        "tool_describe" | "tool_call" => project_named_bridge_arguments(arguments),
        _ => redact_value(arguments),
    }
}

fn project_tool_search_arguments(arguments: &Value) -> Value {
    let mut projected = Map::new();
    if let Some(query) = string_argument(arguments, "query") {
        projected.insert(
            "query".to_owned(),
            Value::String(redacted_bounded_text(query, TOOL_PROGRESS_QUERY_MAX_CHARS)),
        );
    }
    if let Some(limit) = arguments
        .as_object()
        .and_then(|arguments| arguments.get("limit"))
        .and_then(Value::as_u64)
    {
        projected.insert("limit".to_owned(), Value::Number(limit.into()));
    }
    Value::Object(projected)
}

fn project_named_bridge_arguments(arguments: &Value) -> Value {
    let mut projected = Map::new();
    if let Some(name) = string_argument(arguments, "name") {
        projected.insert("name".to_owned(), Value::String(redact_string(name)));
    }
    Value::Object(projected)
}

fn string_argument<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments
        .as_object()
        .and_then(|arguments| arguments.get(key))
        .and_then(Value::as_str)
}

fn redacted_bounded_text(value: &str, max_chars: usize) -> String {
    let redacted = redact_string(value.trim());
    if redacted.chars().count() <= max_chars {
        return redacted;
    }
    let mut bounded = redacted.chars().take(max_chars).collect::<String>();
    bounded.push_str("...");
    bounded
}

pub fn build_tool_event_finish_payload(
    name: impl Into<String>,
    ok: bool,
    detail: impl Into<String>,
) -> ToolProgressEvent {
    ToolProgressEvent {
        name: name.into(),
        status: if ok {
            ProgressEventStatus::Ok
        } else {
            ProgressEventStatus::Error
        },
        detail: detail.into(),
        metadata: None,
    }
}

pub fn build_tool_progress_finish_payload(
    call_id: impl Into<String>,
    name: impl Into<String>,
    arguments: Value,
    result: Value,
    ok: bool,
    detail: impl Into<String>,
) -> ToolProgressPayload {
    let (files, embeds) = tool_event_result_extras(&result);
    ToolProgressPayload {
        version: 1,
        phase: if ok { "end" } else { "error" }.to_owned(),
        call_id: call_id.into(),
        name: name.into(),
        arguments,
        result: ok.then_some(result.clone()),
        error: if ok {
            None
        } else {
            result
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(|text| text.trim().to_owned())
                .or_else(|| Some(detail.into()))
        },
        files,
        embeds,
    }
}

pub fn tool_event_result_extras(result: &Value) -> (Vec<Value>, Vec<Value>) {
    let files = result
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let embeds = result
        .get("embeds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    (files, embeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_structured_progress_payloads() {
        assert_eq!(
            build_tool_event_start_payload("read_file", "a").status,
            ProgressEventStatus::Started
        );
        assert_eq!(
            build_tool_event_finish_payload("read_file", true, "done").status,
            ProgressEventStatus::Ok
        );
        let start = build_tool_progress_start_payload(
            "call-1",
            "read_file",
            serde_json::json!({"path": "a"}),
        );
        assert_eq!(start.version, 1);
        assert_eq!(start.phase, "start");

        let finish = build_tool_progress_finish_payload(
            "call-1",
            "read_file",
            serde_json::json!({}),
            serde_json::json!({"files": ["a.txt"], "embeds": [{"type": "text"}]}),
            true,
            "done",
        );
        assert_eq!(finish.phase, "end");
        assert_eq!(finish.files, vec![serde_json::json!("a.txt")]);
        assert_eq!(finish.embeds, vec![serde_json::json!({"type": "text"})]);
    }

    #[test]
    fn projects_bridge_tool_arguments_without_nested_payloads() {
        let search = project_tool_progress_arguments(
            "tool_search",
            &serde_json::json!({
                "query": "token=SECRET_VALUE read files",
                "limit": 7,
                "ignored": {"secret": "SECRET_VALUE"}
            }),
        );
        assert_eq!(
            search.get("query"),
            Some(&serde_json::json!("token=[REDACTED] read files"))
        );
        assert_eq!(search.get("limit"), Some(&serde_json::json!(7)));
        assert!(search.get("ignored").is_none());

        let describe = project_tool_progress_arguments(
            "tool_describe",
            &serde_json::json!({"name": "mcp_demo", "schema": "RAW_SCHEMA"}),
        );
        assert_eq!(describe, serde_json::json!({"name": "mcp_demo"}));

        let call = project_tool_progress_arguments(
            "tool_call",
            &serde_json::json!({
                "name": "mcp_demo",
                "arguments": {"password": "SECRET_VALUE"}
            }),
        );
        assert_eq!(call, serde_json::json!({"name": "mcp_demo"}));
    }

    #[test]
    fn projects_non_bridge_arguments_with_existing_redaction() {
        let projected = project_tool_progress_arguments(
            "read_file",
            &serde_json::json!({"path": "README.md", "api_key": "SECRET_VALUE"}),
        );
        assert_eq!(projected.get("path"), Some(&serde_json::json!("README.md")));
        assert_eq!(
            projected.get("api_key"),
            Some(&serde_json::json!("[REDACTED]"))
        );
    }
}
