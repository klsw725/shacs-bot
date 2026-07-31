use serde_json::json;
use shacs_core::runtime::{
    evaluate_inherited_ceiling, evaluate_static_rules, late_result_permission_disposition,
    ActionNormalizationState, BoundaryPermissionViolation, ContainerNetworkMode,
    ContainerRuntimeKind, DockerContainmentSnapshot, InheritedPermissionContext,
    LateResultPermissionDisposition, LateResultPermissionInput, PermissionCeilingSnapshot,
    PermissionMode, PermissionModeSnapshot, PermissionRuleInput, PermissionedAction,
    PermissionedActionOrigin, ProcExecSummary, RuntimeBoundaryOrigin, SafetyCapability,
    StaticRuleDecisionKind, StaticRuleReason,
};
use std::error::Error;

fn action(mode: PermissionMode, capabilities: Vec<SafetyCapability>) -> PermissionedAction {
    PermissionedAction {
        action_id: "action-containment".to_owned(),
        provider_tool_call_id: Some("call-containment".to_owned()),
        session_id: "session-containment".to_owned(),
        turn_id: "turn-containment".to_owned(),
        tool_name: "exec".to_owned(),
        capabilities,
        target_refs: Vec::new(),
        action_digest: "action-digest".to_owned(),
        argument_digest: "argument-digest".to_owned(),
        snapshot_digest: "snapshot-digest".to_owned(),
        policy_safety_snapshot_ref: None,
        origin: PermissionedActionOrigin::UserTurn,
        permission_mode_snapshot: PermissionModeSnapshot {
            mode,
            source: Some("baseline".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        containment_snapshot: None,
        intent_snapshot: None,
        redacted_arguments: json!({"command":"[REDACTED]"}),
        secret_ref_evidence: Vec::new(),
        normalization_state: ActionNormalizationState::Ready,
        normalization_errors: Vec::new(),
    }
}

fn proc_summary() -> ProcExecSummary {
    ProcExecSummary {
        command_family: "cargo".to_owned(),
        target_refs: vec!["workspace".to_owned()],
        destructive: false,
        network: false,
        secret_exposure: false,
        summary_available: true,
    }
}

#[test]
fn baseline_unknown_containment_asks_for_proc_exec_and_denies_bypass() -> Result<(), Box<dyn Error>>
{
    let proc_exec = action(PermissionMode::Auto, vec![SafetyCapability::ProcExec]);
    let proc_rules = evaluate_static_rules(
        &proc_exec,
        &PermissionRuleInput {
            containment: DockerContainmentSnapshot::unknown(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );
    let bypass = action(
        PermissionMode::BypassPermissions,
        vec![SafetyCapability::ProcExec],
    );
    let bypass_rules = evaluate_static_rules(
        &bypass,
        &PermissionRuleInput {
            containment: DockerContainmentSnapshot::unknown(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );

    assert_eq!(proc_rules.kind, StaticRuleDecisionKind::AskRequired);
    assert_eq!(proc_rules.reason, StaticRuleReason::ContainmentUnknown);
    assert_eq!(bypass_rules.kind, StaticRuleDecisionKind::Deny);
    assert_eq!(
        bypass_rules.reason,
        StaticRuleReason::BypassContainmentNotConfirmed
    );
    Ok(())
}

#[test]
fn baseline_inherited_ceiling_blocks_mode_capability_app_and_deferred_widening(
) -> Result<(), Box<dyn Error>> {
    let widened = evaluate_inherited_ceiling(&InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::Subagent {
                subagent_id: Some("child".to_owned()),
            },
        },
        requested_mode: PermissionMode::BypassPermissions,
        requested_capabilities: vec![SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    });
    let app_only = evaluate_inherited_ceiling(&InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::AppTask {
                app_id: Some("app".to_owned()),
                task_id: Some("task".to_owned()),
            },
        },
        requested_mode: PermissionMode::Default,
        requested_capabilities: vec![SafetyCapability::FsRead],
        per_action_evaluation_required: true,
    });
    let deferred_bypass = evaluate_inherited_ceiling(&InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::DeferredMcp {
                bridge_name: "bridge".to_owned(),
                scope_digest: "scope".to_owned(),
            },
        },
        requested_mode: PermissionMode::Default,
        requested_capabilities: vec![SafetyCapability::FsRead],
        per_action_evaluation_required: false,
    });

    assert!(!widened.allowed);
    assert!(widened
        .violations
        .contains(&BoundaryPermissionViolation::ModeWidening));
    assert!(widened
        .violations
        .contains(&BoundaryPermissionViolation::CapabilityWidening));
    assert!(!app_only.allowed);
    assert!(app_only
        .violations
        .contains(&BoundaryPermissionViolation::AppDeclarationOnly));
    assert!(!deferred_bypass.allowed);
    assert!(deferred_bypass
        .violations
        .contains(&BoundaryPermissionViolation::DeferredGateBypass));
    Ok(())
}

#[test]
fn baseline_safe_containment_allows_current_bypass_proc_exec_candidate(
) -> Result<(), Box<dyn Error>> {
    let rules = evaluate_static_rules(
        &action(
            PermissionMode::BypassPermissions,
            vec![SafetyCapability::ProcExec],
        ),
        &PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: vec!["workspace".to_owned()],
                network_mode: ContainerNetworkMode::Bridge,
                digest: Some("container-digest".to_owned()),
                summary: Some("docker non-root".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );

    assert_eq!(rules.kind, StaticRuleDecisionKind::AllowCandidate);
    assert_eq!(rules.reason, StaticRuleReason::NoStaticMatch);
    Ok(())
}

#[test]
fn baseline_late_or_cancelled_decision_reuse_stays_non_executable() -> Result<(), Box<dyn Error>> {
    let closed = late_result_permission_disposition(&LateResultPermissionInput {
        turn_open: false,
        active_turn_id: "turn-current".to_owned(),
        result_turn_id: "turn-current".to_owned(),
        decision_snapshot_digest: "snapshot-a".to_owned(),
        action_snapshot_digest: "snapshot-a".to_owned(),
    });
    let stale = late_result_permission_disposition(&LateResultPermissionInput {
        turn_open: true,
        active_turn_id: "turn-current".to_owned(),
        result_turn_id: "turn-current".to_owned(),
        decision_snapshot_digest: "snapshot-old".to_owned(),
        action_snapshot_digest: "snapshot-new".to_owned(),
    });

    assert_eq!(closed, LateResultPermissionDisposition::ClosedTurn);
    assert_eq!(stale, LateResultPermissionDisposition::StaleDecisionReuse);
    assert_ne!(closed, LateResultPermissionDisposition::Executable);
    assert_ne!(stale, LateResultPermissionDisposition::Executable);
    Ok(())
}
