use serde_json::{json, Map, Value};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};
use shacs_core::runtime::{
    AgentLoop, AgentLoopConfig, ContextBuilder, MessageBus, PermissionMode, PermissionModeSnapshot,
    PermissionedActionInput, PermissionedActionOrigin, RuntimeToolCall, SessionManager,
};
use shacs_core::tools::{JsonMap, SchemaFragment, Tool, ToolParameters, ToolRegistry, ToolResult};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct ProcExecCountingTool {
    pub calls: Arc<AtomicUsize>,
}

impl Tool for ProcExecCountingTool {
    fn name(&self) -> &str {
        "exec"
    }
    fn description(&self) -> &str {
        "count proc exec calls"
    }
    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("command"))
            .to_json_schema()
    }
    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "executed".into()
    }
}

pub struct MockProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
}

impl MockProvider {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

impl ProviderClient for MockProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn provider_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}

pub fn tool_call_response(call_id: &str, command: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            call_id,
            "exec",
            Map::from_iter([("command".to_owned(), json!(command))]),
        )],
        ..LlmResponse::default()
    }
}

pub fn runtime<'a>(
    workspace: &Path,
    bus: MessageBus,
    registry: &'a ToolRegistry,
    client: &'a dyn ProviderClient,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    Ok(AgentLoop::new(
        bus,
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        registry,
        client,
        config,
    ))
}

pub fn registry(calls: Arc<AtomicUsize>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool { calls });
    registry
}

pub fn expire_legacy_session_approval(
    workspace: &Path,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = manager
        .load_existing(session_key)
        .ok_or("missing session to expire approval")?;
    if let Some(approvals) = session
        .metadata
        .get_mut("session_permission_approvals")
        .and_then(Value::as_array_mut)
    {
        for approval in approvals {
            approval["approval"]["request"]["expires_at_unix_ms"] = json!(0);
        }
    }
    manager.save(&session)?;
    Ok(())
}

pub fn seed_oversized_remembered_permissions(
    workspace: &Path,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let mut rules = (0..40)
        .map(|index| remembered_exact_rule(session_key, format!("stale-action-{index}"), index))
        .collect::<Vec<_>>();
    rules.push(remembered_fmt_rule(session_key, 40));
    seed_remembered_rules(workspace, session_key, rules)
}

pub fn seed_matching_remembered_allow(
    workspace: &Path,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    seed_remembered_rules(
        workspace,
        session_key,
        vec![remembered_exec_rule(session_key, 1)],
    )
}

fn seed_remembered_rules(
    workspace: &Path,
    session_key: &str,
    rules: Vec<Value>,
) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = manager.get_or_create(session_key);
    session.metadata.insert(
        "session_remembered_permissions_v1".to_owned(),
        json!({ "schema_version": 1, "rules": rules }),
    );
    manager.save(&session)?;
    Ok(())
}

fn remembered_exact_rule(session_key: &str, action_digest: String, created_unix_ms: u64) -> Value {
    json!({ "session_key": session_key, "approval_context_digest": legacy_context_digest(session_key), "effect": RememberedPermissionEffect::Allow, "matcher": RememberedPermissionMatcher::ExactAction { action_digest }, "created_unix_ms": created_unix_ms })
}

fn remembered_exec_rule(session_key: &str, created_unix_ms: u64) -> Value {
    json!({ "session_key": session_key, "approval_context_digest": legacy_context_digest(session_key), "effect": RememberedPermissionEffect::Allow, "matcher": RememberedPermissionMatcher::ExecPrefix { tokens: vec!["cargo".to_owned(), "test".to_owned()] }, "created_unix_ms": created_unix_ms })
}

fn remembered_fmt_rule(session_key: &str, created_unix_ms: u64) -> Value {
    json!({ "session_key": session_key, "approval_context_digest": legacy_context_digest(session_key), "effect": RememberedPermissionEffect::Allow, "matcher": RememberedPermissionMatcher::ExecPrefix { tokens: vec!["cargo".to_owned(), "fmt".to_owned()] }, "created_unix_ms": created_unix_ms })
}

fn legacy_context_digest(session_key: &str) -> String {
    let action = shacs_core::runtime::normalize_runtime_tool_call(
        &registry(Arc::new(AtomicUsize::new(0))),
        &RuntimeToolCall::new("legacy", "exec", json!({ "command": "cargo test" })),
        PermissionedActionInput {
            session_id: session_key.to_owned(),
            turn_id: format!("turn:{session_key}"),
            origin: PermissionedActionOrigin::ChannelInbound {
                channel: "direct".to_owned(),
                message_id: None,
            },
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::Auto,
                source: Some("test".to_owned()),
                scope_ref: None,
            },
            containment_snapshot: None,
            intent_snapshot: None,
        },
    );
    shacs_core::runtime::session_remembered_context_digest(&action)
}
