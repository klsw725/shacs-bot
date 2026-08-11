use serde_json::json;
use shacs_core::runtime::{
    AgentRunSpec, AgentRunner, PluginExecutableCommand, PluginHookCallbackResult,
    PluginHookCommandExecutor, PluginHookCommandInvocation, PluginHookDispatchMode,
    PluginHookEvent, PluginManifestSource, PluginRuntimeHook, PluginRuntimeHookAgentHook,
    PluginRuntimePlugin, PluginRuntimeSnapshot,
};
use shacs_core::tools::{ExecTool, ToolRegistry};
use shacs_projection::HookDenialReason;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[path = "support/spec030_tool_before.rs"]
mod support;
use support::{approved_exec_context, QueueProvider};

struct QueueExecutor(Mutex<Vec<PluginHookCallbackResult>>);

impl PluginHookCommandExecutor for QueueExecutor {
    fn execute(&self, _invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop()
            .unwrap_or_else(|| PluginHookCallbackResult::Output(json!({})))
    }
}

#[test]
fn spec030_tool_before_headless_confirmation_denies_current_exec_call() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let marker = temp.path().join("headless");
    let executor = Arc::new(QueueExecutor(Mutex::new(vec![
        PluginHookCallbackResult::Output(
            json!({"confirm": {"prompt": "Run command?", "reason": "confirmation denied"}}),
        ),
    ])));
    let hook = Arc::new(PluginRuntimeHookAgentHook::with_executor(
        snapshot(),
        PluginHookDispatchMode::LiveDiagnostics,
        executor,
    ));

    let mut registry = ToolRegistry::new();
    registry.register(ExecTool::with_workspace(temp.path()));
    let client = QueueProvider::exec("printf ran > headless");
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "run"})],
        &registry,
        &client,
        "fake",
    );
    spec.max_iterations = 2;
    spec.agent_hook = Some(hook.clone());
    spec.tool_context = approved_exec_context(&registry, "printf ran > headless");
    let messages = AgentRunner::new()
        .run(spec)
        .unwrap_or_else(|error| panic!("agent run failed: {error}"))
        .messages;

    assert!(!marker.exists());
    assert!(messages
        .iter()
        .any(|message| message["tool_call_id"] == "exec-call-030"));
    assert!(client.saw_tool_message("exec-call-030", "confirmation denied"));
    assert_eq!(
        hook.hook_runtime_projection().recent_denials[0].reason,
        HookDenialReason::HeadlessConfirmationDenied
    );
}

fn snapshot() -> PluginRuntimeSnapshot {
    PluginRuntimeSnapshot {
        plugins: vec![PluginRuntimePlugin {
            id: "confirm".to_owned(),
            root: PathBuf::from("."),
            manifest_digest: None,
            source: PluginManifestSource::UserData,
            hooks: vec![PluginRuntimeHook {
                plugin_id: "confirm".to_owned(),
                event: PluginHookEvent::ToolBefore,
                event_name: "tool:before".to_owned(),
                command: PluginExecutableCommand {
                    command_path: PathBuf::from("/tmp/confirm"),
                    args: Vec::new(),
                    timeout_ms: 25,
                },
            }],
        }],
        commands: Vec::new(),
        diagnostics: Vec::new(),
    }
}
