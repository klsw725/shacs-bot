use shacs_core::runtime::{
    AccountingState, AccountingUnavailableReason, AccountingValue, ClassifierActionCorrelation,
    ClassifierCostAccounting, ClassifierDecisionEvidence, ClassifierDisposition,
    ClassifierEvidenceId, ClassifierEvidenceSchemaId, ClassifierLatencyAccounting,
    ClassifierModelEvidence, ClassifierRequestCorrelation, ClassifierRouteEvidence,
    ClassifierRouteKind, ClassifierTokenAccounting, ClassifierVerdictEvidence,
    ContainmentComparisonOutcome, ContainmentPermissionProof,
    ContainmentPermissionProofProjectionInput, EvaluatorConfidence, EvaluatorScopeMatch,
    PermissionCeilingComparisonOutcome, PermissionDiagnosticsSummary, PermissionPolicyDecision,
    PermissionPolicyDecisionKind, PermissionPolicyReason,
    PermissionPolicySafetySnapshotDiagnosticsSummary, PermissionSecretRefAuditSummary,
    PermissionSecretRefDiagnosticsSummary, PermissionSecretRefStatus, PolicySafetyDigest,
    PolicySafetySnapshotId, PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId,
    ProcessAdapterKind, ProcessEnvelopeAdmission, ProcessExecutionReceipt, ProcessIdentity,
    ProcessRedactedCommand, ProcessRedactedSpawnSummary, ProcessTerminalOutcome,
    RedactedPolicySafetySummary, SafetyCapability, StaticPolicyPrecedence,
    WorkspaceComparisonOutcome,
};

pub fn permission_summary(policy_ref: PolicySafetySnapshotRef) -> PermissionDiagnosticsSummary {
    PermissionDiagnosticsSummary {
        allow_count: 1,
        ask_count: 0,
        deny_count: 0,
        auto_approval_reasons: vec![PermissionPolicyReason::EvaluatorAllow],
        ask_reasons: Vec::new(),
        deny_reasons: Vec::new(),
        evaluator_failure_count: 0,
        evaluator_failure_reasons: Vec::new(),
        containment_warning_count: 0,
        containment_warnings: Vec::new(),
        protected_target_count: 0,
        protected_target_reasons: Vec::new(),
        secret_refs: PermissionSecretRefDiagnosticsSummary {
            unresolved_count: 1,
            items: vec![PermissionSecretRefAuditSummary {
                ref_id: "sec_spec030".to_owned(),
                source_kind: "env".to_owned(),
                safe_summary: "env:SPEC030_TOKEN".to_owned(),
                redaction_evidence_ref: "redaction:sec_spec030".to_owned(),
                status: PermissionSecretRefStatus::Unresolved,
                requested_consumer: "process:exec".to_owned(),
            }],
            ..PermissionSecretRefDiagnosticsSummary::default()
        },
        policy_safety_refs: PermissionPolicySafetySnapshotDiagnosticsSummary {
            present_count: 1,
            items: vec![
                shacs_core::runtime::PermissionPolicySafetySnapshotAuditSummary {
                    status: shacs_core::runtime::PermissionPolicySafetySnapshotAuditStatus::Present,
                    snapshot_id: Some(policy_ref.snapshot_id.0.clone()),
                    policy_safety_digest: Some(policy_ref.policy_safety_digest.0.clone()),
                },
            ],
            ..PermissionPolicySafetySnapshotDiagnosticsSummary::default()
        },
    }
}

pub fn process_receipt(
    policy_safety_snapshot_ref: PolicySafetySnapshotRef,
) -> ProcessExecutionReceipt {
    ProcessExecutionReceipt {
        receipt_id: "receipt:aggregate-safe".to_owned(),
        idempotency_key: "idempotency:aggregate-safe".to_owned(),
        identity: ProcessIdentity::new("process-ref", "session-1", "turn-1"),
        adapter: ProcessAdapterKind::ExecTool,
        policy_decision: PermissionPolicyDecision {
            kind: PermissionPolicyDecisionKind::Allow,
            reason: PermissionPolicyReason::EvaluatorAllow,
            evaluator_ref: Some("classifier:evidence".to_owned()),
            approval_ref: None,
            approval_error: None,
            can_handoff_to_tool_runtime: true,
        },
        terminal_outcome: ProcessTerminalOutcome::Succeeded,
        dispatch_count: 1,
        redacted_command: ProcessRedactedCommand {
            command_family: "sh".to_owned(),
            redacted_summary: "redacted command".to_owned(),
            redacted_targets: vec!["workspace".to_owned()],
        },
        redacted_summary: ProcessRedactedSpawnSummary::empty(),
        policy_safety_snapshot_ref,
        secret_ref_count: 1,
    }
}

pub fn containment_proof(receipt: &ProcessExecutionReceipt) -> ContainmentPermissionProof {
    ContainmentPermissionProof {
        proof_id: "containment-proof:aggregate-safe".to_owned(),
        policy_safety_digest: receipt
            .policy_safety_snapshot_ref
            .policy_safety_digest
            .clone(),
        envelope_id: "process:session-1:turn-1:action".to_owned(),
        containment_outcome: ContainmentComparisonOutcome::EqualContainment,
        workspace_outcome: WorkspaceComparisonOutcome::SameScope,
        ceiling_outcome: PermissionCeilingComparisonOutcome::EqualCeiling,
        admission: ProcessEnvelopeAdmission::Admit,
        violations: Vec::new(),
        diagnostics_input: ContainmentPermissionProofProjectionInput {
            proof_id: "containment-proof:aggregate-safe".to_owned(),
            envelope_id: "process:session-1:turn-1:action".to_owned(),
            policy_safety_digest: receipt
                .policy_safety_snapshot_ref
                .policy_safety_digest
                .clone(),
            parent_boundary_kind: shacs_core::runtime::RuntimeBoundaryKind::UserTurn,
            child_boundary_kind: shacs_core::runtime::RuntimeBoundaryKind::ExecTool,
            admission: ProcessEnvelopeAdmission::Admit,
            redacted_summary: "boundary=exec; admission=admit".to_owned(),
        },
        blocked_external_surface: None,
    }
}

pub fn classifier_evidence(
    policy_safety_snapshot_ref: PolicySafetySnapshotRef,
) -> ClassifierDecisionEvidence {
    ClassifierDecisionEvidence {
        schema_id: ClassifierEvidenceSchemaId::V1,
        evidence_id: ClassifierEvidenceId("classifier:evidence".to_owned()),
        created_at_unix_ms: 1_000,
        request: ClassifierRequestCorrelation {
            provider_call_id: Some("provider-call-ref".to_owned()),
            classifier_request_digest: "sha256:classifier-request".to_owned(),
        },
        action: ClassifierActionCorrelation {
            action_id: "action-aggregate-safe".to_owned(),
            provider_tool_call_id: Some("tool-call-ref".to_owned()),
            tool_name: "exec".to_owned(),
            action_digest: "sha256:action".to_owned(),
            argument_digest: "sha256:arguments".to_owned(),
            snapshot_digest: "sha256:legacy-snapshot".to_owned(),
            policy_safety_snapshot_ref: Some(policy_safety_snapshot_ref),
            capabilities: vec![SafetyCapability::ProcExec],
        },
        route: ClassifierRouteEvidence {
            route_id: "permission_classifier.primary".to_owned(),
            kind: ClassifierRouteKind::Primary,
        },
        model: ClassifierModelEvidence {
            model_id: "spec030-model".to_owned(),
            source_ref: Some("provider-config-ref".to_owned()),
        },
        token_accounting: ClassifierTokenAccounting {
            input: measured(7, "tokens"),
            output: unavailable(AccountingUnavailableReason::ProviderOmittedUsage),
        },
        latency: ClassifierLatencyAccounting {
            duration_ms: measured(3, "ms"),
        },
        cost: ClassifierCostAccounting {
            total: unavailable(AccountingUnavailableReason::PriceUnconfigured),
        },
        verdict: ClassifierVerdictEvidence {
            verdict: shacs_core::runtime::AutoEvaluatorVerdictKind::Timeout,
            confidence: EvaluatorConfidence::Unknown,
            scope_match: EvaluatorScopeMatch::Requested,
            prompt_injection_signal_count: 0,
            explanation_refs: Vec::new(),
        },
        precedence: StaticPolicyPrecedence::ClassifierReviewable,
        disposition: ClassifierDisposition::FailedClosed,
        fallback: None,
        diagnostics: Vec::new(),
    }
}

pub fn policy_ref(label: &str, expires_at_unix_ms: Option<u64>) -> PolicySafetySnapshotRef {
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
            containment_digest: Some("containment-digest".to_owned()),
            source_ref_count: 1,
            provenance_ref_count: 1,
        },
    }
}

fn measured(value: u64, unit: &str) -> AccountingValue {
    AccountingValue {
        state: AccountingState::Measured,
        value: Some(value),
        unit: Some(unit.to_owned()),
        unavailable_reason: None,
        estimator_id: None,
        basis: None,
        confidence: None,
    }
}

fn unavailable(reason: AccountingUnavailableReason) -> AccountingValue {
    AccountingValue {
        state: AccountingState::Unavailable,
        value: None,
        unit: None,
        unavailable_reason: Some(reason),
        estimator_id: None,
        basis: None,
        confidence: None,
    }
}
