use serde_json::{json, Map, Value};
use shacs_core::runtime::{
    session_approval_context_digest, AgentLoop, AgentLoopConfig, ApprovalActor, ApprovalCacheEntry,
    ApprovalDecision, ApprovalDecisionKind, ApprovalRequest, ContextBuilder, MessageBus,
    PermissionMode, PermissionModeSnapshot, PermissionedActionInput, PermissionedActionOrigin,
    RuntimeToolCall, SessionApprovalCacheEntry, SessionApprovalReuseMatch, SessionManager,
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

pub fn seed_legacy_session_approvals(
    workspace: &Path,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let valid = legacy_entry(
        session_key,
        ApprovalDecisionKind::ApprovedForSession,
        u64::MAX - 1,
        false,
    );
    let expired = legacy_entry(
        session_key,
        ApprovalDecisionKind::ApprovedForSession,
        1,
        false,
    );
    let denied = legacy_entry(
        session_key,
        ApprovalDecisionKind::DeniedForSession,
        u64::MAX - 1,
        false,
    );
    let consumed = legacy_entry(
        session_key,
        ApprovalDecisionKind::ApprovedForSession,
        u64::MAX - 1,
        true,
    );
    seed_legacy_entries(
        workspace,
        session_key,
        vec![valid, expired, denied, consumed],
    )
}

pub fn seed_mismatched_legacy_session_approval(
    workspace: &Path,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let mut entry = legacy_entry(
        session_key,
        ApprovalDecisionKind::ApprovedForSession,
        u64::MAX - 1,
        false,
    );
    entry.approval_context_digest = "sha256:mismatched-context".to_owned();
    seed_legacy_entries(workspace, session_key, vec![entry])
}

pub fn seed_malformed_legacy_session_approval(
    workspace: &Path,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = manager.get_or_create(session_key);
    session.metadata.insert(
        "session_permission_approvals".to_owned(),
        json!({ "not": "an approval list", "raw": "redacted by diagnostic" }),
    );
    manager.save(&session)?;
    Ok(())
}

fn seed_legacy_entries(
    workspace: &Path,
    session_key: &str,
    entries: Vec<SessionApprovalCacheEntry>,
) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = manager.get_or_create(session_key);
    session.metadata.insert(
        "session_permission_approvals".to_owned(),
        serde_json::to_value(entries)?,
    );
    manager.save(&session)?;
    Ok(())
}

fn legacy_entry(
    session_key: &str,
    decision: ApprovalDecisionKind,
    expires_at_unix_ms: u64,
    consumed: bool,
) -> SessionApprovalCacheEntry {
    let request = ApprovalRequest {
        approval_request_id: format!("approval_{decision:?}_{consumed}"),
        action_digest: "legacy-action".to_owned(),
        snapshot_digest: "legacy-snapshot".to_owned(),
        requested_scope: session_key.to_owned(),
        risk_summary: "Run tool `exec`".to_owned(),
        allowed_decisions: vec![ApprovalDecisionKind::ApprovedForSession],
        expires_at_unix_ms,
    };
    SessionApprovalCacheEntry {
        session_key: session_key.to_owned(),
        approval_context_digest: legacy_context_digest(session_key),
        reuse_match: SessionApprovalReuseMatch::ExecCommandPattern {
            pattern: "cargo fmt *".to_owned(),
        },
        approval: ApprovalCacheEntry {
            decision: ApprovalDecision {
                approval_request_id: request.approval_request_id.clone(),
                action_digest: request.action_digest.clone(),
                snapshot_digest: request.snapshot_digest.clone(),
                decision,
                approved_scope: request.requested_scope.clone(),
                actor: ApprovalActor::LocalUser,
                decided_at_unix_ms: 1,
                consumed,
            },
            request,
        },
    }
}

fn legacy_context_digest(session_key: &str) -> String {
    let action = shacs_core::runtime::normalize_runtime_tool_call(
        &registry(Arc::new(AtomicUsize::new(0))),
        &RuntimeToolCall::new("legacy", "exec", json!({ "command": "cargo fmt --check" })),
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
    session_approval_context_digest(&action)
}
