use serde_json::{json, Map, Value};
use shacs_core::runtime::{
    AgentRunSpec, AgentRunner, PluginHookCallbackResult, PluginHookCommandExecutor,
    PluginHookCommandInvocation, PluginHookDispatchMode, PluginHookEvent, PluginManifestSource,
    PluginRuntimeHook, PluginRuntimeHookAgentHook, PluginRuntimePlugin, PluginRuntimeSnapshot,
};
use shacs_core::tools::{
    IntegerSchema, JsonMap, SchemaFragment, Tool, ToolParameters, ToolRegistry, ToolResult,
    ValidationError,
};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct QueueProvider(Mutex<Vec<Result<LlmResponse, ProviderError>>>);

impl QueueProvider {
    fn new(arguments: Map<String, Value>) -> Self {
        Self(Mutex::new(vec![
            Ok(LlmResponse {
                content: Some("complete".to_owned()),
                finish_reason: "stop".to_owned(),
                ..LlmResponse::default()
            }),
            Ok(LlmResponse {
                tool_calls: vec![ToolCallRequest {
                    id: "exec-call-030".to_owned(),
                    name: "normalized".to_owned(),
                    arguments,
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

struct CapturingExecutor {
    calls: AtomicUsize,
    input: Mutex<Option<Value>>,
}

impl CapturingExecutor {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            input: Mutex::new(None),
        }
    }
}

impl PluginHookCommandExecutor for CapturingExecutor {
    fn execute(&self, invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.input.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(invocation.stdin_payload.clone());
        PluginHookCallbackResult::Output(json!({}))
    }
}

struct CountingNormalizedTool {
    calls: Arc<AtomicUsize>,
    validations: Arc<AtomicUsize>,
    input: Arc<Mutex<Option<JsonMap>>>,
}

impl Tool for CountingNormalizedTool {
    fn name(&self) -> &str {
        "normalized"
    }

    fn description(&self) -> &str {
        "Capture normalized input."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("count", IntegerSchema::new("count"))
            .required(["count"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        if params.contains_key("count") {
            Vec::new()
        } else {
            vec![ValidationError::new("count", "is required")]
        }
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.input.lock().unwrap_or_else(|error| error.into_inner()) = Some(params);
        "normalized-output".into()
    }
}

fn hook(executor: Arc<CapturingExecutor>) -> Arc<PluginRuntimeHookAgentHook> {
    Arc::new(PluginRuntimeHookAgentHook::with_executor(
        PluginRuntimeSnapshot {
            plugins: vec![PluginRuntimePlugin {
                id: "capture".to_owned(),
                root: PathBuf::from("."),
                manifest_digest: None,
                source: PluginManifestSource::UserData,
                hooks: vec![PluginRuntimeHook {
                    plugin_id: "capture".to_owned(),
                    event: PluginHookEvent::ToolBefore,
                    event_name: "tool:before".to_owned(),
                    command: shacs_core::runtime::PluginExecutableCommand {
                        command_path: PathBuf::from("/tmp/capture"),
                        args: Vec::new(),
                        timeout_ms: 25,
                    },
                }],
            }],
            commands: Vec::new(),
            diagnostics: Vec::new(),
        },
        PluginHookDispatchMode::LiveDiagnostics,
        executor,
    ))
}

fn run(
    arguments: Map<String, Value>,
    hook: Arc<PluginRuntimeHookAgentHook>,
    registry: &ToolRegistry,
) -> Vec<Value> {
    let client = QueueProvider::new(arguments);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "run"})],
        registry,
        &client,
        "fake",
    );
    spec.max_iterations = 2;
    spec.agent_hook = Some(hook);
    AgentRunner::new()
        .run(spec)
        .unwrap_or_else(|error| panic!("agent run failed: {error}"))
        .messages
}

#[test]
fn spec030_tool_before_hook_receives_once_prepared_normalized_input() {
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let validations = Arc::new(AtomicUsize::new(0));
    let tool_input = Arc::new(Mutex::new(None));
    let mut registry = ToolRegistry::new();
    registry.register(CountingNormalizedTool {
        calls: tool_calls.clone(),
        validations: validations.clone(),
        input: tool_input.clone(),
    });
    let executor = Arc::new(CapturingExecutor::new());
    let mut arguments = Map::new();
    arguments.insert("count".to_owned(), json!("7"));

    run(arguments, hook(executor.clone()), &registry);

    let hook_input = executor
        .input
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .unwrap_or_else(|| panic!("missing hook input"));
    assert_eq!(hook_input["context"]["tools"][0]["arguments"]["count"], 7);
    assert_eq!(
        tool_input
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .and_then(|input| input.get("count")),
        Some(&json!(7))
    );
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
}

#[test]
fn spec030_tool_before_invalid_input_skips_hook_and_tool_with_validation_message() {
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let validations = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(CountingNormalizedTool {
        calls: tool_calls.clone(),
        validations: validations.clone(),
        input: Arc::new(Mutex::new(None)),
    });
    let executor = Arc::new(CapturingExecutor::new());

    let messages = run(Map::new(), hook(executor.clone()), &registry);

    assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 0);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
    assert!(messages.iter().any(|message| {
        message["tool_call_id"] == "exec-call-030"
            && message["content"]
                .as_str()
                .is_some_and(|content| content.contains("Invalid parameters"))
    }));
}
