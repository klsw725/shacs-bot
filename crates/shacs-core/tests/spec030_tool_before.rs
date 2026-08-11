use serde_json::json;
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    AgentHookContext, AgentRunSpec, AgentRunner, PluginExecutableCommand, PluginHookCallbackResult,
    PluginHookCommandExecutor, PluginHookCommandInvocation, PluginHookDispatchMode,
    PluginHookEvent, PluginManifestSource, PluginRuntimeHook, PluginRuntimeHookAgentHook,
    PluginRuntimePlugin, PluginRuntimeSnapshot, ToolBeforeContext, ToolBeforeDecision,
    ToolBeforeHandler, ToolBeforeOrderKey,
};
use shacs_core::tools::{ExecTool, ToolRegistry};
use shacs_projection::{HookDenialReason, HookDiagnosticKind, Spec030ProjectionProvider};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[path = "support/spec030_tool_before.rs"]
mod support;
use support::{approved_exec_context, QueueProvider};

struct QueueExecutor {
    results: Mutex<Vec<PluginHookCallbackResult>>,
    calls: Mutex<Vec<PluginHookCommandInvocation>>,
}

impl QueueExecutor {
    fn new(mut results: Vec<PluginHookCallbackResult>) -> Self {
        results.reverse();
        Self {
            results: Mutex::new(results),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn count(&self) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }
}

impl PluginHookCommandExecutor for QueueExecutor {
    fn execute(&self, invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        self.calls
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(invocation.clone());
        self.results
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop()
            .unwrap_or_else(|| PluginHookCallbackResult::Output(json!({})))
    }
}

struct RecordingHandler {
    key: &'static str,
    calls: Arc<Mutex<Vec<&'static str>>>,
    panic: bool,
    decision: ToolBeforeDecision,
}

impl ToolBeforeHandler for RecordingHandler {
    fn hook_ref(&self) -> &str {
        self.key
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.key)
    }

    fn evaluate(&self, _context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        self.calls
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(self.key);
        assert!(!self.panic, "intentional trusted hook panic");
        self.decision.clone()
    }
}

fn snapshot(ids: &[&str]) -> PluginRuntimeSnapshot {
    PluginRuntimeSnapshot {
        plugins: ids
            .iter()
            .map(|id| PluginRuntimePlugin {
                id: (*id).to_owned(),
                root: PathBuf::from("."),
                manifest_digest: None,
                source: PluginManifestSource::UserData,
                hooks: vec![PluginRuntimeHook {
                    plugin_id: (*id).to_owned(),
                    event: PluginHookEvent::ToolBefore,
                    event_name: "tool:before".to_owned(),
                    command: PluginExecutableCommand {
                        command_path: PathBuf::from(format!("/tmp/{id}")),
                        args: Vec::new(),
                        timeout_ms: 25,
                    },
                }],
            })
            .collect(),
        commands: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn run_exec(
    workspace: &Path,
    hook: Arc<PluginRuntimeHookAgentHook>,
    marker: &Path,
    model_fragment: &str,
) -> (Vec<serde_json::Value>, bool) {
    let mut registry = ToolRegistry::new();
    registry.register(ExecTool::with_workspace(workspace));
    let marker_name = marker
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("marker must have a UTF-8 file name"));
    let client = QueueProvider::exec(&format!("printf ran > {marker_name}"));
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "run"})],
        &registry,
        &client,
        "fake",
    );
    spec.max_iterations = 2;
    spec.agent_hook = Some(hook);
    spec.tool_context = approved_exec_context(&registry, &format!("printf ran > {marker_name}"));
    let messages = AgentRunner::new()
        .run(spec)
        .unwrap_or_else(|error| panic!("agent run failed: {error}"))
        .messages;
    let model_saw_message = client.saw_tool_message("exec-call-030", model_fragment);
    (messages, model_saw_message)
}

#[test]
fn spec030_tool_before_allows_actual_exec() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let marker = temp.path().join("allowed");
    let executor = Arc::new(QueueExecutor::new(vec![PluginHookCallbackResult::Output(
        json!({}),
    )]));
    let hook = Arc::new(PluginRuntimeHookAgentHook::with_executor(
        snapshot(&["allow"]),
        PluginHookDispatchMode::LiveDiagnostics,
        executor,
    ));

    let (messages, model_saw_output) = run_exec(temp.path(), hook, &marker, "Exit code: 0");

    assert!(marker.exists(), "{messages:?}");
    assert!(model_saw_output);
    assert!(messages
        .iter()
        .any(|message| message["tool_call_id"] == "exec-call-030"));
}

#[test]
fn spec030_tool_before_first_block_stops_later_handler_and_actual_exec() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let marker = temp.path().join("blocked");
    let executor = Arc::new(QueueExecutor::new(vec![PluginHookCallbackResult::Output(
        json!({"block": {"reason": "dangerous command"}}),
    )]));
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let hook = Arc::new(
        PluginRuntimeHookAgentHook::with_executor(
            snapshot(&["a-block", "z-later"]),
            PluginHookDispatchMode::LiveDiagnostics,
            executor.clone(),
        )
        .with_spec030_fact_store(facts.clone()),
    );

    let (messages, model_saw_denial) =
        run_exec(temp.path(), hook.clone(), &marker, "dangerous command");

    assert!(!marker.exists());
    assert_eq!(executor.count(), 1);
    let tool = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap_or_else(|| panic!("missing tool message"));
    assert_eq!(tool["tool_call_id"], "exec-call-030");
    assert!(tool["content"]
        .as_str()
        .unwrap_or_default()
        .contains("dangerous command"));
    assert!(model_saw_denial);
    assert_eq!(
        hook.hook_runtime_projection().recent_denials[0].reason,
        HookDenialReason::ExtensionBlocked
    );
    assert_eq!(
        LocalSpec030ProjectionProvider::new(facts)
            .projection()
            .hooks()
            .recent_denials[0]
            .reason,
        HookDenialReason::ExtensionBlocked
    );
}

#[test]
fn spec030_tool_before_failures_record_diagnostics_and_continue_in_order() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let handlers: Vec<Arc<dyn ToolBeforeHandler>> = vec![
        Arc::new(RecordingHandler {
            key: "b-panic",
            calls: calls.clone(),
            panic: true,
            decision: ToolBeforeDecision::Allow,
        }),
        Arc::new(RecordingHandler {
            key: "z-continue",
            calls: calls.clone(),
            panic: false,
            decision: ToolBeforeDecision::Allow,
        }),
    ];
    let executor = Arc::new(QueueExecutor::new(vec![
        PluginHookCallbackResult::Timeout("timed out".to_owned()),
        PluginHookCallbackResult::Output(json!({"block": false})),
    ]));
    let hook = PluginRuntimeHookAgentHook::with_executor(
        snapshot(&["a-timeout", "c-invalid"]),
        PluginHookDispatchMode::LiveDiagnostics,
        executor,
    )
    .with_trusted_handlers(handlers);
    let call =
        shacs_core::runtime::RuntimeToolCall::new("call-diag", "exec", json!({"command": "true"}));

    let blocked = hook.blocked_tool_messages(
        &AgentHookContext {
            iteration: 0,
            messages: Vec::new(),
        },
        &[call],
    );

    assert!(blocked.is_empty());
    assert_eq!(
        *calls.lock().unwrap_or_else(|error| error.into_inner()),
        vec!["b-panic", "z-continue"]
    );
    let kinds = hook
        .hook_runtime_projection()
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            HookDiagnosticKind::Timeout,
            HookDiagnosticKind::Panic,
            HookDiagnosticKind::InvalidOutput
        ]
    );
}
