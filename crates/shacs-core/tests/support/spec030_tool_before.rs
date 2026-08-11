use serde_json::json;
use shacs_core::runtime::{
    ApprovalActor, ApprovalCacheEntry, ApprovalDecision, ApprovalDecisionKind,
    ContainerNetworkMode, ContainerRuntimeKind, DockerContainmentSnapshot,
    PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot, PermissionRuleInput,
    ProcExecSummary, RuntimeBoundaryOrigin, RuntimeInterrupt, RuntimeToolCall, RuntimeToolExecutor,
    SafetyCapability, ToolExecutionContext,
};
use shacs_core::tools::ToolRegistry;
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::sync::Mutex;

pub struct QueueProvider {
    responses: Mutex<Vec<Result<LlmResponse, ProviderError>>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl QueueProvider {
    pub fn exec(command: &str) -> Self {
        let mut arguments = serde_json::Map::new();
        arguments.insert("command".to_owned(), json!(command));
        Self {
            responses: Mutex::new(vec![
                Ok(LlmResponse {
                    content: Some("complete".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                }),
                Ok(LlmResponse {
                    tool_calls: vec![ToolCallRequest {
                        id: "exec-call-030".to_owned(),
                        name: "exec".to_owned(),
                        arguments,
                        extra_content: None,
                        provider_specific_fields: None,
                        function_provider_specific_fields: None,
                    }],
                    finish_reason: "tool_calls".to_owned(),
                    ..LlmResponse::default()
                }),
            ]),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn saw_tool_message(&self, call_id: &str, content: &str) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .flat_map(|request| request.messages.iter())
            .any(|message| {
                message["tool_call_id"] == call_id
                    && message["content"]
                        .as_str()
                        .is_some_and(|value| value.contains(content))
            })
    }
}

impl ProviderClient for QueueProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(request);
        self.responses
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

pub fn approved_exec_context(registry: &ToolRegistry, command: &str) -> ToolExecutionContext {
    let context = ToolExecutionContext {
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("spec030_tool_before_test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        permission_ceiling_snapshot: Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Auto,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["workspace".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        }),
        permission_interactive: true,
        permission_rule_input: PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: Vec::new(),
                network_mode: ContainerNetworkMode::None,
                digest: Some("spec030-test-contained".to_owned()),
                summary: Some("non-privileged test containment".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: "printf".to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
        },
        ..ToolExecutionContext::default()
    };
    let call = RuntimeToolCall::new("exec-call-030", "exec", json!({ "command": command }));
    let request = match RuntimeToolExecutor::new(registry)
        .execute_tool_calls(vec![call], &context)
        .interrupt
    {
        Some(RuntimeInterrupt::PermissionApproval {
            approval_request, ..
        }) => approval_request,
        other => panic!("missing exec approval request: {other:?}"),
    };
    let decided_at_unix_ms = request.expires_at_unix_ms.saturating_sub(1);
    ToolExecutionContext {
        permission_approval_cache: Some(ApprovalCacheEntry {
            request: (*request).clone(),
            decision: ApprovalDecision {
                approval_request_id: request.approval_request_id.clone(),
                action_digest: request.action_digest.clone(),
                snapshot_digest: request.snapshot_digest.clone(),
                decision: ApprovalDecisionKind::Approved,
                approved_scope: request.requested_scope.clone(),
                actor: ApprovalActor::LocalUser,
                decided_at_unix_ms,
                consumed: false,
                policy_safety_snapshot_ref: request.policy_safety_snapshot_ref.clone(),
                secret_ref_evidence: request.secret_ref_evidence.clone(),
            },
        }),
        ..context
    }
}
