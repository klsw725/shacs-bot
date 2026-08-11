use crate::runtime::{
    CapabilityCeilingRef, PolicySafetyProvenanceKind, PolicySafetyProvenanceRef,
    PolicySafetySnapshot, PolicySafetySnapshotCreationReason, PolicySafetySnapshotError,
    PolicySafetySnapshotInput, PolicySafetySnapshotRef, PolicySafetySourceKind,
    PolicySafetySourceRef, ResolvedDeferredToolCall, RuntimeToolCall,
};
use crate::tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
pub use shacs_config::{PermissionMode, SafetyCapability};
use shacs_redaction::{
    redact_string, redact_value, RedactionEvidence, RedactionEvidenceRef, SecretRef, REDACTED,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionModeSnapshot {
    pub mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_ref: Option<String>,
}

impl Default for PermissionModeSnapshot {
    fn default() -> Self {
        Self {
            mode: PermissionMode::Default,
            source: None,
            scope_ref: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainmentSnapshotRef {
    pub contained: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentSnapshotRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRef {
    pub kind: String,
    pub digest: String,
    pub redacted_value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PermissionedActionOrigin {
    UserTurn,
    Subagent {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subagent_id: Option<String>,
    },
    CronWake {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
    },
    AppTask {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
    },
    LocalApi {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    ChannelInbound {
        channel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    DeferredBridge {
        original_call_id: String,
        bridge_name: String,
        scope_digest: String,
        parent_origin: Box<PermissionedActionOrigin>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionNormalizationError {
    MissingToolCallId,
    UnknownTool { tool_name: String },
    InvalidArguments { tool_name: String, detail: String },
    UnsafeRawSecret { field: String },
    RedactionFailed { field: String },
    PolicySafetySnapshotInvalid { detail: String },
    SecretRefMalformed { detail: String },
    SecretRefStale { ref_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionNormalizationState {
    Ready,
    DenyCandidate,
    ErrorCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionedActionInput {
    pub session_id: String,
    pub turn_id: String,
    pub origin: PermissionedActionOrigin,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_snapshot: Option<ContainmentSnapshotRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_snapshot: Option<IntentSnapshotRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionedAction {
    pub action_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tool_call_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub tool_name: String,
    pub capabilities: Vec<SafetyCapability>,
    pub target_refs: Vec<TargetRef>,
    pub action_digest: String,
    pub argument_digest: String,
    pub snapshot_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    pub origin: PermissionedActionOrigin,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_snapshot: Option<ContainmentSnapshotRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_snapshot: Option<IntentSnapshotRef>,
    pub redacted_arguments: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_ref_evidence: Vec<PermissionSecretRefEvidence>,
    pub normalization_state: ActionNormalizationState,
    pub normalization_errors: Vec<ActionNormalizationError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionSecretRefStatus {
    Resolved,
    Unresolved,
    Missing,
    Stale,
    Unsupported,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSecretRefEvidence {
    pub secret_ref: SecretRef,
    pub redaction_evidence: RedactionEvidence,
    pub status: PermissionSecretRefStatus,
    pub requested_consumer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDecisionInput {
    pub action: PermissionedAction,
    #[serde(default)]
    pub prior_approval_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics_ref: Option<String>,
}

pub fn normalize_runtime_tool_call(
    registry: &ToolRegistry,
    call: &RuntimeToolCall,
    input: PermissionedActionInput,
) -> PermissionedAction {
    normalize_tool_candidate(
        registry,
        &call.id,
        &call.name,
        &call.arguments,
        input,
        false,
    )
}

pub(crate) fn normalize_prepared_runtime_tool_call(
    registry: &ToolRegistry,
    call: &RuntimeToolCall,
    input: PermissionedActionInput,
) -> PermissionedAction {
    normalize_tool_candidate(registry, &call.id, &call.name, &call.arguments, input, true)
}

pub fn normalize_resolved_deferred_tool_call(
    registry: &ToolRegistry,
    call: &ResolvedDeferredToolCall,
    mut input: PermissionedActionInput,
) -> PermissionedAction {
    let parent_origin = input.origin;
    input.origin = PermissionedActionOrigin::DeferredBridge {
        original_call_id: call.original_call_id.clone(),
        bridge_name: call.bridge_name.clone(),
        scope_digest: call.scope_digest.clone(),
        parent_origin: Box::new(parent_origin),
    };
    normalize_tool_candidate(
        registry,
        &call.original_call_id,
        &call.underlying_name,
        &call.underlying_arguments,
        input,
        false,
    )
}

fn normalize_tool_candidate(
    registry: &ToolRegistry,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &Value,
    input: PermissionedActionInput,
    prepared: bool,
) -> PermissionedAction {
    let input = sanitize_input(input);
    let mut normalization_errors = Vec::new();
    let redacted_arguments = checked_redacted_arguments(arguments, &mut normalization_errors);
    let secret_ref_evidence =
        secret_ref_evidence_from_arguments(arguments, tool_name, &mut normalization_errors);
    let argument_digest = digest_json(&redacted_arguments);
    let mut capabilities = infer_capabilities(registry, tool_name);
    if capabilities.is_empty()
        && tool_name != "ask_user"
        && registry.get(tool_name).is_some_and(|tool| tool.read_only())
    {
        capabilities.push(SafetyCapability::FsRead);
    }
    capabilities.sort_by_key(|capability| capability_label(*capability));
    capabilities.dedup();
    let target_refs = target_refs_from_arguments(&redacted_arguments);
    let action_digest = digest_json(&action_digest_material(
        tool_name,
        &argument_digest,
        &target_refs,
        &capabilities,
        &secret_ref_evidence,
    ));
    let snapshot_digest = digest_json(&json!({
        "permission_mode_snapshot": &input.permission_mode_snapshot,
        "containment_snapshot": &input.containment_snapshot,
        "intent_snapshot": &input.intent_snapshot,
        "origin": &input.origin,
        "session_id": &input.session_id,
        "turn_id": &input.turn_id,
    }));
    let provider_tool_call_id = non_empty_redacted(tool_call_id);
    if provider_tool_call_id.is_none() {
        normalization_errors.push(ActionNormalizationError::MissingToolCallId);
    }
    if !prepared && registry.has(tool_name) {
        if let Err(detail) = registry.prepare_call(tool_name, arguments.clone()) {
            normalization_errors.push(ActionNormalizationError::InvalidArguments {
                tool_name: tool_name.to_owned(),
                detail: redact_string(&detail),
            });
        }
    } else if !prepared {
        normalization_errors.push(ActionNormalizationError::UnknownTool {
            tool_name: tool_name.to_owned(),
        });
    }
    let action_id = action_id(
        tool_name,
        provider_tool_call_id.as_deref(),
        &action_digest,
        &snapshot_digest,
    );
    let policy_safety_snapshot_ref = match policy_safety_snapshot_ref(
        &input,
        &capabilities,
        input.containment_snapshot.clone(),
        &action_digest,
    ) {
        Ok(reference) => Some(reference),
        Err(error) => {
            normalization_errors.push(ActionNormalizationError::PolicySafetySnapshotInvalid {
                detail: redact_string(&error.to_string()),
            });
            None
        }
    };
    let normalization_state = normalization_state(&normalization_errors);

    PermissionedAction {
        action_id,
        provider_tool_call_id,
        session_id: input.session_id,
        turn_id: input.turn_id,
        tool_name: tool_name.to_owned(),
        capabilities,
        target_refs,
        action_digest,
        argument_digest,
        snapshot_digest,
        policy_safety_snapshot_ref,
        origin: input.origin,
        permission_mode_snapshot: input.permission_mode_snapshot,
        containment_snapshot: input.containment_snapshot,
        intent_snapshot: input.intent_snapshot,
        redacted_arguments,
        secret_ref_evidence,
        normalization_state,
        normalization_errors,
    }
}

fn action_digest_material(
    tool_name: &str,
    argument_digest: &str,
    target_refs: &[TargetRef],
    capabilities: &[SafetyCapability],
    secret_ref_evidence: &[PermissionSecretRefEvidence],
) -> Value {
    let mut material = Map::from_iter([
        ("tool_name".to_owned(), json!(tool_name)),
        ("argument_digest".to_owned(), json!(argument_digest)),
        ("target_refs".to_owned(), json!(target_refs)),
        ("capabilities".to_owned(), json!(capabilities)),
    ]);
    if !secret_ref_evidence.is_empty() {
        material.insert(
            "secret_ref_evidence".to_owned(),
            secret_ref_correlation_material(secret_ref_evidence),
        );
    }
    Value::Object(material)
}

fn secret_ref_evidence_from_arguments(
    arguments: &Value,
    tool_name: &str,
    normalization_errors: &mut Vec<ActionNormalizationError>,
) -> Vec<PermissionSecretRefEvidence> {
    let Some(secret_refs_value) = arguments.get("secret_refs") else {
        return Vec::new();
    };
    let Some(secret_refs) = secret_refs_value.as_array() else {
        normalization_errors.push(ActionNormalizationError::SecretRefMalformed {
            detail: "secret_refs must be an array".to_owned(),
        });
        return Vec::new();
    };
    secret_refs
        .iter()
        .filter_map(|value| match SecretRef::from_value(value.clone()) {
            Ok(secret_ref) => {
                let status = secret_ref_status(&secret_ref);
                let safe_summary_digest = digest_json(
                    &serde_json::to_value(&secret_ref.safe_summary)
                        .unwrap_or_else(|_| json!({ "summary": REDACTED })),
                );
                let evidence_id = RedactionEvidenceRef::new(format!(
                    "red_{}_{}",
                    secret_ref.ref_id.as_str(),
                    safe_summary_digest.chars().take(12).collect::<String>()
                ));
                Some(PermissionSecretRefEvidence {
                    redaction_evidence: RedactionEvidence::for_secret_ref(
                        evidence_id,
                        secret_ref.ref_id.clone(),
                        "permission_action",
                        safe_summary_digest,
                    ),
                    secret_ref,
                    status,
                    requested_consumer: format!("tool:{tool_name}"),
                })
            }
            Err(error) => {
                normalization_errors.push(ActionNormalizationError::SecretRefMalformed {
                    detail: redact_string(&error.to_string()),
                });
                None
            }
        })
        .collect()
}

fn secret_ref_status(_secret_ref: &SecretRef) -> PermissionSecretRefStatus {
    PermissionSecretRefStatus::Unresolved
}

pub(crate) fn secret_ref_correlation_material(evidence: &[PermissionSecretRefEvidence]) -> Value {
    Value::Array(
        evidence
            .iter()
            .map(|item| {
                json!({
                    "ref_id": item.secret_ref.ref_id.as_str(),
                    "source_kind": item.secret_ref.source_kind,
                    "locator_digest": item.secret_ref.locator_digest,
                    "staleness_token": item.secret_ref.staleness_token,
                    "safe_summary": item.secret_ref.safe_summary,
                    "redaction_evidence": item.redaction_evidence,
                    "status": item.status,
                    "requested_consumer": item.requested_consumer,
                })
            })
            .collect(),
    )
}

fn policy_safety_snapshot_ref(
    input: &PermissionedActionInput,
    capabilities: &[SafetyCapability],
    containment: Option<ContainmentSnapshotRef>,
    action_digest: &str,
) -> Result<PolicySafetySnapshotRef, PolicySafetySnapshotError> {
    let snapshot = PolicySafetySnapshot::create(PolicySafetySnapshotInput {
        snapshot_id: format!("permissioned_action_{action_digest}"),
        created_at_unix_ms: 0,
        expires_at_unix_ms: None,
        permission_mode: input.permission_mode_snapshot.clone(),
        capability_ceiling: CapabilityCeilingRef {
            capabilities: capabilities.to_vec(),
        },
        containment,
        source_refs: vec![
            PolicySafetySourceRef {
                kind: PolicySafetySourceKind::RuntimePolicy,
                ref_id: "permission_action_normalization".to_owned(),
                digest: Some(action_digest.to_owned()),
            },
            PolicySafetySourceRef {
                kind: PolicySafetySourceKind::PermissionConfig,
                ref_id: input
                    .permission_mode_snapshot
                    .scope_ref
                    .clone()
                    .unwrap_or_else(|| "default_scope".to_owned()),
                digest: input.permission_mode_snapshot.source.clone(),
            },
        ],
        provenance_refs: vec![PolicySafetyProvenanceRef {
            kind: PolicySafetyProvenanceKind::RuntimeEventRef,
            ref_id: policy_safety_provenance_ref_id(input),
            digest: Some(input.session_id.clone()),
        }],
        creation_reason: PolicySafetySnapshotCreationReason::PermissionedAction,
    });
    snapshot.map(|snapshot| snapshot.reference())
}

fn policy_safety_provenance_ref_id(input: &PermissionedActionInput) -> String {
    match &input.origin {
        PermissionedActionOrigin::ChannelInbound { channel, .. } => format!("channel:{channel}"),
        PermissionedActionOrigin::UserTurn
        | PermissionedActionOrigin::Subagent { .. }
        | PermissionedActionOrigin::CronWake { .. }
        | PermissionedActionOrigin::AppTask { .. }
        | PermissionedActionOrigin::LocalApi { .. }
        | PermissionedActionOrigin::DeferredBridge { .. } => input.turn_id.clone(),
    }
}

fn non_empty_redacted(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| redact_string(trimmed))
}

fn sanitize_input(input: PermissionedActionInput) -> PermissionedActionInput {
    PermissionedActionInput {
        session_id: redact_string(&input.session_id),
        turn_id: redact_string(&input.turn_id),
        origin: sanitize_origin(input.origin),
        permission_mode_snapshot: sanitize_permission_snapshot(input.permission_mode_snapshot),
        containment_snapshot: input
            .containment_snapshot
            .map(sanitize_containment_snapshot),
        intent_snapshot: input.intent_snapshot.map(sanitize_intent_snapshot),
    }
}

fn sanitize_permission_snapshot(snapshot: PermissionModeSnapshot) -> PermissionModeSnapshot {
    PermissionModeSnapshot {
        mode: snapshot.mode,
        source: snapshot.source.map(|value| redact_string(&value)),
        scope_ref: snapshot.scope_ref.map(|value| redact_string(&value)),
    }
}

fn sanitize_containment_snapshot(snapshot: ContainmentSnapshotRef) -> ContainmentSnapshotRef {
    ContainmentSnapshotRef {
        contained: snapshot.contained,
        backend: snapshot.backend.map(|value| redact_string(&value)),
        digest: snapshot.digest.map(|value| redact_string(&value)),
        summary: snapshot.summary.map(|value| redact_string(&value)),
    }
}

fn sanitize_intent_snapshot(snapshot: IntentSnapshotRef) -> IntentSnapshotRef {
    IntentSnapshotRef {
        intent_id: snapshot.intent_id.map(|value| redact_string(&value)),
        digest: snapshot.digest.map(|value| redact_string(&value)),
        summary: snapshot.summary.map(|value| redact_string(&value)),
    }
}

fn sanitize_origin(origin: PermissionedActionOrigin) -> PermissionedActionOrigin {
    match origin {
        PermissionedActionOrigin::UserTurn => PermissionedActionOrigin::UserTurn,
        PermissionedActionOrigin::Subagent { subagent_id } => PermissionedActionOrigin::Subagent {
            subagent_id: subagent_id.map(|value| redact_string(&value)),
        },
        PermissionedActionOrigin::CronWake { job_id } => PermissionedActionOrigin::CronWake {
            job_id: job_id.map(|value| redact_string(&value)),
        },
        PermissionedActionOrigin::AppTask { app_id, task_id } => {
            PermissionedActionOrigin::AppTask {
                app_id: app_id.map(|value| redact_string(&value)),
                task_id: task_id.map(|value| redact_string(&value)),
            }
        }
        PermissionedActionOrigin::LocalApi { request_id } => PermissionedActionOrigin::LocalApi {
            request_id: request_id.map(|value| redact_string(&value)),
        },
        PermissionedActionOrigin::ChannelInbound {
            channel,
            message_id,
        } => PermissionedActionOrigin::ChannelInbound {
            channel: redact_string(&channel),
            message_id: message_id.map(|value| redact_string(&value)),
        },
        PermissionedActionOrigin::DeferredBridge {
            original_call_id,
            bridge_name,
            scope_digest,
            parent_origin,
        } => PermissionedActionOrigin::DeferredBridge {
            original_call_id: redact_string(&original_call_id),
            bridge_name: redact_string(&bridge_name),
            scope_digest: redact_string(&scope_digest),
            parent_origin: Box::new(sanitize_origin(*parent_origin)),
        },
    }
}

fn normalization_state(errors: &[ActionNormalizationError]) -> ActionNormalizationState {
    if errors.is_empty() || errors == [ActionNormalizationError::MissingToolCallId] {
        return ActionNormalizationState::Ready;
    }
    if errors
        .iter()
        .any(|error| matches!(error, ActionNormalizationError::UnknownTool { .. }))
    {
        ActionNormalizationState::DenyCandidate
    } else {
        ActionNormalizationState::ErrorCandidate
    }
}

fn action_id(
    tool_name: &str,
    provider_tool_call_id: Option<&str>,
    action_digest: &str,
    snapshot_digest: &str,
) -> String {
    let digest = digest_json(&json!({
        "tool_name": tool_name,
        "provider_tool_call_id": provider_tool_call_id,
        "action_digest": action_digest,
        "snapshot_digest": snapshot_digest,
    }));
    format!("action_{}", digest.chars().take(32).collect::<String>())
}

fn infer_capabilities(registry: &ToolRegistry, tool_name: &str) -> Vec<SafetyCapability> {
    if registry
        .get(tool_name)
        .is_some_and(|tool| tool.process_adapter_kind().is_some())
    {
        return vec![SafetyCapability::ProcExec];
    }
    match tool_name {
        "read_file" | "list_dir" | "glob" | "grep" | "notebook_read" => {
            vec![SafetyCapability::FsRead]
        }
        "write_file" | "edit_file" | "notebook_edit" => vec![SafetyCapability::FsWrite],
        "exec" => vec![SafetyCapability::ProcExec],
        "web_fetch" | "web_search" => vec![SafetyCapability::NetOutbound],
        "message" => vec![SafetyCapability::ExternalDelivery],
        "cron" => vec![SafetyCapability::AutomationSchedule],
        "spawn" => vec![SafetyCapability::ProcExec],
        "my" | "self" => vec![SafetyCapability::RuntimeConfigWrite],
        "image_generate" => vec![SafetyCapability::NetOutbound],
        name if name.starts_with("mcp_") => vec![SafetyCapability::ProcExec],
        "ask_user" => Vec::new(),
        _ => Vec::new(),
    }
}

fn capability_label(capability: SafetyCapability) -> &'static str {
    match capability {
        SafetyCapability::FsRead => "fs_read",
        SafetyCapability::FsWrite => "fs_write",
        SafetyCapability::ProcExec => "proc_exec",
        SafetyCapability::NetOutbound => "net_outbound",
        SafetyCapability::SecretRead => "secret_read",
        SafetyCapability::ExternalDelivery => "external_delivery",
        SafetyCapability::AutomationSchedule => "automation_schedule",
        SafetyCapability::AppInstall => "app_install",
        SafetyCapability::RuntimeConfigWrite => "runtime_config_write",
        SafetyCapability::SelfModification => "self_modification",
    }
}

fn checked_redacted_arguments(
    arguments: &Value,
    normalization_errors: &mut Vec<ActionNormalizationError>,
) -> Value {
    let redacted = redact_value(arguments);
    canonicalize_value(&replace_residual_secrets(
        &redacted,
        "arguments",
        normalization_errors,
    ))
}

fn replace_residual_secrets(
    value: &Value,
    path: &str,
    normalization_errors: &mut Vec<ActionNormalizationError>,
) -> Value {
    match value {
        Value::String(text) if contains_residual_secret(text) => {
            normalization_errors.push(ActionNormalizationError::UnsafeRawSecret {
                field: path.to_owned(),
            });
            normalization_errors.push(ActionNormalizationError::RedactionFailed {
                field: path.to_owned(),
            });
            Value::String(REDACTED.to_owned())
        }
        Value::String(text) => Value::String(text.clone()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    replace_residual_secrets(
                        item,
                        &format!("{path}[{index}]"),
                        normalization_errors,
                    )
                })
                .collect(),
        ),
        Value::Object(object) => {
            let mut replaced = Map::new();
            for (key, item) in object {
                replaced.insert(
                    key.clone(),
                    replace_residual_secrets(item, &format!("{path}.{key}"), normalization_errors),
                );
            }
            Value::Object(replaced)
        }
        other => other.clone(),
    }
}

fn contains_residual_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_authorization_scheme(&lower, "basic")
        || contains_authorization_scheme(&lower, "bearer")
        || lower.starts_with("basic ")
        || lower.starts_with("sk-")
        || lower.contains(" sk-")
        || lower.starts_with("ghp_")
        || lower.contains(" ghp_")
        || lower.starts_with("github_pat_")
        || lower.contains(" github_pat_")
        || lower.starts_with("xoxb-")
        || lower.contains(" xoxb-")
        || lower.starts_with("xoxp-")
        || lower.contains(" xoxp-")
        || lower.starts_with("ya29.")
        || lower.contains(" ya29.")
}

fn contains_authorization_scheme(text: &str, scheme: &str) -> bool {
    let mut remaining = text;

    while let Some(index) = remaining.find("authorization:") {
        let after_colon = &remaining[index + "authorization:".len()..];
        let after_ows =
            after_colon.trim_start_matches(|character: char| character.is_ascii_whitespace());
        if let Some(remainder) = after_ows.strip_prefix(scheme) {
            if remainder.is_empty()
                || remainder
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_whitespace())
            {
                return true;
            }
        }
        remaining = after_colon;
    }

    false
}

fn target_refs_from_arguments(arguments: &Value) -> Vec<TargetRef> {
    let Some(object) = arguments.as_object() else {
        return Vec::new();
    };
    let mut targets = object
        .iter()
        .filter(|(key, _)| is_target_key(key))
        .map(|(key, value)| TargetRef {
            kind: key.to_owned(),
            digest: digest_json(value),
            redacted_value: canonicalize_value(value),
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then(left.digest.cmp(&right.digest))
    });
    targets
}

fn is_target_key(key: &str) -> bool {
    matches!(
        key,
        "path"
            | "file"
            | "file_path"
            | "directory"
            | "url"
            | "command"
            | "channel"
            | "chat_id"
            | "name"
    )
}

fn digest_json(value: &Value) -> String {
    let canonical = canonicalize_value(value);
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let mut canonical = Map::new();
            for key in keys {
                if let Some(value) = object.get(key) {
                    canonical.insert(key.clone(), canonicalize_value(value));
                }
            }
            Value::Object(canonical)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        other => other.clone(),
    }
}
