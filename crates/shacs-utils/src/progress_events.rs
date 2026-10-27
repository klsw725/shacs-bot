use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use shacs_redaction::{redact_string, redact_value};

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
        "propose_web_watch" => project_web_watch_fields(arguments, &WEB_WATCH_PROPOSAL_FIELDS),
        "web_watch_status" => project_web_watch_fields(arguments, &["work_id"]),
        _ => redact_value(arguments),
    }
}

/// A tool's output as progress and diagnostics show it: a web watch tool's output repeats the
/// call's fields, so they are projected like its arguments, and its errors (which can repeat a
/// field the model wrote) keep only that they are errors.
pub fn project_tool_progress_result(name: &str, result: Value) -> Value {
    match (name, result) {
        ("propose_web_watch", Value::Object(mut result)) => {
            if let Some(proposal) = result.get("proposal") {
                let projected = project_web_watch_fields(proposal, &WEB_WATCH_PROPOSAL_FIELDS);
                result.insert("proposal".to_owned(), projected);
            }
            Value::Object(result)
        }
        ("web_watch_status", Value::Object(mut result)) => {
            if let Some(work_id) = result.get("work_id") {
                let shown = web_watch_field("work_id", work_id);
                result.insert("work_id".to_owned(), shown);
            }
            Value::Object(result)
        }
        ("propose_web_watch", Value::String(text)) => {
            refused_or_set(&text, "Error: the web watch proposal was refused")
        }
        ("web_watch_status", Value::String(text)) => {
            refused_or_set(&text, "Error: the web watch status request was refused")
        }
        (_, result) => result,
    }
}

fn refused_or_set(text: &str, refused: &str) -> Value {
    Value::String(
        if text.trim_start().starts_with("Error") {
            refused
        } else {
            "<set>"
        }
        .to_owned(),
    )
}

/// The fields of a web watch proposal, as the `propose_web_watch` tool takes them.
const WEB_WATCH_PROPOSAL_FIELDS: [&str; 17] = [
    "work_id",
    "url",
    "value_label",
    "value_phrase",
    "area_label",
    "area_until",
    "condition",
    "condition_value",
    "notify_email",
    "deadline_at",
    "goal_text",
    "cadence_seconds",
    "failure_threshold_seconds",
    "remind_before_seconds",
    "decimal_comma",
    "remind_now",
    "clear_deadline",
];

/// A web watch tool call's address, email and texts can carry a URL query or fragment, a mail
/// address or words of the user or a page, which diagnostics never show. Numbers and flags
/// stay, and so do the fields that only name a choice: a condition kind or work id written as
/// a plain word, and a threshold written as a number. Every other text only shows that it is
/// set; fields the tool does not take (not in `fields`) are left out.
fn project_web_watch_fields(arguments: &Value, fields: &[&str]) -> Value {
    let projected = arguments
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| fields.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), web_watch_field(key, value)));
    Value::Object(projected.collect())
}

fn web_watch_field(key: &str, value: &Value) -> Value {
    let plain_word = |text: &str| {
        !text.is_empty()
            && text.len() <= 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    };
    let number = |text: &str| {
        !text.is_empty()
            && text.len() <= 32
            && text.bytes().any(|byte| byte.is_ascii_digit())
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b',' | b'-' | b'+'))
    };
    match (key, value) {
        (_, Value::Bool(_) | Value::Number(_) | Value::Null) => value.clone(),
        ("condition" | "work_id", Value::String(text)) if plain_word(text) => {
            Value::String(redact_string(text))
        }
        ("condition_value", Value::String(text)) if number(text) => value.clone(),
        _ => Value::String("<set>".to_owned()),
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
    fn a_web_watch_proposal_shows_no_address_email_or_text() {
        let proposal = serde_json::json!({
            "url": "https://shop.example/item?coupon=private-ref#checkout",
            "goal_text": "PrivateGoal",
            "value_label": "Price",
            "value_phrase": "sale",
            "notify_email": "user@example.test",
            "condition": "at_most",
            "condition_value": "12,500",
            "cadence_seconds": 600,
            "remind_now": false,
            "unknown": "kept-nowhere"
        });
        let expected = serde_json::json!({
            "url": "<set>",
            "goal_text": "<set>",
            "value_label": "<set>",
            "value_phrase": "<set>",
            "notify_email": "<set>",
            "condition": "at_most",
            "condition_value": "12,500",
            "cadence_seconds": 600,
            "remind_now": false
        });

        let arguments = project_tool_progress_arguments("propose_web_watch", &proposal);
        let result = project_tool_progress_result(
            "propose_web_watch",
            serde_json::json!({"kind": "web_watch_proposal", "proposal": proposal}),
        );

        assert_eq!(arguments, expected);
        assert_eq!(result.get("proposal"), Some(&expected));
        assert_eq!(
            project_tool_progress_result(
                "propose_web_watch",
                serde_json::json!("Error: unknown field https://x.example/?k=v")
            ),
            serde_json::json!("Error: the web watch proposal was refused")
        );
        assert_eq!(
            project_tool_progress_result("read_file", serde_json::json!({"proposal": "kept"})),
            serde_json::json!({"proposal": "kept"})
        );
    }

    #[test]
    fn a_web_watch_status_request_shows_only_a_work_id_written_as_a_plain_word() {
        let address = "https://shop.example/item?coupon=private-ref#checkout";

        let plain = project_tool_progress_arguments(
            "web_watch_status",
            &serde_json::json!({"work_id": "work-0a1b2c3d4e5f", "unknown": address}),
        );
        let arguments = project_tool_progress_arguments(
            "web_watch_status",
            &serde_json::json!({"work_id": address}),
        );
        let result = project_tool_progress_result(
            "web_watch_status",
            serde_json::json!({"kind": "web_watch_status_request", "work_id": address}),
        );
        let error = project_tool_progress_result(
            "web_watch_status",
            serde_json::json!(format!("Error: work_id {address}")),
        );

        assert_eq!(plain, serde_json::json!({"work_id": "work-0a1b2c3d4e5f"}));
        assert_eq!(arguments, serde_json::json!({"work_id": "<set>"}));
        assert_eq!(
            result,
            serde_json::json!({"kind": "web_watch_status_request", "work_id": "<set>"})
        );
        assert_eq!(
            error,
            serde_json::json!("Error: the web watch status request was refused")
        );
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
