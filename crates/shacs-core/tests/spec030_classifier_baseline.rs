use serde_json::{json, Value};
use shacs_core::runtime::{
    classifier_decision_evidence, decide_permission, evaluate_static_rules,
    recent_auto_mode_denial_from_classifier_decision, skipped_classifier_evidence, AccountingState,
    AccountingUnavailableReason, ActionNormalizationState, AutoEvaluatorVerdict,
    AutoEvaluatorVerdictKind, ClassifierAttemptStatus, ClassifierDisposition,
    ClassifierEvidenceInput, ClassifierFallbackCause, ContainerNetworkMode, ContainerRuntimeKind,
    DockerContainmentSnapshot, EvaluatorConfidence, EvaluatorScopeMatch,
    InheritedPermissionContext, PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot,
    PermissionPolicyDecisionKind, PermissionPolicyInput, PermissionPolicyReason,
    PermissionRuleInput, PermissionedAction, PermissionedActionOrigin, PolicySafetyDigest,
    PolicySafetySnapshotId, PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId, ProcExecSummary,
    RecentAutoModeRetryToken, RecentAutoModeRetryTokenConsumeError, RecentAutoModeRetryTokenMatch,
    RecentAutoModeRetryTokenStore, RedactedPolicySafetySummary, RuntimeBoundaryOrigin,
    RuntimeToolCall, SafetyCapability, StaticPolicyPrecedence, StaticRuleDecision,
    StaticRuleDecisionKind, StaticRuleReason, TargetRef,
};
use std::collections::BTreeMap;
use std::error::Error;

fn action(
    tool_name: &str,
    capability: SafetyCapability,
    target_value: Value,
) -> PermissionedAction {
    PermissionedAction {
        action_id: format!("spec030-{tool_name}"),
        provider_tool_call_id: Some("call-spec030".to_owned()),
        session_id: "session-spec030".to_owned(),
        turn_id: "turn-spec030".to_owned(),
        tool_name: tool_name.to_owned(),
        capabilities: vec![capability],
        target_refs: vec![TargetRef {
            kind: "path".to_owned(),
            digest: "target-digest-spec030".to_owned(),
            redacted_value: target_value,
        }],
        action_digest: "action-digest-spec030".to_owned(),
        argument_digest: "argument-digest-spec030".to_owned(),
        snapshot_digest: "snapshot-digest-spec030".to_owned(),
        policy_safety_snapshot_ref: None,
        origin: PermissionedActionOrigin::UserTurn,
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("spec030-baseline".to_owned()),
            scope_ref: Some("workspace".to_owned()),
        },
        containment_snapshot: None,
        intent_snapshot: None,
        redacted_arguments: json!({}),
        secret_ref_evidence: Vec::new(),
        normalization_state: ActionNormalizationState::Ready,
        normalization_errors: Vec::new(),
    }
}

fn policy_input(action: PermissionedAction, rules: StaticRuleDecision) -> PermissionPolicyInput {
    PermissionPolicyInput {
        action,
        static_rule_decision: rules,
        evaluator: None,
        approval: None,
        inherited_context: None,
        remembered_rules: Vec::new(),
        remembered_store_unavailable: false,
        interactive: true,
    }
}

fn classifier_verdict(kind: AutoEvaluatorVerdictKind) -> AutoEvaluatorVerdict {
    AutoEvaluatorVerdict {
        verdict: kind,
        confidence: match kind {
            AutoEvaluatorVerdictKind::AllowCandidate | AutoEvaluatorVerdictKind::DenyCandidate => {
                EvaluatorConfidence::High
            }
            AutoEvaluatorVerdictKind::AskUser
            | AutoEvaluatorVerdictKind::InsufficientContext
            | AutoEvaluatorVerdictKind::Timeout
            | AutoEvaluatorVerdictKind::ParseFailure => EvaluatorConfidence::Unknown,
        },
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: "spec030 classifier baseline".to_owned(),
        evidence_refs: vec!["classifier:evidence".to_owned()],
        expires_at_unix_ms: 2_000,
        evaluator_ref: Some("auto-mode-classifier".to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

fn policy_safety_ref(label: &str, expires_at_unix_ms: Option<u64>) -> PolicySafetySnapshotRef {
    PolicySafetySnapshotRef {
        schema_id: PolicySafetySnapshotSchemaId::V1,
        snapshot_id: PolicySafetySnapshotId(format!("snapshot-{label}")),
        policy_safety_digest: PolicySafetyDigest(
            "1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
        ),
        created_at_unix_ms: 500,
        expires_at_unix_ms,
        redacted_summary: RedactedPolicySafetySummary {
            permission_mode: "auto".to_owned(),
            capability_count: 1,
            containment_digest: Some(format!("containment-{label}")),
            source_ref_count: 1,
            provenance_ref_count: 1,
        },
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
        digest: Some("containment-digest-spec030".to_owned()),
        summary: Some("non-privileged docker".to_owned()),
    }
}

fn proc_summary() -> ProcExecSummary {
    ProcExecSummary {
        command_family: "pwd".to_owned(),
        target_refs: vec!["workspace".to_owned()],
        destructive: false,
        network: false,
        secret_exposure: false,
        summary_available: true,
    }
}

#[test]
fn classifier_allow_cannot_override_static_deny() -> Result<(), Box<dyn Error>> {
    let protected = action(
        "write_file",
        SafetyCapability::FsWrite,
        json!(".git/config"),
    );
    let rules = evaluate_static_rules(&protected, &PermissionRuleInput::default());
    let mut input = policy_input(protected, rules.clone());
    input.evaluator = Some(classifier_verdict(AutoEvaluatorVerdictKind::AllowCandidate));

    let decision = decide_permission(input);

    assert_eq!(rules.kind, StaticRuleDecisionKind::Deny);
    assert_eq!(rules.reason, StaticRuleReason::ProtectedTarget);
    assert_eq!(decision.kind, PermissionPolicyDecisionKind::Ask);
    assert_eq!(decision.reason, PermissionPolicyReason::ProtectedTarget);
    assert!(!decision.can_handoff_to_tool_runtime);
    Ok(())
}

#[test]
fn classifier_allow_cannot_override_inherited_ceiling() -> Result<(), Box<dyn Error>> {
    let exec = action("exec", SafetyCapability::ProcExec, json!("pwd"));
    let rules = evaluate_static_rules(
        &exec,
        &PermissionRuleInput {
            containment: safe_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );
    let mut input = policy_input(exec, rules);
    input.evaluator = Some(classifier_verdict(AutoEvaluatorVerdictKind::AllowCandidate));
    input.inherited_context = Some(InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::Subagent {
                subagent_id: Some("spec030-child".to_owned()),
            },
        },
        requested_mode: PermissionMode::Auto,
        requested_capabilities: vec![SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    });

    let decision = decide_permission(input);

    assert_eq!(decision.kind, PermissionPolicyDecisionKind::Deny);
    assert_eq!(decision.reason, PermissionPolicyReason::CeilingViolation);
    assert!(!decision.can_handoff_to_tool_runtime);
    Ok(())
}

#[test]
fn classifier_allow_executes_only_after_static_allow_candidate() -> Result<(), Box<dyn Error>> {
    let exec = action("exec", SafetyCapability::ProcExec, json!("pwd"));
    let rules = evaluate_static_rules(
        &exec,
        &PermissionRuleInput {
            containment: safe_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );
    let mut input = policy_input(exec, rules.clone());
    input.evaluator = Some(classifier_verdict(AutoEvaluatorVerdictKind::AllowCandidate));

    let decision = decide_permission(input);

    assert_eq!(rules.kind, StaticRuleDecisionKind::AllowCandidate);
    assert_eq!(decision.kind, PermissionPolicyDecisionKind::Allow);
    assert_eq!(decision.reason, PermissionPolicyReason::EvaluatorAllow);
    assert!(decision.can_handoff_to_tool_runtime);
    Ok(())
}

#[test]
fn classifier_deny_and_parse_failure_remain_approval_gated() -> Result<(), Box<dyn Error>> {
    for verdict in [
        AutoEvaluatorVerdictKind::DenyCandidate,
        AutoEvaluatorVerdictKind::ParseFailure,
    ] {
        let read = action("read_file", SafetyCapability::FsRead, json!("src/lib.rs"));
        let rules = evaluate_static_rules(&read, &PermissionRuleInput::default());
        let mut input = policy_input(read, rules.clone());
        input.evaluator = Some(classifier_verdict(verdict));

        let decision = decide_permission(input);

        assert_eq!(rules.kind, StaticRuleDecisionKind::AllowCandidate);
        assert_eq!(decision.kind, PermissionPolicyDecisionKind::Ask);
        assert_eq!(decision.reason, PermissionPolicyReason::EvaluatorUncertain);
        assert!(!decision.can_handoff_to_tool_runtime);
    }
    Ok(())
}

#[test]
fn recent_classifier_denial_is_sanitized_retryable_and_single_use() -> Result<(), Box<dyn Error>> {
    let read = action("read_file", SafetyCapability::FsRead, json!("src/lib.rs"));
    let rules = evaluate_static_rules(&read, &PermissionRuleInput::default());
    let evaluator = classifier_verdict(AutoEvaluatorVerdictKind::DenyCandidate);
    let mut input = policy_input(read.clone(), rules);
    input.evaluator = Some(evaluator.clone());
    let decision = decide_permission(input);

    let denial =
        recent_auto_mode_denial_from_classifier_decision(&read, &decision, &evaluator, 500)
            .ok_or("expected recent classifier denial")?;
    let serialized = serde_json::to_string(&denial)?;
    assert_eq!(denial.tool_name, "read_file");
    assert_eq!(
        denial.decision_reason,
        PermissionPolicyReason::EvaluatorUncertain
    );
    assert_eq!(
        denial.classifier_verdict,
        AutoEvaluatorVerdictKind::DenyCandidate
    );
    assert!(denial.retryable);
    assert!(denial
        .target_summary
        .iter()
        .all(|target| target.starts_with("target:")));
    assert!(!serialized.contains("src/lib.rs"));

    let mut tokens = RecentAutoModeRetryTokenStore::default();
    tokens.insert(RecentAutoModeRetryToken::new(
        &denial,
        RuntimeToolCall::new("call-spec030", "read_file", json!({ "path": "src/lib.rs" })),
        Default::default(),
        1_000,
    ));
    assert!(tokens.is_available(&denial.denial_id, 999));
    let consumed = tokens
        .consume(
            &denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&denial),
            999,
        )
        .map_err(|error| format!("retry token should be consumable: {error:?}"))?;
    assert_eq!(consumed.denial_id(), denial.denial_id);
    assert_eq!(
        tokens.consume(
            &denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&denial),
            999,
        ),
        Err(RecentAutoModeRetryTokenConsumeError::Consumed)
    );
    Ok(())
}

#[test]
fn classifier_evidence_records_ceiling_skip_with_required_names() -> Result<(), Box<dyn Error>> {
    let exec = action("exec", SafetyCapability::ProcExec, json!("pwd"));
    let rules = evaluate_static_rules(
        &exec,
        &PermissionRuleInput {
            containment: safe_containment(),
            protected_targets: Vec::new(),
            proc_exec_summary: Some(proc_summary()),
        },
    );
    let mut input = policy_input(exec.clone(), rules);
    input.inherited_context = Some(InheritedPermissionContext {
        ceiling: PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Default,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: Vec::new(),
            origin: RuntimeBoundaryOrigin::Subagent {
                subagent_id: Some("spec030-child".to_owned()),
            },
        },
        requested_mode: PermissionMode::Auto,
        requested_capabilities: vec![SafetyCapability::ProcExec],
        per_action_evaluation_required: true,
    });
    let decision = decide_permission(input);

    let evidence = skipped_classifier_evidence(
        1_000,
        "spec030-classifier-fixture-model",
        &exec,
        &decision,
        ClassifierFallbackCause::StaticPolicyNotReviewable,
    );
    let serialized = serde_json::to_string(&evidence)?;

    assert_eq!(evidence.precedence, StaticPolicyPrecedence::CeilingWins);
    assert_eq!(
        evidence.disposition,
        ClassifierDisposition::NotInvokedCeiling
    );
    assert!(serialized.contains("\"precedence\":\"ceiling_wins\""));
    assert!(serialized.contains("\"disposition\":\"not_invoked_ceiling\""));
    assert!(!serialized.contains("allow_candidate_consumed"));
    Ok(())
}

#[test]
fn classifier_config_unavailable_records_failed_closed_without_zeroes() -> Result<(), Box<dyn Error>>
{
    let read = action("read_file", SafetyCapability::FsRead, json!("src/lib.rs"));
    let rules = evaluate_static_rules(&read, &PermissionRuleInput::default());
    let decision = decide_permission(policy_input(read.clone(), rules));

    let evidence = skipped_classifier_evidence(
        1_000,
        "permission_classifier.primary",
        &read,
        &decision,
        ClassifierFallbackCause::ConfigUnavailable,
    );
    let serialized = serde_json::to_string(&evidence)?;

    assert_eq!(evidence.disposition, ClassifierDisposition::FailedClosed);
    assert!(serialized.contains("\"fallback_cause\":\"config_unavailable\""));
    assert!(serialized.contains("\"unavailable_reason\":\"config_unavailable\""));
    assert!(!serialized.contains("\"value\":0"));
    Ok(())
}

#[test]
fn classifier_provider_timeout_records_timeout_fallback_without_zeroes(
) -> Result<(), Box<dyn Error>> {
    let read = action("read_file", SafetyCapability::FsRead, json!("src/lib.rs"));
    let rules = evaluate_static_rules(&read, &PermissionRuleInput::default());
    let initial_decision = decide_permission(policy_input(read.clone(), rules.clone()));
    let mut final_input = policy_input(read.clone(), rules);
    final_input.evaluator = Some(classifier_verdict(AutoEvaluatorVerdictKind::Timeout));
    let final_decision = decide_permission(final_input);
    let request_payload = json!({"fixture_case":"provider_timeout"});
    let verdict = classifier_verdict(AutoEvaluatorVerdictKind::Timeout);

    let evidence = classifier_decision_evidence(ClassifierEvidenceInput {
        created_at_unix_ms: 1_000,
        completed_at_unix_ms: Some(1_125),
        model_id: "spec030-classifier-fixture-model",
        action: &read,
        initial_decision: &initial_decision,
        final_decision: &final_decision,
        request_payload: &request_payload,
        verdict: &verdict,
        usage: None,
        attempt_status: ClassifierAttemptStatus::ProviderTimeout,
    });
    let serialized = serde_json::to_string(&evidence)?;

    assert_eq!(evidence.disposition, ClassifierDisposition::FailedClosed);
    assert!(serialized.contains("\"fallback_cause\":\"provider_timeout\""));
    assert!(serialized.contains("\"unavailable_reason\":\"provider_error\""));
    assert!(!serialized.contains("\"value\":0"));
    Ok(())
}

#[test]
fn classifier_stale_policy_snapshot_evidence_is_not_success_accounting(
) -> Result<(), Box<dyn Error>> {
    let mut read = action("read_file", SafetyCapability::FsRead, json!("src/lib.rs"));
    read.policy_safety_snapshot_ref = Some(policy_safety_ref("stale-classifier", Some(499)));
    let rules = evaluate_static_rules(&read, &PermissionRuleInput::default());
    let decision = decide_permission(policy_input(read.clone(), rules));

    let evidence = skipped_classifier_evidence(
        1_000,
        "spec030-classifier-fixture-model",
        &read,
        &decision,
        ClassifierFallbackCause::ConfigUnavailable,
    );
    let serialized = serde_json::to_string(&evidence)?;

    assert!(serialized.contains("snapshot-stale-classifier"));
    assert!(serialized.contains("\"disposition\":\"failed_closed\""));
    assert!(!serialized.contains("allow_candidate_consumed"));
    Ok(())
}

#[test]
fn classifier_fixture_cases_match_prd004_contract_and_drive_evidence() -> Result<(), Box<dyn Error>>
{
    let fixture =
        include_str!("fixtures/spec030_prd004_classifier_accounting/provider-fixture.jsonl");
    let observed = fixture
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let cases = observed
        .iter()
        .filter_map(|value| value.get("case").and_then(Value::as_str))
        .collect::<Vec<_>>();

    for required in [
        "normal",
        "missing_accounting",
        "provider_error",
        "malformed_verdict",
        "static_deny_precedence",
        "diagnostics_bundle",
    ] {
        assert!(
            cases.contains(&required),
            "missing fixture case {required}: {cases:?}"
        );
    }

    let normal = observed
        .iter()
        .find(|value| value.get("case").and_then(Value::as_str) == Some("normal"))
        .ok_or("normal fixture case missing")?;
    let read = action("read_file", SafetyCapability::FsRead, json!("src/lib.rs"));
    let rules = evaluate_static_rules(&read, &PermissionRuleInput::default());
    let initial_decision = decide_permission(policy_input(read.clone(), rules.clone()));
    let mut final_input = policy_input(read.clone(), rules);
    final_input.evaluator = Some(classifier_verdict(AutoEvaluatorVerdictKind::AllowCandidate));
    let final_decision = decide_permission(final_input);
    let verdict = classifier_verdict(AutoEvaluatorVerdictKind::AllowCandidate);
    let usage = BTreeMap::from([
        (
            "prompt_tokens".to_owned(),
            normal
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .ok_or("normal prompt_tokens missing")?,
        ),
        (
            "completion_tokens".to_owned(),
            normal
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .ok_or("normal completion_tokens missing")?,
        ),
    ]);

    let evidence = classifier_decision_evidence(ClassifierEvidenceInput {
        created_at_unix_ms: 1_000,
        completed_at_unix_ms: Some(1_007),
        model_id: "spec030-classifier-fixture-model",
        action: &read,
        initial_decision: &initial_decision,
        final_decision: &final_decision,
        request_payload: normal,
        verdict: &verdict,
        usage: Some(&usage),
        attempt_status: ClassifierAttemptStatus::Success,
    });

    assert_eq!(
        evidence.token_accounting.input.state,
        AccountingState::Measured
    );
    assert_eq!(evidence.token_accounting.input.value, Some(17));
    assert_eq!(evidence.token_accounting.output.value, Some(3));
    assert_eq!(
        evidence.cost.total.unavailable_reason,
        Some(AccountingUnavailableReason::PriceUnconfigured)
    );
    Ok(())
}
