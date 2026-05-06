use crate::tools::{ArraySchema, JsonMap, StringSchema, Tool, ToolParameters, ToolResult};
use crate::tools::{SchemaFragment, ValidationError};
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
        "Pause and ask the user a question when their answer is required to continue. Use options for likely answers; the user's reply, typed or selected, is returned as the tool result. For non-blocking notifications or buttons, use the message tool instead."
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

pub fn pending_ask_user_id(history: &[Value]) -> Option<String> {
    let mut pending = Vec::<(String, String)>::new();
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
                    if let Some((_, name)) = pending
                        .iter_mut()
                        .find(|(existing_id, _)| existing_id == id)
                    {
                        *name = tool_call_name(tool_call).to_owned();
                    } else {
                        pending.push((id.to_owned(), tool_call_name(tool_call).to_owned()));
                    }
                }
            }
        } else if role == "tool" {
            if let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) {
                pending.retain(|(id, _)| id != tool_call_id);
            }
        }
    }
    pending
        .iter()
        .rev()
        .find_map(|(tool_call_id, name)| (name == "ask_user").then(|| tool_call_id.clone()))
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

fn tool_call_arguments(tool_call: &Value) -> Map<String, Value> {
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
