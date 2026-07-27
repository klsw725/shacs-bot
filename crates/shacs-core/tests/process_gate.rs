use serde_json::json;
use shacs_core::runtime::{
    ActionNormalizationState, ContainerNetworkMode, ContainerRuntimeKind,
    ContainmentComparisonOutcome, ContainmentPermissionProof,
    ContainmentPermissionProofProjectionInput, DockerContainmentSnapshot,
    InheritedPermissionContext, PermissionCeilingComparisonOutcome, PermissionCeilingSnapshot,
    PermissionMode, PermissionModeSnapshot, PermissionPolicyDecisionKind, PermissionPolicyReason,
    PermissionRuleInput, PermissionSecretRefEvidence, PermissionSecretRefStatus,
    PermissionedAction, PermissionedActionOrigin, PolicySafetyDigest, PolicySafetySnapshotId,
    PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId, ProcExecSummary, ProcessAdapterKind,
    ProcessContainmentProofCandidate, ProcessEnvelopeAdmission, ProcessExecutionEnvelope,
    ProcessExecutionEnvelopeInput, ProcessGate, ProcessGateError, ProcessGateInput,
    ProcessGateTerminalPrecondition, ProcessIdentity, ProcessRedactedCommand,
    ProcessRedactedSpawnSummary, ProcessRedactedStatus, ProcessRedactedStreamKind,
    ProcessRedactedStreamSummary, ProcessSpawnReport, ProcessTerminalOutcome,
    RedactedPolicySafetySummary, RuntimeBoundaryKind, RuntimeBoundaryOrigin, SafetyCapability,
    WorkspaceComparisonOutcome,
};
use shacs_redaction::{
    RedactionEvidence, RedactionEvidenceRef, SafeSecretSummary, SecretLocator, SecretRef,
    SecretRefId, SecretRefKind, SecretSourceKind,
};
use std::{cell::Cell, error::Error};

fn envelope(mode: PermissionMode) -> ProcessExecutionEnvelope {
    ProcessExecutionEnvelope::try_from_input(ProcessExecutionEnvelopeInput {
        identity: ProcessIdentity::new("proc-spec030", "session-1", "turn-1"),
        adapter: ProcessAdapterKind::ExecTool,
        action: action(mode),
        required_secret_ref_count: 1,
        redacted_command: ProcessRedactedCommand {
            command_family: "sh".to_owned(),
            redacted_summary: "noop command".to_owned(),
            redacted_targets: Vec::new(),
        },
    })
    .expect("fixture envelope should be valid")
}

fn action(mode: PermissionMode) -> PermissionedAction {
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
            mode,
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
            "process_gate",
            "sha256:safe-summary",
        ),
        status: PermissionSecretRefStatus::Unresolved,
        requested_consumer: "process:exec".to_owned(),
    }
}

fn process_input(envelope: ProcessExecutionEnvelope) -> ProcessGateInput {
    ProcessGateInput {
        envelope,
        permission_rules: PermissionRuleInput {
            containment: safe_containment(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: "sh".to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
            protected_targets: Vec::new(),
        },
        inherited_context: None,
        evaluator: None,
        approval: None,
        containment_proof: ProcessContainmentProofCandidate::Missing,
        interactive: false,
        terminal_precondition: ProcessGateTerminalPrecondition::Ready,
        now_unix_ms: 200,
    }
}

fn admitted_proof(envelope: &ProcessExecutionEnvelope) -> ContainmentPermissionProof {
    let proof_id = format!("containment-proof:{}", envelope.envelope_id);
    ContainmentPermissionProof {
        proof_id: proof_id.clone(),
        policy_safety_digest: envelope
            .policy_safety_snapshot_ref
            .policy_safety_digest
            .clone(),
        envelope_id: envelope.envelope_id.clone(),
        containment_outcome: ContainmentComparisonOutcome::EqualContainment,
        workspace_outcome: WorkspaceComparisonOutcome::SameScope,
        ceiling_outcome: PermissionCeilingComparisonOutcome::EqualCeiling,
        admission: ProcessEnvelopeAdmission::Admit,
        violations: Vec::new(),
        diagnostics_input: ContainmentPermissionProofProjectionInput {
            proof_id,
            envelope_id: envelope.envelope_id.clone(),
            policy_safety_digest: envelope
                .policy_safety_snapshot_ref
                .policy_safety_digest
                .clone(),
            parent_boundary_kind: RuntimeBoundaryKind::UserTurn,
            child_boundary_kind: RuntimeBoundaryKind::ExecTool,
            admission: ProcessEnvelopeAdmission::Admit,
            redacted_summary: "boundary=ExecTool; admission=Admit".to_owned(),
        },
        blocked_external_surface: None,
    }
}

fn safe_containment() -> DockerContainmentSnapshot {
    DockerContainmentSnapshot {
        contained: Some(true),
        runtime: ContainerRuntimeKind::Docker,
        root_user: Some(false),
        privileged: Some(false),
        host_mounts_summary: vec!["workspace".to_owned()],
        network_mode: ContainerNetworkMode::Bridge,
        digest: Some("container-digest".to_owned()),
        summary: Some("docker non-root".to_owned()),
    }
}

#[test]
fn process_gate_never_spawns_without_containment_proof() -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let input = process_input(envelope(PermissionMode::BypassPermissions));

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    Ok(())
}

#[test]
fn process_gate_never_spawns_when_containment_proof_envelope_mismatches(
) -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut input = process_input(envelope(PermissionMode::BypassPermissions));
    let mut proof = admitted_proof(&input.envelope);
    proof.envelope_id = "process:other-session:other-turn:other-action".to_owned();
    input.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(proof));

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    Ok(())
}

#[test]
fn process_gate_never_spawns_when_containment_proof_policy_digest_mismatches(
) -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut input = process_input(envelope(PermissionMode::BypassPermissions));
    let mut proof = admitted_proof(&input.envelope);
    proof.policy_safety_digest = PolicySafetyDigest(
        "2222222222222222222222222222222222222222222222222222222222222222".to_owned(),
    );
    input.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(proof));

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    Ok(())
}

#[test]
fn process_gate_never_spawns_on_denied_policy() -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut input = process_input(envelope(PermissionMode::DontAsk));
    if let Some(summary) = input.permission_rules.proc_exec_summary.as_mut() {
        summary.destructive = true;
    }

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(
        receipt.policy_decision.kind,
        PermissionPolicyDecisionKind::Deny
    );
    assert_eq!(
        receipt.policy_decision.reason,
        PermissionPolicyReason::StaticDeny
    );
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    Ok(())
}

#[test]
fn process_gate_never_spawns_on_widened_ceiling() -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut input = process_input(envelope(PermissionMode::BypassPermissions));
    input.inherited_context = Some(InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: vec!["approval-1".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        },
        requested_mode: PermissionMode::BypassPermissions,
        requested_capabilities: vec![SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    });

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(
        receipt.policy_decision.reason,
        PermissionPolicyReason::CeilingViolation
    );
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    Ok(())
}

#[test]
fn process_gate_never_spawns_on_replay_timeout_cancel_or_repeated_interruption(
) -> Result<(), Box<dyn Error>> {
    for precondition in [
        ProcessGateTerminalPrecondition::Replay,
        ProcessGateTerminalPrecondition::TimedOut,
        ProcessGateTerminalPrecondition::Cancelled,
        ProcessGateTerminalPrecondition::InterruptedAgain,
    ] {
        let counter = Cell::new(0);
        let mut input = process_input(envelope(PermissionMode::BypassPermissions));
        input.terminal_precondition = precondition;

        let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
            counter.set(counter.get() + 1);
            ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
        })?;

        assert_eq!(counter.get(), 0);
        assert_eq!(receipt.dispatch_count, 0);
        assert!(receipt.terminal_outcome != ProcessTerminalOutcome::Succeeded);
    }
    Ok(())
}

#[test]
fn process_gate_allowed_noop_yields_typed_redacted_receipt() -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut input = process_input(envelope(PermissionMode::BypassPermissions));
    input.containment_proof =
        ProcessContainmentProofCandidate::Proof(Box::new(admitted_proof(&input.envelope)));

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |authorization| {
        counter.set(counter.get() + 1);
        assert_eq!(
            authorization.envelope().adapter,
            ProcessAdapterKind::ExecTool
        );
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    let serialized = serde_json::to_string(&receipt)?;
    assert_eq!(counter.get(), 1);
    assert_eq!(receipt.dispatch_count, 1);
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Succeeded);
    assert_eq!(receipt.redacted_command.command_family, "sh");
    assert!(!serialized.contains("sk-spec030-raw-token"));
    assert!(!serialized.contains("stdout raw"));
    assert!(!serialized.contains("stderr raw"));
    Ok(())
}

#[test]
fn process_gate_rejects_fixed_fake_policy_snapshot_ref_before_dispatch(
) -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut envelope = envelope(PermissionMode::BypassPermissions);
    envelope.policy_safety_snapshot_ref.policy_safety_digest = PolicySafetyDigest(
        "2222222222222222222222222222222222222222222222222222222222222222".to_owned(),
    );
    envelope.action.policy_safety_snapshot_ref = Some(envelope.policy_safety_snapshot_ref.clone());
    let mut input = process_input(envelope);
    input.containment_proof =
        ProcessContainmentProofCandidate::Proof(Box::new(admitted_proof(&input.envelope)));

    let error = ProcessGate::new()
        .evaluate_and_maybe_spawn(input, |_authorization| {
            counter.set(counter.get() + 1);
            ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
        })
        .unwrap_err();

    assert_eq!(counter.get(), 0);
    assert_eq!(error, ProcessGateError::RejectedPolicySafetySnapshotRef);
    Ok(())
}

#[test]
fn process_gate_receipt_preserves_typed_redacted_spawn_summary_without_raw_streams(
) -> Result<(), Box<dyn Error>> {
    let mut input = process_input(envelope(PermissionMode::BypassPermissions));
    input.containment_proof =
        ProcessContainmentProofCandidate::Proof(Box::new(admitted_proof(&input.envelope)));

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        ProcessSpawnReport {
            terminal_outcome: ProcessTerminalOutcome::Failed,
            redacted_summary: ProcessRedactedSpawnSummary {
                status: Some(ProcessRedactedStatus {
                    code: "exit_code_7".to_owned(),
                    summary: "process exited with non-zero status".to_owned(),
                }),
                stdout: ProcessRedactedStreamSummary {
                    stream: ProcessRedactedStreamKind::Stdout,
                    byte_count: 31,
                    redacted_preview: Some("safe stdout summary".to_owned()),
                    evidence_refs: vec!["redaction:stdout".to_owned()],
                },
                stderr: ProcessRedactedStreamSummary {
                    stream: ProcessRedactedStreamKind::Stderr,
                    byte_count: 29,
                    redacted_preview: Some("safe stderr summary".to_owned()),
                    evidence_refs: vec!["redaction:stderr".to_owned()],
                },
            },
        }
    })?;

    let serialized = serde_json::to_string(&receipt)?;
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Failed);
    assert_eq!(
        receipt
            .redacted_summary
            .status
            .as_ref()
            .map(|status| status.code.as_str()),
        Some("exit_code_7")
    );
    assert!(serialized.contains("safe stdout summary"));
    assert!(serialized.contains("safe stderr summary"));
    assert!(serialized.contains("redaction:stdout"));
    assert!(serialized.contains("redaction:stderr"));
    assert!(!serialized.contains("stdout raw"));
    assert!(!serialized.contains("stderr raw"));
    assert!(!serialized.contains("sk-spec030-raw-token"));
    Ok(())
}
