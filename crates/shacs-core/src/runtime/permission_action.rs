use crate::runtime::{ResolvedDeferredToolCall, RuntimeToolCall};
use crate::tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
pub use shacs_config::{PermissionMode, SafetyCapability};
use shacs_utils::redaction::{redact_string, redact_value, REDACTED};

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
    pub origin: PermissionedActionOrigin,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_snapshot: Option<ContainmentSnapshotRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_snapshot: Option<IntentSnapshotRef>,
    pub redacted_arguments: Value,
    pub normalization_state: ActionNormalizationState,
    pub normalization_errors: Vec<ActionNormalizationError>,
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
    normalize_tool_candidate(registry, &call.id, &call.name, &call.arguments, input)
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
    )
}

fn normalize_tool_candidate(
    registry: &ToolRegistry,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &Value,
    input: PermissionedActionInput,
) -> PermissionedAction {
    let input = sanitize_input(input);
    let mut normalization_errors = Vec::new();
    let redacted_arguments = checked_redacted_arguments(arguments, &mut normalization_errors);
    let argument_digest = digest_json(&redacted_arguments);
    let mut capabilities = infer_capabilities(tool_name);
    capabilities.sort_by_key(|capability| capability_label(*capability));
    capabilities.dedup();
    let target_refs = target_refs_from_arguments(&redacted_arguments);
    let action_digest = digest_json(&json!({
        "tool_name": tool_name,
        "argument_digest": argument_digest,
        "target_refs": target_refs,
        "capabilities": capabilities,
    }));
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
    if registry.has(tool_name) {
        if let Err(detail) = registry.prepare_call(tool_name, arguments.clone()) {
            normalization_errors.push(ActionNormalizationError::InvalidArguments {
                tool_name: tool_name.to_owned(),
                detail: redact_string(&detail),
            });
        }
    } else {
        normalization_errors.push(ActionNormalizationError::UnknownTool {
            tool_name: tool_name.to_owned(),
        });
    }
    let normalization_state = normalization_state(&normalization_errors);
    let action_id = action_id(
        tool_name,
        provider_tool_call_id.as_deref(),
        &action_digest,
        &snapshot_digest,
    );

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
        origin: input.origin,
        permission_mode_snapshot: input.permission_mode_snapshot,
        containment_snapshot: input.containment_snapshot,
        intent_snapshot: input.intent_snapshot,
        redacted_arguments,
        normalization_state,
        normalization_errors,
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

fn infer_capabilities(tool_name: &str) -> Vec<SafetyCapability> {
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
        "ask_user" => Vec::new(),
        name if name.starts_with("mcp_") => Vec::new(),
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
