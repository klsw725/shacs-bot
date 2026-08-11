use super::super::*;
use shacs_core::tools::{JsonMap, Tool, ToolResult};
use shacs_providers::ProviderRequest;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn adapter(
    config_path: &Path,
    workspace: &Path,
    calls: Arc<AtomicUsize>,
) -> AgentLoopChatCompletionAdapter {
    let mut tools = ToolRegistry::new();
    tools.register(CountingExecTool { calls });
    AgentLoopChatCompletionAdapter {
        configured_model: "openai/gpt-5".to_owned(),
        provider_id: "openai".to_owned(),
        defaults: AgentDefaults {
            model: "openai/gpt-5".to_owned(),
            max_tool_iterations: 2,
            ..AgentDefaults::default()
        },
        resolved_model: "gpt-5".to_owned(),
        native_image_input_supported: true,
        client: Arc::new(LoopingProviderClient::default()),
        retry_mode: ProviderRetryMode::Standard,
        workspace: workspace.to_path_buf(),
        config_path: config_path.to_path_buf(),
        media_dir: config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("media")
            .join("api"),
        tools,
        message_tool: None,
        _mcp_runtime: None,
        _mcp_reports: Vec::new(),
        allow_side_effect_tools: true,
        send_progress: true,
        send_tool_hints: false,
        send_max_retries: 0,
        runtime_verbose: false,
        session_turn_lock: SessionTurnLock::new(),
        exec_timeout_seconds: 60,
        exec_sandbox: None,
        exec_path_append: None,
        exec_allowed_env_keys: Vec::new(),
        exec_env: BTreeMap::new(),
        tool_search: ToolSearchConfig::default(),
        containment_snapshot: None,
        #[cfg(not(test))]
        permission_rule_containment: runtime_permission_rule_containment_from_snapshot(None),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("spec031-test-fixture".to_owned()),
            scope_ref: None,
        },
        plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        #[cfg(not(test))]
        trusted_tool_before_handlers: Vec::new(),
        plugin_skill_roots: Vec::new(),
        #[cfg(not(test))]
        spec030_provider: shacs_core::runtime::trusted_runtime::LocalSpec030ProjectionProvider::new(
            shacs_core::runtime::trusted_runtime::Spec030FactStore::new(
                shacs_core::runtime::trusted_runtime::WorkspaceTrustObservation::Trusted,
            ),
        ),
    }
}

pub(super) fn create_pending(
    adapter: &AgentLoopChatCompletionAdapter,
    content: &str,
) -> Result<(), Box<dyn Error>> {
    let mut config = adapter.loop_config();
    config.permission_interactive = true;
    let inbound = InboundMessage::new("cli", "user", "direct", content)
        .with_session_key_override("cli:surface-approval");
    let (turn, _outbound) = adapter.process_inbound_with_outbound(inbound, config, None, &[])?;
    if turn.stop_reason != "ask_user" {
        return Err(format!("fixture did not pause for permission approval: {turn:?}").into());
    }
    Ok(())
}

pub(super) fn write_owner_marker(
    data_dir: &Path,
    pid: u32,
    started_at_ms: u64,
    updated_at_ms: u64,
) -> Result<String, Box<dyn Error>> {
    let marker_path = runtime_ownership_marker_path(data_dir);
    write_runtime_marker_atomically(
        &marker_path,
        &runtime_ownership_marker_value(
            pid,
            started_at_ms,
            updated_at_ms,
            "runtime-start",
            &data_dir.join("config.json"),
            &data_dir.join("workspace"),
        ),
    )?;
    Ok(runtime_owner_id(pid, started_at_ms))
}

#[derive(Default)]
struct LoopingProviderClient {
    calls: Mutex<usize>,
}

impl ProviderClient for LoopingProviderClient {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let mut calls = self.calls.lock().map_err(|_| ProviderError::Api {
            status: Some(500),
            message: "spec031 provider fixture lock poisoned".to_owned(),
            retryable: false,
            headers: BTreeMap::new(),
            body: None,
        })?;
        *calls += 1;
        if *calls % 2 == 1 {
            return Ok(LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![shacs_providers::ToolCallRequest::new(
                    format!("exec-surface-approval-{calls}"),
                    "exec",
                    Map::from_iter([("command".to_owned(), json!("cargo test"))]),
                )],
                ..LlmResponse::default()
            });
        }
        Ok(LlmResponse {
            content: Some("surface approval resumed".to_owned()),
            usage: BTreeMap::from([("request_model_len".to_owned(), request.model.len() as u64)]),
            ..LlmResponse::default()
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct CountingExecTool {
    calls: Arc<AtomicUsize>,
}

impl Tool for CountingExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Count owner-approved exec attempts."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "exec-output".into()
    }
}
