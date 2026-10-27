use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shacs_redaction::redact_string;

use super::{JsonMap, Tool, ToolResult};

pub const PROPOSE_WEB_WATCH_TOOL_NAME: &str = "propose_web_watch";
pub const WEB_WATCH_STATUS_TOOL_NAME: &str = "web_watch_status";

/// Whether `name` is a web watch tool, whose arguments and output repeat what the model wrote
/// (which can hold an address with its query), so diagnostics, plugin hooks and session exports
/// show them projected.
pub(crate) fn is_web_watch_tool(name: &str) -> bool {
    name == PROPOSE_WEB_WATCH_TOOL_NAME || name == WEB_WATCH_STATUS_TOOL_NAME
}
const MAX_GOAL_CHARS: usize = 240;
const MAX_FIELD_CHARS: usize = 2_048;
const TEXT_FIELDS: [&str; 11] = [
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
];
const NUMBER_FIELDS: [&str; 3] = [
    "cadence_seconds",
    "failure_threshold_seconds",
    "remind_before_seconds",
];
const FLAG_FIELDS: [&str; 3] = ["decimal_comma", "remind_now", "clear_deadline"];

/// What the model proposes; the runtime checks every field and reads the page itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebWatchProposalDraft {
    pub work_id: Option<String>,
    pub goal_text: Option<String>,
    pub url: Option<String>,
    pub value_label: Option<String>,
    pub value_phrase: Option<String>,
    pub area_label: Option<String>,
    pub area_until: Option<String>,
    #[serde(default)]
    pub decimal_comma: bool,
    pub condition: Option<String>,
    pub condition_value: Option<String>,
    pub cadence_seconds: Option<u64>,
    pub notify_email: Option<String>,
    pub failure_threshold_seconds: Option<u64>,
    pub deadline_at: Option<String>,
    pub remind_before_seconds: Option<u64>,
    #[serde(default)]
    pub remind_now: bool,
    #[serde(default)]
    pub clear_deadline: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProposeWebWatchTool;

impl ProposeWebWatchTool {
    pub const fn new() -> Self {
        Self
    }
}

impl Tool for ProposeWebWatchTool {
    fn name(&self) -> &str {
        PROPOSE_WEB_WATCH_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Propose watching one value on a public web page and emailing when its condition changes, \
         for the user to confirm. Use the page address and email address exactly as the user wrote \
         them, and do not read the page yourself: the runtime reads it and shows the user a summary. \
         Conditions: at_most or at_least with condition_value, below_current or above_current for a \
         number; text_present or text_absent for a phrase. Give work_id with only the fields to \
         change an existing watch. This never sets anything up or grants permission."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "work_id": {"type": "string", "description": "Existing watch to change"},
                "goal_text": {"type": "string"},
                "url": {"type": "string"},
                "value_label": {"type": "string", "description": "Text shown right before the number"},
                "decimal_comma": {"type": "boolean"},
                "value_phrase": {"type": "string", "description": "Phrase to watch for"},
                "area_label": {"type": "string", "description": "Text that starts the phrase's area"},
                "area_until": {"type": "string", "description": "Text that ends the phrase's area"},
                "condition": {"type": "string", "description": "at_most, at_least, below_current, above_current, text_present or text_absent"},
                "condition_value": {"type": "string"},
                "cadence_seconds": {"type": "integer", "description": "600 to 86400"},
                "notify_email": {"type": "string"},
                "failure_threshold_seconds": {"type": "integer"},
                "deadline_at": {"type": "string", "description": "RFC 3339 with an offset"},
                "remind_before_seconds": {"type": "integer"},
                "remind_now": {"type": "boolean"},
                "clear_deadline": {"type": "boolean"}
            },
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let mut proposal = serde_json::Map::new();
        for (key, value) in &params {
            let kept = if TEXT_FIELDS.contains(&key.as_str()) {
                value.as_str().map(|text| bounded(key, text))
            } else if NUMBER_FIELDS.contains(&key.as_str()) {
                value.as_u64().map(Value::from)
            } else if FLAG_FIELDS.contains(&key.as_str()) {
                value.as_bool().map(Value::Bool)
            } else {
                return format!("Error: unknown field {key}").into();
            };
            let Some(kept) = kept else {
                return format!("Error: {key} has the wrong type").into();
            };
            proposal.insert(key.clone(), kept);
        }
        ToolResult::Json(json!({
            "kind": "web_watch_proposal",
            "proposal": proposal,
            "requires_explicit_confirmation": true,
            "permission_granted": false
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WebWatchStatusTool;

impl WebWatchStatusTool {
    pub const fn new() -> Self {
        Self
    }
}

impl Tool for WebWatchStatusTool {
    fn name(&self) -> &str {
        WEB_WATCH_STATUS_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Show the stored state of this conversation's web page watch: when the page was last read \
         successfully, the current decision, a failure in progress, the next check, the email target \
         and the deadline. The runtime answers the user with these facts directly."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"work_id": {"type": "string"}},
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let work_id = params.get("work_id").and_then(Value::as_str).map(str::trim);
        ToolResult::Json(json!({
            "kind": "web_watch_status_request",
            "work_id": work_id.filter(|work_id| !work_id.is_empty()),
        }))
    }
}

pub(crate) fn web_watch_proposal_from_tool_content(content: &str) -> Option<WebWatchProposalDraft> {
    let value: Value = serde_json::from_str(content).ok()?;
    if value.get("kind").and_then(Value::as_str) != Some("web_watch_proposal")
        || value
            .get("requires_explicit_confirmation")
            .and_then(Value::as_bool)
            != Some(true)
        || value.get("permission_granted").and_then(Value::as_bool) != Some(false)
    {
        return None;
    }
    serde_json::from_value(value.get("proposal")?.clone()).ok()
}

/// The work a status request names, `Some(None)` when it names none.
pub(crate) fn web_watch_status_request_from_tool_content(content: &str) -> Option<Option<String>> {
    let value: Value = serde_json::from_str(content).ok()?;
    (value.get("kind").and_then(Value::as_str) == Some("web_watch_status_request")).then(|| {
        value
            .get("work_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn bounded(key: &str, text: &str) -> Value {
    let text = text.trim();
    Value::String(if key == "goal_text" {
        redact_string(text).chars().take(MAX_GOAL_CHARS).collect()
    } else {
        text.chars().take(MAX_FIELD_CHARS).collect()
    })
}
