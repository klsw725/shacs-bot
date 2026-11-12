use crate::tools::{
    ArraySchema, JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolResult,
    ValidationError,
};
use serde_json::{json, Map, Value};

const STRUCTURED_BUTTON_CHANNELS: &[&str] = &["telegram", "websocket"];

#[derive(Debug, Clone, Default)]
pub struct AskUserTool;

impl AskUserTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Pause and ask the user a question when their answer is required to continue. Use options for ordinary questions. For a proposed externally committing reservation, purchase, or send on entrusted work, include external_action; only its exact /action decision can resume it, and tool permission remains separate. For non-blocking notifications or buttons, use the message tool instead."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "question",
                StringSchema::new(
                    "The question to ask before continuing. Use this only when the task needs the user's answer.",
                ),
            )
            .property(
                "options",
                ArraySchema::new(StringSchema::new("A possible answer label"))
                    .description("Optional choices. The user may still reply with free text."),
            )
            .raw_property("external_action", super::ask_action::schema())
            .required(["question"])
            .to_json_schema()
    }

    fn exclusive(&self) -> bool {
        true
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        crate::tools::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        )
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        if params.contains_key("external_action") {
            return ToolResult::AskUserInterrupt {
                question: "External action requires a current entrusted-work user decision."
                    .to_owned(),
                options: Vec::new(),
            };
        }
        let question = params
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .filter(|option| !option.is_empty())
            .collect();
        ToolResult::AskUserInterrupt { question, options }
    }
}

/// What kind of call paused a turn to wait for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PausedCallKind {
    AskUser,
    /// The `browser` tool's `handoff` (Spec 050 PRD 010 scope 8).
    BrowserHandoff,
}

/// A call that paused its turn and has no result yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausedCall {
    pub id: String,
    /// The name the call carries in the history; its result carries the same.
    pub name: String,
    pub kind: PausedCallKind,
}

/// The newest unanswered call that pauses a turn: `ask_user`, or `browser`
/// with action `handoff`.
pub fn pending_paused_call(history: &[Value]) -> Option<PausedCall> {
    let mut pending = Vec::<(String, String, Option<PausedCallKind>)>::new();
    for message in history {
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            continue;
        };
        if role == "assistant" {
            for tool_call in message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                    let name = tool_call_name(tool_call).to_owned();
                    let kind = paused_call_kind(tool_call);
                    if let Some(entry) = pending
                        .iter_mut()
                        .find(|(existing_id, _, _)| existing_id == id)
                    {
                        entry.1 = name;
                        entry.2 = kind;
                    } else {
                        pending.push((id.to_owned(), name, kind));
                    }
                }
            }
        } else if role == "tool" {
            if let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) {
                pending.retain(|(id, _, _)| id != tool_call_id);
            }
        }
    }
    pending
        .into_iter()
        .rev()
        .find_map(|(id, name, kind)| kind.map(|kind| PausedCall { id, name, kind }))
}

pub fn pending_ask_user_id(history: &[Value]) -> Option<String> {
    pending_paused_call(history).map(|call| call.id)
}

/// The name of the call `id` in the history.
pub(crate) fn tool_call_name_by_id(history: &[Value], id: &str) -> Option<String> {
    history.iter().rev().find_map(|message| {
        message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|call| call.get("id").and_then(Value::as_str) == Some(id))
            .map(|call| tool_call_name(call).to_owned())
    })
}

fn paused_call_kind(tool_call: &Value) -> Option<PausedCallKind> {
    let name = tool_call_name(tool_call);
    if name == "ask_user" {
        return Some(PausedCallKind::AskUser);
    }
    let arguments = tool_call_arguments(tool_call);
    // A call made through the `tool_call` bridge names its tool inside.
    let handoff = match name {
        "browser" => arguments.get("action").and_then(Value::as_str) == Some("handoff"),
        "tool_call" => {
            arguments.get("name").and_then(Value::as_str) == Some("browser")
                && bridged_arguments(&arguments)
                    .get("action")
                    .and_then(Value::as_str)
                    == Some("handoff")
        }
        _ => false,
    };
    handoff.then_some(PausedCallKind::BrowserHandoff)
}

fn bridged_arguments(arguments: &Map<String, Value>) -> Map<String, Value> {
    match arguments.get("arguments") {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    }
}

pub fn ask_user_tool_result_messages(
    system_prompt: &str,
    history: &[Value],
    tool_call_id: &str,
    content: &str,
) -> Vec<Value> {
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(json!({ "role": "system", "content": system_prompt }));
    messages.extend(history.iter().cloned());
    messages.push(json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "name": "ask_user",
        "content": content,
    }));
    messages
}

pub fn ask_user_options_from_messages(messages: &[Value]) -> Vec<String> {
    for message in messages.iter().rev() {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        for tool_call in tool_calls.iter().rev() {
            if tool_call_name(tool_call) != "ask_user" {
                continue;
            }
            return tool_call_arguments(tool_call)
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
        }
    }
    Vec::new()
}

pub fn ask_user_outbound(
    content: Option<&str>,
    options: &[String],
    channel: &str,
) -> (Option<String>, Vec<Vec<String>>) {
    if options.is_empty() {
        return (content.map(str::to_owned), Vec::new());
    }
    if STRUCTURED_BUTTON_CHANNELS.contains(&channel) {
        return (content.map(str::to_owned), vec![options.to_vec()]);
    }
    let option_text = options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("{}. {option}", index + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = content
        .filter(|content| !content.is_empty())
        .map_or(option_text.clone(), |content| {
            format!("{content}\n\n{option_text}")
        });
    (Some(rendered), Vec::new())
}

fn tool_call_name(tool_call: &Value) -> &str {
    tool_call
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| tool_call.get("name").and_then(Value::as_str))
        .unwrap_or_default()
}

pub(crate) fn tool_call_arguments(tool_call: &Value) -> Map<String, Value> {
    let raw = tool_call
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("arguments"))
        .or_else(|| tool_call.get("arguments"));
    match raw {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    }
}
