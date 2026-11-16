use crate::spec048_auto_mode::harness::{self, RoutingProvider};
use crate::spec048_auto_mode::support::{Options, Setup};
use serde_json::Value;
use shacs_core::runtime::{
    AgentLoop, AgentLoopConfig, ContextBuilder, ExecutionOutcome, LateResultDecision, MessageBus,
    PermissionMode, RuntimeExecutionLedger, SessionManager, ToolEvent, ToolOutcomeKind,
};
use shacs_core::tools::{
    ExecConfig, ExecTool, JsonMap, ReadFileTool, Tool, ToolRegistry, ToolResult, WriteFileTool,
};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) fn config(setup: &Setup) -> AgentLoopConfig {
    let mut config = harness::agent_loop_config(setup, true);
    let context = setup.context(&Options::mode(PermissionMode::BypassPermissions, true));
    config.permission_mode_snapshot = context.permission_mode_snapshot;
    config.permission_ceiling_snapshot = context.permission_ceiling_snapshot;
    config
}

pub(crate) fn record_refusals(agent: AgentLoop<'_>) -> (AgentLoop<'_>, Arc<Mutex<Vec<ToolEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let agent = agent.with_tool_event_callback(Arc::new(move |event| {
        if event.name == "tool_refusal" {
            sink.lock()
                .expect("refusal events lock poisoned")
                .push(event.clone());
        }
    }));
    (agent, events)
}

pub(crate) fn tool_outcomes(agent: &AgentLoop<'_>) -> Result<Vec<ToolOutcomeKind>, Box<dyn Error>> {
    let session = agent
        .session_manager()
        .read_session_file("cli:refusals")
        .ok_or("session")?;
    let ledger: RuntimeExecutionLedger =
        serde_json::from_value(session["metadata"]["runtime_execution"].clone())?;
    let mut outcomes = Vec::new();
    for entry in ledger.outcomes {
        if let ExecutionOutcome::Tool(outcome) = entry.fact.outcome {
            if entry.decision != LateResultDecision::Accepted {
                return Err(format!(
                    "unexpected duplicate or late tool outcome: {:?}",
                    entry.decision
                )
                .into());
            }
            outcomes.push(outcome);
        }
    }
    Ok(outcomes)
}

pub(super) fn file_refusal(setup: &Setup, path: &str) -> String {
    format!(
        "Error reading file: Path {path} is outside allowed directory {}\n\n[Analyze the error above and try a different approach.]",
        setup.workspace.display(),
    )
}

pub(super) struct ReadOnlyProbe(pub(super) Arc<AtomicUsize>);

impl Tool for ReadOnlyProbe {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "Record whether the read started"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        "read started".into()
    }
}

pub(super) fn tools(setup: &Setup) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    let mut exec = ExecConfig::new(setup.path_context());
    exec.restrict_to_workspace = true;
    tools.register(ExecTool::new(exec));
    tools.register(ReadFileTool::new(setup.path_context()));
    tools.register(WriteFileTool::new(setup.path_context()));
    tools.register(DeferredReader(ReadFileTool::new(setup.path_context())));
    tools
}

pub(crate) fn agent<'a>(
    setup: &Setup,
    tools: &'a ToolRegistry,
    provider: &'a RoutingProvider,
    config: AgentLoopConfig,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&setup.workspace)?,
        ContextBuilder::new(&setup.workspace),
        tools,
        provider,
        config,
    ))
}

/// A deferred MCP-shaped reader with the real file tool's boundary check.
struct DeferredReader(ReadFileTool);

impl Tool for DeferredReader {
    fn name(&self) -> &str {
        "mcp_files_read"
    }
    fn description(&self) -> &str {
        "Read a file through the deferred bridge"
    }
    fn parameters(&self) -> Value {
        self.0.parameters()
    }
    fn read_only(&self) -> bool {
        true
    }
    fn preflight_refusal(&self, params: &JsonMap) -> Option<String> {
        self.0.preflight_refusal(params)
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        self.0.execute(params)
    }
}
