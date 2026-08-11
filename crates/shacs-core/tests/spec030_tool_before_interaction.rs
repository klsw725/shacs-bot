use serde_json::json;
use shacs_core::runtime::{
    AgentHookContext, PluginHookCallbackResult, PluginHookCommandExecutor,
    PluginHookCommandInvocation, PluginHookDispatchMode, PluginRuntimeHookAgentHook,
    PluginRuntimeSnapshot, RuntimeToolCall, ToolBeforeConfirmRequest, ToolBeforeConfirmation,
    ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler, ToolBeforeInteraction,
    ToolBeforeNotifyRequest, ToolBeforeOrderKey, ToolBeforeSelectRequest,
};
use std::sync::{Arc, Mutex};

struct UnusedExecutor;

impl PluginHookCommandExecutor for UnusedExecutor {
    fn execute(&self, _invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        panic!("command executor must not run")
    }
}

#[derive(Default)]
struct RecordingInteraction(Mutex<Vec<(String, &'static str)>>);

impl RecordingInteraction {
    fn events(&self) -> Vec<(String, &'static str)> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl ToolBeforeInteraction for RecordingInteraction {
    fn confirm(&self, request: &ToolBeforeConfirmRequest) -> ToolBeforeConfirmation {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((request.call_id.clone(), "confirm"));
        ToolBeforeConfirmation::Confirmed
    }

    fn select(&self, request: &ToolBeforeSelectRequest) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((request.call_id.clone(), "select"));
        request.options.first().cloned()
    }

    fn notify(&self, request: &ToolBeforeNotifyRequest) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((request.call_id.clone(), "notify"));
    }
}

struct InteractionHandler;

impl ToolBeforeHandler for InteractionHandler {
    fn hook_ref(&self) -> &str {
        "trusted:interaction"
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.hook_ref())
    }

    fn evaluate(&self, context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        context.notify("starting confirmation");
        let selected = context.select("mode", vec!["once".to_owned()]);
        assert_eq!(selected.as_deref(), Some("once"));
        assert_eq!(
            context.confirm("execute current call"),
            ToolBeforeConfirmation::Confirmed
        );
        ToolBeforeDecision::Allow
    }
}

#[test]
fn spec030_tool_before_interaction_contract_is_ephemeral_per_call() {
    let interaction = Arc::new(RecordingInteraction::default());
    let hook = PluginRuntimeHookAgentHook::with_executor(
        PluginRuntimeSnapshot::default(),
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(UnusedExecutor),
    )
    .with_trusted_handlers(vec![Arc::new(InteractionHandler)])
    .with_interaction(interaction.clone());
    let calls = [
        RuntimeToolCall::new("call-a", "exec", json!({"command": "true"})),
        RuntimeToolCall::new("call-b", "exec", json!({"command": "true"})),
    ];

    let blocked = hook.blocked_tool_messages(
        &AgentHookContext {
            iteration: 0,
            messages: Vec::new(),
        },
        &calls,
    );

    assert!(blocked.is_empty());
    assert_eq!(
        interaction.events(),
        vec![
            ("call-a".to_owned(), "notify"),
            ("call-a".to_owned(), "select"),
            ("call-a".to_owned(), "confirm"),
            ("call-b".to_owned(), "notify"),
            ("call-b".to_owned(), "select"),
            ("call-b".to_owned(), "confirm"),
        ]
    );
    assert_eq!(hook.hook_runtime_projection().registered_handlers, 1);
}
