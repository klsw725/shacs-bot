use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ask::{pending_ask_user_id, tool_call_arguments};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExternalActionCategory {
    Reserve,
    Purchase,
    Send,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalActionDraft {
    pub work_id: String,
    pub category: ExternalActionCategory,
    pub summary: String,
    pub reference: String,
}

impl ExternalActionDraft {
    pub fn parse(value: Value) -> Option<Self> {
        let mut draft: Self = serde_json::from_value(value).ok()?;
        if !safe_ref(&draft.work_id)
            || !safe_ref(&draft.reference)
            || draft.summary.trim().is_empty()
            || draft.summary.chars().count() > 240
            || draft.summary.chars().any(char::is_control)
        {
            return None;
        }
        draft.summary = shacs_redaction::redact_string(draft.summary.trim());
        draft.reference = shacs_redaction::redact_string(&draft.reference);
        Some(draft)
    }
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "object",
        "description": "Required for a proposed externally committing reserve/purchase/send action on current entrusted work. This pauses for a proposal-bound decision, never grants tool permission. Use a short non-sensitive summary and opaque reference, not raw payloads or credentials.",
        "properties": {
            "work_id": {"type":"string", "minLength":1, "maxLength":160},
            "category": {"type":"string", "enum":["reserve", "purchase", "send"]},
            "summary": {"type":"string", "minLength":1, "maxLength":240},
            "reference": {"type":"string", "minLength":1, "maxLength":160}
        },
        "required": ["work_id", "category", "summary", "reference"],
        "additionalProperties": false
    })
}

pub(crate) fn pending_call(history: &[Value]) -> Option<(String, Value)> {
    let pending_id = pending_ask_user_id(history)?;
    history
        .iter()
        .rev()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .flat_map(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find(|call| call.get("id").and_then(Value::as_str) == Some(&pending_id))
        .and_then(|call| tool_call_arguments(call).remove("external_action"))
        .map(|value| (pending_id, value))
}

pub(crate) fn is_command(content: &str) -> bool {
    content.split_whitespace().next() == Some("/action")
}

fn safe_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":_-.".contains(&byte))
}
