use serde_json::{json, Value};
use shacs_core::runtime::*;
use shacs_core::tools::{AskUserTool, JsonMap, Tool, ToolRegistry, ToolResult};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::collections::VecDeque;
use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[path = "spec037_method_selection/calculation.rs"]
mod calculation;
#[path = "spec037_method_selection/calculation_gates.rs"]
mod calculation_gates;
#[path = "spec037_method_selection/calculation_terminal.rs"]
mod calculation_terminal;
#[path = "spec037_method_selection/changes.rs"]
mod changes;
#[path = "spec037_method_selection/composed.rs"]
mod composed;
#[path = "spec037_method_selection/composed_change.rs"]
mod composed_change;
#[path = "spec037_method_selection/composed_continuity.rs"]
mod composed_continuity;
#[path = "spec037_method_selection/composed_errors.rs"]
mod composed_errors;
#[path = "spec037_method_selection/composed_gate.rs"]
mod composed_gate;
#[path = "spec037_method_selection/composed_gate2.rs"]
mod composed_gate2;
#[path = "spec037_method_selection/composed_held.rs"]
mod composed_held;
#[path = "spec037_method_selection/composed_image.rs"]
mod composed_image;
#[path = "spec037_method_selection/composed_keep.rs"]
mod composed_keep;
#[path = "spec037_method_selection/composed_names.rs"]
mod composed_names;
#[path = "spec037_method_selection/composed_output.rs"]
mod composed_output;
#[path = "spec037_method_selection/composed_quiet.rs"]
mod composed_quiet;
#[path = "spec037_method_selection/composed_quote.rs"]
mod composed_quote;
#[path = "spec037_method_selection/composed_reentry.rs"]
mod composed_reentry;
#[path = "spec037_method_selection/composed_tools.rs"]
mod composed_tools;
#[path = "spec037_method_selection/composed_unoffered.rs"]
mod composed_unoffered;
#[path = "spec037_method_selection/effects.rs"]
mod effects;
#[path = "spec037_method_selection/image.rs"]
mod image;
#[path = "spec037_method_selection/image_checkpoint.rs"]
mod image_checkpoint;
#[path = "spec037_method_selection/image_hooks.rs"]
mod image_hooks;
#[path = "spec037_method_selection/image_revision.rs"]
mod image_revision;
#[path = "spec037_method_selection/image_stream.rs"]
mod image_stream;
#[path = "spec037_method_selection/image_support.rs"]
mod image_support;
#[path = "spec037_method_selection/image_tools.rs"]
mod image_tools;
#[path = "spec037_method_selection/image_workflow.rs"]
mod image_workflow;
#[path = "spec037_method_selection/report.rs"]
mod report;
#[path = "spec037_method_selection/report_checks.rs"]
mod report_checks;
#[path = "spec037_method_selection/report_refresh.rs"]
mod report_refresh;
#[path = "spec037_method_selection/report_revision.rs"]
mod report_revision;
#[path = "spec037_method_selection/report_terminal.rs"]
mod report_terminal;
#[path = "spec037_method_selection/selection.rs"]
mod selection;
#[path = "spec037_method_selection/turns.rs"]
mod turns;

struct Provider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
}
impl Provider {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl ProviderClient for Provider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests.lock().unwrap().push(request);
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider call"))
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct RegisteredTool {
    name: &'static str,
    counter: Arc<AtomicUsize>,
    read_only: bool,
}
impl Tool for RegisteredTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "Isolated method availability and execution fixture"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"command":{"type":"string"},"path":{"type":"string"},"content":{"type":"string"}}})
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        self.counter.fetch_add(1, Ordering::SeqCst);
        "completed".into()
    }
}

fn tools(names: &[&'static str], counter: &Arc<AtomicUsize>) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    for name in names {
        tools.register(RegisteredTool {
            name,
            counter: counter.clone(),
            read_only: *name == "read_file",
        });
    }
    tools
}
fn runtime<'a>(
    workspace: &Path,
    tools: &'a ToolRegistry,
    provider: &'a Provider,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.permission_interactive = true;
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        tools,
        provider,
        config,
    ))
}
fn inbound(content: &str) -> InboundMessage {
    InboundMessage::new("chat", "alice", "room", content)
}
fn text(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        ..LlmResponse::default()
    }
}
fn call(name: &str, arguments: Value) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            format!("call-{name}"),
            name,
            arguments.as_object().unwrap().clone(),
        )],
        ..LlmResponse::default()
    }
}
fn saved(agent: &AgentLoop<'_>) -> Value {
    agent
        .session_manager()
        .read_session_file("chat:room")
        .expect("persisted session")
}
fn methods(agent: &AgentLoop<'_>) -> Value {
    saved(agent)["metadata"]["task_method_requests_v1"].clone()
}
fn output(result: AgentLoopTurnResult) -> String {
    result.final_content.unwrap_or_default()
}
