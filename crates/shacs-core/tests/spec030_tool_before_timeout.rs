use serde_json::{json, Map, Value};
use shacs_core::runtime::{
    AgentRunSpec, AgentRunner, PluginHookCallbackResult, PluginHookCommandExecutor,
    PluginHookCommandInvocation, PluginHookDispatchMode, PluginRuntimeHookAgentHook,
    PluginRuntimeSnapshot, ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler,
    ToolBeforeOrderKey,
};
use shacs_core::tools::{JsonMap, SchemaFragment, Tool, ToolParameters, ToolRegistry, ToolResult};
use shacs_projection::HookDiagnosticKind;
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct UnusedExecutor;

impl PluginHookCommandExecutor for UnusedExecutor {
    fn execute(&self, _invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        panic!("command executor must not run")
    }
}

struct HangHandler;

impl ToolBeforeHandler for HangHandler {
    fn hook_ref(&self) -> &str {
        "a-hang"
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.hook_ref())
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(20)
    }

    fn evaluate(&self, _context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        std::thread::sleep(Duration::from_millis(150));
        ToolBeforeDecision::Block {
            reason: "late block must be ignored".to_owned(),
        }
    }
}

struct LaterHandler(Arc<AtomicUsize>);

impl ToolBeforeHandler for LaterHandler {
    fn hook_ref(&self) -> &str {
        "z-later"
    }

    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.hook_ref())
    }

    fn evaluate(&self, _context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolBeforeDecision::Allow
    }
}

struct CountingTool(Arc<AtomicUsize>);

impl Tool for CountingTool {
    fn name(&self) -> &str {
        "counting"
    }

    fn description(&self) -> &str {
        "Count executions."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        "executed".into()
    }
}

struct QueueProvider(Mutex<Vec<Result<LlmResponse, ProviderError>>>);

impl QueueProvider {
    fn new() -> Self {
        Self(Mutex::new(vec![
            Ok(LlmResponse {
                content: Some("complete".to_owned()),
                finish_reason: "stop".to_owned(),
                ..LlmResponse::default()
            }),
            Ok(LlmResponse {
                tool_calls: vec![ToolCallRequest {
                    id: "timeout-call".to_owned(),
                    name: "counting".to_owned(),
                    arguments: Map::new(),
                    extra_content: None,
                    provider_specific_fields: None,
                    function_provider_specific_fields: None,
                }],
                finish_reason: "tool_calls".to_owned(),
                ..LlmResponse::default()
            }),
        ]))
    }
}

impl ProviderClient for QueueProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop()
            .unwrap_or_else(|| Ok(LlmResponse::default()))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

#[test]
fn spec030_tool_before_trusted_timeout_continues_later_handler_and_runtime() {
    let later_calls = Arc::new(AtomicUsize::new(0));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let hook = Arc::new(
        PluginRuntimeHookAgentHook::with_executor(
            PluginRuntimeSnapshot::default(),
            PluginHookDispatchMode::LiveDiagnostics,
            Arc::new(UnusedExecutor),
        )
        .with_trusted_handlers(vec![
            Arc::new(HangHandler),
            Arc::new(LaterHandler(later_calls.clone())),
        ]),
    );
    let mut registry = ToolRegistry::new();
    registry.register(CountingTool(tool_calls.clone()));
    let client = QueueProvider::new();
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "run"})],
        &registry,
        &client,
        "fake",
    );
    spec.max_iterations = 2;
    spec.agent_hook = Some(hook.clone());

    let started = Instant::now();
    AgentRunner::new()
        .run(spec)
        .unwrap_or_else(|error| panic!("agent run failed: {error}"));

    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(later_calls.load(Ordering::SeqCst), 1);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        hook.hook_runtime_projection().diagnostics[0].kind,
        HookDiagnosticKind::Timeout
    );
}
