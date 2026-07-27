use serde_json::json;
use shacs_core::runtime::{
    containment_permission_proof_for_process_gate, evaluate_containment_permission,
    ActionNormalizationState, ContainerNetworkMode, ContainerRuntimeKind, ContainmentBoundaryRef,
    ContainmentEvidenceState, DockerContainmentSnapshot, InheritedPermissionContext,
    PermissionCeilingProofInput, PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot,
    PermissionPolicyDecisionKind, PermissionRuleInput, PermissionedAction,
    PermissionedActionOrigin, PolicySafetyDigest, PolicySafetySnapshotId, PolicySafetySnapshotRef,
    PolicySafetySnapshotSchemaId, ProcExecSummary, ProcessAdapterKind,
    ProcessContainmentProofCandidate, ProcessExecutionEnvelope, ProcessExecutionEnvelopeInput,
    ProcessGate, ProcessGateInput, ProcessGateTerminalPrecondition, ProcessIdentity,
    ProcessRedactedCommand, ProcessSpawnReport, ProcessTerminalOutcome,
    RedactedPolicySafetySummary, RuntimeBoundaryKind, RuntimeBoundaryOrigin, SafetyCapability,
    WorkspaceScopeProof,
};
use std::{cell::Cell, error::Error};

#[test]
fn process_gate_never_spawns_on_non_admitted_containment_proof() -> Result<(), Box<dyn Error>> {
    let counter = Cell::new(0);
    let mut input = process_input()?;
    input.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(
        evaluate_containment_permission(proof_input())?,
    ));

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    assert_eq!(
        receipt.policy_decision.kind,
        PermissionPolicyDecisionKind::Allow
    );
    Ok(())
}

#[test]
fn process_gate_rejects_child_scope_outside_authoritative_parent_refs() -> Result<(), Box<dyn Error>>
{
    let counter = Cell::new(0);
    let mut input = process_input()?;
    let inherited_context = InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::BypassPermissions,
            capability_ceiling: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["workspace/sub".to_owned()],
            origin: RuntimeBoundaryOrigin::UserTurn,
        },
        requested_mode: PermissionMode::BypassPermissions,
        requested_capabilities: vec![SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    };
    input.containment_proof = ProcessContainmentProofCandidate::Proof(Box::new(
        containment_permission_proof_for_process_gate(
            &input.envelope,
            &input.permission_rules,
            Some(&inherited_context),
            200,
        )?,
    ));
    input.inherited_context = Some(inherited_context);

    let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input, |_authorization| {
        counter.set(counter.get() + 1);
        ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
    })?;

    assert_eq!(counter.get(), 0);
    assert_eq!(receipt.dispatch_count, 0);
    assert_eq!(receipt.terminal_outcome, ProcessTerminalOutcome::Denied);
    Ok(())
}

fn process_input() -> Result<ProcessGateInput, Box<dyn Error>> {
    Ok(ProcessGateInput {
        envelope: ProcessExecutionEnvelope::try_from_input(ProcessExecutionEnvelopeInput {
            identity: ProcessIdentity::new("proc-proof", "session-proof", "turn-proof"),
            adapter: ProcessAdapterKind::ExecTool,
            action: action(),
            required_secret_ref_count: 0,
            redacted_command: ProcessRedactedCommand {
                command_family: "sh".to_owned(),
                redacted_summary: "proof command".to_owned(),
                redacted_targets: Vec::new(),
            },
        })?,
        permission_rules: PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: vec!["workspace".to_owned()],
                network_mode: ContainerNetworkMode::Bridge,
                digest: Some("container".to_owned()),
                summary: Some("docker non-root".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: "sh".to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
        },
        inherited_context: None,
        evaluator: None,
        approval: None,
        containment_proof: ProcessContainmentProofCandidate::Missing,
        interactive: false,
        terminal_precondition: ProcessGateTerminalPrecondition::Ready,
        now_unix_ms: 200,
    })
}

fn proof_input() -> shacs_core::runtime::ContainmentPermissionInput {
    let child = ContainmentBoundaryRef {
        boundary_id: "child-proof".to_owned(),
        boundary_kind: RuntimeBoundaryKind::ExecTool,
        origin: RuntimeBoundaryOrigin::UserTurn,
        containment_state: ContainmentEvidenceState::Stale,
        containment_digest: Some("containment".to_owned()),
        workspace_scope: WorkspaceScopeProof::same("workspace", "scope"),
        permission_ceiling: PermissionCeilingProofInput {
            parent_mode: PermissionMode::BypassPermissions,
            requested_mode: PermissionMode::BypassPermissions,
            parent_capabilities: vec![SafetyCapability::ProcExec],
            requested_capabilities: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["scope".to_owned()],
            requested_scope_ref: "scope".to_owned(),
            per_action_evaluation_required: true,
        },
        created_at_unix_ms: 100,
    };
    shacs_core::runtime::ContainmentPermissionInput {
        parent: child.parent_boundary(),
        child,
        policy_safety_digest: PolicySafetyDigest("digest".to_owned()),
        process_envelope_id: "process:session-proof:turn-proof:action-digest".to_owned(),
        now_unix_ms: 200,
        cancelled_at_unix_ms: None,
        untrusted_metadata: None,
    }
}

fn action() -> PermissionedAction {
    PermissionedAction {
        action_id: "action-proof".to_owned(),
        provider_tool_call_id: Some("call-proof".to_owned()),
        session_id: "session-proof".to_owned(),
        turn_id: "turn-proof".to_owned(),
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
        secret_ref_evidence: Vec::new(),
        normalization_state: ActionNormalizationState::Ready,
        normalization_errors: Vec::new(),
    }
}

fn policy_ref() -> PolicySafetySnapshotRef {
    PolicySafetySnapshotRef {
        schema_id: PolicySafetySnapshotSchemaId::V1,
        snapshot_id: PolicySafetySnapshotId("snapshot-proof".to_owned()),
        policy_safety_digest: PolicySafetyDigest("digest".to_owned()),
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
