use serde_json::{json, Value};
use shacs_core::runtime::{
    ActionNormalizationState, PermissionMode, PermissionModeSnapshot, PermissionSecretRefEvidence,
    PermissionSecretRefStatus, PermissionedAction, PermissionedActionOrigin, PolicySafetyDigest,
    PolicySafetySnapshotId, PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId,
    ProcessAdapterKind, ProcessEnvelopeError, ProcessExecutionEnvelope,
    ProcessExecutionEnvelopeInput, ProcessIdentity, ProcessRedactedCommand,
    RedactedPolicySafetySummary, SafetyCapability,
};
use shacs_redaction::{
    RedactionEvidence, RedactionEvidenceRef, SafeSecretSummary, SecretLocator, SecretRef,
    SecretRefId, SecretRefKind, SecretSourceKind,
};
use std::error::Error;

fn action() -> PermissionedAction {
    PermissionedAction {
        action_id: "action-exec".to_owned(),
        provider_tool_call_id: Some("call-1".to_owned()),
        session_id: "session-1".to_owned(),
        turn_id: "turn-1".to_owned(),
        tool_name: "exec".to_owned(),
        capabilities: vec![SafetyCapability::ProcExec],
        target_refs: Vec::new(),
        action_digest: "action-digest".to_owned(),
        argument_digest: "argument-digest".to_owned(),
        snapshot_digest: "snapshot-digest".to_owned(),
        policy_safety_snapshot_ref: Some(policy_ref()),
        origin: PermissionedActionOrigin::UserTurn,
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::BypassPermissions,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        containment_snapshot: None,
        intent_snapshot: None,
        redacted_arguments: json!({"cmd":"[REDACTED]"}),
        secret_ref_evidence: vec![secret_evidence()],
        normalization_state: ActionNormalizationState::Ready,
        normalization_errors: Vec::new(),
    }
}

fn policy_ref() -> PolicySafetySnapshotRef {
    PolicySafetySnapshotRef {
        schema_id: PolicySafetySnapshotSchemaId::V1,
        snapshot_id: PolicySafetySnapshotId("snapshot-process".to_owned()),
        policy_safety_digest: PolicySafetyDigest(
            "1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
        ),
        created_at_unix_ms: 100,
        expires_at_unix_ms: None,
        redacted_summary: RedactedPolicySafetySummary {
            permission_mode: "bypass_permissions".to_owned(),
            capability_count: 1,
            containment_digest: None,
            source_ref_count: 1,
            provenance_ref_count: 1,
        },
    }
}

fn secret_evidence() -> PermissionSecretRefEvidence {
    let secret_ref = SecretRef {
        kind: SecretRefKind::SecretRef,
        schema_version: 1,
        ref_id: SecretRefId::new("sec_process"),
        source_kind: SecretSourceKind::Env,
        locator: SecretLocator::EnvVar {
            name: "SPEC030_API_KEY".to_owned(),
        },
        owner: "spec035-config-profile".to_owned(),
        scope: "provider-auth".to_owned(),
        created_by: Some("config-profile".to_owned()),
        created_at_ms: Some(0),
        locator_digest: "sha256:locator".to_owned(),
        staleness_token: "opaque-owner-token".to_owned(),
        safe_summary: SafeSecretSummary {
            label: "env:SPEC030_API_KEY".to_owned(),
            required: true,
        },
    };
    PermissionSecretRefEvidence {
        secret_ref: secret_ref.clone(),
        redaction_evidence: RedactionEvidence::for_secret_ref(
            RedactionEvidenceRef::new("red_process"),
            secret_ref.ref_id,
            "process_envelope",
            "sha256:safe-summary",
        ),
        status: PermissionSecretRefStatus::Unresolved,
        requested_consumer: "process:exec".to_owned(),
    }
}

fn input(action: PermissionedAction) -> ProcessExecutionEnvelopeInput {
    ProcessExecutionEnvelopeInput {
        identity: ProcessIdentity::new("proc-spec030", "session-1", "turn-1"),
        adapter: ProcessAdapterKind::ExecTool,
        action,
        required_secret_ref_count: 1,
        redacted_command: ProcessRedactedCommand {
            command_family: "sh".to_owned(),
            redacted_summary: "noop command".to_owned(),
            redacted_targets: Vec::new(),
        },
    }
}

#[test]
fn process_envelope_rejects_missing_policy_snapshot_ref() -> Result<(), Box<dyn Error>> {
    let mut action = action();
    action.policy_safety_snapshot_ref = None;

    let result = ProcessExecutionEnvelope::try_from_input(input(action));

    assert_eq!(
        result,
        Err(ProcessEnvelopeError::MissingPolicySafetySnapshotRef)
    );
    Ok(())
}

#[test]
fn process_envelope_rejects_missing_secret_ref_evidence() -> Result<(), Box<dyn Error>> {
    let mut action = action();
    action.secret_ref_evidence.clear();

    let result = ProcessExecutionEnvelope::try_from_input(input(action));

    assert_eq!(result, Err(ProcessEnvelopeError::MissingSecretRefs));
    Ok(())
}

#[test]
fn process_envelope_serializes_only_redacted_process_material() -> Result<(), Box<dyn Error>> {
    let envelope = ProcessExecutionEnvelope::try_from_input(input(action()))?;

    let serialized = serde_json::to_value(&envelope)?;

    assert_absent(&serialized, "sk-spec030-raw-token")?;
    assert_absent(&serialized, "stdout raw")?;
    assert_absent(&serialized, "stderr raw")?;
    assert_eq!(serialized["adapter"], "exec_tool");
    assert_eq!(serialized["redacted_command"]["command_family"], "sh");
    Ok(())
}

fn assert_absent(value: &Value, needle: &str) -> Result<(), Box<dyn Error>> {
    let text = serde_json::to_string(value)?;
    if text.contains(needle) {
        return Err(format!("serialized envelope leaked {needle}: {text}").into());
    }
    Ok(())
}
