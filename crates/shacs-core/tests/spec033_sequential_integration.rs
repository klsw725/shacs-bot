use shacs_core::runtime::{
    coordinate_automation_run, create_persistent_goal, replay_recorded_trajectory,
    AutomationSourceEvent, AutomationSourceEventKind, RecordedBoundaryRequirement,
    RecordedTrajectoryInput, RecordedTrajectoryStore,
};
use shacs_eval::completion_boundary::{
    record_evaluator_boundary, DeliveryOutcome, EvaluatorBoundaryContext,
    EvaluatorBoundaryRecordInput, EvaluatorRoute, OwnerResultLocator, TaskResultOutcome,
};
use shacs_eval::evaluator::{
    AutomationExecutionMode, AutomationRecursionGuard, ConfidenceBand, EvaluationTriggerSource,
    EvaluatorRequestEnvelope, EvaluatorVerdictEnvelope, ProjectionStatus, RedactionStatus,
    ReplayDatasetItem, SuggestedNextAction, TaskOutcomeClass, VerdictKind,
};

#[test]
fn goal_evaluator_automation_and_replay_preserve_owner_boundaries(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let goal = create_persistent_goal("session-1", "ship it", "t0", 2);
    let evaluator = record_evaluator_boundary(EvaluatorBoundaryRecordInput {
        input: EvaluatorRequestEnvelope {
            request_id: "request-1".to_owned(),
            evaluator_kind: shacs_eval::evaluator::EvaluatorKind::GoalCompletion,
            correlation_id: "corr-1".to_owned(),
            session_id: Some("session-1".to_owned()),
            turn_id: None,
            source: EvaluationTriggerSource::SessionTurn,
            snapshot_digest: "snapshot-1".to_owned(),
            redaction_profile: "default".to_owned(),
            caller_intent: "advisory".to_owned(),
        },
        output: EvaluatorVerdictEnvelope {
            verdict_kind: VerdictKind::Pass,
            reason: "evidence accepted".to_owned(),
            confidence: 0.9,
            evidence_refs: Vec::new(),
            suggested_next_action: SuggestedNextAction::None,
            expires_at_ms: None,
            redaction_status: RedactionStatus::AlreadySafe,
            evaluator_version: "eval-v1".to_owned(),
        },
        requested_route: EvaluatorRoute::Continue,
        owner_result_locator: OwnerResultLocator::new("goal://goal-1"),
        task_outcome: TaskResultOutcome::Succeeded,
        delivery_outcome: DeliveryOutcome::NotRequested,
        context: EvaluatorBoundaryContext {
            user_interrupted: false,
            continuation_budget_remaining: 2,
        },
    });
    let automation = coordinate_automation_run(&automation_event(&goal.id), &[]);

    // When
    let replay = replay_receipt()?;

    // Then
    assert!(!evaluator.grants_execution_authority());
    assert_eq!(
        automation.prd008_linkage.goal_id.as_deref(),
        Some(goal.id.as_str())
    );
    assert!(automation.request.is_some());
    assert!(!automation.task_outcome_eligibility.direct_execution_allowed);
    assert!(
        !automation
            .task_outcome_eligibility
            .app_authority_can_apply_self_improvement
    );
    assert_eq!(replay.compared_recorded_outcomes, 1);
    Ok(())
}

fn automation_event(goal_id: &str) -> AutomationSourceEvent {
    AutomationSourceEvent {
        runtime_service_event_id: "event-1".to_owned(),
        source_owner: "runtime-service".to_owned(),
        received_at_ms: 1,
        job_id: "job-1".to_owned(),
        session_id: Some("session-1".to_owned()),
        goal_id: Some(goal_id.to_owned()),
        active_goal: true,
        pending_automation: true,
        execution_mode: AutomationExecutionMode::NoAgentCheck,
        timeout_policy_ref: "timeout-1".to_owned(),
        retry_policy_ref: "retry-1".to_owned(),
        delivery_policy_ref: "delivery-1".to_owned(),
        recursion_guard: AutomationRecursionGuard {
            token: "guard-1".to_owned(),
            source_run_id: None,
            depth: 0,
            max_depth: 3,
            parent_refs: Vec::new(),
            blocked_reason: None,
        },
        prd008_goal_gate_ref: Some("goal-gate-1".to_owned()),
        source: AutomationSourceEventKind::ManualResume {
            resume_ref: "resume-1".to_owned(),
        },
    }
}

fn replay_receipt(
) -> Result<shacs_core::runtime::RecordedTrajectoryReplayReceipt, Box<dyn std::error::Error>> {
    let snapshot = replay_snapshot()?;
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    store.write(RecordedTrajectoryInput {
        trajectory_id: "trajectory-1".to_owned(),
        snapshot,
        sources: Vec::new(),
        owner_outcome: ReplayDatasetItem {
            dataset_id: "dataset-1".to_owned(),
            case_id: "case-1".to_owned(),
            trajectory_refs: Vec::new(),
            expected_verdict: VerdictKind::Pass,
            expected_outcome: TaskOutcomeClass::Notify,
            expected_projection_status: ProjectionStatus::Success,
            expected_confidence_band: ConfidenceBand::High,
            allowed_judge_roles: Vec::new(),
            redaction_profile: "default".to_owned(),
            tool_outcome_policies: Vec::new(),
            actual_verdict: Some(VerdictKind::Pass),
            actual_outcome: Some(TaskOutcomeClass::Notify),
            actual_projection_status: Some(ProjectionStatus::Success),
            actual_confidence_band: Some(ConfidenceBand::High),
            auxiliary_judge_routes: Vec::new(),
            diagnostics_refs: Vec::new(),
            coverage_refs: Vec::new(),
        },
        boundary_requirement: RecordedBoundaryRequirement::RecordedOnly,
        origin: shacs_core::runtime::RecordedTrajectoryOrigin::Fixture,
    })?;
    Ok(replay_recorded_trajectory(&store, "trajectory-1", "run-1")?)
}

fn replay_snapshot() -> Result<shacs_core::runtime::ExecutionSnapshot, Box<dyn std::error::Error>> {
    use shacs_core::runtime::{
        ConfigMigrationState, ConfigSnapshotRef, CredentialSnapshotRef, DataDisclosureWarning,
        ExecutionSnapshot, ExecutionSnapshotInput, ProfileSelectionSnapshot, ProviderInputSnapshot,
        ReplayContract, TokenBudgetSnapshot, TrustedRuntimeFactRef,
    };
    use shacs_projection::{CredentialFingerprintStatus, CredentialStatus};

    Ok(ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: "snapshot-1".to_owned(),
        created_at_unix_ms: 1,
        config: ConfigSnapshotRef {
            source_ref: "config:1".to_owned(),
            schema_version: 1,
            migration_state: ConfigMigrationState::Current,
        },
        profiles: ProfileSelectionSnapshot {
            provider: None,
            trusted_runtime: Some("trusted:1".to_owned()),
            context: None,
        },
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: 1,
            profile_ref: "trusted:1".to_owned(),
            projection_digest: "sha256:trusted".to_owned(),
        },
        sandbox: Vec::new(),
        credential: CredentialSnapshotRef {
            source_kind: None,
            status: CredentialStatus::Resolved,
            fingerprint_status: CredentialFingerprintStatus::Current,
        },
        context_sources: Vec::new(),
        selected_tools: Vec::new(),
        selected_resources: Vec::new(),
        provider: ProviderInputSnapshot {
            provider: "provider".to_owned(),
            model: "model".to_owned(),
            shaping_version: "v1".to_owned(),
            messages_digest: "sha256:messages".to_owned(),
            tools_digest: "sha256:tools".to_owned(),
        },
        token_budget: TokenBudgetSnapshot {
            tokenizer: "estimate".to_owned(),
            estimator_uncertainty_percent: 0,
            budget_tokens: 100,
            reserved_tokens: 0,
            used_context_tokens: 0,
            estimated_input_tokens: 0,
        },
        disclosure: DataDisclosureWarning {
            raw_content_possible: false,
            surfaces: Vec::new(),
        },
        replay: ReplayContract::diagnostic_only(),
    })?)
}
