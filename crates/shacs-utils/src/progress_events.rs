use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}
