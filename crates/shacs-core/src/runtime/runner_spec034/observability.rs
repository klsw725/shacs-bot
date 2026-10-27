use crate::runtime::{RuntimeToolCall, ToolEvent};
use serde_json::{json, Map, Value};
use shacs_providers::{LlmResponse, ProviderEvent};
use shacs_redaction::redact_value;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ProviderStreamCounts {
    text_delta: usize,
    reasoning_delta: usize,
    tool_call_start: usize,
    tool_call_delta: usize,
    tool_call_ready: usize,
    media_lifecycle: usize,
    finish: usize,
}

impl ProviderStreamCounts {
    pub(crate) fn observe(&mut self, event: &ProviderEvent) {
        match event {
            ProviderEvent::TextDelta { .. } => self.text_delta += 1,
            ProviderEvent::ReasoningDelta { .. } => self.reasoning_delta += 1,
            ProviderEvent::ToolCallStart { .. } => self.tool_call_start += 1,
            ProviderEvent::ToolCallDelta { .. } => self.tool_call_delta += 1,
            ProviderEvent::ToolCallReady { .. } => self.tool_call_ready += 1,
            ProviderEvent::MediaLifecycle(_) => self.media_lifecycle += 1,
            ProviderEvent::Finish { .. } => self.finish += 1,
        }
    }

    pub(crate) fn detail(self) -> String {
        json!({
            "stream_events": {
                "total": self.text_delta
                    + self.reasoning_delta
                    + self.tool_call_start
                    + self.tool_call_delta
                    + self.tool_call_ready
                    + self.media_lifecycle
                    + self.finish,
                "text_delta": self.text_delta,
                "reasoning_delta": self.reasoning_delta,
                "tool_call_start": self.tool_call_start,
                "tool_call_delta": self.tool_call_delta,
                "tool_call_ready": self.tool_call_ready,
                "media_lifecycle": self.media_lifecycle,
                "finish": self.finish,
            }
        })
        .to_string()
    }
}

pub(crate) fn observable_provider_event(event: &ProviderEvent) -> ProviderEvent {
    match event {
        ProviderEvent::TextDelta { text } => ProviderEvent::TextDelta { text: text.clone() },
        ProviderEvent::ReasoningDelta { text } => {
            ProviderEvent::ReasoningDelta { text: text.clone() }
        }
        ProviderEvent::ToolCallStart { id, name } => ProviderEvent::ToolCallStart {
            id: observable_call_id(id),
            name: name.clone(),
        },
        ProviderEvent::ToolCallDelta { id, .. } => ProviderEvent::ToolCallDelta {
            id: observable_call_id(id),
            delta: "<redacted>".to_owned(),
        },
        ProviderEvent::ToolCallReady { id, name, input } => ProviderEvent::ToolCallReady {
            id: observable_call_id(id),
            name: name.clone(),
            input: observable_tool_arguments(name, input),
        },
        ProviderEvent::Finish { usage, reason } => ProviderEvent::Finish {
            usage: usage.clone(),
            reason: reason.clone(),
        },
        ProviderEvent::MediaLifecycle(observation) => {
            ProviderEvent::MediaLifecycle(observation.clone())
        }
    }
}

/// A call's arguments as plugin hooks get them: a web watch tool's as its progress events show
/// them, any other tool's as written, since plugin policies judge them.
pub(crate) fn plugin_tool_arguments(name: &str, arguments: &Value) -> Value {
    if crate::tools::is_web_watch_tool(name) {
        shacs_utils::progress_events::project_tool_progress_arguments(name, arguments)
    } else {
        arguments.clone()
    }
}

pub(crate) fn observable_tool_arguments(name: &str, arguments: &Value) -> Value {
    if permission_sensitive_observability_tool(name) {
        return json!({ "redacted": true });
    }
    if crate::tools::is_web_watch_tool(name) {
        return plugin_tool_arguments(name, arguments);
    }
    redact_value(arguments)
}

/// Whether a provider's tool call id is a plain token (ASCII letters, digits, `_-.:`, at most 128
/// bytes), which cannot hold an address with its query, fragment or credentials.
fn plain_call_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// A tool call id as diagnostics, hooks, run results and the execution record show it: the
/// provider's id when it is a plain token, else its full digest (`sha256:` and 64 hex), since
/// the provider writes it and could put an address (with its query) there. A plain id that
/// starts with `sha256:` is digested too, so no plain id can take another call's digest and
/// calls are paired back by it without mixing two up.
pub(crate) fn observable_call_id(id: &str) -> String {
    if plain_call_id(id) && !id.starts_with("sha256:") {
        id.to_owned()
    } else {
        crate::runtime::quiet_tools::opaque_call_id(id)
    }
}

pub(crate) fn observable_tool_event(mut event: ToolEvent) -> ToolEvent {
    event.call_id = event.call_id.as_deref().map(observable_call_id);
    event
}

pub(crate) fn observable_tool_calls(calls: &[RuntimeToolCall]) -> Vec<RuntimeToolCall> {
    calls
        .iter()
        .map(|call| RuntimeToolCall {
            id: observable_call_id(&call.id),
            name: call.name.clone(),
            arguments: observable_tool_arguments(&call.name, &call.arguments),
        })
        .collect()
}

pub(crate) fn observable_llm_response(response: &LlmResponse) -> LlmResponse {
    let mut observable = response.clone();
    for call in &mut observable.tool_calls {
        call.id = observable_call_id(&call.id);
        let arguments =
            observable_tool_arguments(&call.name, &Value::Object(call.arguments.clone()));
        call.arguments = match arguments {
            Value::Object(arguments) => arguments,
            _ => Map::new(),
        };
    }
    observable.media_candidates.clear();
    observable
}

fn permission_sensitive_observability_tool(name: &str) -> bool {
    matches!(
        name,
        "exec"
            | "spawn"
            | "message"
            | "write_file"
            | "edit_file"
            | "notebook_edit"
            | "cron"
            | "tool_call"
    ) || name.starts_with("mcp_")
}
