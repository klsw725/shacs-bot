use serde_json::{json, Value};
use shacs_core::runtime::{
    normalize_resolved_deferred_tool_call, normalize_runtime_tool_call, ActionNormalizationError,
    ActionNormalizationState, CapabilityCeilingRef, ContainmentSnapshotRef, PermissionMode,
    PermissionModeSnapshot, PermissionSecretRefStatus, PermissionedActionInput,
    PermissionedActionOrigin, PolicySafetyProvenanceKind, PolicySafetyProvenanceRef,
    PolicySafetySnapshot, PolicySafetySnapshotCreationReason, PolicySafetySnapshotError,
    PolicySafetySnapshotInput, PolicySafetySourceKind, PolicySafetySourceRef,
    ResolvedDeferredToolCall, RuntimeToolCall, SafetyCapability as PermissionedSafetyCapability,
};
use shacs_core::tools::{
    AskUserTool, JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolRegistry,
    ToolResult,
};
use std::error::Error;

const SPEC030_SECRET_VALUE: &str = "sk-spec030-raw-secret";

struct EchoTool;

struct ReadOnlyCustomTool;

struct PathTool(&'static str);

impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo a message."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("message", StringSchema::new("Message").min_length(1))
            .required(["message"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let message = match params.get("message").and_then(Value::as_str) {
            Some(message) => message.to_owned(),
            None => String::new(),
        };
        ToolResult::Text(message)
    }
}

impl Tool for ReadOnlyCustomTool {
    fn name(&self) -> &str {
        "custom_lookup"
    }

    fn description(&self) -> &str {
        "Look up custom data."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text("ok".to_owned())
    }
}

impl Tool for PathTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "Operate on a path."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", StringSchema::new("Path").min_length(1))
            .required(["path"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = match params.get("path").and_then(Value::as_str) {
            Some(path) => path.to_owned(),
            None => String::new(),
        };
        ToolResult::Text(path)
    }
}

fn registry_with_echo() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(EchoTool);
    registry
}

fn input() -> PermissionedActionInput {
    PermissionedActionInput {
        session_id: "session-1".to_owned(),
        turn_id: "turn-1".to_owned(),
        origin: PermissionedActionOrigin::UserTurn,
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Default,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        containment_snapshot: Some(ContainmentSnapshotRef {
            contained: Some(true),
            backend: None,
            digest: Some("container-a".to_owned()),
            summary: Some("docker".to_owned()),
        }),
        intent_snapshot: None,
    }
}

fn secret_ref_value(ref_id: &str, label: &str, token: &str) -> Value {
    json!({
        "kind": "secret_ref",
        "schema_version": 1,
        "ref_id": ref_id,
        "source_kind": "env",
        "locator": {
            "kind": "env_var",
            "name": label,
        },
        "owner": "spec035-config-profile",
        "scope": "provider-auth",
        "created_by": "config-profile",
        "created_at_ms": 0,
        "locator_digest": "sha256:current-token",
        "staleness_token": token,
        "safe_summary": {
            "label": format!("env:{label}"),
            "required": true,
        },
    })
}

#[test]
fn direct_tool_call_normalizes_to_permissioned_action() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        input(),
    );

    if action.normalization_state != ActionNormalizationState::Ready
        || action.provider_tool_call_id.as_deref() != Some("call-1")
        || action.tool_name != "echo"
        || action.session_id != "session-1"
        || action.turn_id != "turn-1"
        || action.action_digest.len() != 64
        || action.argument_digest.len() != 64
        || action.snapshot_digest.len() != 64
        || match action.policy_safety_snapshot_ref.as_ref() {
            Some(snapshot_ref) => snapshot_ref.policy_safety_digest.0.len() != 64,
            None => true,
        }
        || action.action_id.is_empty()
    {
        return Err(format!("unexpected normalized action: {action:?}").into());
    }
    Ok(())
}

#[test]
fn policy_safety_snapshot_construction_failure_blocks_action_ready() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let mut input = input();
    input.turn_id = String::new();

    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        input,
    );

    if action.normalization_state == ActionNormalizationState::Ready
        || !action.normalization_errors.iter().any(|error| {
            matches!(
                error,
                ActionNormalizationError::PolicySafetySnapshotInvalid { .. }
            )
        })
    {
        return Err(format!(
            "policy safety snapshot construction failure should block readiness: {action:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn legacy_snapshot_digest_is_pinned_for_permission_context() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();

    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        input(),
    );

    if action.argument_digest != "9b2d43affbf49a367028df2e1414f84c0e099ac98c3d54a8a80157fd7771af25"
        || action.action_digest
            != "2da5b7acdd901d02bcfc328f502dd4ab497e33ecd84c32862f6488b6a6e132aa"
        || action.snapshot_digest
            != "59e0546dd2273ce462bf1da0c92ba07ebd27ff46ad76bd402b1e20f80eff7ae6"
        || !action.action_id.starts_with("action_")
        || action.action_id.len() != "action_".len() + 32
    {
        return Err(format!("legacy permission digest drifted: {action:?}").into());
    }
    Ok(())
}

#[test]
fn registered_read_only_custom_tool_normalizes_to_fs_read() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(ReadOnlyCustomTool);
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-custom", "custom_lookup", json!({})),
        input(),
    );

    if action.normalization_state != ActionNormalizationState::Ready
        || action.capabilities != vec![PermissionedSafetyCapability::FsRead]
    {
        return Err(format!("read-only custom tool was not fs_read: {action:?}").into());
    }
    Ok(())
}

#[test]
fn missing_id_fallback_is_stable_across_object_key_order() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let first = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("", "echo", json!({ "message": "hello", "z": 1 })),
        input(),
    );
    let second = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(" ", "echo", json!({ "z": 1, "message": "hello" })),
        input(),
    );

    if first.provider_tool_call_id.is_some()
        || second.provider_tool_call_id.is_some()
        || first.action_id != second.action_id
        || first.action_digest != second.action_digest
        || first.argument_digest != second.argument_digest
        || !first
            .normalization_errors
            .contains(&ActionNormalizationError::MissingToolCallId)
    {
        return Err(format!("missing id fallback was not stable: {first:?} {second:?}").into());
    }
    Ok(())
}

#[test]
fn action_digest_captures_target_refs_and_capability_set() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(PathTool("read_file"));
    registry.register(PathTool("write_file"));

    let read_action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("read-1", "read_file", json!({ "path": "src/lib.rs" })),
        input(),
    );
    let write_action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("write-1", "write_file", json!({ "path": "src/lib.rs" })),
        input(),
    );
    let other_target = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("read-2", "read_file", json!({ "path": "src/main.rs" })),
        input(),
    );

    if read_action.action_digest.len() != 64
        || read_action.target_refs.len() != 1
        || read_action.target_refs[0].kind != "path"
        || read_action.capabilities != vec![PermissionedSafetyCapability::FsRead]
        || write_action.capabilities != vec![PermissionedSafetyCapability::FsWrite]
        || read_action.action_digest == write_action.action_digest
        || read_action.action_digest == other_target.action_digest
    {
        return Err(
            format!("action digest did not capture target/capability material: {read_action:?} {write_action:?} {other_target:?}")
                .into(),
        );
    }
    Ok(())
}

#[test]
fn deferred_mcp_tool_maps_to_proc_exec_capability() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(PathTool("mcp_echo_lookup"));
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("mcp-1", "mcp_echo_lookup", json!({ "path": "query" })),
        input(),
    );

    if action.capabilities != vec![PermissionedSafetyCapability::ProcExec] {
        return Err(format!("MCP tool capability drifted: {action:?}").into());
    }
    Ok(())
}

#[test]
fn mcp_ask_user_maps_to_proc_exec_capability() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(PathTool("mcp_ask_user"));
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("mcp-ask-1", "mcp_ask_user", json!({ "path": "question" })),
        input(),
    );

    if action.capabilities != vec![PermissionedSafetyCapability::ProcExec] {
        return Err(format!("mcp_ask_user capability drifted: {action:?}").into());
    }
    Ok(())
}

#[test]
fn unknown_tool_is_explicit_deny_candidate() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-unknown",
            "does_not_exist",
            json!({ "path": "src/lib.rs" }),
        ),
        input(),
    );

    if action.normalization_state != ActionNormalizationState::DenyCandidate
        || !action
            .normalization_errors
            .contains(&ActionNormalizationError::UnknownTool {
                tool_name: "does_not_exist".to_owned(),
            })
    {
        return Err(format!("unknown tool was not explicit: {action:?}").into());
    }
    Ok(())
}

#[test]
fn malformed_non_object_arguments_are_error_candidates() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-bad", "echo", json!("not an object")),
        input(),
    );

    if action.normalization_state != ActionNormalizationState::ErrorCandidate
        || !action
            .normalization_errors
            .iter()
            .any(|error| matches!(error, ActionNormalizationError::InvalidArguments { .. }))
    {
        return Err(format!("malformed args were not captured: {action:?}").into());
    }
    Ok(())
}

#[test]
fn redacted_argument_snapshot_does_not_expose_raw_secret() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-secret",
            "echo",
            json!({ "message": "hello", "api_key": "sk-raw-secret" }),
        ),
        input(),
    );
    let serialized = serde_json::to_string(&action)?;

    if serialized.contains("sk-raw-secret") || action.redacted_arguments["api_key"] != "[REDACTED]"
    {
        return Err(format!("secret leaked into permission action: {serialized}").into());
    }
    Ok(())
}

#[test]
fn spec030_action_normalization_projects_secret_ref_evidence_without_raw_values(
) -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-secret-ref",
            "echo",
            json!({
                "message": "hello",
                "secret_refs": [secret_ref_value(
                    "sec_spec030_env",
                    "SPEC030_API_KEY",
                    "opaque-owner-state-a",
                )],
                "note": SPEC030_SECRET_VALUE,
            }),
        ),
        input(),
    );
    let serialized = serde_json::to_string(&action)?;

    if action.normalization_state != ActionNormalizationState::Ready
        || action.secret_ref_evidence.len() != 1
        || action.secret_ref_evidence[0].secret_ref.ref_id.as_str() != "sec_spec030_env"
        || action.secret_ref_evidence[0].status != PermissionSecretRefStatus::Unresolved
        || action.secret_ref_evidence[0].requested_consumer != "tool:echo"
        || action.secret_ref_evidence[0]
            .redaction_evidence
            .raw_value_persisted
        || !serialized.contains("sec_spec030_env")
        || !serialized.contains("env:SPEC030_API_KEY")
        || serialized.contains(SPEC030_SECRET_VALUE)
    {
        return Err(format!("secret ref evidence was not safe: {serialized}").into());
    }
    Ok(())
}

#[test]
fn spec030_secret_ref_summary_redacts_raw_looking_label_and_locator() -> Result<(), Box<dyn Error>>
{
    let registry = registry_with_echo();
    let raw_inline_env = "OPENAI_API_KEY=sk-spec030-inline-secret";
    let raw_private_key = "-----BEGIN PRIVATE KEY-----spec030-----END PRIVATE KEY-----";
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-secret-ref-unsafe-summary",
            "echo",
            json!({
                "message": "hello",
                "secret_refs": [{
                    "kind": "secret_ref",
                    "schema_version": 1,
                    "ref_id": "sec_spec030_unsafe_summary",
                    "source_kind": "env",
                    "locator": {"kind": "env_var", "name": raw_inline_env},
                    "owner": "spec035-config-profile",
                    "scope": "provider-auth",
                    "created_by": "config-profile",
                    "created_at_ms": 0,
                    "locator_digest": "sha256:unsafe-summary-locator",
                    "staleness_token": "opaque-owner-state-b",
                    "safe_summary": {"label": raw_private_key, "required": true},
                }],
            }),
        ),
        input(),
    );
    let serialized = serde_json::to_string(&action)?;

    if action.normalization_state != ActionNormalizationState::Ready
        || action.secret_ref_evidence.len() != 1
        || action.secret_ref_evidence[0].secret_ref.safe_summary.label != "[REDACTED]"
        || !serialized.contains("[REDACTED]")
        || serialized.contains("sk-spec030-inline-secret")
        || serialized.contains("BEGIN PRIVATE KEY")
    {
        return Err(format!(
            "unsafe safe_summary/locator leaked through action evidence: {serialized}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn spec030_secret_refs_non_array_fails_before_action_ready() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-secret-ref-non-array",
            "echo",
            json!({
                "message": "hello",
                "secret_refs": {"unexpected": true},
            }),
        ),
        input(),
    );

    if action.normalization_state == ActionNormalizationState::Ready
        || !action
            .normalization_errors
            .iter()
            .any(|error| matches!(error, ActionNormalizationError::SecretRefMalformed { .. }))
    {
        return Err(format!("non-array secret_refs should block readiness: {action:?}").into());
    }
    Ok(())
}

#[test]
fn spec030_secret_ref_staleness_token_is_opaque_and_not_self_resolved() -> Result<(), Box<dyn Error>>
{
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-secret-ref-opaque-token",
            "echo",
            json!({
                "message": "hello",
                "secret_refs": [secret_ref_value(
                    "sec_spec030_opaque",
                    "SPEC030_API_KEY",
                    "sha256:current-token",
                )],
            }),
        ),
        input(),
    );

    if action.normalization_state != ActionNormalizationState::Ready
        || action.secret_ref_evidence[0].status != PermissionSecretRefStatus::Unresolved
        || action
            .normalization_errors
            .iter()
            .any(|error| matches!(error, ActionNormalizationError::SecretRefStale { .. }))
    {
        return Err(
            format!("opaque staleness token was treated as freshness proof: {action:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn spec030_malformed_secret_ref_fails_before_action_ready() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new(
            "call-malformed-secret-ref",
            "echo",
            json!({
                "message": "hello",
                "secret_refs": [{
                    "kind": "secret_ref",
                    "schema_version": 1,
                    "ref_id": "sec_bad",
                    "source_kind": "env",
                    "locator": {"kind": "env_var", "env_value": SPEC030_SECRET_VALUE},
                    "owner": "spec035-config-profile",
                    "scope": "provider-auth",
                    "locator_digest": "sha256:current-token",
                    "staleness_token": "sha256:current-token",
                    "safe_summary": {"label": "env:SPEC030_API_KEY", "required": true},
                }],
            }),
        ),
        input(),
    );
    let serialized = serde_json::to_string(&action)?;

    if action.normalization_state == ActionNormalizationState::Ready
        || !action
            .normalization_errors
            .iter()
            .any(|error| matches!(error, ActionNormalizationError::SecretRefMalformed { .. }))
        || serialized.contains(SPEC030_SECRET_VALUE)
    {
        return Err(format!("malformed secret ref should block readiness: {serialized}").into());
    }
    Ok(())
}

#[test]
fn snapshot_and_origin_material_do_not_expose_raw_secret() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        PermissionedActionInput {
            session_id: "session api_key=sk-session-secret".to_owned(),
            turn_id: "turn bearer sk-turn-secret".to_owned(),
            origin: PermissionedActionOrigin::LocalApi {
                request_id: Some("request token=sk-request-secret".to_owned()),
            },
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::Default,
                source: Some("source password=sk-source-secret".to_owned()),
                scope_ref: Some("scope token=sk-scope-secret".to_owned()),
            },
            containment_snapshot: Some(ContainmentSnapshotRef {
                contained: Some(true),
                backend: Some("backend token=sk-backend-secret".to_owned()),
                digest: Some("container token=sk-container-secret".to_owned()),
                summary: Some("summary bearer sk-summary-secret".to_owned()),
            }),
            intent_snapshot: None,
        },
    );
    let serialized = serde_json::to_string(&action)?;

    for secret in [
        "sk-session-secret",
        "sk-turn-secret",
        "sk-request-secret",
        "sk-source-secret",
        "sk-scope-secret",
        "sk-backend-secret",
        "sk-container-secret",
        "sk-summary-secret",
    ] {
        if serialized.contains(secret) {
            return Err(format!("secret leaked into action snapshot: {serialized}").into());
        }
    }
    Ok(())
}

#[test]
fn snapshot_digest_changes_when_permission_context_changes() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let default_action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        input(),
    );
    let mut changed_input = input();
    changed_input.permission_mode_snapshot.mode = PermissionMode::Auto;
    let changed_action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        changed_input,
    );

    if default_action.snapshot_digest == changed_action.snapshot_digest {
        return Err("snapshot digest did not change with permission mode".into());
    }
    Ok(())
}

#[test]
fn snapshot_digest_changes_when_containment_context_changes() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let default_action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        input(),
    );
    let mut changed_input = input();
    changed_input.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(false),
        backend: None,
        digest: Some("container-b".to_owned()),
        summary: Some("host".to_owned()),
    });
    let changed_action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("call-1", "echo", json!({ "message": "hello" })),
        changed_input,
    );

    if default_action.snapshot_digest == changed_action.snapshot_digest {
        return Err("snapshot digest did not change with containment context".into());
    }
    Ok(())
}

#[test]
fn deferred_bridge_call_normalizes_to_same_envelope_shape() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let action = normalize_resolved_deferred_tool_call(
        &registry,
        &ResolvedDeferredToolCall {
            original_call_id: "bridge-call-1".to_owned(),
            bridge_name: "tool_call".to_owned(),
            underlying_name: "echo".to_owned(),
            underlying_arguments: json!({ "message": "hello" }),
            scope_digest: "scope-123".to_owned(),
        },
        input(),
    );

    if action.tool_name != "echo"
        || action.provider_tool_call_id.as_deref() != Some("bridge-call-1")
        || !matches!(
            action.origin,
            PermissionedActionOrigin::DeferredBridge {
                ref bridge_name,
                ref scope_digest,
                ref parent_origin,
                ..
            } if bridge_name == "tool_call"
                && scope_digest == "scope-123"
                && matches!(**parent_origin, PermissionedActionOrigin::UserTurn)
        )
        || action.normalization_state != ActionNormalizationState::Ready
    {
        return Err(format!("deferred bridge shape drifted: {action:?}").into());
    }
    Ok(())
}

#[test]
fn my_self_tool_maps_to_runtime_config_write() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(PathTool("my"));
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("my-1", "my", json!({ "path": "max_iterations" })),
        input(),
    );

    if action.capabilities != vec![PermissionedSafetyCapability::RuntimeConfigWrite] {
        return Err(format!("my capability mapping drifted: {action:?}").into());
    }
    Ok(())
}

#[test]
fn unsafe_basic_authorization_material_is_not_retained() -> Result<(), Box<dyn Error>> {
    let registry = registry_with_echo();
    let raw_credential = "dXNlcjpwYXNz";
    for message in [
        format!("Authorization:Basic {raw_credential}"),
        format!("Authorization:\tBasic {raw_credential}"),
        format!("Authorization:  Basic {raw_credential}"),
    ] {
        let action = normalize_runtime_tool_call(
            &registry,
            &RuntimeToolCall::new("call-basic", "echo", json!({ "message": message })),
            input(),
        );
        let serialized = serde_json::to_string(&action)?;

        if serialized.contains(raw_credential)
            || action.redacted_arguments["message"] != "[REDACTED]"
            || action.normalization_state != ActionNormalizationState::ErrorCandidate
            || !action
                .normalization_errors
                .iter()
                .any(|error| matches!(error, ActionNormalizationError::UnsafeRawSecret { .. }))
            || !action
                .normalization_errors
                .iter()
                .any(|error| matches!(error, ActionNormalizationError::RedactionFailed { .. }))
        {
            return Err(
                format!("basic authorization was not safely represented: {serialized}").into(),
            );
        }
    }
    Ok(())
}

#[test]
fn ask_user_is_tool_action_not_formal_approval() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    let action = normalize_runtime_tool_call(
        &registry,
        &RuntimeToolCall::new("ask-1", "ask_user", json!({ "question": "Continue?" })),
        input(),
    );
    let serialized = serde_json::to_string(&action)?;

    if action.tool_name != "ask_user"
        || !action.capabilities.is_empty()
        || serialized.contains("approval")
    {
        return Err(format!("ask_user was modeled as approval: {serialized}").into());
    }
    Ok(())
}

fn policy_snapshot_input() -> PolicySafetySnapshotInput {
    PolicySafetySnapshotInput {
        snapshot_id: "snapshot-task5".to_owned(),
        created_at_unix_ms: 1_000,
        expires_at_unix_ms: Some(2_000),
        permission_mode: PermissionModeSnapshot {
            mode: PermissionMode::Default,
            source: Some("policy-profile".to_owned()),
            scope_ref: Some("workspace-ref".to_owned()),
        },
        capability_ceiling: CapabilityCeilingRef {
            capabilities: vec![
                PermissionedSafetyCapability::FsRead,
                PermissionedSafetyCapability::FsWrite,
            ],
        },
        containment: Some(ContainmentSnapshotRef {
            contained: Some(true),
            backend: Some("docker".to_owned()),
            digest: Some("sha256:containment-a".to_owned()),
            summary: Some("contained".to_owned()),
        }),
        source_refs: vec![
            PolicySafetySourceRef {
                kind: PolicySafetySourceKind::RuntimePolicy,
                ref_id: "runtime-policy-a".to_owned(),
                digest: Some("sha256:runtime-policy".to_owned()),
            },
            PolicySafetySourceRef {
                kind: PolicySafetySourceKind::PermissionConfig,
                ref_id: "permission-config-a".to_owned(),
                digest: Some("sha256:permission-config".to_owned()),
            },
        ],
        provenance_refs: vec![PolicySafetyProvenanceRef {
            kind: PolicySafetyProvenanceKind::RuntimeEventRef,
            ref_id: "runtime-event-a".to_owned(),
            digest: Some("sha256:runtime-event".to_owned()),
        }],
        creation_reason: PolicySafetySnapshotCreationReason::PermissionedAction,
    }
}

#[test]
fn policy_safety_snapshot_rejects_unknown_schema() -> Result<(), Box<dyn Error>> {
    let snapshot = PolicySafetySnapshot::create(policy_snapshot_input())?;
    let mut serialized_ref = serde_json::to_value(snapshot.reference())?;
    serialized_ref["schema_id"] = json!("policy_safety_snapshot.v999");

    let result = PolicySafetySnapshot::parse_ref(serialized_ref);

    if !matches!(result, Err(PolicySafetySnapshotError::UnknownSchema { .. })) {
        return Err(format!("unknown schema was not rejected: {result:?}").into());
    }
    Ok(())
}

#[test]
fn foundation_consumer_rejects_missing_policy_safety_ref() -> Result<(), Box<dyn Error>> {
    let result = PolicySafetySnapshot::require_ref(None);

    if !matches!(result, Err(PolicySafetySnapshotError::MissingRef)) {
        return Err(format!("missing ref was not rejected: {result:?}").into());
    }
    Ok(())
}

#[test]
fn policy_safety_snapshot_rejects_digest_mismatch() -> Result<(), Box<dyn Error>> {
    let snapshot = PolicySafetySnapshot::create(policy_snapshot_input())?;
    let mut serialized_ref = serde_json::to_value(snapshot.reference())?;
    serialized_ref["policy_safety_digest"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    let changed_ref = PolicySafetySnapshot::parse_ref(serialized_ref)?;

    let result = snapshot.validate_ref(&changed_ref, 1_500);

    if !matches!(
        result,
        Err(PolicySafetySnapshotError::DigestMismatch { .. })
    ) {
        return Err(format!("digest mismatch was not rejected: {result:?}").into());
    }
    Ok(())
}

#[test]
fn policy_safety_snapshot_rejects_stale_snapshot() -> Result<(), Box<dyn Error>> {
    let snapshot = PolicySafetySnapshot::create(policy_snapshot_input())?;
    let snapshot_ref = snapshot.reference();

    let result = snapshot.validate_ref(&snapshot_ref, 2_001);

    if !matches!(result, Err(PolicySafetySnapshotError::StaleSnapshot { .. })) {
        return Err(format!("stale snapshot was not rejected: {result:?}").into());
    }
    Ok(())
}

#[test]
fn policy_safety_digest_is_stable_under_source_and_provenance_ordering(
) -> Result<(), Box<dyn Error>> {
    let first = PolicySafetySnapshot::create(policy_snapshot_input())?;
    let mut reordered_input = policy_snapshot_input();
    reordered_input.source_refs.reverse();
    reordered_input.provenance_refs.reverse();
    let second = PolicySafetySnapshot::create(reordered_input)?;

    if first.reference().policy_safety_digest != second.reference().policy_safety_digest {
        return Err(
            format!("digest changed after canonical ref ordering: {first:?} {second:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn policy_safety_digest_changes_with_policy_ceiling_containment_and_provenance(
) -> Result<(), Box<dyn Error>> {
    let baseline = PolicySafetySnapshot::create(policy_snapshot_input())?;
    let mut changed_policy = policy_snapshot_input();
    changed_policy.permission_mode.mode = PermissionMode::Auto;
    let mut changed_ceiling = policy_snapshot_input();
    changed_ceiling.capability_ceiling.capabilities = vec![PermissionedSafetyCapability::FsRead];
    let mut changed_containment = policy_snapshot_input();
    changed_containment.containment = Some(ContainmentSnapshotRef {
        contained: Some(false),
        backend: Some("host".to_owned()),
        digest: Some("sha256:containment-b".to_owned()),
        summary: Some("unknown".to_owned()),
    });
    let mut changed_provenance = policy_snapshot_input();
    changed_provenance.provenance_refs = vec![PolicySafetyProvenanceRef {
        kind: PolicySafetyProvenanceKind::DiagnosticsRef,
        ref_id: "diagnostics-a".to_owned(),
        digest: Some("sha256:diagnostics".to_owned()),
    }];

    for changed in [
        PolicySafetySnapshot::create(changed_policy)?,
        PolicySafetySnapshot::create(changed_ceiling)?,
        PolicySafetySnapshot::create(changed_containment)?,
        PolicySafetySnapshot::create(changed_provenance)?,
    ] {
        if baseline.reference().policy_safety_digest == changed.reference().policy_safety_digest {
            return Err(
                format!("digest did not change for changed safety material: {changed:?}").into(),
            );
        }
    }
    Ok(())
}

#[test]
fn policy_safety_ref_serialization_is_raw_safe() -> Result<(), Box<dyn Error>> {
    let mut raw_input = policy_snapshot_input();
    raw_input.source_refs.push(PolicySafetySourceRef {
        kind: PolicySafetySourceKind::ExternalExecutionSnapshotRef,
        ref_id: "/Users/example/.shacs-bot/config-with-sk-raw-secret".to_owned(),
        digest: Some("sha256:raw".to_owned()),
    });

    let result = PolicySafetySnapshot::create(raw_input);

    if !matches!(
        result,
        Err(PolicySafetySnapshotError::RawMaterialRejected { .. })
    ) {
        return Err(
            format!("raw material was not rejected before ref serialization: {result:?}").into(),
        );
    }
    Ok(())
}
