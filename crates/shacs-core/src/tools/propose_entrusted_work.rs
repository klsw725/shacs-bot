use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shacs_redaction::redact_string;

use super::{JsonMap, Tool, ToolResult};

pub const PROPOSE_ENTRUSTED_WORK_TOOL_NAME: &str = "propose_entrusted_work";
const MAX_GOAL_CHARS: usize = 240;
const MAX_INSTRUCTIONS_CHARS: usize = 2_000;
const MAX_TURN_BUDGET: u32 = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntrustedWorkProposalDraft {
    pub goal_text: String,
    pub instructions: String,
    pub turn_budget: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProposeEntrustedWorkTool;

impl ProposeEntrustedWorkTool {
    pub const fn new() -> Self {
        Self
    }
}

impl Tool for ProposeEntrustedWorkTool {
    fn name(&self) -> &str {
        PROPOSE_ENTRUSTED_WORK_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Propose persistent entrusted work for explicit user acceptance. This never creates work or grants permission."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "goal_text": {"type": "string", "minLength": 1, "maxLength": MAX_GOAL_CHARS},
                "instructions": {"type": "string", "minLength": 1, "maxLength": MAX_INSTRUCTIONS_CHARS},
                "turn_budget": {"type": "integer", "minimum": 1, "maximum": MAX_TURN_BUDGET}
            },
            "required": ["goal_text", "instructions"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let Some(goal_text) = params.get("goal_text").and_then(Value::as_str) else {
            return "Error: goal_text must be a string".into();
        };
        let Some(instructions) = params.get("instructions").and_then(Value::as_str) else {
            return "Error: instructions must be a string".into();
        };
        let turn_budget = params
            .get("turn_budget")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(crate::runtime::DEFAULT_GOAL_TURN_BUDGET)
            .clamp(1, MAX_TURN_BUDGET);
        let draft = EntrustedWorkProposalDraft {
            goal_text: bounded_redacted(goal_text, MAX_GOAL_CHARS),
            instructions: bounded_redacted(instructions, MAX_INSTRUCTIONS_CHARS),
            turn_budget,
        };
        ToolResult::Json(json!({
            "kind": "entrusted_work_proposal",
            "proposal": draft,
            "requires_explicit_acceptance": true,
            "permission_granted": false
        }))
    }
}

pub(crate) fn proposal_draft_from_tool_content(
    content: &str,
) -> Option<EntrustedWorkProposalDraft> {
    let value: Value = serde_json::from_str(content).ok()?;
    if value.get("kind").and_then(Value::as_str) != Some("entrusted_work_proposal")
        || value
            .get("requires_explicit_acceptance")
            .and_then(Value::as_bool)
            != Some(true)
        || value.get("permission_granted").and_then(Value::as_bool) != Some(false)
    {
        return None;
    }
    serde_json::from_value(value.get("proposal")?.clone()).ok()
}

fn bounded_redacted(value: &str, limit: usize) -> String {
    redact_string(value.trim()).chars().take(limit).collect()
}
