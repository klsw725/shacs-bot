use serde_json::{json, Value};
use shacs_core::runtime::{
    normalize_resolved_deferred_tool_call, normalize_runtime_tool_call, ActionNormalizationError,
    ActionNormalizationState, ContainmentSnapshotRef, PermissionMode, PermissionModeSnapshot,
    PermissionedActionInput, PermissionedActionOrigin, ResolvedDeferredToolCall, RuntimeToolCall,
    SafetyCapability as PermissionedSafetyCapability,
};
use shacs_core::tools::{
    AskUserTool, JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolRegistry,
    ToolResult,
};
use std::error::Error;

struct EchoTool;

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
            digest: Some("container-a".to_owned()),
            summary: Some("docker".to_owned()),
        }),
        intent_snapshot: None,
    }
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
        || action.action_id.is_empty()
    {
        return Err(format!("unexpected normalized action: {action:?}").into());
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
