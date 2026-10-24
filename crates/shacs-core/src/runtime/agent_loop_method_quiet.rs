//! Runtime hooks (verbose logs, observability, plugins) see none of the
//! model's words in an image or composed request: no text or reasoning, no
//! assistant text in their context, of each call's arguments and each tool's
//! output only what `quiet_tools` keeps, and none of a provider's own fields
//! (extension maps, error strings) on a response or a call. A call to a tool
//! the model was not offered never runs, and shows under `UNOFFERED_TOOL` with
//! no arguments. A hook that decides on a call (a plugin's `tool:before`
//! policy) still gets the arguments of the tools the model was offered as
//! written.

use super::*;
use crate::runtime::quiet_tools::{
    quiet_arguments, quiet_finish_reason, quiet_id, quiet_tool_calls, quiet_tool_message,
    quiet_usage, rename_unoffered, UNOFFERED_TOOL,
};
use crate::runtime::AgentHookContext;

pub(super) struct Quiet {
    pub(super) rest: Arc<dyn AgentHook>,
    pub(super) offered: Vec<String>,
}

/// A tool message as hooks see it: its output and call id as digests.
fn quiet_tool(message: &mut Value) {
    quiet_tool_message(message);
    if let Some(id) = message.get("tool_call_id").and_then(Value::as_str) {
        message["tool_call_id"] = Value::String(quiet_id(id));
    }
}

/// An assistant message as hooks see it: no text, reasoning or thinking, and
/// each call rebuilt from its id and arguments as digests and its name.
fn quiet_assistant(message: &mut Value) {
    message["content"] = Value::String(String::new());
    let Some(message) = message.as_object_mut() else {
        return;
    };
    message.remove("reasoning_content");
    message.remove("thinking_blocks");
    let calls = message.get_mut("tool_calls").and_then(Value::as_array_mut);
    for call in calls.into_iter().flatten() {
        let id = call.get("id").and_then(Value::as_str).map(quiet_id);
        let written = call["function"]["arguments"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(Value::Null);
        // Only these: a provider's extension fields may carry what the model wrote.
        *call = json!({"id": id, "type": "function", "function": {
            "name": call["function"]["name"],
            "arguments": quiet_arguments(&written).to_string(),
        }});
    }
}

impl Quiet {
    fn offered(&self, name: &str) -> bool {
        self.offered.iter().any(|offered| offered == name)
    }

    fn context(&self, context: &AgentHookContext) -> AgentHookContext {
        let mut quiet = context.clone();
        for message in &mut quiet.messages {
            rename_unoffered(message, &|name| self.offered(name));
            match message.get("role").and_then(Value::as_str) {
                Some("assistant") => quiet_assistant(message),
                _ => quiet_tool(message),
            }
        }
        quiet
    }

    /// What hooks get of a provider's response: none of its words or of its
    /// own fields (extension maps, error strings), only its shape.
    fn response(&self, response: &shacs_providers::LlmResponse) -> shacs_providers::LlmResponse {
        let calls = response.tool_calls.iter().map(|call| {
            let name = if self.offered(&call.name) {
                call.name.as_str()
            } else {
                UNOFFERED_TOOL
            };
            let arguments = match quiet_arguments(&Value::Object(call.arguments.clone())) {
                Value::Object(arguments) => arguments,
                _ => Map::new(),
            };
            shacs_providers::ToolCallRequest::new(quiet_id(&call.id), name, arguments)
        });
        let usage = quiet_usage(&json!(response.usage));
        shacs_providers::LlmResponse {
            tool_calls: calls.collect(),
            finish_reason: quiet_finish_reason(&response.finish_reason),
            usage: serde_json::from_value(usage).unwrap_or_default(),
            retry_after: response.retry_after,
            error_status_code: response.error_status_code,
            error_retry_after_s: response.error_retry_after_s,
            error_should_retry: response.error_should_retry,
            ..shacs_providers::LlmResponse::default()
        }
    }

    fn offered_calls(&self, calls: &[RuntimeToolCall]) -> Vec<RuntimeToolCall> {
        calls
            .iter()
            .map(|call| {
                let mut call = call.clone();
                if !self.offered(&call.name) {
                    call.name = UNOFFERED_TOOL.to_owned();
                    call.arguments = Value::Object(Map::new());
                }
                call
            })
            .collect()
    }
}

impl AgentHook for Quiet {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn before_iteration(&self, context: &AgentHookContext) {
        self.rest.before_iteration(&self.context(context));
    }
    fn on_stream_end(&self, context: &AgentHookContext, resuming: bool) {
        self.rest.on_stream_end(&self.context(context), resuming);
    }
    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        let calls = quiet_tool_calls(&self.offered_calls(calls));
        self.rest
            .before_execute_tools(&self.context(context), &calls);
    }
    fn block_tool_calls(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.rest
            .block_tool_calls_quietly(&self.context(context), &self.offered_calls(calls))
    }
    fn after_response(&self, context: &AgentHookContext, response: &shacs_providers::LlmResponse) {
        self.rest
            .after_response(&self.context(context), &self.response(response));
    }
    fn after_iteration(&self, context: &AgentHookContext) {
        self.rest.after_iteration(&self.context(context));
    }
}

/// The hooks of a run whose request is no longer an image or composed one
/// (after a method change) while it still holds what it saved as one: they
/// see the messages of the calls saved then as hooks of such a request do
/// (the outputs given back to the model included), and the run's own calls
/// as they are.
pub(super) struct Unheld {
    pub(super) rest: Arc<dyn AgentHook>,
    pub(super) held: super::held::Held,
}

impl Unheld {
    fn earlier(&self, id: Option<&str>) -> bool {
        id.is_some_and(|id| self.held.calls.contains_key(id) || self.held.outputs.contains_key(id))
    }

    fn context(&self, context: &AgentHookContext) -> AgentHookContext {
        let mut quiet = context.clone();
        for message in &mut quiet.messages {
            if message.get("role").and_then(Value::as_str) == Some("assistant") {
                let calls = message.get("tool_calls").and_then(Value::as_array);
                if calls
                    .into_iter()
                    .flatten()
                    .any(|call| self.earlier(call.get("id").and_then(Value::as_str)))
                {
                    quiet_assistant(message);
                }
            } else if self.earlier(message.get("tool_call_id").and_then(Value::as_str)) {
                quiet_tool(message);
            }
        }
        quiet
    }
}

impl AgentHook for Unheld {
    fn receives_tool_arguments(&self) -> bool {
        self.rest.receives_tool_arguments()
    }
    fn wants_streaming(&self) -> bool {
        self.rest.wants_streaming()
    }
    fn before_iteration(&self, context: &AgentHookContext) {
        self.rest.before_iteration(&self.context(context));
    }
    fn on_stream(&self, context: &AgentHookContext, text: &str) {
        self.rest.on_stream(&self.context(context), text);
    }
    fn on_stream_end(&self, context: &AgentHookContext, resuming: bool) {
        self.rest.on_stream_end(&self.context(context), resuming);
    }
    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        self.rest
            .before_execute_tools(&self.context(context), calls);
    }
    fn block_tool_calls(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.rest.block_tool_calls(&self.context(context), calls)
    }
    fn block_tool_calls_quietly(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.rest
            .block_tool_calls_quietly(&self.context(context), calls)
    }
    fn after_response(&self, context: &AgentHookContext, response: &shacs_providers::LlmResponse) {
        self.rest.after_response(&self.context(context), response);
    }
    fn after_iteration(&self, context: &AgentHookContext) {
        self.rest.after_iteration(&self.context(context));
    }
    fn finalize_content(&self, context: &AgentHookContext, content: String) -> String {
        self.rest.finalize_content(&self.context(context), content)
    }
}
