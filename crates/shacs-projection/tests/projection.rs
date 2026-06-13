use serde_json::to_string;
use shacs_eval::evaluator::{
    spec018_acknowledgement_is_user_decision, spec018_evidence_ref_has_owner_and_redaction,
    spec018_ledger_inspect_links_runtime_projection_and_diagnostics,
    spec018_manifest_includes_all_evidence_categories, spec018_manifest_redaction_is_valid,
    DeliverySeverity, EvidenceKind, EvidenceRef, ProjectionSurface, RedactionStatus,
    Spec018AllowedDecision, Spec018ApprovalDecisionKind, Spec018ApprovalProjectionItem,
    Spec018AutomationDeliveryStatus, Spec018BlockedProjectionItem, Spec018BlockedReasonClass,
    Spec018ClosureCoverageBucket, Spec018LedgerInspectQuery, Spec018LedgerInspectQueryKind,
    Spec018Projection, Spec018ProjectionStatus, Spec018ProjectionStatusKind, Spec018ReleaseBlocker,
    Spec018ReleaseBlockerCategory, Spec018ReleaseBlockerSeverity, Spec018ReleaseCoverageEntry,
    Spec018ReleaseCoverageStatus, Spec018RetryEligibility, Spec018RollbackEligibility,
    Spec018SkippedEvidence, Spec018SkippedEvidenceClassification,
    Spec018VerificationProjectionItem, Spec018VerificationResultKind,
    SPEC018_PROJECTION_SCHEMA_LABEL, SPEC018_PROJECTION_SCHEMA_VERSION,
};
use shacs_projection::{
    build_spec018_diagnostics_manifest, build_spec018_ledger_inspect_result,
    build_spec018_projection, evaluate_spec018_release_gate, runtime_spec018_channel_projection,
    runtime_spec018_local_api_projection, tool_search_prd005_release_evidence_checklist,
    tool_search_prd006_release_evidence_checklist, RuntimeSpec018DiagnosticsManifestInput,
    RuntimeSpec018LedgerInspectInput, RuntimeSpec018ProjectionInput,
    RuntimeSpec018ReleaseGateInput, ToolSearchReleaseEvidence, ToolSearchReleaseEvidenceBucket,
};
use std::error::Error;

fn spec018_evidence_ref(
    kind: EvidenceKind,
    id: &str,
    redaction_status: RedactionStatus,
) -> EvidenceRef {
    EvidenceRef {
        kind,
        id: id.to_owned(),
        digest: format!("digest-{id}"),
        summary: format!("summary-{id}"),
        redaction_status,
        owner_spec: Some("018".to_owned()),
        locator: Some(format!("inspect://{id}")),
        retention_hint: Some("local".to_owned()),
    }
}

fn prd005_evidence_ref(id: &str, redaction_status: RedactionStatus) -> EvidenceRef {
    EvidenceRef {
        kind: EvidenceKind::DiagnosticRecord,
        id: id.to_owned(),
        digest: format!("digest-{id}"),
        summary: format!("summary-{id}"),
        redaction_status,
        owner_spec: Some("020".to_owned()),
        locator: Some(format!("prd005://{id}")),
        retention_hint: Some("release_evidence".to_owned()),
    }
}

fn runtime_spec018_release_entry(
    bucket: Spec018ClosureCoverageBucket,
    id: &str,
) -> Spec018ReleaseCoverageEntry {
    Spec018ReleaseCoverageEntry {
        entry_id: id.to_owned(),
        capability_area: bucket,
        required_evidence: vec![spec018_evidence_ref(
            EvidenceKind::DiagnosticRecord,
            &format!("required-{id}"),
            RedactionStatus::Redacted,
        )],
        test_refs: vec![spec018_evidence_ref(
            EvidenceKind::TaskResult,
            &format!("test-{id}"),
            RedactionStatus::AlreadySafe,
        )],
        replay_refs: vec![spec018_evidence_ref(
            EvidenceKind::ReplayResult,
            &format!("replay-{id}"),
            RedactionStatus::Redacted,
        )],
        manual_refs: vec![spec018_evidence_ref(
            EvidenceKind::DiagnosticRecord,
            &format!("manual-{id}"),
            RedactionStatus::AlreadySafe,
        )],
        diagnostics_artifact_refs: vec![spec018_evidence_ref(
            EvidenceKind::DiagnosticRecord,
            &format!("diagnostics-{id}"),
            RedactionStatus::Redacted,
        )],
        status: Spec018ReleaseCoverageStatus::Pass,
        blocker_refs: Vec::new(),
    }
}

fn spec018_status(
    kind: Spec018ProjectionStatusKind,
    evidence_refs: Vec<EvidenceRef>,
) -> Spec018ProjectionStatus {
    Spec018ProjectionStatus {
        kind,
        severity: None,
        blocked_reason_class: None,
        user_action_hint: None,
        evidence_refs,
        retry_eligibility: None,
    }
}

#[test]
fn runtime_spec018_projection_keeps_schema_metadata_and_redacted_evidence_only(
) -> Result<(), Box<dyn Error>> {
    let safe_goal_ref = spec018_evidence_ref(
        EvidenceKind::SessionEvent,
        "goal-safe",
        RedactionStatus::AlreadySafe,
    );
    let unsafe_goal_ref = spec018_evidence_ref(
        EvidenceKind::SessionEvent,
        "goal-unsafe",
        RedactionStatus::RedactionFailed,
    );
    let safe_goal_status_ref = spec018_evidence_ref(
        EvidenceKind::EvaluatorSummary,
        "goal-status-safe",
        RedactionStatus::Redacted,
    );
    let safe_automation_ref = spec018_evidence_ref(
        EvidenceKind::ChannelMessage,
        "automation-safe",
        RedactionStatus::Redacted,
    );
    let safe_approval_ref = spec018_evidence_ref(
        EvidenceKind::ProviderSnapshot,
        "approval-safe",
        RedactionStatus::AlreadySafe,
    );
    let unsafe_approval_status_ref = spec018_evidence_ref(
        EvidenceKind::ToolPayload,
        "approval-status-unsafe",
        RedactionStatus::RedactionFailed,
    );
    let safe_blocked_diagnostics_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "blocked-diagnostics-safe",
        RedactionStatus::AlreadySafe,
    );
    let unsafe_blocked_ref = spec018_evidence_ref(
        EvidenceKind::TaskResult,
        "blocked-unsafe",
        RedactionStatus::RedactionFailed,
    );
    let safe_verification_ref = spec018_evidence_ref(
        EvidenceKind::ReplayRecord,
        "verification-safe",
        RedactionStatus::Redacted,
    );
    let unsafe_verification_status_ref = spec018_evidence_ref(
        EvidenceKind::MemoryEvidenceSet,
        "verification-status-unsafe",
        RedactionStatus::RedactionFailed,
    );
    let raw_secret = "sk-projection-secret";

    let projection = build_spec018_projection(RuntimeSpec018ProjectionInput {
        generated_at_ms: 42,
        session_id: "session-018",
        goal_summaries: &[shacs_eval::evaluator::Spec018GoalSummary {
            goal_id: "goal-018".to_owned(),
            summary: format!("ship the runtime projection with {raw_secret}"),
            status: spec018_status(
                Spec018ProjectionStatusKind::Completed,
                vec![safe_goal_status_ref.clone()],
            ),
            evidence_refs: vec![safe_goal_ref.clone(), unsafe_goal_ref.clone()],
        }],
        automation_summaries: &[Spec018AutomationDeliveryStatus {
            delivery_id: "delivery-018".to_owned(),
            run_id: "run-018".to_owned(),
            target_surface: ProjectionSurface::Channel,
            severity: DeliverySeverity::Warning,
            suppress_reason: None,
            acknowledged: false,
            status: spec018_status(Spec018ProjectionStatusKind::Completed, vec![]),
            evidence_refs: vec![safe_automation_ref.clone()],
        }],
        approval_summaries: &[Spec018ApprovalProjectionItem {
            proposal_id: "proposal-visible".to_owned(),
            target_kind: "local_tool".to_owned(),
            requested_scope: vec!["scope:runtime".to_owned()],
            risk_summary: format!("visible approval {raw_secret}"),
            rollback_summary: format!("rollback available {raw_secret}"),
            allowed_decisions: vec![
                Spec018AllowedDecision {
                    decision: Spec018ApprovalDecisionKind::Approve,
                    unavailable_reason: None,
                },
                Spec018AllowedDecision {
                    decision: Spec018ApprovalDecisionKind::InspectEvidence,
                    unavailable_reason: Some(format!("inspect only {raw_secret}")),
                },
            ],
            status: spec018_status(
                Spec018ProjectionStatusKind::ApprovalRequired,
                vec![unsafe_approval_status_ref.clone()],
            ),
            evidence_refs: vec![safe_approval_ref.clone()],
        }],
        blocked_summaries: &[Spec018BlockedProjectionItem {
            source_kind: "runtime".to_owned(),
            source_ref: "blocked-018".to_owned(),
            blocked_reason_class: Spec018BlockedReasonClass::CapabilityDenied,
            blocked_reason: format!("local capability denied {raw_secret}"),
            user_action_hint: format!("approve local capability {raw_secret}"),
            retry_eligibility: Spec018RetryEligibility::RetryAfterUserAction,
            diagnostics_ref: safe_blocked_diagnostics_ref.clone(),
            evidence_refs: vec![unsafe_blocked_ref.clone()],
        }],
        verification_summaries: &[Spec018VerificationProjectionItem {
            proposal_id: Some("proposal-018".to_owned()),
            replay_case_id: None,
            expected_behavior: format!("verification stayed user-visible {raw_secret}"),
            last_result: Spec018VerificationResultKind::Failed,
            failure_reason: Some(format!("one assertion failed {raw_secret}")),
            rollback_eligibility: Spec018RollbackEligibility::Available,
            status: spec018_status(
                Spec018ProjectionStatusKind::VerificationFailed,
                vec![unsafe_verification_status_ref.clone()],
            ),
            evidence_refs: vec![safe_verification_ref.clone()],
        }],
        replay_summaries: &[],
        recent_evaluator_decision_summaries: &[],
    });

    assert_eq!(projection.schema_label, "018Projection");
    assert_eq!(projection.schema_version, "018Projection.v1");
    assert_eq!(projection.session_id, "session-018");
    assert_eq!(
        projection.evidence_refs,
        vec![
            safe_goal_ref,
            safe_goal_status_ref,
            safe_automation_ref,
            safe_approval_ref,
            safe_blocked_diagnostics_ref,
            safe_verification_ref,
        ]
    );
    let serialized = to_string(&projection)?;
    assert!(!serialized.contains("goal-unsafe"));
    assert!(!serialized.contains("approval-status-unsafe"));
    assert!(!serialized.contains("blocked-unsafe"));
    assert!(!serialized.contains("verification-status-unsafe"));
    assert!(!serialized.contains(raw_secret));

    Ok(())
}

#[test]
fn runtime_spec018_channel_projection_filters_hidden_items_and_keeps_visible_statuses(
) -> Result<(), Box<dyn Error>> {
    let visible_delivery = Spec018AutomationDeliveryStatus {
        delivery_id: "delivery-visible".to_owned(),
        run_id: "run-visible".to_owned(),
        target_surface: ProjectionSurface::Channel,
        severity: DeliverySeverity::Info,
        suppress_reason: None,
        acknowledged: true,
        status: spec018_status(
            Spec018ProjectionStatusKind::WaitingForUser,
            vec![spec018_evidence_ref(
                EvidenceKind::ChannelMessage,
                "delivery-visible-status",
                RedactionStatus::AlreadySafe,
            )],
        ),
        evidence_refs: vec![spec018_evidence_ref(
            EvidenceKind::ChannelMessage,
            "delivery-visible",
            RedactionStatus::AlreadySafe,
        )],
    };
    let suppressed_delivery = Spec018AutomationDeliveryStatus {
        delivery_id: "delivery-hidden".to_owned(),
        run_id: "run-hidden".to_owned(),
        target_surface: ProjectionSurface::Channel,
        severity: DeliverySeverity::Warning,
        suppress_reason: Some("internal noise".to_owned()),
        acknowledged: false,
        status: spec018_status(Spec018ProjectionStatusKind::Suppressed, vec![]),
        evidence_refs: vec![spec018_evidence_ref(
            EvidenceKind::ChannelMessage,
            "delivery-hidden",
            RedactionStatus::Redacted,
        )],
    };
    let visible_approval = Spec018ApprovalProjectionItem {
        proposal_id: "proposal-visible".to_owned(),
        target_kind: "local_tool".to_owned(),
        requested_scope: vec!["scope:runtime".to_owned()],
        risk_summary: "visible approval".to_owned(),
        rollback_summary: "rollback available".to_owned(),
        allowed_decisions: vec![
            Spec018AllowedDecision {
                decision: Spec018ApprovalDecisionKind::Approve,
                unavailable_reason: None,
            },
            Spec018AllowedDecision {
                decision: Spec018ApprovalDecisionKind::InspectEvidence,
                unavailable_reason: Some("inspect only".to_owned()),
            },
        ],
        status: spec018_status(Spec018ProjectionStatusKind::ApprovalRequired, vec![]),
        evidence_refs: vec![spec018_evidence_ref(
            EvidenceKind::ProviderSnapshot,
            "approval-visible",
            RedactionStatus::AlreadySafe,
        )],
    };
    let inspect_only_approval = Spec018ApprovalProjectionItem {
        proposal_id: "proposal-hidden".to_owned(),
        target_kind: "local_tool".to_owned(),
        requested_scope: vec!["scope:runtime".to_owned()],
        risk_summary: "hidden approval".to_owned(),
        rollback_summary: "rollback available".to_owned(),
        allowed_decisions: vec![Spec018AllowedDecision {
            decision: Spec018ApprovalDecisionKind::InspectEvidence,
            unavailable_reason: None,
        }],
        status: spec018_status(Spec018ProjectionStatusKind::ApprovalRequired, vec![]),
        evidence_refs: vec![spec018_evidence_ref(
            EvidenceKind::ProviderSnapshot,
            "approval-hidden",
            RedactionStatus::Redacted,
        )],
    };
    let blocked_item = Spec018BlockedProjectionItem {
        source_kind: "runtime".to_owned(),
        source_ref: "blocked-visible".to_owned(),
        blocked_reason_class: Spec018BlockedReasonClass::CapabilityDenied,
        blocked_reason: "local capability denied".to_owned(),
        user_action_hint: "approve local capability".to_owned(),
        retry_eligibility: Spec018RetryEligibility::RetryAfterUserAction,
        diagnostics_ref: spec018_evidence_ref(
            EvidenceKind::DiagnosticRecord,
            "blocked-diagnostics",
            RedactionStatus::AlreadySafe,
        ),
        evidence_refs: vec![],
    };
    let verification_item = Spec018VerificationProjectionItem {
        proposal_id: Some("proposal-visible".to_owned()),
        replay_case_id: None,
        expected_behavior: "verification failed visibly".to_owned(),
        last_result: Spec018VerificationResultKind::Failed,
        failure_reason: Some("one assertion failed".to_owned()),
        rollback_eligibility: Spec018RollbackEligibility::Available,
        status: spec018_status(Spec018ProjectionStatusKind::VerificationFailed, vec![]),
        evidence_refs: vec![spec018_evidence_ref(
            EvidenceKind::ReplayRecord,
            "verification-visible",
            RedactionStatus::AlreadySafe,
        )],
    };
    let projection = build_spec018_projection(RuntimeSpec018ProjectionInput {
        generated_at_ms: 42,
        session_id: "session-018",
        goal_summaries: &[],
        automation_summaries: &[visible_delivery.clone(), suppressed_delivery],
        approval_summaries: &[visible_approval.clone(), inspect_only_approval],
        blocked_summaries: std::slice::from_ref(&blocked_item),
        verification_summaries: std::slice::from_ref(&verification_item),
        replay_summaries: &[],
        recent_evaluator_decision_summaries: &[],
    });

    let channel_projection = runtime_spec018_channel_projection(&projection);
    let acknowledged_as_decision =
        spec018_acknowledgement_is_user_decision(&visible_delivery, &visible_approval);

    assert_eq!(channel_projection.automation_summaries.len(), 1);
    assert_eq!(
        channel_projection.automation_summaries[0].delivery_id,
        "delivery-visible"
    );
    assert_eq!(channel_projection.approval_summaries.len(), 1);
    assert_eq!(
        channel_projection.approval_summaries[0].proposal_id,
        "proposal-visible"
    );
    assert_eq!(channel_projection.blocked_summaries.len(), 1);
    assert_eq!(
        channel_projection.blocked_summaries[0].source_ref,
        "blocked-visible"
    );
    assert_eq!(channel_projection.verification_summaries.len(), 1);
    assert_eq!(
        channel_projection.verification_summaries[0].status.kind,
        Spec018ProjectionStatusKind::VerificationFailed
    );
    let serialized = to_string(&channel_projection)?;
    assert!(!serialized.contains("delivery-hidden"));
    assert!(!serialized.contains("approval-hidden"));
    assert!(!channel_projection
        .evidence_refs
        .iter()
        .any(|evidence_ref| evidence_ref.id == "delivery-hidden"
            || evidence_ref.id == "approval-hidden"));
    assert!(!acknowledged_as_decision);

    let unsafe_ref = spec018_evidence_ref(
        EvidenceKind::ChannelMessage,
        "channel-unsafe",
        RedactionStatus::RedactionFailed,
    );
    let unsafe_projection = Spec018Projection {
        schema_label: SPEC018_PROJECTION_SCHEMA_LABEL.to_owned(),
        schema_version: SPEC018_PROJECTION_SCHEMA_VERSION.to_owned(),
        generated_at_ms: 42,
        session_id: "session-018".to_owned(),
        goal_summaries: vec![],
        automation_summaries: vec![Spec018AutomationDeliveryStatus {
            delivery_id: "delivery-unsafe-nested".to_owned(),
            run_id: "run-unsafe-nested".to_owned(),
            target_surface: ProjectionSurface::Channel,
            severity: DeliverySeverity::Info,
            suppress_reason: Some("contains sk-channel-secret".to_owned()),
            acknowledged: false,
            status: spec018_status(
                Spec018ProjectionStatusKind::WaitingForUser,
                vec![unsafe_ref.clone()],
            ),
            evidence_refs: vec![unsafe_ref.clone()],
        }],
        approval_summaries: vec![],
        blocked_summaries: vec![],
        verification_summaries: vec![],
        replay_summaries: vec![],
        recent_evaluator_decision_summaries: vec![],
        evidence_refs: vec![unsafe_ref],
    };
    let sanitized_channel = runtime_spec018_channel_projection(&unsafe_projection);
    let serialized = to_string(&sanitized_channel)?;
    assert!(sanitized_channel.evidence_refs.is_empty());
    assert!(sanitized_channel.automation_summaries[0]
        .status
        .evidence_refs
        .is_empty());
    assert!(!serialized.contains("channel-unsafe"));
    assert!(!serialized.contains("sk-channel-secret"));

    Ok(())
}

#[test]
fn runtime_spec018_local_api_projection_sanitizes_unsanitized_nested_refs(
) -> Result<(), Box<dyn Error>> {
    let safe_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "diagnostics-018",
        RedactionStatus::AlreadySafe,
    );
    let unsafe_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "diagnostics-unsafe",
        RedactionStatus::RedactionFailed,
    );
    let projection = Spec018Projection {
        schema_label: SPEC018_PROJECTION_SCHEMA_LABEL.to_owned(),
        schema_version: SPEC018_PROJECTION_SCHEMA_VERSION.to_owned(),
        generated_at_ms: 42,
        session_id: "session-018".to_owned(),
        goal_summaries: vec![shacs_eval::evaluator::Spec018GoalSummary {
            goal_id: "goal-local-api".to_owned(),
            summary: "local api projection".to_owned(),
            status: Spec018ProjectionStatus {
                kind: Spec018ProjectionStatusKind::Running,
                severity: None,
                blocked_reason_class: None,
                user_action_hint: None,
                evidence_refs: vec![safe_ref.clone(), unsafe_ref.clone()],
                retry_eligibility: None,
            },
            evidence_refs: vec![unsafe_ref.clone()],
        }],
        automation_summaries: vec![],
        approval_summaries: vec![],
        blocked_summaries: vec![],
        verification_summaries: vec![],
        replay_summaries: vec![],
        recent_evaluator_decision_summaries: vec![],
        evidence_refs: vec![safe_ref.clone(), unsafe_ref.clone()],
    };

    let local_projection = runtime_spec018_local_api_projection(&projection);
    let serialized = to_string(&local_projection)?;

    assert_eq!(
        local_projection.goal_summaries[0].status.evidence_refs,
        vec![safe_ref.clone()]
    );
    assert_eq!(
        local_projection.goal_summaries[0].evidence_refs,
        Vec::<EvidenceRef>::new()
    );
    assert_eq!(local_projection.evidence_refs, vec![safe_ref]);
    assert!(!serialized.contains("diagnostics-unsafe"));

    Ok(())
}

#[test]
fn runtime_spec018_diagnostics_manifest_builds_all_categories_without_raw_secret(
) -> Result<(), Box<dyn Error>> {
    let evaluator_ref = spec018_evidence_ref(
        EvidenceKind::EvaluatorSummary,
        "evaluator-ref",
        RedactionStatus::Redacted,
    );
    let ledger_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "ledger-ref",
        RedactionStatus::AlreadySafe,
    );
    let automation_ref = spec018_evidence_ref(
        EvidenceKind::TaskResult,
        "automation-ref",
        RedactionStatus::Redacted,
    );
    let memory_ref = spec018_evidence_ref(
        EvidenceKind::MemoryEvidenceSet,
        "memory-ref",
        RedactionStatus::Redacted,
    );
    let improvement_ref = spec018_evidence_ref(
        EvidenceKind::ImprovementApplyRecord,
        "improvement-ref",
        RedactionStatus::Redacted,
    );
    let replay_ref = spec018_evidence_ref(
        EvidenceKind::ReplayResult,
        "replay-ref",
        RedactionStatus::Redacted,
    );
    let projection_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "projection-ref",
        RedactionStatus::AlreadySafe,
    );
    let skipped = Spec018SkippedEvidence {
        source_ref: spec018_evidence_ref(
            EvidenceKind::EvaluatorSummary,
            "stale-verdict",
            RedactionStatus::Redacted,
        ),
        classification: Spec018SkippedEvidenceClassification::Stale,
        redacted_summary: "stale verdict skipped".to_owned(),
    };
    let diagnostics_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "diagnostics-artifact",
        RedactionStatus::Redacted,
    );
    let raw_secret = "sk-runtime-secret";

    let manifest = build_spec018_diagnostics_manifest(RuntimeSpec018DiagnosticsManifestInput {
        manifest_id: "manifest-018",
        generated_at_ms: 42,
        redaction_profile: "default",
        evaluator_refs: std::slice::from_ref(&evaluator_ref),
        ledger_refs: std::slice::from_ref(&ledger_ref),
        automation_refs: std::slice::from_ref(&automation_ref),
        memory_refs: std::slice::from_ref(&memory_ref),
        improvement_refs: std::slice::from_ref(&improvement_ref),
        replay_refs: std::slice::from_ref(&replay_ref),
        projection_refs: std::slice::from_ref(&projection_ref),
        skipped_evidence: std::slice::from_ref(&skipped),
        diagnostics_artifact_refs: std::slice::from_ref(&diagnostics_ref),
    });
    let serialized = to_string(&manifest)?;

    assert!(spec018_manifest_includes_all_evidence_categories(&manifest));
    assert!(spec018_manifest_redaction_is_valid(&manifest));
    assert_eq!(manifest.redaction_summary.skipped_ref_count, 1);
    assert!(!serialized.contains(raw_secret));

    Ok(())
}

#[test]
fn runtime_spec018_ledger_inspect_links_verdict_to_decision_projection_and_diagnostics() {
    let query = Spec018LedgerInspectQuery {
        query_kind: Spec018LedgerInspectQueryKind::VerdictId,
        target_ref: "verdict-1".to_owned(),
        include_skipped: true,
        include_diagnostics_refs: true,
        redaction_profile: "default".to_owned(),
    };
    let source_ref = spec018_evidence_ref(
        EvidenceKind::EvaluatorSummary,
        "verdict-1",
        RedactionStatus::Redacted,
    );
    let consumption_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "consumption-1",
        RedactionStatus::AlreadySafe,
    );
    let decision_ref = spec018_evidence_ref(
        EvidenceKind::TaskResult,
        "runtime-decision-1",
        RedactionStatus::Redacted,
    );
    let projection_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "projection-item-1",
        RedactionStatus::Redacted,
    );
    let diagnostics_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "diagnostics-1",
        RedactionStatus::Redacted,
    );
    let skipped = Spec018SkippedEvidence {
        source_ref: spec018_evidence_ref(
            EvidenceKind::EvaluatorSummary,
            "superseded-verdict",
            RedactionStatus::Redacted,
        ),
        classification: Spec018SkippedEvidenceClassification::Superseded,
        redacted_summary: "superseded verdict skipped".to_owned(),
    };

    let result = build_spec018_ledger_inspect_result(RuntimeSpec018LedgerInspectInput {
        query: &query,
        source_refs: std::slice::from_ref(&source_ref),
        consumption_record_refs: std::slice::from_ref(&consumption_ref),
        runtime_decision_refs: std::slice::from_ref(&decision_ref),
        projection_item_refs: std::slice::from_ref(&projection_ref),
        diagnostics_artifact_refs: std::slice::from_ref(&diagnostics_ref),
        skipped_evidence: std::slice::from_ref(&skipped),
    });

    assert!(spec018_ledger_inspect_links_runtime_projection_and_diagnostics(&result));
    assert_eq!(result.skipped_evidence.len(), 1);
    assert_eq!(result.diagnostics_artifact_refs.len(), 1);
}

#[test]
fn runtime_spec018_release_gate_blocks_until_all_buckets_and_no_blockers() {
    let entries: Vec<_> = [
        Spec018ClosureCoverageBucket::EvaluatorFoundation,
        Spec018ClosureCoverageBucket::GoalContinuation,
        Spec018ClosureCoverageBucket::ApprovalGate,
        Spec018ClosureCoverageBucket::AutomationRuntime,
        Spec018ClosureCoverageBucket::MemorySkillIntegration,
        Spec018ClosureCoverageBucket::SelfImprovementWiring,
        Spec018ClosureCoverageBucket::ReplayRunner,
        Spec018ClosureCoverageBucket::ProjectionSemantics,
        Spec018ClosureCoverageBucket::DiagnosticsIntegration,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, bucket)| runtime_spec018_release_entry(bucket, &format!("entry-{index}")))
    .collect();
    let manifest_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "release-manifest",
        RedactionStatus::Redacted,
    );
    let ledger_inspect_ref = spec018_evidence_ref(
        EvidenceKind::DiagnosticRecord,
        "release-ledger-inspect",
        RedactionStatus::Redacted,
    );

    let passing = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &entries,
        blockers: &[],
        diagnostics_manifest_ref: Some(&manifest_ref),
        ledger_inspect_ref: Some(&ledger_inspect_ref),
    });
    assert!(passing.final_closure_passed);

    let missing_diagnostics = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &entries,
        blockers: &[],
        diagnostics_manifest_ref: None,
        ledger_inspect_ref: Some(&ledger_inspect_ref),
    });
    assert!(!missing_diagnostics.final_closure_passed);

    let mut ownerless_manifest_ref = manifest_ref.clone();
    ownerless_manifest_ref.owner_spec = None;
    let invalid_manifest = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &entries,
        blockers: &[],
        diagnostics_manifest_ref: Some(&ownerless_manifest_ref),
        ledger_inspect_ref: Some(&ledger_inspect_ref),
    });
    assert!(!invalid_manifest.final_closure_passed);
    assert!(invalid_manifest
        .blockers
        .iter()
        .all(|blocker| spec018_evidence_ref_has_owner_and_redaction(&blocker.source_ref)));

    let mut failed_redaction_ledger_ref = ledger_inspect_ref.clone();
    failed_redaction_ledger_ref.redaction_status = RedactionStatus::RedactionFailed;
    let invalid_ledger = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &entries,
        blockers: &[],
        diagnostics_manifest_ref: Some(&manifest_ref),
        ledger_inspect_ref: Some(&failed_redaction_ledger_ref),
    });
    assert!(!invalid_ledger.final_closure_passed);
    assert!(invalid_ledger
        .blockers
        .iter()
        .all(|blocker| spec018_evidence_ref_has_owner_and_redaction(&blocker.source_ref)));

    let mut missing_entry_refs = entries.clone();
    missing_entry_refs[0].diagnostics_artifact_refs.clear();
    let missing_entry_ref_outcome = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &missing_entry_refs,
        blockers: &[],
        diagnostics_manifest_ref: Some(&manifest_ref),
        ledger_inspect_ref: Some(&ledger_inspect_ref),
    });
    assert!(!missing_entry_ref_outcome.final_closure_passed);

    let unverified_improvement = Spec018ReleaseBlocker {
        blocker_id: "unverified-improvement".to_owned(),
        category: Spec018ReleaseBlockerCategory::UnverifiedAppliedImprovement,
        source_ref: spec018_evidence_ref(
            EvidenceKind::ImprovementApplyRecord,
            "apply-record",
            RedactionStatus::Redacted,
        ),
        severity: Spec018ReleaseBlockerSeverity::Blocking,
        redacted_summary: "applied improvement lacks verification".to_owned(),
        resolution_hint: "run local verification and attach redacted ref".to_owned(),
    };
    let failed_replay = Spec018ReleaseBlocker {
        blocker_id: "failed-replay".to_owned(),
        category: Spec018ReleaseBlockerCategory::FailedReplayRegression,
        source_ref: spec018_evidence_ref(
            EvidenceKind::ReplayResult,
            "failed-replay-result",
            RedactionStatus::Redacted,
        ),
        severity: Spec018ReleaseBlockerSeverity::Blocking,
        redacted_summary: "replay regression failed".to_owned(),
        resolution_hint: "inspect replay result and fix regression".to_owned(),
    };
    let blocked = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &entries,
        blockers: &[unverified_improvement, failed_replay],
        diagnostics_manifest_ref: Some(&manifest_ref),
        ledger_inspect_ref: Some(&ledger_inspect_ref),
    });
    assert!(!blocked.final_closure_passed);

    let incomplete = evaluate_spec018_release_gate(RuntimeSpec018ReleaseGateInput {
        coverage_entries: &entries[..entries.len() - 1],
        blockers: &[],
        diagnostics_manifest_ref: Some(&manifest_ref),
        ledger_inspect_ref: Some(&ledger_inspect_ref),
    });
    assert!(!incomplete.final_closure_passed);
    assert_eq!(incomplete.missing_buckets.len(), 1);
}

#[test]
fn tool_search_prd005_release_evidence_checklist_requires_all_buckets() {
    let evidence = [
        (
            ToolSearchReleaseEvidenceBucket::Config,
            "tool_search_off_preserves_definition_order_without_catalog",
        ),
        (
            ToolSearchReleaseEvidenceBucket::Assembler,
            "tool_search_diagnostics_summary_reports_activation_reason_families",
        ),
        (
            ToolSearchReleaseEvidenceBucket::Bridge,
            "runtime_runner_bridge_events_use_redacted_bounded_evidence",
        ),
        (
            ToolSearchReleaseEvidenceBucket::RunnerWiring,
            "runtime_runner_bridge_search_describe_call_roundtrip_completes_turn",
        ),
        (
            ToolSearchReleaseEvidenceBucket::McpDefaultDeny,
            "mcp_default_deny_excludes_disabled_capabilities_from_tool_search_bridge",
        ),
        (
            ToolSearchReleaseEvidenceBucket::SubagentScope,
            "subagent_tool_search_catalog_uses_child_registry_not_parent_definitions",
        ),
        (
            ToolSearchReleaseEvidenceBucket::ReplaySafety,
            "replay_runner_executes_selected_cases_only_and_never_dispatches_live_tools",
        ),
        (
            ToolSearchReleaseEvidenceBucket::Diagnostics,
            "runtime_runner_tool_search_activation_diagnostics_are_observable",
        ),
    ]
    .into_iter()
    .map(|(bucket, test_name)| ToolSearchReleaseEvidence {
        bucket,
        test_names: vec![test_name.to_owned()],
        manual_qa_refs: Vec::new(),
        evidence_refs: vec![prd005_evidence_ref(test_name, RedactionStatus::Redacted)],
    })
    .collect::<Vec<_>>();

    let passing = tool_search_prd005_release_evidence_checklist(&evidence);
    assert!(passing.passed);
    assert_eq!(passing.required_buckets.len(), 8);
    assert_eq!(passing.covered_buckets.len(), 8);
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Config));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Assembler));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Bridge));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::RunnerWiring));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::McpDefaultDeny));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::SubagentScope));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::ReplaySafety));
    assert!(passing
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Diagnostics));

    let incomplete = tool_search_prd005_release_evidence_checklist(&evidence[..evidence.len() - 1]);
    assert!(!incomplete.passed);
    assert_eq!(
        incomplete.missing_buckets,
        vec![ToolSearchReleaseEvidenceBucket::Diagnostics]
    );

    let label_only = tool_search_prd005_release_evidence_checklist(&[ToolSearchReleaseEvidence {
        bucket: ToolSearchReleaseEvidenceBucket::Config,
        test_names: vec!["label_without_evidence".to_owned()],
        manual_qa_refs: Vec::new(),
        evidence_refs: Vec::new(),
    }]);
    assert!(!label_only.passed);
    assert!(!label_only
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Config));

    let mut ownerless_ref = prd005_evidence_ref("ownerless", RedactionStatus::Redacted);
    ownerless_ref.owner_spec = None;
    let ownerless = tool_search_prd005_release_evidence_checklist(&[ToolSearchReleaseEvidence {
        bucket: ToolSearchReleaseEvidenceBucket::Bridge,
        test_names: vec!["ownerless_ref".to_owned()],
        manual_qa_refs: Vec::new(),
        evidence_refs: vec![ownerless_ref],
    }]);
    assert!(!ownerless
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Bridge));

    let redaction_failed =
        tool_search_prd005_release_evidence_checklist(&[ToolSearchReleaseEvidence {
            bucket: ToolSearchReleaseEvidenceBucket::Diagnostics,
            test_names: Vec::new(),
            manual_qa_refs: vec!["manual-label".to_owned()],
            evidence_refs: vec![prd005_evidence_ref(
                "redaction-failed",
                RedactionStatus::RedactionFailed,
            )],
        }]);
    assert!(!redaction_failed
        .covered_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::Diagnostics));
}

#[test]
fn tool_search_prd006_release_evidence_requires_user_facing_bucket_only() {
    let mut evidence = ToolSearchReleaseEvidenceBucket::required_prd005_buckets()
        .into_iter()
        .map(|bucket| ToolSearchReleaseEvidence {
            bucket,
            test_names: vec![format!("{bucket:?}_test")],
            manual_qa_refs: Vec::new(),
            evidence_refs: vec![prd005_evidence_ref(
                &format!("{bucket:?}"),
                RedactionStatus::Redacted,
            )],
        })
        .collect::<Vec<_>>();

    let prd005_only = tool_search_prd006_release_evidence_checklist(&evidence);
    assert!(!prd005_only.passed);
    assert_eq!(
        prd005_only.missing_buckets,
        vec![ToolSearchReleaseEvidenceBucket::UserFacingConfig]
    );

    evidence.push(ToolSearchReleaseEvidence {
        bucket: ToolSearchReleaseEvidenceBucket::UserFacingConfig,
        test_names: vec![
            "runtime_runner_tool_search_activation_diagnostics_are_observable".to_owned(),
        ],
        manual_qa_refs: Vec::new(),
        evidence_refs: vec![prd005_evidence_ref(
            "user-facing-config",
            RedactionStatus::Redacted,
        )],
    });

    let complete = tool_search_prd006_release_evidence_checklist(&evidence);
    assert!(complete.passed);
    assert_eq!(complete.required_buckets.len(), 9);
    assert!(!complete
        .required_buckets
        .contains(&ToolSearchReleaseEvidenceBucket::PluginToolIntegration));
}
