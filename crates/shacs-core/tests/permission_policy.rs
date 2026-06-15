use serde_json::{json, Value};
use shacs_core::runtime::{
    app_declaration_grants_permission, build_permission_audit_record,
    build_permission_diagnostics_summary, correlate_approval, decide_permission,
    evaluate_inherited_ceiling, evaluate_permission_replay, evaluate_static_rules,
    late_result_permission_disposition, permission_prd005_006_contract_cases,
    permission_release_evidence_complete, required_permission_release_evidence_buckets,
    ActionNormalizationError, ActionNormalizationState, AppDeclarationPermissionInput,
    ApprovalActor, ApprovalCorrelation, ApprovalCorrelationError, ApprovalDecision,
    ApprovalDecisionKind, ApprovalRequest, AutoEvaluatorVerdict, AutoEvaluatorVerdictKind,
    BoundaryPermissionViolation, ContainerNetworkMode, ContainerRuntimeKind,
    DockerContainmentSnapshot, EvaluatorConfidence, EvaluatorScopeMatch,
    InheritedPermissionContext, LateResultPermissionDisposition, LateResultPermissionInput,
    PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot,
    PermissionPolicyDecisionKind, PermissionPolicyInput, PermissionPolicyReason,
    PermissionReleaseEvidence, PermissionReleaseEvidenceBucket, PermissionReplayInput,
    PermissionReplayInvariant, PermissionReplayViolation, PermissionRuleInput, PermissionedAction,
    PermissionedActionOrigin, ProcExecSummary, ProtectedTargetClass, RuntimeBoundaryOrigin,
    SafetyCapability, StaticRuleDecisionKind, StaticRuleReason, TargetRef,
};
use std::error::Error;

fn action(
    mode: PermissionMode,
    tool_name: &str,
    capabilities: Vec<SafetyCapability>,
    target_refs: Vec<TargetRef>,
) -> PermissionedAction {
    PermissionedAction {
        action_id: format!("action-{tool_name}"),
        provider_tool_call_id: Some("call-1".to_owned()),
        session_id: "session-1".to_owned(),
        turn_id: "turn-1".to_owned(),
        tool_name: tool_name.to_owned(),
        capabilities,
        target_refs,
        action_digest: "action-digest".to_owned(),
        argument_digest: "argument-digest".to_owned(),
        snapshot_digest: "snapshot-digest".to_owned(),
        origin: PermissionedActionOrigin::UserTurn,
        permission_mode_snapshot: PermissionModeSnapshot {
            mode,
            source: Some("test".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        containment_snapshot: None,
        intent_snapshot: None,
        redacted_arguments: json!({}),
        normalization_state: ActionNormalizationState::Ready,
        normalization_errors: Vec::new(),
    }
}

fn target(value: Value) -> TargetRef {
    TargetRef {
        kind: "path".to_owned(),
        digest: "target-digest".to_owned(),
        redacted_value: value,
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

fn proc_summary() -> ProcExecSummary {
    ProcExecSummary {
        command_family: "cargo".to_owned(),
        target_refs: vec!["crates/shacs-core".to_owned()],
        destructive: false,
        network: false,
        secret_exposure: false,
        summary_available: true,
    }
}

fn evaluator(
    verdict: AutoEvaluatorVerdictKind,
    confidence: EvaluatorConfidence,
) -> AutoEvaluatorVerdict {
    AutoEvaluatorVerdict {
        verdict,
        confidence,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: "verify requested change".to_owned(),
        evidence_refs: vec!["intent-1".to_owned()],
        expires_at_unix_ms: 2_000,
        evaluator_ref: Some("eval-1".to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

fn policy_input(
    action: PermissionedAction,
    rules: shacs_core::runtime::StaticRuleDecision,
) -> PermissionPolicyInput {
    PermissionPolicyInput {
        action,
        static_rule_decision: rules,
        evaluator: None,
        approval: None,
        inherited_context: None,
        interactive: true,
    }
}

#[test]
fn protected_targets_fail_closed_before_policy_allow() -> Result<(), Box<dyn Error>> {
    let action = action(
        PermissionMode::Auto,
        "write_file",
        vec![SafetyCapability::FsWrite],
        vec![target(json!(".git/config"))],
    );
    let rules = evaluate_static_rules(
        &action,
        &PermissionRuleInput {
            containment: safe_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        },
    );
    let mut input = policy_input(action, rules.clone());
    input.evaluator = Some(evaluator(
        AutoEvaluatorVerdictKind::AllowCandidate,
        EvaluatorConfidence::High,
    ));
    let decision = decide_permission(input);

    if rules.kind != StaticRuleDecisionKind::Deny
        || rules.reason != StaticRuleReason::ProtectedTarget
        || !rules
            .diagnostics
            .protected_targets
            .contains(&ProtectedTargetClass::GitState)
        || decision.kind != PermissionPolicyDecisionKind::Deny
        || decision.reason != PermissionPolicyReason::ProtectedTarget
        || decision.can_handoff_to_tool_runtime
    {
        return Err(format!("protected target was not fail-closed: {rules:?} {decision:?}").into());
    }
    Ok(())
}

#[test]
fn unknown_target_and_invalid_action_are_never_allowable() -> Result<(), Box<dyn Error>> {
    let unknown_target = action(
        PermissionMode::Auto,
        "write_file",
        vec![SafetyCapability::FsWrite],
        vec![target(json!({"opaque": true}))],
    );
    let unknown_rules = evaluate_static_rules(&unknown_target, &PermissionRuleInput::default());

    let mut invalid = action(
        PermissionMode::Auto,
        "write_file",
        vec![SafetyCapability::FsWrite],
        vec![target(json!("src/lib.rs"))],
    );
    invalid.normalization_state = ActionNormalizationState::ErrorCandidate;
    invalid
        .normalization_errors
        .push(ActionNormalizationError::InvalidArguments {
            tool_name: "write_file".to_owned(),
            detail: "schema mismatch".to_owned(),
        });
    let invalid_rules = evaluate_static_rules(&invalid, &PermissionRuleInput::default());

    if unknown_rules.kind != StaticRuleDecisionKind::Deny
        || unknown_rules.reason != StaticRuleReason::UnknownTargetClassification
        || invalid_rules.kind != StaticRuleDecisionKind::Deny
        || invalid_rules.reason != StaticRuleReason::NormalizationError
    {
        return Err(format!(
            "unsafe candidates were not denied: {unknown_rules:?} {invalid_rules:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn redacted_blank_and_non_string_targets_fail_closed_as_unknown() -> Result<(), Box<dyn Error>> {
    for value in [json!("[REDACTED]"), json!("   "), json!(["src/lib.rs"])] {
        let unknown_target = action(
            PermissionMode::Default,
            "write_file",
            vec![SafetyCapability::FsWrite],
            vec![target(value)],
        );
        let rules = evaluate_static_rules(&unknown_target, &PermissionRuleInput::default());
        let decision = decide_permission(policy_input(unknown_target, rules.clone()));

        if rules.kind != StaticRuleDecisionKind::Deny
            || rules.reason != StaticRuleReason::UnknownTargetClassification
            || !rules.diagnostics.unknown_classification
            || decision.kind != PermissionPolicyDecisionKind::Deny
            || decision.can_handoff_to_tool_runtime
        {
            return Err(format!(
                "unknown target reference did not fail closed: {rules:?} {decision:?}"
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn empty_capability_sets_do_not_receive_mode_baseline_allow() -> Result<(), Box<dyn Error>> {
    for mode in [
        PermissionMode::Plan,
        PermissionMode::Default,
        PermissionMode::AcceptEdits,
        PermissionMode::BypassPermissions,
    ] {
        let no_capability_action = action(mode, "local_noop", Vec::new(), Vec::new());
        let rule_input = if mode == PermissionMode::BypassPermissions {
            PermissionRuleInput {
                containment: safe_containment(),
                protected_targets: Vec::new(),
                proc_exec_summary: None,
            }
        } else {
            PermissionRuleInput::default()
        };
        let rules = evaluate_static_rules(&no_capability_action, &rule_input);
        let decision = decide_permission(policy_input(no_capability_action, rules.clone()));

        if rules.kind != StaticRuleDecisionKind::AllowCandidate
            || decision.kind == PermissionPolicyDecisionKind::Allow
            || decision.reason == PermissionPolicyReason::ModeBaselineAllow
            || decision.can_handoff_to_tool_runtime
        {
            return Err(format!(
                "empty capability action received baseline allow in {mode:?}: {rules:?} {decision:?}"
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn approval_cannot_allow_non_ask_user_empty_capability_actions() -> Result<(), Box<dyn Error>> {
    let no_capability_action = action(
        PermissionMode::BypassPermissions,
        "local_noop",
        Vec::new(),
        Vec::new(),
    );
    let rules = evaluate_static_rules(
        &no_capability_action,
        &PermissionRuleInput {
            containment: safe_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: None,
        },
    );
    let mut input = policy_input(no_capability_action, rules);
    input.approval = Some(ApprovalCorrelation::approved("approval-1".to_owned()));
    let decision = decide_permission(input);

    if decision.kind != PermissionPolicyDecisionKind::Deny
        || decision.reason != PermissionPolicyReason::ModeBaselineDeny
        || decision.can_handoff_to_tool_runtime
    {
        return Err(format!("approval allowed empty capability action: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn unknown_containment_blocks_proc_exec_auto_and_bypass() -> Result<(), Box<dyn Error>> {
    let exec = action(
        PermissionMode::Auto,
        "exec",
        vec![SafetyCapability::ProcExec],
        vec![target(json!("cargo test"))],
    );
    let rules = evaluate_static_rules(
        &exec,
        &PermissionRuleInput {
            containment: DockerContainmentSnapshot::unknown(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );
    let mut input = policy_input(exec, rules.clone());
    input.evaluator = Some(evaluator(
        AutoEvaluatorVerdictKind::AllowCandidate,
        EvaluatorConfidence::High,
    ));
    let decision = decide_permission(input);

    let bypass = action(
        PermissionMode::BypassPermissions,
        "exec",
        vec![SafetyCapability::ProcExec],
        vec![target(json!("cargo test"))],
    );
    let bypass_rules = evaluate_static_rules(
        &bypass,
        &PermissionRuleInput {
            containment: DockerContainmentSnapshot::unknown(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );

    if rules.kind != StaticRuleDecisionKind::AskRequired
        || rules.reason != StaticRuleReason::ContainmentUnknown
        || decision.kind == PermissionPolicyDecisionKind::Allow
        || decision.can_handoff_to_tool_runtime
        || bypass_rules.kind != StaticRuleDecisionKind::Deny
        || bypass_rules.reason != StaticRuleReason::BypassContainmentNotConfirmed
    {
        return Err(format!(
            "unknown containment did not block exec: {rules:?} {decision:?} {bypass_rules:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn secret_read_and_raw_auth_export_are_denied() -> Result<(), Box<dyn Error>> {
    let secret = action(
        PermissionMode::Auto,
        "read_secret",
        vec![SafetyCapability::SecretRead],
        Vec::new(),
    );
    let auth = action(
        PermissionMode::Auto,
        "read_file",
        vec![SafetyCapability::FsRead],
        vec![target(json!(".shacs-bot/auth.json"))],
    );
    let secret_rules = evaluate_static_rules(&secret, &PermissionRuleInput::default());
    let auth_rules = evaluate_static_rules(&auth, &PermissionRuleInput::default());

    if secret_rules.kind != StaticRuleDecisionKind::Deny
        || secret_rules.reason != StaticRuleReason::SecretRead
        || auth_rules.kind != StaticRuleDecisionKind::Deny
        || auth_rules.reason != StaticRuleReason::RawAuthExport
    {
        return Err(
            format!("secret/auth export was not denied: {secret_rules:?} {auth_rules:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn approval_correlation_rejects_mismatched_expired_inspect_only_and_consumed(
) -> Result<(), Box<dyn Error>> {
    let request = ApprovalRequest {
        approval_request_id: "approval-1".to_owned(),
        action_digest: "action-a".to_owned(),
        snapshot_digest: "snapshot-a".to_owned(),
        requested_scope: "turn".to_owned(),
        risk_summary: "write file".to_owned(),
        allowed_decisions: vec![
            ApprovalDecisionKind::Approved,
            ApprovalDecisionKind::InspectOnly,
        ],
        expires_at_unix_ms: 1_000,
    };
    let base = ApprovalDecision {
        approval_request_id: "approval-1".to_owned(),
        action_digest: "action-a".to_owned(),
        snapshot_digest: "snapshot-a".to_owned(),
        decision: ApprovalDecisionKind::Approved,
        approved_scope: "turn".to_owned(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: 500,
        consumed: false,
    };

    let cases = [
        (
            ApprovalDecision {
                approval_request_id: "other".to_owned(),
                ..base.clone()
            },
            ApprovalCorrelationError::RequestMismatch,
            500,
        ),
        (
            ApprovalDecision {
                action_digest: "other".to_owned(),
                ..base.clone()
            },
            ApprovalCorrelationError::ActionMismatch,
            500,
        ),
        (
            ApprovalDecision {
                snapshot_digest: "other".to_owned(),
                ..base.clone()
            },
            ApprovalCorrelationError::SnapshotMismatch,
            500,
        ),
        (base.clone(), ApprovalCorrelationError::Expired, 1_001),
        (
            ApprovalDecision {
                decision: ApprovalDecisionKind::InspectOnly,
                ..base.clone()
            },
            ApprovalCorrelationError::InspectOnly,
            500,
        ),
        (
            ApprovalDecision {
                consumed: true,
                ..base.clone()
            },
            ApprovalCorrelationError::Consumed,
            500,
        ),
    ];

    for (decision, expected, now) in cases {
        let correlation = correlate_approval(&request, &decision, now);
        if correlation.error != Some(expected) || correlation.is_approved() {
            return Err(format!("approval rejection drifted: {correlation:?}").into());
        }
    }

    let accepted = correlate_approval(&request, &base, 500);
    if !accepted.is_approved() || accepted.approval_ref.as_deref() != Some("approval-1") {
        return Err(format!("valid approval was not accepted: {accepted:?}").into());
    }
    Ok(())
}

#[test]
fn evaluator_uncertainty_and_prompt_injection_never_allow() -> Result<(), Box<dyn Error>> {
    let exec = action(
        PermissionMode::Auto,
        "exec",
        vec![SafetyCapability::ProcExec],
        vec![target(json!("cargo test"))],
    );
    let rules = evaluate_static_rules(
        &exec,
        &PermissionRuleInput {
            containment: safe_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );

    for verdict in [
        evaluator(
            AutoEvaluatorVerdictKind::Timeout,
            EvaluatorConfidence::Unknown,
        ),
        evaluator(
            AutoEvaluatorVerdictKind::ParseFailure,
            EvaluatorConfidence::Unknown,
        ),
        evaluator(
            AutoEvaluatorVerdictKind::AllowCandidate,
            EvaluatorConfidence::Low,
        ),
    ] {
        let mut input = policy_input(exec.clone(), rules.clone());
        input.evaluator = Some(verdict);
        let decision = decide_permission(input);
        if decision.kind == PermissionPolicyDecisionKind::Allow
            || decision.can_handoff_to_tool_runtime
        {
            return Err(format!("uncertain evaluator allowed execution: {decision:?}").into());
        }
    }

    let mut injected = evaluator(
        AutoEvaluatorVerdictKind::AllowCandidate,
        EvaluatorConfidence::High,
    );
    injected
        .prompt_injection_signals
        .push(shacs_core::runtime::PromptInjectionSignal {
            source_ref: "webpage".to_owned(),
            reason: "asked to run unrelated command".to_owned(),
            confidence: EvaluatorConfidence::High,
        });
    let mut input = policy_input(exec, rules);
    input.evaluator = Some(injected);
    let decision = decide_permission(input);
    if decision.kind == PermissionPolicyDecisionKind::Allow
        || decision.reason != PermissionPolicyReason::PromptInjectionSignal
    {
        return Err(format!("prompt injection signal did not block allow: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn inherited_ceiling_cannot_widen_mode_or_capabilities() -> Result<(), Box<dyn Error>> {
    let context = InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::Subagent {
                subagent_id: Some("child-1".to_owned()),
            },
        },
        requested_mode: PermissionMode::Auto,
        requested_capabilities: vec![SafetyCapability::FsRead, SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    };
    let ceiling = evaluate_inherited_ceiling(&context);

    if ceiling.allowed
        || !ceiling
            .violations
            .contains(&BoundaryPermissionViolation::ModeWidening)
        || !ceiling
            .violations
            .contains(&BoundaryPermissionViolation::CapabilityWidening)
    {
        return Err(format!("ceiling widening was not rejected: {ceiling:?}").into());
    }
    Ok(())
}

#[test]
fn minimal_audit_record_is_redacted_and_has_decision_evidence() -> Result<(), Box<dyn Error>> {
    let mut action = action(
        PermissionMode::Auto,
        "write_file",
        vec![SafetyCapability::FsWrite],
        vec![target(json!("src/lib.rs"))],
    );
    action.session_id = "session-[REDACTED]".to_owned();
    action.redacted_arguments = json!({"token": "[REDACTED]"});
    action.containment_snapshot = Some(shacs_core::runtime::ContainmentSnapshotRef {
        contained: Some(true),
        digest: Some("container".to_owned()),
        summary: Some("docker".to_owned()),
    });
    let rules = evaluate_static_rules(&action, &PermissionRuleInput::default());
    let decision = decide_permission(policy_input(action.clone(), rules));
    let audit = build_permission_audit_record(&action, &decision, 123);
    let serialized = serde_json::to_string(&audit)?;

    if audit.action_id != action.action_id
        || audit.argument_digest != "argument-digest"
        || audit.containment_summary.as_deref() != Some("docker")
        || serialized.contains("sk-")
        || serialized.contains("raw-token")
    {
        return Err(format!("audit record drifted or leaked: {serialized}").into());
    }
    Ok(())
}

#[test]
fn late_results_from_closed_superseded_or_stale_turns_are_not_executable(
) -> Result<(), Box<dyn Error>> {
    let base = LateResultPermissionInput {
        turn_open: true,
        active_turn_id: "turn-1".to_owned(),
        result_turn_id: "turn-1".to_owned(),
        decision_snapshot_digest: "snapshot-a".to_owned(),
        action_snapshot_digest: "snapshot-a".to_owned(),
    };

    let cases = [
        (
            LateResultPermissionInput {
                turn_open: false,
                ..base.clone()
            },
            LateResultPermissionDisposition::ClosedTurn,
        ),
        (
            LateResultPermissionInput {
                result_turn_id: "turn-0".to_owned(),
                ..base.clone()
            },
            LateResultPermissionDisposition::SupersededTurn,
        ),
        (
            LateResultPermissionInput {
                decision_snapshot_digest: "snapshot-old".to_owned(),
                ..base.clone()
            },
            LateResultPermissionDisposition::StaleDecisionReuse,
        ),
        (base, LateResultPermissionDisposition::Executable),
    ];

    for (input, expected) in cases {
        let disposition = late_result_permission_disposition(&input);
        if disposition != expected {
            return Err(format!("late result disposition drifted: {disposition:?}").into());
        }
    }
    Ok(())
}

#[test]
fn app_declaration_only_and_deferred_gate_bypass_do_not_grant_approval(
) -> Result<(), Box<dyn Error>> {
    let declaration = AppDeclarationPermissionInput {
        app_id: "app-1".to_owned(),
        declared_capabilities: vec![SafetyCapability::FsRead],
        requested_capabilities: vec![SafetyCapability::FsRead],
    };
    if app_declaration_grants_permission(&declaration) {
        return Err("app declaration unexpectedly granted approval".into());
    }

    let app_context = InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::AppTask {
                app_id: Some("app-1".to_owned()),
                task_id: Some("task-1".to_owned()),
            },
        },
        requested_mode: PermissionMode::Default,
        requested_capabilities: vec![SafetyCapability::FsRead],
        per_action_evaluation_required: true,
    };
    let app_evaluation = evaluate_inherited_ceiling(&app_context);
    if app_evaluation.allowed
        || !app_evaluation
            .violations
            .contains(&BoundaryPermissionViolation::AppDeclarationOnly)
    {
        return Err(format!("app declaration was treated as approval: {app_evaluation:?}").into());
    }

    let deferred_context = InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::DeferredMcp {
                bridge_name: "filesystem".to_owned(),
                scope_digest: "scope-1".to_owned(),
            },
        },
        requested_mode: PermissionMode::Default,
        requested_capabilities: vec![SafetyCapability::FsRead],
        per_action_evaluation_required: false,
    };
    let deferred_evaluation = evaluate_inherited_ceiling(&deferred_context);
    if deferred_evaluation.allowed
        || !deferred_evaluation
            .violations
            .contains(&BoundaryPermissionViolation::DeferredGateBypass)
    {
        return Err(
            format!("deferred gate bypass was not flagged: {deferred_evaluation:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn permission_audit_diagnostics_count_decisions_and_failure_reasons() -> Result<(), Box<dyn Error>>
{
    let allow_action = action(
        PermissionMode::Default,
        "read_file",
        vec![SafetyCapability::FsRead],
        vec![target(json!("src/lib.rs"))],
    );
    let allow_rules = evaluate_static_rules(&allow_action, &PermissionRuleInput::default());
    let allow_decision = decide_permission(policy_input(allow_action.clone(), allow_rules));

    let ask_action = action(
        PermissionMode::Auto,
        "read_file",
        vec![SafetyCapability::FsRead],
        vec![target(json!("src/lib.rs"))],
    );
    let ask_rules = evaluate_static_rules(&ask_action, &PermissionRuleInput::default());
    let ask_decision = decide_permission(policy_input(ask_action.clone(), ask_rules));

    let containment_action = action(
        PermissionMode::Auto,
        "exec",
        vec![SafetyCapability::ProcExec],
        vec![target(json!("cargo test"))],
    );
    let containment_rules = evaluate_static_rules(
        &containment_action,
        &PermissionRuleInput {
            containment: DockerContainmentSnapshot::unknown(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );

    let deny_action = action(
        PermissionMode::Auto,
        "write_file",
        vec![SafetyCapability::FsWrite],
        vec![target(json!(".git/config"))],
    );
    let deny_rules = evaluate_static_rules(&deny_action, &PermissionRuleInput::default());
    let deny_decision = decide_permission(policy_input(deny_action.clone(), deny_rules.clone()));

    let records = vec![
        build_permission_audit_record(&allow_action, &allow_decision, 1),
        build_permission_audit_record(&ask_action, &ask_decision, 2),
        build_permission_audit_record(&deny_action, &deny_decision, 3),
    ];
    let diagnostics = vec![containment_rules.diagnostics, deny_rules.diagnostics];
    let summary = build_permission_diagnostics_summary(&records, &diagnostics);

    if summary.allow_count != 1
        || summary.ask_count != 1
        || summary.deny_count != 1
        || summary.evaluator_failure_count != 1
        || !summary
            .evaluator_failure_reasons
            .contains(&PermissionPolicyReason::EvaluatorUnavailable)
        || summary.containment_warning_count != 1
        || summary.protected_target_count != 1
        || !summary
            .protected_target_reasons
            .contains(&ProtectedTargetClass::GitState)
    {
        return Err(format!("permission diagnostics summary drifted: {summary:?}").into());
    }
    Ok(())
}

#[test]
fn permission_replay_invariants_are_fail_closed_for_old_denies() -> Result<(), Box<dyn Error>> {
    let base = PermissionReplayInput {
        recorded_snapshot_digest: "snapshot-a".to_owned(),
        replay_snapshot_digest: "snapshot-a".to_owned(),
        recorded_rule_version: "rules-a".to_owned(),
        replay_rule_version: "rules-a".to_owned(),
        recorded_decision: PermissionPolicyDecisionKind::Allow,
        replay_decision: PermissionPolicyDecisionKind::Allow,
        replay_reason: PermissionPolicyReason::ModeBaselineAllow,
    };

    let same = evaluate_permission_replay(&base);
    if same.invariant != Some(PermissionReplayInvariant::SameSnapshotSameDecision) || !same.accepted
    {
        return Err(format!("same snapshot replay did not preserve decision: {same:?}").into());
    }

    let drift = evaluate_permission_replay(&PermissionReplayInput {
        replay_decision: PermissionPolicyDecisionKind::Deny,
        replay_reason: PermissionPolicyReason::StaticDeny,
        ..base.clone()
    });
    if drift.violation != Some(PermissionReplayViolation::SameSnapshotDecisionDrift)
        || drift.accepted
    {
        return Err(format!("same snapshot decision drift was accepted: {drift:?}").into());
    }

    let stricter = evaluate_permission_replay(&PermissionReplayInput {
        replay_snapshot_digest: "snapshot-b".to_owned(),
        replay_rule_version: "rules-b".to_owned(),
        replay_decision: PermissionPolicyDecisionKind::Deny,
        replay_reason: PermissionPolicyReason::StaticDeny,
        ..base.clone()
    });
    if stricter.invariant != Some(PermissionReplayInvariant::StricterReplayDeniedRecordedAllow)
        || !stricter.accepted
    {
        return Err(format!("stricter replay was not accepted safely: {stricter:?}").into());
    }

    let looser = evaluate_permission_replay(&PermissionReplayInput {
        recorded_snapshot_digest: "snapshot-old".to_owned(),
        replay_snapshot_digest: "snapshot-new".to_owned(),
        recorded_rule_version: "rules-old".to_owned(),
        replay_rule_version: "rules-new".to_owned(),
        recorded_decision: PermissionPolicyDecisionKind::Deny,
        replay_decision: PermissionPolicyDecisionKind::Allow,
        replay_reason: PermissionPolicyReason::ModeBaselineAllow,
    });
    if looser.violation != Some(PermissionReplayViolation::LooserReplayAllowedRecordedDeny)
        || looser.accepted
    {
        return Err(format!("looser replay was incorrectly accepted: {looser:?}").into());
    }
    Ok(())
}

#[test]
fn permission_contract_matrix_declares_required_release_evidence_buckets(
) -> Result<(), Box<dyn Error>> {
    let cases = permission_prd005_006_contract_cases();
    let required_buckets = required_permission_release_evidence_buckets();
    let evidence = PermissionReleaseEvidence {
        buckets: required_buckets.clone(),
    };

    for bucket in [
        PermissionReleaseEvidenceBucket::InheritedBoundaryCases,
        PermissionReleaseEvidenceBucket::PermissionAuditDiagnostics,
        PermissionReleaseEvidenceBucket::PermissionReplayInvariants,
        PermissionReleaseEvidenceBucket::ContractMatrix,
        PermissionReleaseEvidenceBucket::ReleaseEvidence,
    ] {
        if !required_buckets.contains(&bucket) {
            return Err(format!("missing required release evidence bucket: {bucket:?}").into());
        }
    }

    if cases.len() < 4 || !permission_release_evidence_complete(&evidence) {
        return Err(format!("contract matrix release evidence incomplete: {cases:?}").into());
    }
    Ok(())
}
