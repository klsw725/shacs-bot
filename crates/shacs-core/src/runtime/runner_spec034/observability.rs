use crate::runtime::RuntimeToolCall;
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
            id: id.clone(),
            name: name.clone(),
        },
        ProviderEvent::ToolCallDelta { id, .. } => ProviderEvent::ToolCallDelta {
            id: id.clone(),
            delta: "<redacted>".to_owned(),
        },
        ProviderEvent::ToolCallReady { id, name, input } => ProviderEvent::ToolCallReady {
            id: id.clone(),
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

pub(crate) fn observable_tool_arguments(name: &str, arguments: &Value) -> Value {
    if permission_sensitive_observability_tool(name) {
        return json!({ "redacted": true });
    }
    redact_value(arguments)
}

pub(crate) fn observable_tool_calls(calls: &[RuntimeToolCall]) -> Vec<RuntimeToolCall> {
    calls
        .iter()
        .map(|call| RuntimeToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: observable_tool_arguments(&call.name, &call.arguments),
        })
        .collect()
}

pub(crate) fn observable_llm_response(response: &LlmResponse) -> LlmResponse {
    let mut observable = response.clone();
    for call in &mut observable.tool_calls {
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
