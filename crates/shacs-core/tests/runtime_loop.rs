use serde_json::{json, Map, Value};
use shacs_config::AutoApprovalConfig;
use shacs_core::runtime::{
    app_provided_skill_reference_evidence, authored_skill_ready_for_active_registry,
    bridge_underlying_mapping_evidence_ref, build_runtime_memory_evidence,
    build_spec018_projection, build_subagent_tool_registry, consume_evaluator_decision,
    coordinate_automation_run, create_persistent_goal, dispatch_bridge_tool_call,
    evaluator_consumption_idempotency_key, format_partial_progress_from_tool_events,
    freeze_session_search_snapshot, run_local_replay, runtime_curator_proposal_record,
    runtime_improvement_apply_readiness, runtime_improvement_apply_record,
    runtime_improvement_proposal_behavior_inert, runtime_improvement_rollback_projection,
    runtime_improvement_status_after_apply_record, runtime_improvement_verification_record,
    runtime_mcp_exposure_projection, runtime_memory_evidence_request,
    runtime_skill_list_disclosure, runtime_skill_reference_evidence, runtime_skill_view_disclosure,
    runtime_spec018_local_api_projection, ActiveLoopTask, AgentLoop, AgentLoopCommandResult,
    AgentLoopConfig, AgentLoopError, AgentRunSpec, AgentRunner, AutoCompact, AutomationSourceEvent,
    AutomationSourceEventKind, BridgeUnderlyingMappingEvidence, CancellationToken,
    ChildResultEnvelope, ChildResultStatus, ContainerNetworkMode, ContainerRuntimeKind,
    ContainmentSnapshotRef, ContextBuilder, DockerContainmentSnapshot, DreamLifecycle,
    EvaluatorDecisionInput, GoalCompletionVerdict, InboundMessage, LedgerConsumptionStatus,
    LoopTaskRegisterResult, McpLifecycle, MergeDecision, MessageBus, PermissionMode,
    PermissionModeSnapshot, PermissionRuleInput, PermissionedActionOrigin, PersistentGoal,
    PersistentGoalStatus, ProcExecSummary, ProviderHotSwapResult, ProviderSelectionSnapshot,
    RuntimeCapabilityStatus, RuntimeContextTools, RuntimeDecisionKind,
    RuntimeMemoryEvidenceRequestInput, RuntimePolicyGateResults, RuntimeReplayInput,
    RuntimeSelectedAction, RuntimeSpec018ProjectionInput, RuntimeToolCall, RuntimeToolExecutor,
    Session, SessionManager, SessionTurnAcquireError, SessionTurnLock, StaticProviderSelector,
    SubagentExecutionConfig, SubagentMergeState, SubagentProgressUpdate, SubagentRuntime,
    SubagentRuntimeConfig, ToolEvent, ToolExecutionContext, ToolSearchConfig, ToolSearchMode,
    ToolSearchRuntimeInput, ToolStatus, PERSISTENT_GOAL_METADATA_KEY,
};
use shacs_core::tools::{
    assemble_tool_surface, ActivationState, AskUserTool, JsonMap, MessageTool, SchemaFragment,
    SpawnRequest, SpawnTool, Tool, ToolParameters, ToolRegistry, ToolResult,
    ToolSurfaceAssemblyInput,
};
use shacs_eval::evaluator::{
    ApprovalDecisionKind, ApprovalDecisionRef, ApprovalRequestRef, ApprovalRequestStatus,
    AuthoredSkillLifecycleState, AutomationExecutionMode, AutomationRecursionGuard,
    AuxiliaryJudgeRole, AuxiliaryJudgeRoute, AuxiliaryJudgeRouteFinalStatus,
    CheckpointGateDecision, CheckpointGateStatus, ConfidenceBand, CuratorActionProposed,
    CuratorProposalFinalStatus, CuratorTargetKind, DeliverySeverity, EvaluatorKind, EvidenceKind,
    EvidenceRef, ImprovementActorAuthority, ImprovementApplyRecord, ImprovementApproval,
    ImprovementAuthorityAction, ImprovementCheckpoint, ImprovementProposal,
    ImprovementProposalStatus, ImprovementRollbackResult, ImprovementVerificationNextAction,
    JudgeFallbackReason, MemoryEvidenceOmittedReason, OwnerPrimitiveRef, ProjectionStatus,
    ProjectionSurface, ProviderFallbackStep, ProviderModelSnapshot, ProviderRouteRole,
    RedactionStatus, ReplayComparisonSeverity, ReplayComparisonStatus, ReplayDatasetItem,
    ReplayRunStatus, ReplaySafeMockOutcome, ReplayToolOutcomePolicy, SuggestedNextAction,
    TaskOutcomeClass, TrajectoryRecord, TrajectoryStats, VerdictKind,
};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest,
    ToolCallRequest,
};
use shacs_skills::{
    SkillDescriptor, SkillRegistry, SkillRegistryEntry, SkillRegistryStatus, SkillSourceKind,
};
use shacs_utils::gitstore::{GitCliStore, GitStore};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct ProcExecCountingTool {
    calls: Arc<AtomicUsize>,
}

struct NamedProcExecCountingTool {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

struct ApprovalMetadataProbeTool {
    workspace: PathBuf,
    session_key: &'static str,
    calls: Arc<AtomicUsize>,
    observed_status: Arc<Mutex<Option<String>>>,
}

impl Tool for ProcExecCountingTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Count proc exec attempts."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "exec-output".into()
    }
}

impl Tool for NamedProcExecCountingTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Count named proc exec attempts."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "exec-output".into()
    }
}

impl Tool for ApprovalMetadataProbeTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Probe approval metadata while counting proc exec attempts."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let observed = SessionManager::new(&self.workspace)
            .ok()
            .and_then(|manager| manager.read_session_file(self.session_key))
            .and_then(|raw| {
                raw["metadata"]["pending_permission_approval"]["status"]
                    .as_str()
                    .map(str::to_owned)
            });
        if let Ok(mut status) = self.observed_status.lock() {
            *status = observed;
        }
        "exec-output".into()
    }
}

fn runtime_eval_evidence() -> EvidenceRef {
    EvidenceRef {
        kind: EvidenceKind::SessionEvent,
        id: "event-1".to_owned(),
        digest: "event-digest".to_owned(),
        summary: "session event".to_owned(),
        redaction_status: RedactionStatus::AlreadySafe,
        owner_spec: None,
        locator: None,
        retention_hint: None,
    }
}

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

fn replay_evidence(kind: EvidenceKind, id: &str) -> EvidenceRef {
    EvidenceRef {
        kind,
        id: id.to_owned(),
        digest: format!("digest-{id}"),
        summary: "redacted replay evidence".to_owned(),
        redaction_status: RedactionStatus::Redacted,
        owner_spec: Some("018".to_owned()),
        locator: Some(format!("replay://{id}")),
        retention_hint: Some("local".to_owned()),
    }
}

fn confirmed_non_privileged_permission_input() -> PermissionRuleInput {
    PermissionRuleInput {
        containment: DockerContainmentSnapshot {
            contained: Some(true),
            runtime: ContainerRuntimeKind::Docker,
            root_user: Some(false),
            privileged: Some(false),
            host_mounts_summary: Vec::new(),
            network_mode: ContainerNetworkMode::None,
            digest: Some("test-contained".to_owned()),
            summary: Some("non-privileged test containment".to_owned()),
        },
        protected_targets: Vec::new(),
        proc_exec_summary: Some(ProcExecSummary {
            command_family: "test".to_owned(),
            target_refs: Vec::new(),
            destructive: false,
            network: false,
            secret_exposure: false,
            summary_available: true,
        }),
    }
}

fn safe_bypass_agent_loop_config(workspace: impl Into<std::path::PathBuf>) -> AgentLoopConfig {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(true),
        digest: Some("test-contained".to_owned()),
        summary: Some("non-privileged test containment".to_owned()),
    });
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::BypassPermissions,
        source: Some("runtime_loop_test".to_owned()),
        scope_ref: None,
    };
    config.permission_rule_input = confirmed_non_privileged_permission_input();
    config.permission_interactive = false;
    config
}

fn inbound_with_message_id(
    channel: &str,
    sender_id: &str,
    chat_id: &str,
    content: &str,
    message_id: &str,
) -> InboundMessage {
    InboundMessage::new(channel, sender_id, chat_id, content).with_metadata(Map::from_iter([(
        "message_id".to_owned(),
        json!(message_id),
    )]))
}

fn replay_tool_policy(recorded: bool, safe_mock_schema: Option<&str>) -> ReplayToolOutcomePolicy {
    ReplayToolOutcomePolicy {
        tool_call_ref: replay_evidence(EvidenceKind::ToolPayload, "tool-1"),
        expected_schema_digest: "schema-a".to_owned(),
        recorded_outcome_ref: recorded
            .then(|| replay_evidence(EvidenceKind::ReplayResult, "recorded-1")),
        safe_mock_outcome: safe_mock_schema.map(|schema| ReplaySafeMockOutcome {
            mock_reason: "local safe destructive substitute".to_owned(),
            source: "redacted local fixture".to_owned(),
            expected_schema_digest: schema.to_owned(),
            limitations: vec!["does not prove live side effects".to_owned()],
            outcome_ref: replay_evidence(EvidenceKind::ReplayResult, "mock-1"),
        }),
        blocked_reason: None,
    }
}

fn replay_route() -> AuxiliaryJudgeRoute {
    let fallback_step = ProviderFallbackStep {
        provider_id: "primary-judge".to_owned(),
        model_id: "primary-model".to_owned(),
        reason: JudgeFallbackReason::PrimaryUnavailable,
    };

    AuxiliaryJudgeRoute {
        route_id: "route-1".to_owned(),
        judge_role: AuxiliaryJudgeRole::ReplayJudge,
        provider_snapshot: ProviderModelSnapshot {
            snapshot_id: "snapshot-fallback-judge".to_owned(),
            provider_id: "fallback-judge".to_owned(),
            model_id: "fallback-model".to_owned(),
            profile_ref: "profile://local".to_owned(),
            role: ProviderRouteRole::AuxiliaryJudge,
            routing_reason: "primary judge unavailable".to_owned(),
            fallback_chain: vec![fallback_step.clone()],
            evaluator_role: Some(AuxiliaryJudgeRole::ReplayJudge),
        },
        fallback_chain: vec![fallback_step],
        routing_reason: "primary judge unavailable".to_owned(),
        final_status: AuxiliaryJudgeRouteFinalStatus::FallbackSelected,
    }
}

fn replay_item(case_id: &str, policies: Vec<ReplayToolOutcomePolicy>) -> ReplayDatasetItem {
    ReplayDatasetItem {
        dataset_id: "dataset-1".to_owned(),
        case_id: case_id.to_owned(),
        trajectory_refs: vec![replay_evidence(
            EvidenceKind::TrajectoryRecord,
            "trajectory-1",
        )],
        expected_verdict: VerdictKind::Pass,
        expected_outcome: TaskOutcomeClass::Verify,
        expected_projection_status: ProjectionStatus::Success,
        expected_confidence_band: ConfidenceBand::High,
        allowed_judge_roles: vec![AuxiliaryJudgeRole::ReplayJudge],
        redaction_profile: "default".to_owned(),
        tool_outcome_policies: policies,
        actual_verdict: Some(VerdictKind::Pass),
        actual_outcome: Some(TaskOutcomeClass::Verify),
        actual_projection_status: Some(ProjectionStatus::Success),
        actual_confidence_band: Some(ConfidenceBand::High),
        auxiliary_judge_routes: vec![replay_route()],
        diagnostics_refs: vec![replay_evidence(
            EvidenceKind::DiagnosticRecord,
            "diagnostic-1",
        )],
        coverage_refs: vec![replay_evidence(EvidenceKind::ReplayRecord, "coverage-1")],
    }
}

fn replay_input<'a>(
    dataset: &'a [ReplayDatasetItem],
    selected: &'a [String],
) -> RuntimeReplayInput<'a> {
    RuntimeReplayInput {
        run_id: "run-1".to_owned(),
        dataset_id: "dataset-1".to_owned(),
        dataset,
        selected_case_ids: selected,
        started_at_ms: 10,
        completed_at_ms: 20,
        diagnostics_ref: replay_evidence(EvidenceKind::DiagnosticRecord, "run-diagnostic"),
    }
}

fn runtime_eval_goal() -> PersistentGoal {
    create_persistent_goal("session-1", "ship it", "created", 2)
}

#[test]
fn replay_runner_executes_selected_cases_only_and_never_dispatches_live_tools() {
    let dataset = vec![
        replay_item("case-1", vec![replay_tool_policy(true, None)]),
        replay_item("case-2", vec![replay_tool_policy(false, None)]),
    ];
    let selected = vec!["case-1".to_owned()];

    let outcome = run_local_replay(replay_input(&dataset, &selected));

    assert_eq!(outcome.run_record.case_results.len(), 1);
    assert_eq!(outcome.run_record.case_results[0].case_id, "case-1");
    assert_eq!(outcome.live_tool_dispatch_count, 0);
    assert_eq!(outcome.replayed_tool_policy_count, 1);
    assert_eq!(outcome.run_record.status, ReplayRunStatus::Passed);
}

#[test]
fn replay_runner_replays_recorded_outcome_and_records_auxiliary_fallback_route() {
    let dataset = vec![replay_item("case-1", vec![replay_tool_policy(true, None)])];
    let selected = vec!["case-1".to_owned()];

    let outcome = run_local_replay(replay_input(&dataset, &selected));
    let route = &outcome.auxiliary_judge_routes[0];

    assert_eq!(
        outcome.run_record.case_results[0].comparison_status,
        ReplayComparisonStatus::Match
    );
    assert_eq!(route.provider_snapshot.provider_id, "fallback-judge");
    assert_eq!(
        route.fallback_chain[0].reason,
        JudgeFallbackReason::PrimaryUnavailable
    );
    assert_eq!(
        route.final_status,
        AuxiliaryJudgeRouteFinalStatus::FallbackSelected
    );
}

#[test]
fn replay_runner_blocks_safe_mock_schema_mismatch_and_release_gate_pass() {
    let dataset = vec![replay_item(
        "case-1",
        vec![replay_tool_policy(false, Some("schema-b"))],
    )];
    let selected = vec!["case-1".to_owned()];

    let outcome = run_local_replay(replay_input(&dataset, &selected));
    let case = &outcome.run_record.case_results[0];

    assert_eq!(
        case.comparison_status,
        ReplayComparisonStatus::SchemaMismatch
    );
    assert_eq!(case.severity, ReplayComparisonSeverity::Blocked);
    assert_eq!(outcome.run_record.status, ReplayRunStatus::Blocked);
}

#[test]
fn replay_runner_blocks_empty_and_unknown_selected_cases() {
    let dataset = vec![replay_item("case-1", vec![replay_tool_policy(true, None)])];
    let empty = Vec::new();
    let unknown = vec!["missing-case".to_owned()];

    let empty_outcome = run_local_replay(replay_input(&dataset, &empty));
    let unknown_outcome = run_local_replay(replay_input(&dataset, &unknown));

    assert_eq!(empty_outcome.run_record.status, ReplayRunStatus::Blocked);
    assert_eq!(unknown_outcome.run_record.status, ReplayRunStatus::Blocked);
    assert_eq!(
        empty_outcome.run_record.case_results[0].case_id,
        "__selection__"
    );
    assert_eq!(
        unknown_outcome.run_record.case_results[0].case_id,
        "missing-case"
    );
    assert_eq!(
        unknown_outcome.run_record.case_results[0]
            .blocked_reason
            .as_deref(),
        Some("blocked_unknown_selected_replay_case")
    );
}

#[test]
fn replay_runner_blocks_disallowed_auxiliary_judge_role() {
    let mut item = replay_item("case-1", vec![replay_tool_policy(true, None)]);
    item.allowed_judge_roles = vec![AuxiliaryJudgeRole::GoalCompletion];
    let dataset = vec![item];
    let selected = vec!["case-1".to_owned()];

    let outcome = run_local_replay(replay_input(&dataset, &selected));

    assert_eq!(outcome.run_record.status, ReplayRunStatus::Blocked);
    assert_eq!(
        outcome.run_record.case_results[0].blocked_reason.as_deref(),
        Some("blocked_disallowed_judge_role")
    );
}

#[test]
fn replay_runner_separates_verdict_and_confidence_mismatch_severity() {
    let mut verdict_case = replay_item("case-1", vec![replay_tool_policy(true, None)]);
    verdict_case.actual_verdict = Some(VerdictKind::Fail);
    let mut confidence_case = replay_item("case-2", vec![replay_tool_policy(true, None)]);
    confidence_case.actual_confidence_band = Some(ConfidenceBand::Medium);
    let dataset = vec![verdict_case, confidence_case];
    let selected = vec!["case-1".to_owned(), "case-2".to_owned()];

    let outcome = run_local_replay(replay_input(&dataset, &selected));

    assert_eq!(
        outcome.run_record.case_results[0].comparison_status,
        ReplayComparisonStatus::VerdictKindMismatch
    );
    assert_eq!(
        outcome.run_record.case_results[0].severity,
        ReplayComparisonSeverity::High
    );
    assert_eq!(
        outcome.run_record.case_results[1].comparison_status,
        ReplayComparisonStatus::ConfidenceBandMismatch
    );
    assert_eq!(
        outcome.run_record.case_results[1].severity,
        ReplayComparisonSeverity::Low
    );
}

#[test]
fn replay_runner_does_not_mutate_session_or_config_values_passed_around() {
    let session = Session::new("session-1");
    let config = AgentLoopConfig::new("/tmp/shacs", "test-model");
    let original_session = session.clone();
    let original_config = config.clone();
    let dataset = vec![replay_item("case-1", vec![replay_tool_policy(true, None)])];
    let selected = vec!["case-1".to_owned()];

    let outcome = run_local_replay(replay_input(&dataset, &selected));

    assert_eq!(session, original_session);
    assert_eq!(config.workspace, original_config.workspace);
    assert_eq!(config.model, original_config.model);
    assert_eq!(config.max_iterations, original_config.max_iterations);
    assert_eq!(outcome.run_record.status, ReplayRunStatus::Passed);
}

fn runtime_eval_gates(now_ms: u64) -> RuntimePolicyGateResults {
    RuntimePolicyGateResults {
        now_ms,
        ..RuntimePolicyGateResults::all_passed()
    }
}

fn runtime_eval_input(
    evaluator_kind: EvaluatorKind,
    goal: Option<&PersistentGoal>,
    verdict: Option<GoalCompletionVerdict>,
) -> EvaluatorDecisionInput {
    EvaluatorDecisionInput {
        verdict_id: "verdict-1".to_owned(),
        evaluator_kind,
        evaluator_version: "eval-v1".to_owned(),
        source_ledger_ref: "evaluation-ledger:verdict-1".to_owned(),
        frozen_snapshot_digest: "snapshot-digest".to_owned(),
        current_target_snapshot_digest: "snapshot-digest".to_owned(),
        goal_id: goal.map(|goal| goal.id.clone()),
        turn_id: Some("turn-1".to_owned()),
        expires_at_ms: None,
        suggested_action: SuggestedNextAction::None,
        confidence: 0.9,
        evidence_refs: vec![runtime_eval_evidence()],
        redaction_status: RedactionStatus::AlreadySafe,
        explicit_goal_completion_verdict: verdict,
        blocked_reason: None,
        unblock_hint: None,
        created_at_ms: 100,
        correlation_id: "corr-1".to_owned(),
        superseding_verdict_ref: None,
        task_outcome_class: None,
    }
}

fn automation_source_event(source: AutomationSourceEventKind) -> AutomationSourceEvent {
    AutomationSourceEvent {
        runtime_service_event_id: "runtime-event-1".to_owned(),
        source_owner: "runtime-service".to_owned(),
        received_at_ms: 100,
        job_id: "job-1".to_owned(),
        session_id: Some("session-1".to_owned()),
        goal_id: Some("goal-1".to_owned()),
        active_goal: true,
        pending_automation: false,
        execution_mode: AutomationExecutionMode::SkillBackedAgent,
        timeout_policy_ref: "timeout-policy-1".to_owned(),
        retry_policy_ref: "retry-policy-1".to_owned(),
        delivery_policy_ref: "delivery-policy-1".to_owned(),
        recursion_guard: AutomationRecursionGuard {
            token: "guard-1".to_owned(),
            source_run_id: None,
            depth: 0,
            max_depth: 3,
            parent_refs: Vec::new(),
            blocked_reason: None,
        },
        prd008_goal_gate_ref: Some("goal-gate-1".to_owned()),
        source,
    }
}

#[test]
fn automation_heartbeat_without_goal_or_pending_work_is_suppressed() {
    let mut event = automation_source_event(AutomationSourceEventKind::Heartbeat);
    event.active_goal = false;
    event.pending_automation = false;
    event.goal_id = None;

    let outcome = coordinate_automation_run(&event, &[]);

    assert!(outcome.request.is_none());
    assert!(outcome.run_state_record.is_none());
    assert!(outcome.delivery_record.is_none());
    assert_eq!(
        outcome.suppress_reason.as_deref(),
        Some("heartbeat has no active goal or pending automation")
    );
    assert!(!outcome.task_outcome_eligibility.evaluator_should_run);
}

#[test]
fn automation_duplicate_cron_wake_is_idempotently_suppressed() {
    let event = automation_source_event(AutomationSourceEventKind::Cron {
        approved_automation_rule_ref: Some("rule-1".to_owned()),
    });
    let first = coordinate_automation_run(&event, &[]);
    let existing = vec![first.run_state_record.clone().expect("first run state")];
    let second = coordinate_automation_run(&event, &existing);

    assert!(first.request.is_some());
    assert!(second.request.is_none());
    assert_eq!(
        second.suppress_reason.as_deref(),
        Some("duplicate automation wake idempotency key")
    );
}

#[test]
fn automation_app_task_result_carries_required_evidence_without_app_apply_authority() {
    let event = automation_source_event(AutomationSourceEventKind::AppTaskResult {
        app_task_id: Some("app-task-1".to_owned()),
        manifest_ref: Some("manifest-1".to_owned()),
        capability_scope: Some("capability:write".to_owned()),
        evidence_ref: "evidence-1".to_owned(),
        self_improvement_apply_requested: true,
    });

    let outcome = coordinate_automation_run(&event, &[]);

    assert!(outcome.request.is_some());
    assert!(outcome
        .task_outcome_eligibility
        .evidence_refs
        .contains(&"app-task-1".to_owned()));
    assert!(outcome
        .task_outcome_eligibility
        .evidence_refs
        .contains(&"manifest-1".to_owned()));
    assert!(outcome
        .task_outcome_eligibility
        .evidence_refs
        .contains(&"capability:write".to_owned()));
    assert!(
        !outcome
            .task_outcome_eligibility
            .app_authority_can_apply_self_improvement
    );
}

#[test]
fn automation_recursion_guard_suppresses_self_triggered_loop() {
    let event = automation_source_event(AutomationSourceEventKind::ManualResume {
        resume_ref: "resume-1".to_owned(),
    });
    let first = coordinate_automation_run(&event, &[]);
    let mut loop_event = event.clone();
    loop_event.recursion_guard.source_run_id =
        first.request.as_ref().map(|request| request.run_id.clone());

    let outcome = coordinate_automation_run(&loop_event, &[]);

    assert!(outcome.request.is_none());
    assert_eq!(
        outcome.suppress_reason.as_deref(),
        Some("self-triggered automation loop")
    );
    assert_eq!(
        outcome
            .run_state_record
            .as_ref()
            .and_then(|record| record.suppress_reason.as_deref()),
        Some("self-triggered automation loop")
    );
}

#[test]
fn automation_local_api_background_keeps_refs_without_raw_payload() {
    let event = automation_source_event(AutomationSourceEventKind::LocalApiBackground {
        caller_auth_ref: Some("auth-ref-1".to_owned()),
        redaction_profile_ref: Some("redaction-profile-1".to_owned()),
        redacted_evidence_ref: "redacted-evidence-1".to_owned(),
    });

    let outcome = coordinate_automation_run(&event, &[]);
    let serialized = serde_json::to_string(&event).expect("event should serialize");

    assert!(outcome.request.is_some());
    assert!(outcome
        .task_outcome_eligibility
        .evidence_refs
        .contains(&"auth-ref-1".to_owned()));
    assert!(outcome
        .task_outcome_eligibility
        .evidence_refs
        .contains(&"redaction-profile-1".to_owned()));
    assert!(!serialized.contains("raw_payload"));
}

#[test]
fn automation_channel_event_projects_delivery_only_when_user_visible() {
    let visible = automation_source_event(AutomationSourceEventKind::ChannelEvent {
        channel_event_ref: "channel-event-1".to_owned(),
        user_visible: true,
        redacted_message: "deployment finished".to_owned(),
        target_surface: ProjectionSurface::Channel,
        severity: DeliverySeverity::Info,
    });
    let hidden = automation_source_event(AutomationSourceEventKind::ChannelEvent {
        channel_event_ref: "channel-event-2".to_owned(),
        user_visible: false,
        redacted_message: "internal typing signal".to_owned(),
        target_surface: ProjectionSurface::Channel,
        severity: DeliverySeverity::Info,
    });

    let visible_outcome = coordinate_automation_run(&visible, &[]);
    let hidden_outcome = coordinate_automation_run(&hidden, &[]);

    assert!(visible_outcome.delivery_record.is_some());
    assert!(hidden_outcome.delivery_record.is_none());
    assert!(hidden_outcome.request.is_none());
}

#[test]
fn runtime_projection_reexports_remain_available_from_core_runtime() {
    let projection = build_spec018_projection(RuntimeSpec018ProjectionInput {
        generated_at_ms: 7,
        session_id: "session-reexport",
        goal_summaries: &[],
        automation_summaries: &[],
        approval_summaries: &[],
        blocked_summaries: &[],
        verification_summaries: &[],
        replay_summaries: &[],
        recent_evaluator_decision_summaries: &[],
    });
    let local_projection = runtime_spec018_local_api_projection(&projection);

    assert_eq!(local_projection.session_id, "session-reexport");
    assert_eq!(local_projection.generated_at_ms, 7);
}

#[test]
fn bridge_underlying_mapping_evidence_ref_is_safe_for_trajectory_tool_refs(
) -> Result<(), Box<dyn Error>> {
    let evidence_ref = bridge_underlying_mapping_evidence_ref(&BridgeUnderlyingMappingEvidence {
        bridge_call_id: "bridge-call-token=RAW_BRIDGE_SECRET".to_owned(),
        bridge_name: "tool_call".to_owned(),
        underlying_name: "mcp_parent_only".to_owned(),
        scope_digest: "scope-RAW_SCHEMA_SHOULD_NOT_LEAK".to_owned(),
    });
    if evidence_ref.kind != EvidenceKind::ToolPayload
        || evidence_ref.owner_spec.as_deref() != Some("020")
        || evidence_ref.redaction_status != RedactionStatus::Redacted
    {
        return Err(format!("mapping evidence ref contract drifted: {evidence_ref:?}").into());
    }

    let trajectory = TrajectoryRecord {
        trajectory_id: "trajectory-bridge-mapping".to_owned(),
        session_ref: spec018_evidence_ref(
            EvidenceKind::SessionEvent,
            "trajectory-session",
            RedactionStatus::Redacted,
        ),
        event_refs: Vec::new(),
        model_call_refs: Vec::new(),
        tool_refs: vec![evidence_ref],
        evaluator_refs: Vec::new(),
        provider_snapshot_refs: Vec::new(),
        redaction_profile: "prd005-redacted".to_owned(),
        stats: TrajectoryStats {
            started_at_ms: 1,
            completed_at_ms: Some(2),
            model_call_count: 1,
            tool_call_count: 1,
            input_tokens: None,
            output_tokens: None,
        },
        correlation_id: "corr-bridge-mapping".to_owned(),
    };
    let serialized = serde_json::to_string(&trajectory)?;
    for forbidden in [
        "RAW_BRIDGE_SECRET",
        "RAW_SCHEMA_SHOULD_NOT_LEAK",
        "bridge-call-token",
        "scope-RAW_SCHEMA",
        "arguments",
        "schema",
    ] {
        if serialized.contains(forbidden) {
            return Err(format!("trajectory tool_ref leaked {forbidden}: {serialized}").into());
        }
    }
    Ok(())
}

#[test]
fn automation_continue_eligibility_returns_prd008_gate_metadata_without_execution() {
    let event = automation_source_event(AutomationSourceEventKind::SubagentResult {
        merge_state: SubagentMergeState::Reviewable,
        result_ref: "subagent-result-1".to_owned(),
    });

    let outcome = coordinate_automation_run(&event, &[]);

    assert!(outcome.request.is_some());
    assert!(outcome.task_outcome_eligibility.evaluator_should_run);
    assert!(
        outcome
            .task_outcome_eligibility
            .continue_requires_prd008_goal_gate
    );
    assert!(!outcome.task_outcome_eligibility.direct_execution_allowed);
    assert_eq!(
        outcome.prd008_linkage.goal_gate_ref.as_deref(),
        Some("goal-gate-1")
    );
    assert!(outcome.prd008_linkage.can_build_evaluator_decision_input);
}

#[test]
fn evaluator_consumption_is_idempotent_for_terminal_verdict() {
    let goal = runtime_eval_goal();
    let input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Done),
    );
    let gates = runtime_eval_gates(100);

    let (first_decision, first_record) =
        consume_evaluator_decision(&input, Some(&goal), &[], &gates);
    let existing = vec![first_record.clone()];
    let (second_decision, second_record) =
        consume_evaluator_decision(&input, Some(&goal), &existing, &gates);

    assert_eq!(first_record.status, LedgerConsumptionStatus::Consumed);
    assert_eq!(first_record, second_record);
    assert_eq!(
        first_record.idempotency_key,
        evaluator_consumption_idempotency_key(&input)
    );
    assert_eq!(
        first_decision.selected_action,
        RuntimeSelectedAction::CompleteGoal
    );
    assert_eq!(second_decision.selected_action, RuntimeSelectedAction::None);
    assert!(second_decision.next_goal_state.is_none());
}

#[test]
fn evaluator_consumption_discards_stale_snapshot_without_goal_effect() {
    let goal = runtime_eval_goal();
    let mut input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Continue),
    );
    input.current_target_snapshot_digest = "new-snapshot-digest".to_owned();

    let (decision, record) =
        consume_evaluator_decision(&input, Some(&goal), &[], &runtime_eval_gates(100));

    assert_eq!(record.status, LedgerConsumptionStatus::DiscardedStale);
    assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
    assert!(decision.next_goal_state.is_none());
    let stale = decision.stale_verdict.expect("stale evidence");
    assert_eq!(stale.expected_digest, "snapshot-digest");
    assert_eq!(stale.current_digest, "new-snapshot-digest");
}

#[test]
fn evaluator_consumption_discards_expired_verdict_without_continuation() {
    let goal = runtime_eval_goal();
    let mut input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Continue),
    );
    input.expires_at_ms = Some(99);

    let (decision, record) =
        consume_evaluator_decision(&input, Some(&goal), &[], &runtime_eval_gates(100));

    assert_eq!(record.status, LedgerConsumptionStatus::DiscardedExpired);
    assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
    assert!(decision.continuation.is_none());
    assert!(decision.next_goal_state.is_none());
}

#[test]
fn evaluator_continue_does_not_reactivate_paused_or_cleared_goal() {
    for status in [PersistentGoalStatus::Paused, PersistentGoalStatus::Cleared] {
        let mut goal = runtime_eval_goal();
        goal.status = status;
        let input = runtime_eval_input(
            EvaluatorKind::GoalCompletion,
            Some(&goal),
            Some(GoalCompletionVerdict::Continue),
        );

        let (decision, record) =
            consume_evaluator_decision(&input, Some(&goal), &[], &runtime_eval_gates(100));

        assert_eq!(record.status, LedgerConsumptionStatus::BlockedByPolicy);
        assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
        assert!(decision.next_goal_state.is_none());
    }
}

#[test]
fn evaluator_continue_requires_all_runtime_gates() {
    let goal = runtime_eval_goal();
    let input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Continue),
    );
    let gates = runtime_eval_gates(100);

    let (decision, record) = consume_evaluator_decision(&input, Some(&goal), &[], &gates);
    assert_eq!(record.status, LedgerConsumptionStatus::Consumed);
    assert_eq!(
        decision.selected_action,
        RuntimeSelectedAction::ContinueGoal
    );
    assert_eq!(
        decision
            .next_goal_state
            .as_ref()
            .map(|goal| goal.turns_used),
        Some(1)
    );

    let mut exhausted_goal = goal.clone();
    exhausted_goal.turns_used = exhausted_goal.turn_budget;
    let blocked_cases: Vec<(Option<PersistentGoal>, RuntimePolicyGateResults)> = vec![
        (None, runtime_eval_gates(100)),
        (Some(exhausted_goal), runtime_eval_gates(100)),
        (
            Some(goal.clone()),
            RuntimePolicyGateResults {
                user_interrupted: true,
                ..runtime_eval_gates(100)
            },
        ),
        (
            Some(goal.clone()),
            RuntimePolicyGateResults {
                permission_gate_passed: false,
                ..runtime_eval_gates(100)
            },
        ),
        (
            Some(goal.clone()),
            RuntimePolicyGateResults {
                recursion_guard_passed: false,
                ..runtime_eval_gates(100)
            },
        ),
        (
            Some(goal.clone()),
            RuntimePolicyGateResults {
                runtime_cancelled: true,
                ..runtime_eval_gates(100)
            },
        ),
    ];

    for (blocked_goal, blocked_gates) in blocked_cases {
        let (decision, record) =
            consume_evaluator_decision(&input, blocked_goal.as_ref(), &[], &blocked_gates);
        assert_eq!(record.status, LedgerConsumptionStatus::BlockedByPolicy);
        assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
        assert!(decision.next_goal_state.is_none());
    }
}

#[test]
fn evaluator_done_returns_next_goal_state_without_mutating_session_truth() {
    let goal = runtime_eval_goal();
    let input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Done),
    );

    let (decision, record) =
        consume_evaluator_decision(&input, Some(&goal), &[], &runtime_eval_gates(100));

    assert_eq!(record.status, LedgerConsumptionStatus::Consumed);
    assert_eq!(goal.status, PersistentGoalStatus::Active);
    assert_eq!(
        decision.selected_action,
        RuntimeSelectedAction::CompleteGoal
    );
    assert_eq!(
        decision.next_goal_state.as_ref().map(|goal| goal.status),
        Some(PersistentGoalStatus::Done)
    );
}

#[test]
fn evaluator_blocked_projects_reason_and_unblock_hint() {
    let goal = runtime_eval_goal();
    let mut input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Blocked),
    );
    input.blocked_reason = Some("needs user choice".to_owned());
    input.unblock_hint = Some("answer the pending question".to_owned());

    let (decision, record) =
        consume_evaluator_decision(&input, Some(&goal), &[], &runtime_eval_gates(100));

    assert_eq!(record.status, LedgerConsumptionStatus::Consumed);
    assert_eq!(decision.selected_action, RuntimeSelectedAction::BlockGoal);
    assert_eq!(
        decision.blocked_reason.as_deref(),
        Some("needs user choice")
    );
    assert_eq!(
        decision.unblock_hint.as_deref(),
        Some("answer the pending question")
    );
    assert_eq!(
        decision.next_goal_state.as_ref().map(|goal| goal.status),
        Some(PersistentGoalStatus::Blocked)
    );
}

#[test]
fn evaluator_capability_requires_approval_and_permission_gates() {
    let mut input = runtime_eval_input(EvaluatorKind::SafetyCapability, None, None);
    input.goal_id = None;

    for gates in [
        RuntimePolicyGateResults {
            approval_gate_passed: false,
            ..runtime_eval_gates(100)
        },
        RuntimePolicyGateResults {
            permission_gate_passed: false,
            ..runtime_eval_gates(100)
        },
    ] {
        let (decision, record) = consume_evaluator_decision(&input, None, &[], &gates);
        assert_eq!(record.status, LedgerConsumptionStatus::BlockedByPolicy);
        assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
    }

    let (decision, record) =
        consume_evaluator_decision(&input, None, &[], &runtime_eval_gates(100));
    assert_eq!(record.status, LedgerConsumptionStatus::Consumed);
    assert_eq!(decision.decision_kind, RuntimeDecisionKind::Capability);
    assert_eq!(
        decision.selected_action,
        RuntimeSelectedAction::ApplyCapability
    );
}

#[test]
fn evaluator_consumption_fails_without_any_evidence_link() {
    let goal = runtime_eval_goal();
    let mut input = runtime_eval_input(
        EvaluatorKind::GoalCompletion,
        Some(&goal),
        Some(GoalCompletionVerdict::Done),
    );
    input.source_ledger_ref.clear();
    input.evidence_refs.clear();

    let (decision, record) =
        consume_evaluator_decision(&input, Some(&goal), &[], &runtime_eval_gates(100));

    assert_eq!(record.status, LedgerConsumptionStatus::FailedToApply);
    assert_eq!(decision.decision_kind, RuntimeDecisionKind::FailedToApply);
    assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
    assert!(decision.next_goal_state.is_none());
}

#[test]
fn evaluator_task_outcome_verify_and_rollback_require_owner_primitives() {
    for task_class in [TaskOutcomeClass::Verify, TaskOutcomeClass::Rollback] {
        let mut input = runtime_eval_input(EvaluatorKind::TaskOutcome, None, None);
        input.task_outcome_class = Some(task_class.clone());
        let blocked_gates = RuntimePolicyGateResults {
            owner_primitive_ready: false,
            ..runtime_eval_gates(100)
        };

        let (decision, record) = consume_evaluator_decision(&input, None, &[], &blocked_gates);
        assert_eq!(record.status, LedgerConsumptionStatus::BlockedByPolicy);
        assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
    }
}

#[test]
fn loop_process_direct_saves_turn_and_publishes_outbound() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("hello back".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("hello", Some("cli:thread-1"))?;
    if result.session_key != "cli:thread-1"
        || result.final_content.as_deref() != Some("hello back")
        || result.outbound_count != 1
    {
        return Err(format!("unexpected loop result: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing outbound")?;
    if outbound.content != "hello back" || outbound.metadata["session_key"] != "cli:thread-1" {
        return Err(format!("unexpected outbound: {outbound:?}").into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:thread-1")
        .ok_or("missing session")?;
    if raw["messages"].as_array().map(Vec::len) != Some(2)
        || raw["messages"][0]["role"] != "user"
        || raw["messages"][0]["content"] != "hello"
        || raw["messages"][1]["role"] != "assistant"
        || raw["messages"][1]["content"] != "hello back"
    {
        return Err(format!("session messages drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_history_command_returns_recent_visible_messages_without_provider_call(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut sessions = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:history");
    session.add_message("user", "alpha", Map::new());
    session.add_message("assistant", "beta", Map::new());
    session.add_message("user", "gamma", Map::new());
    sessions.save_with_fsync(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        sessions,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/history 2", Some("cli:history"))?;

    assert_eq!(result.command, Some(AgentLoopCommandResult::History));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        0
    );
    let outbound = bus.consume_outbound().ok_or("missing history outbound")?;
    assert!(outbound.content.contains("assistant: beta"));
    assert!(outbound.content.contains("user: gamma"));
    assert!(!outbound.content.contains("alpha"));
    Ok(())
}

#[test]
fn loop_invalid_history_and_help_publish_without_provider_call() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let invalid = loop_runtime.process_direct("/history abc", Some("cli:commands"))?;
    assert_eq!(invalid.command, Some(AgentLoopCommandResult::History));
    let invalid_outbound = bus
        .consume_outbound()
        .ok_or("missing invalid history outbound")?;
    assert!(invalid_outbound.content.contains("Usage: /history [n]"));

    let help = loop_runtime.process_direct("/help", Some("cli:commands"))?;
    assert_eq!(help.command, Some(AgentLoopCommandResult::Help));
    let help_outbound = bus.consume_outbound().ok_or("missing help outbound")?;
    assert!(help_outbound.content.contains("/dream-restore"));
    assert!(client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());
    Ok(())
}

#[test]
fn loop_permission_wizard_saves_default_and_auto_without_provider_call(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let saved_modes = Arc::new(Mutex::new(Vec::new()));
    let callback_modes = saved_modes.clone();
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_setter = Some(Arc::new(move |mode| {
        callback_modes
            .lock()
            .map_err(|error| error.to_string())?
            .push(mode);
        Ok(PermissionModeSnapshot {
            mode,
            source: Some("runtime_loop_test".to_owned()),
            scope_ref: None,
        })
    }));
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let start = loop_runtime.process_direct("/permission", Some("cli:permission"))?;
    assert_eq!(start.command, Some(AgentLoopCommandResult::Permission));
    let start_outbound = bus
        .consume_outbound()
        .ok_or("missing permission outbound")?;
    assert!(start_outbound.content.contains("default"));
    assert!(start_outbound.content.contains("auto"));
    assert!(start_outbound.content.contains("bypass_permissions"));
    let saved_default = loop_runtime.process_direct("default", Some("cli:permission"))?;
    assert_eq!(saved_default.stop_reason, "permission_saved");
    let _ = bus
        .consume_outbound()
        .ok_or("missing saved default outbound")?;

    loop_runtime.process_direct("/permission", Some("cli:permission"))?;
    let _ = bus
        .consume_outbound()
        .ok_or("missing second permission outbound")?;
    let saved_auto = loop_runtime.process_direct("auto", Some("cli:permission"))?;
    assert_eq!(saved_auto.stop_reason, "permission_saved");
    let auto_outbound = bus
        .consume_outbound()
        .ok_or("missing saved auto outbound")?;
    assert!(auto_outbound.content.contains("saved and applied"));
    assert!(!auto_outbound.content.contains("Restart"));

    let modes = saved_modes
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    assert_eq!(modes, vec![PermissionMode::Default, PermissionMode::Auto]);
    assert!(client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:permission")
        .ok_or("missing permission session")?;
    assert!(raw["metadata"].get("pending_permission_wizard").is_none());
    Ok(())
}

#[test]
fn loop_permission_wizard_requires_bypass_confirmation_and_supports_cancel(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let saved_modes = Arc::new(Mutex::new(Vec::new()));
    let callback_modes = saved_modes.clone();
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_setter = Some(Arc::new(move |mode| {
        callback_modes
            .lock()
            .map_err(|error| error.to_string())?
            .push(mode);
        Ok(PermissionModeSnapshot {
            mode,
            source: Some("runtime_loop_test".to_owned()),
            scope_ref: None,
        })
    }));
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_direct("/permission", Some("cli:permission-bypass"))?;
    let _ = bus
        .consume_outbound()
        .ok_or("missing permission outbound")?;
    let bypass_prompt =
        loop_runtime.process_direct("bypass_permissions", Some("cli:permission-bypass"))?;
    assert_eq!(bypass_prompt.stop_reason, "permission_confirm_bypass");
    let _ = bus
        .consume_outbound()
        .ok_or("missing bypass prompt outbound")?;
    assert!(saved_modes
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());

    let repeated =
        loop_runtime.process_direct("bypass_permissions", Some("cli:permission-bypass"))?;
    assert_eq!(repeated.stop_reason, "permission_confirm_bypass");
    let _ = bus
        .consume_outbound()
        .ok_or("missing repeated bypass outbound")?;
    assert!(saved_modes
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());

    let saved =
        loop_runtime.process_direct("confirm bypass_permissions", Some("cli:permission-bypass"))?;
    assert_eq!(saved.stop_reason, "permission_saved");
    let _ = bus
        .consume_outbound()
        .ok_or("missing bypass saved outbound")?;
    assert_eq!(
        saved_modes
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
        vec![PermissionMode::BypassPermissions]
    );

    loop_runtime.process_direct("/permission", Some("cli:permission-bypass"))?;
    let _ = bus
        .consume_outbound()
        .ok_or("missing cancel start outbound")?;
    let cancelled = loop_runtime.process_direct("cancel", Some("cli:permission-bypass"))?;
    assert_eq!(cancelled.stop_reason, "permission_cancelled");
    let _ = bus.consume_outbound().ok_or("missing cancel outbound")?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:permission-bypass")
        .ok_or("missing permission session")?;
    assert!(raw["metadata"].get("pending_permission_wizard").is_none());
    Ok(())
}

#[test]
fn loop_permission_wizard_rejects_different_sender() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let saved_modes = Arc::new(Mutex::new(Vec::new()));
    let callback_modes = saved_modes.clone();
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_setter = Some(Arc::new(move |mode| {
        callback_modes
            .lock()
            .map_err(|error| error.to_string())?
            .push(mode);
        Ok(PermissionModeSnapshot {
            mode,
            source: Some("runtime_loop_test".to_owned()),
            scope_ref: None,
        })
    }));
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let mut original = InboundMessage::new("telegram", "alice", "chat-1", "/permission");
    original.session_key_override = Some("telegram:shared".to_owned());
    loop_runtime.process_message(original)?;
    let _ = bus
        .consume_outbound()
        .ok_or("missing permission outbound")?;

    let mut intruder = InboundMessage::new("telegram", "bob", "chat-1", "auto");
    intruder.session_key_override = Some("telegram:shared".to_owned());
    let rejected = loop_runtime.process_message(intruder)?;
    assert_eq!(rejected.stop_reason, "permission_pending");
    let rejected_outbound = bus.consume_outbound().ok_or("missing rejected outbound")?;
    assert!(rejected_outbound.content.contains("original requester"));
    assert!(saved_modes
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());

    let mut owner = InboundMessage::new("telegram", "alice", "chat-1", "auto");
    owner.session_key_override = Some("telegram:shared".to_owned());
    let saved = loop_runtime.process_message(owner)?;
    assert_eq!(saved.stop_reason, "permission_saved");
    let _ = bus
        .consume_outbound()
        .ok_or("missing owner saved outbound")?;
    assert_eq!(
        saved_modes
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
        vec![PermissionMode::Auto]
    );
    Ok(())
}

#[test]
fn loop_goal_lifecycle_persists_metadata_without_provider_call() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let set = loop_runtime.process_direct("/goal ship PRD 001", Some("cli:goal"))?;
    assert_eq!(set.command, Some(AgentLoopCommandResult::Goal));
    assert!(bus
        .consume_outbound()
        .ok_or("missing set outbound")?
        .content
        .contains("Goal set: ship PRD 001"));

    let conflict = loop_runtime.process_direct("/goal replace it", Some("cli:goal"))?;
    assert_eq!(conflict.command, Some(AgentLoopCommandResult::Goal));
    assert!(bus
        .consume_outbound()
        .ok_or("missing conflict outbound")?
        .content
        .contains("already active"));

    for (command, expected_status, expected_content) in [
        ("/goal pause", PersistentGoalStatus::Paused, "Goal paused."),
        (
            "/goal resume",
            PersistentGoalStatus::Active,
            "Goal resumed.",
        ),
    ] {
        let result = loop_runtime.process_direct(command, Some("cli:goal"))?;
        assert_eq!(result.command, Some(AgentLoopCommandResult::Goal));
        assert!(bus
            .consume_outbound()
            .ok_or("missing lifecycle outbound")?
            .content
            .contains(expected_content));
        let raw = loop_runtime
            .session_manager()
            .read_session_file("cli:goal")
            .ok_or("missing goal session")?;
        assert_eq!(
            raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
            serde_json::to_value(expected_status)?
        );
    }

    let status = loop_runtime.process_direct("/goal status", Some("cli:goal"))?;
    assert_eq!(status.command, Some(AgentLoopCommandResult::Goal));
    let status_outbound = bus.consume_outbound().ok_or("missing status outbound")?;
    assert!(status_outbound.content.contains("Goal: ship PRD 001"));
    assert!(status_outbound.content.contains("Budget: 0/8 turns used"));

    let blocked = loop_runtime.process_direct("/goal blocked waiting", Some("cli:goal"))?;
    assert_eq!(blocked.command, Some(AgentLoopCommandResult::Goal));
    assert!(bus
        .consume_outbound()
        .ok_or("missing blocked outbound")?
        .content
        .contains("Goal marked blocked."));
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal")
        .ok_or("missing blocked session")?;
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
        serde_json::to_value(PersistentGoalStatus::Blocked)?
    );
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["blocked_reason"],
        "waiting"
    );

    loop_runtime.process_direct("/goal resume", Some("cli:goal"))?;
    let _ = bus.consume_outbound().ok_or("missing resume outbound")?;
    loop_runtime.process_direct("/goal done", Some("cli:goal"))?;
    assert!(bus
        .consume_outbound()
        .ok_or("missing done outbound")?
        .content
        .contains("Goal marked done."));
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal")
        .ok_or("missing done session")?;
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
        serde_json::to_value(PersistentGoalStatus::Done)?
    );

    loop_runtime.process_direct("/goal next goal", Some("cli:goal"))?;
    assert!(bus
        .consume_outbound()
        .ok_or("missing replacement outbound")?
        .content
        .contains("Goal set: next goal"));
    loop_runtime.process_direct("/goal clear", Some("cli:goal"))?;
    assert!(bus
        .consume_outbound()
        .ok_or("missing clear outbound")?
        .content
        .contains("Goal cleared."));
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal")
        .ok_or("missing cleared goal session")?;
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
        serde_json::to_value(PersistentGoalStatus::Cleared)?
    );
    assert!(client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());
    Ok(())
}

#[test]
fn loop_new_clears_persistent_goal_metadata() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/goal ship PRD 001", Some("cli:goal-new"))?;
    let _ = bus.consume_outbound().ok_or("missing set outbound")?;
    loop_runtime.process_direct("/new", Some("cli:goal-new"))?;
    let _ = bus.consume_outbound().ok_or("missing new outbound")?;

    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal-new")
        .ok_or("missing new goal session")?;
    assert!(raw["metadata"].get(PERSISTENT_GOAL_METADATA_KEY).is_none());
    Ok(())
}

#[test]
fn loop_exact_command_with_extra_text_runs_as_normal_user_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("normal turn".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/status now", Some("cli:direct"))?;

    assert_eq!(result.command, None);
    assert_eq!(result.final_content.as_deref(), Some("normal turn"));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn loop_restart_and_dream_commands_publish_local_responses() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let restart = loop_runtime.process_direct("/restart", Some("cli:commands"))?;
    assert_eq!(
        restart.command,
        Some(AgentLoopCommandResult::RestartRequested)
    );
    assert!(bus
        .consume_outbound()
        .ok_or("missing restart outbound")?
        .content
        .contains("Restart requested"));

    let dream = loop_runtime.process_direct("/dream", Some("cli:commands"))?;
    assert_eq!(dream.command, Some(AgentLoopCommandResult::Dream));
    assert!(bus
        .consume_outbound()
        .ok_or("missing dream outbound")?
        .content
        .contains("Dream idle"));

    let log = loop_runtime.process_direct("/dream-log", Some("cli:commands"))?;
    assert_eq!(log.command, Some(AgentLoopCommandResult::DreamLog));
    assert!(bus
        .consume_outbound()
        .ok_or("missing dream log outbound")?
        .content
        .contains("no saved versions"));

    let restore = loop_runtime.process_direct("/dream-restore", Some("cli:commands"))?;
    assert_eq!(restore.command, Some(AgentLoopCommandResult::DreamRestore));
    assert!(bus
        .consume_outbound()
        .ok_or("missing dream restore outbound")?
        .content
        .contains("no saved versions"));
    Ok(())
}

#[test]
fn loop_dream_log_defaults_to_latest_diff_and_restore_lists_versions() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let git = GitCliStore::new(
        workspace.path(),
        [
            "memory/MEMORY.md".to_owned(),
            "SOUL.md".to_owned(),
            "USER.md".to_owned(),
        ],
    );
    git.init()?;
    std::fs::write(workspace.path().join("memory/MEMORY.md"), "remember this\n")?;
    let sha = git
        .auto_commit("dream: update memory")?
        .ok_or("missing dream commit")?;

    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let log = loop_runtime.process_direct("/dream-log", Some("cli:commands"))?;
    assert_eq!(log.command, Some(AgentLoopCommandResult::DreamLog));
    let log_outbound = bus.consume_outbound().ok_or("missing dream log outbound")?;
    assert!(log_outbound
        .content
        .contains("Here is the latest Dream memory change"));
    assert!(log_outbound.content.contains("```diff"));
    assert!(log_outbound.content.contains("remember this"));

    let restore = loop_runtime.process_direct("/dream-restore", Some("cli:commands"))?;
    assert_eq!(restore.command, Some(AgentLoopCommandResult::DreamRestore));
    let restore_outbound = bus
        .consume_outbound()
        .ok_or("missing dream restore outbound")?;
    assert!(restore_outbound.content.contains("## Dream Restore"));
    assert!(restore_outbound.content.contains(&sha));
    Ok(())
}

#[test]
fn loop_consolidates_over_budget_session_before_building_context_and_preserves_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("old turn summary".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("fresh answer".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("direct:thread");
    session.add_message("user", "old question ".repeat(900), Map::new());
    session.add_message("assistant", "old answer ".repeat(900), Map::new());
    session.add_message("user", "recent question", Map::new());
    session.add_message("assistant", "recent answer", Map::new());
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.context_window_tokens = Some(2_200);
    config.settings = GenerationSettings {
        max_tokens: 1,
        ..GenerationSettings::default()
    };
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let result = loop_runtime.process_direct("fresh question", Some("direct:thread"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("direct:thread")
        .ok_or("missing consolidated session")?;
    let history = shacs_core::runtime::MemoryStore::new(workspace.path())?.read_entries();
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let archive_prompt = requests
        .first()
        .and_then(|request| request.messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if result.final_content.as_deref() != Some("fresh answer")
        || raw["last_consolidated"] != 2
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
        || raw["metadata"]["_last_summary"]["text"] != "old turn summary"
        || history.first().map(|entry| entry.content.as_str()) != Some("old turn summary")
        || requests.len() != 2
        || !archive_prompt.contains("truncated")
        || archive_prompt.chars().count() > 820
    {
        return Err(format!(
            "loop token consolidation drifted: result={result:?} raw={raw:?} history={history:?} archive_prompt_len={}",
            archive_prompt.chars().count()
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_token_consolidation_raw_fallback_advances_cursor_and_keeps_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("provider refused".to_owned()),
            finish_reason: "error".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("fresh answer".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("direct:thread");
    session.add_message("user", "old question ".repeat(900), Map::new());
    session.add_message("assistant", "old answer ".repeat(900), Map::new());
    session.add_message("user", "recent question", Map::new());
    session.add_message("assistant", "recent answer", Map::new());
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.context_window_tokens = Some(2_200);
    config.settings = GenerationSettings {
        max_tokens: 1,
        ..GenerationSettings::default()
    };
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_direct("fresh question", Some("direct:thread"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("direct:thread")
        .ok_or("missing fallback session")?;
    let history = shacs_core::runtime::MemoryStore::new(workspace.path())?.read_entries();
    if raw["last_consolidated"] != 2
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
        || raw["metadata"].get("_last_summary").is_some()
        || history.len() != 1
        || !history[0].content.contains("[RAW] 2 messages")
        || !history[0].content.contains("truncated")
    {
        return Err(format!(
            "loop raw fallback consolidation drifted: raw={raw:?} history={history:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_priority_new_clears_session_and_publishes_without_provider() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:direct");
    session.add_message("user", "old", Map::new());
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/new", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::NewSession) || result.outbound_count != 1 {
        return Err(format!("/new result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /new outbound")?;
    if !outbound.content.contains("new session") {
        return Err(format!("/new outbound drifted: {outbound:?}").into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:direct")
        .ok_or("missing cleared session")?;
    if raw["messages"].as_array().map(Vec::is_empty) != Some(true) {
        return Err(format!("/new did not clear session: {raw:?}").into());
    }
    if !client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("/new should not call provider".into());
    }
    Ok(())
}

#[test]
fn loop_priority_new_cancels_registered_task_before_clearing_session() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    let register_result = loop_task_registry.register(ActiveLoopTask::new(
        "cli:direct",
        "task-1",
        cancellation.clone(),
    ));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    let result = loop_runtime.process_direct("/new", Some("cli:direct"))?;

    assert_eq!(result.command, Some(AgentLoopCommandResult::NewSession));
    assert!(cancellation.is_cancelled());
    Ok(())
}

#[test]
fn loop_priority_status_publishes_without_provider_call() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/status", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::Status) {
        return Err(format!("/status result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /status outbound")?;
    if !outbound.content.contains("no active task") {
        return Err(format!("/status outbound drifted: {outbound:?}").into());
    }
    if !client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("/status should not call provider".into());
    }
    Ok(())
}

#[test]
fn loop_priority_status_reports_registered_async_task() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let register_result = loop_task_registry.register(ActiveLoopTask::new(
        "cli:direct",
        "task-1",
        CancellationToken::new(),
    ));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    let result = loop_runtime.process_direct("/status", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::Status) {
        return Err(format!("/status result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /status outbound")?;
    if !outbound.content.contains("active async task task-1") {
        return Err(format!("/status should report registered task: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn session_turn_lock_rejects_duplicate_active_session() -> Result<(), Box<dyn Error>> {
    let lock = SessionTurnLock::new();
    let guard = lock
        .acquire("cli:direct")
        .map_err(|error| format!("first acquire should succeed: {error:?}"))?;
    let duplicate = lock.acquire("cli:direct");
    if !matches!(
        duplicate,
        Err(SessionTurnAcquireError::AlreadyActive { ref session_key }) if session_key == "cli:direct"
    ) {
        return Err(format!("duplicate acquire should fail: {duplicate:?}").into());
    }
    if lock.active_session_keys() != ["cli:direct".to_owned()] {
        return Err("active session was not tracked".into());
    }
    drop(guard);
    if lock.acquire("cli:direct").is_err() {
        return Err("guard drop should release active session".into());
    }
    Ok(())
}

#[test]
fn stop_without_async_task_preserves_current_message() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::StopRequested) {
        return Err(format!("/stop result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /stop outbound")?;
    if !outbound.content.contains("No async task is running") {
        return Err(format!("/stop message should preserve no-task text: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn stop_without_async_task_does_not_block_next_user_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("after stop".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    let _ = bus.consume_outbound().ok_or("missing /stop outbound")?;
    let result = loop_runtime.process_direct("hello again", Some("cli:direct"))?;

    assert_eq!(result.command, None);
    assert_eq!(result.final_content.as_deref(), Some("after stop"));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn stop_requests_cancel_for_registered_task() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    let register_result = loop_task_registry.register(ActiveLoopTask::new(
        "cli:direct",
        "task-1",
        cancellation.clone(),
    ));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    let outbound = bus.consume_outbound().ok_or("missing /stop outbound")?;
    if !cancellation.is_cancelled() || !outbound.content.contains("Cancellation requested") {
        return Err(format!("/stop should request cancellation only: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_result_with_wrong_child_id_is_stale() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let expected = shacs_core::runtime::SpawnEnvelope::new("cli:parent", "child-a", "inspect");
    let result = ChildResultEnvelope::new("cli:parent", "child-b", "done");

    let decision = runtime.classify_result(&expected, &result);
    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("child id mismatch"))
    {
        return Err(format!("wrong child id should be stale: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_result_with_matching_parent_and_child_accepts_summary() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let expected = shacs_core::runtime::SpawnEnvelope::new("cli:parent", "child-a", "inspect");
    let result = ChildResultEnvelope::new("cli:parent", "child-a", "done");

    let decision = runtime.classify_result(&expected, &result);
    if decision != MergeDecision::AcceptSummaryOnly {
        return Err(format!("matching child result should be accepted: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_result_with_wrong_parent_session_is_stale() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let expected = shacs_core::runtime::SpawnEnvelope::new("cli:parent", "child-a", "inspect");
    let result = ChildResultEnvelope::new("cli:other", "child-a", "done");

    let decision = runtime.classify_result(&expected, &result);
    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("parent session mismatch"))
    {
        return Err(format!("wrong parent session should be stale: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_spawn_registers_active_task_and_cancels_by_session() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect docs".to_owned(),
        label: Some("docs".to_owned()),
        origin_channel: "telegram".to_owned(),
        origin_chat_id: "chat-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;

    if runtime.running_count() != 1
        || runtime.running_count_by_session("session-1") != 1
        || !outcome.user_message.contains("Subagent [docs] started")
    {
        return Err(format!("subagent spawn tracking drifted: {outcome:?}").into());
    }
    let status = runtime
        .snapshot(&outcome.envelope.child_task_id)
        .ok_or("missing subagent status")?;
    if status.label != "docs" || status.task_description != "Inspect docs" {
        return Err(format!("subagent status drifted: {status:?}").into());
    }
    let status = runtime
        .update_progress(
            &outcome.envelope.child_task_id,
            SubagentProgressUpdate {
                phase: "awaiting_tools".to_owned(),
                iteration: 2,
                tool_events: vec![json!({"name":"read_file","status":"ok"})],
                usage: json!({"input_tokens": 10}),
                error: None,
            },
        )
        .ok_or("missing updated subagent status")?;
    if status.iteration != 2
        || status.tool_events.len() != 1
        || status.usage["input_tokens"] != 10
        || status.state != shacs_core::runtime::SubagentState::Running
    {
        return Err(format!("subagent progress update drifted: {status:?}").into());
    }
    if runtime.cancel_by_session("session-1") != 1 {
        return Err("cancel_by_session should cancel one active child".into());
    }
    let status = runtime
        .snapshot(&outcome.envelope.child_task_id)
        .ok_or("missing cancelled subagent status")?;
    if status.state != shacs_core::runtime::SubagentState::Cancelled {
        return Err(format!("cancelled status drifted: {status:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_spawn_inherits_snapshot_contract() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect docs".to_owned(),
        label: Some("docs".to_owned()),
        origin_channel: "telegram".to_owned(),
        origin_chat_id: "chat-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;

    if outcome.envelope.inherited_context_snapshot["origin_channel"] != "telegram"
        || outcome.envelope.inherited_context_snapshot["origin_chat_id"] != "chat-1"
        || outcome.envelope.inherited_policy_snapshot["capability_ceiling"] != "parent"
        || outcome.envelope.parent_turn_id != "turn:session-1"
        || outcome.envelope.parallelism_group != "session-1"
    {
        return Err(format!("subagent spawn snapshots drifted: {outcome:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_execution_config_defaults_to_empty_runtime_snapshots() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let config = SubagentExecutionConfig::new(workspace.path(), "test-model");

    if config.containment_snapshot.is_some()
        || config.permission_mode_snapshot != PermissionModeSnapshot::default()
    {
        return Err(format!("subagent config default snapshots drifted: {config:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_permissioned_action_context_inherits_snapshots_and_origin() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    std::fs::write(workspace.path().join("note.txt"), "hello")?;
    let config = SubagentExecutionConfig::new(workspace.path(), "test-model");
    let registry = build_subagent_tool_registry(&config);
    let containment_snapshot = ContainmentSnapshotRef {
        contained: Some(true),
        digest: Some("containment-digest".to_owned()),
        summary: Some("workspace containment".to_owned()),
    };
    let permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::AcceptEdits,
        source: Some("test-source".to_owned()),
        scope_ref: Some("scope:test".to_owned()),
    };
    let context = ToolExecutionContext {
        channel: "cli".to_owned(),
        chat_id: "direct".to_owned(),
        message_id: Some("turn:cli:direct".to_owned()),
        metadata: json!({ "subagent_task_id": "child-1" }),
        session_key: Some("cli:direct".to_owned()),
        containment_snapshot: Some(containment_snapshot.clone()),
        permission_mode_snapshot: permission_mode_snapshot.clone(),
        permission_rule_input: Default::default(),
        permission_auto_approval: AutoApprovalConfig::default(),
        permission_ceiling_snapshot: None,
        permission_evaluator: None,
        permission_interactive: false,
        permission_approval_cache: None,
        permission_session_approval_cache: Vec::new(),
        in_cron_context: false,
        record_channel_delivery: false,
    };

    let report = RuntimeToolExecutor::new(&registry).execute_tool_calls(
        vec![RuntimeToolCall::new(
            "read-1",
            "read_file",
            json!({ "path": "note.txt" }),
        )],
        &context,
    );
    let action = report
        .permissioned_actions
        .first()
        .ok_or("missing permissioned action")?;

    if action.containment_snapshot.as_ref() != Some(&containment_snapshot)
        || action.permission_mode_snapshot != permission_mode_snapshot
        || !matches!(
            &action.origin,
            PermissionedActionOrigin::Subagent {
                subagent_id: Some(subagent_id),
            } if subagent_id == "child-1"
        )
    {
        return Err(format!("subagent permissioned action context drifted: {action:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_finish_publishes_synthetic_inbound_and_closes_active_task() -> Result<(), Box<dyn Error>>
{
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: None,
        origin_channel: "slack".to_owned(),
        origin_chat_id: "thread-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let result = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Runtime summary",
    );

    let decision = runtime.publish_child_result(result);
    if decision != MergeDecision::AcceptSummaryOnly || runtime.running_count() != 0 {
        return Err(format!("subagent finish drifted: {decision:?}").into());
    }
    let inbound = bus
        .consume_inbound()
        .ok_or("missing synthetic subagent inbound")?;
    if inbound.channel != "system"
        || inbound.sender_id != "subagent"
        || inbound.session_key_override.as_deref() != Some("session-1")
        || inbound.metadata["injected_event"] != "subagent_result"
        || inbound.metadata["subagent_task_id"] != outcome.envelope.child_task_id
        || !inbound
            .content
            .contains("[Subagent 'Summarize runtime' completed successfully]")
        || !inbound.content.contains("Task: Summarize runtime")
        || !inbound.content.contains("Runtime summary")
        || inbound.content.contains("Merge decision")
    {
        return Err(format!("synthetic subagent inbound drifted: {inbound:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_run_spawn_executes_agent_and_publishes_result() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let skill_dir = workspace.path().join("skills").join("configured-env");
    std::fs::create_dir_all(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Configured env skill\nrequires.env: SHACS_SUBAGENT_TEST_CONFIGURED_ENV_ONLY\n---\nUse configured env.\n",
    )?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: Some("runtime".to_owned()),
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "cli:direct".to_owned(),
    })?;
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("subagent done".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut config = SubagentExecutionConfig::new(workspace.path(), "test-model");
    config.exec_env = BTreeMap::from([(
        "SHACS_SUBAGENT_TEST_CONFIGURED_ENV_ONLY".to_owned(),
        "configured".to_owned(),
    )]);
    let result = runtime.run_spawn(outcome.envelope.clone(), &client, config);
    if result.status != ChildResultStatus::Completed
        || result.summary != "subagent done"
        || runtime.running_count() != 0
    {
        return Err(format!("subagent run_spawn drifted: result={result:?}").into());
    }
    let inbound = bus
        .consume_inbound()
        .ok_or("missing run_spawn synthetic inbound")?;
    if !inbound.content.contains("subagent done")
        || inbound.session_key_override.as_deref() != Some("cli:direct")
    {
        return Err(format!("run_spawn announcement drifted: {inbound:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let system_prompt = requests[0]
        .messages
        .first()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if requests.len() != 1
        || requests[0].model != "test-model"
        || !requests[0]
            .tools
            .iter()
            .any(|tool| tool.to_string().contains("read_file"))
        || requests[0]
            .tools
            .iter()
            .any(|tool| tool.to_string().contains("spawn"))
        || !system_prompt.contains("# Subagent")
        || !system_prompt.contains("**configured-env**")
        || system_prompt.contains("SHACS_SUBAGENT_TEST_CONFIGURED_ENV_ONLY")
    {
        return Err(format!("run_spawn provider request drifted: {requests:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_cancel_before_run_cleans_without_announcement() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Long task".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "cli:direct".to_owned(),
    })?;
    if runtime.cancel_by_session("cli:direct") != 1 {
        return Err("cancel_by_session should cancel spawned child".into());
    }
    let client = MockProvider::new(Vec::new());
    let result = runtime.run_spawn(
        outcome.envelope,
        &client,
        SubagentExecutionConfig::new(workspace.path(), "test-model"),
    );
    if result.status != ChildResultStatus::Cancelled
        || runtime.running_count() != 0
        || bus.try_consume_inbound().is_some()
    {
        return Err(format!("cancelled subagent cleanup drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_tool_registry_excludes_parent_only_tools() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut config = SubagentExecutionConfig::new(workspace.path(), "test-model");
    config.allow_side_effect_tools = true;
    config.enable_exec = true;
    config.enable_web = true;
    let registry = build_subagent_tool_registry(&config);
    for expected in [
        "read_file",
        "write_file",
        "edit_file",
        "list_dir",
        "glob",
        "grep",
        "exec",
        "web_fetch",
        "web_search",
    ] {
        if !registry.has(expected) {
            return Err(format!("subagent registry missing {expected}").into());
        }
    }
    for forbidden in ["spawn", "message", "ask_user", "my", "cron"] {
        if registry.has(forbidden) {
            return Err(format!("subagent registry should exclude {forbidden}").into());
        }
    }
    Ok(())
}

#[test]
fn subagent_tool_search_catalog_uses_child_registry_not_parent_definitions(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let parent_calls = Arc::new(Mutex::new(0usize));
    let mut parent_registry = ToolRegistry::new();
    parent_registry.register(ParentOnlyMcpTool {
        calls: parent_calls.clone(),
    });
    let parent_surface = assemble_tool_surface(ToolSurfaceAssemblyInput {
        definitions: parent_registry.definitions(),
        runtime: child_tool_search_runtime(),
    });
    let parent_catalog = parent_surface
        .catalog
        .as_ref()
        .ok_or("parent MCP catalog should exist")?;
    if !parent_catalog
        .entries
        .iter()
        .any(|entry| entry.name == "mcp_parent_only")
    {
        return Err(format!(
            "parent-only MCP tool missing from parent catalog: {parent_catalog:?}"
        )
        .into());
    }

    let mut config = SubagentExecutionConfig::new(workspace.path(), "test-model");
    config.allow_side_effect_tools = true;
    config.enable_exec = true;
    config.enable_web = true;
    let child_registry = build_subagent_tool_registry(&config);
    let child_definitions = child_registry.definitions();
    let child_definition_names = tool_definition_names(&child_definitions)?;
    if child_definition_names
        .iter()
        .any(|name| name == "mcp_parent_only")
    {
        return Err(format!(
            "child definitions included parent-only tool: {child_definition_names:?}"
        )
        .into());
    }

    let child_surface = assemble_tool_surface(ToolSurfaceAssemblyInput {
        definitions: child_definitions,
        runtime: child_tool_search_runtime(),
    });
    if child_surface.activation_state != ActivationState::PassThrough
        || child_surface.catalog.is_some()
    {
        return Err(format!(
            "child Tool Search surface should be built from child definitions only: {child_surface:?}"
        )
        .into());
    }

    let child_executor = RuntimeToolExecutor::new(&child_registry);
    for (call_id, bridge_name, arguments) in [
        (
            "child-search-parent-only",
            "tool_search",
            json!({ "query": "parent only" }),
        ),
        (
            "child-describe-parent-only",
            "tool_describe",
            json!({ "name": "mcp_parent_only" }),
        ),
        (
            "child-call-parent-only",
            "tool_call",
            json!({ "name": "mcp_parent_only", "arguments": {} }),
        ),
    ] {
        let report = dispatch_bridge_tool_call(
            RuntimeToolCall::new(call_id, bridge_name, arguments),
            child_surface.catalog.as_ref(),
            &child_registry,
            &child_executor,
            &ToolExecutionContext::default(),
        );
        let messages = report.messages();
        let message = messages.first().ok_or("missing child bridge rejection")?;
        if message.tool_call_id != call_id
            || !message
                .content
                .contains("deferred tool catalog is not available")
        {
            return Err(
                format!("parent-only child bridge path did not fail closed: {report:?}").into(),
            );
        }
    }

    let parent_call_count = parent_calls.lock().map_err(|error| error.to_string())?;
    if *parent_call_count != 0 {
        return Err("child bridge dispatch executed parent-only tool".into());
    }
    Ok(())
}

#[test]
fn core_bridge_tool_events_serialize_safe_for_subagent_progress() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(ParentOnlyMcpTool {
        calls: Arc::new(Mutex::new(0)),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![
                ToolCallRequest::new(
                    "search-1",
                    "tool_search",
                    Map::from_iter([
                        (
                            "query".to_owned(),
                            json!("token=RAW_BRIDGE_SECRET parent only"),
                        ),
                        ("limit".to_owned(), json!(5)),
                    ]),
                ),
                ToolCallRequest::new(
                    "describe-1",
                    "tool_describe",
                    Map::from_iter([("name".to_owned(), json!("mcp_parent_only"))]),
                ),
                ToolCallRequest::new(
                    "call-1",
                    "tool_call",
                    Map::from_iter([
                        ("name".to_owned(), json!("mcp_parent_only")),
                        (
                            "arguments".to_owned(),
                            json!({
                                "token": "RAW_BRIDGE_SECRET",
                                "raw_schema": "RAW_SCHEMA_SHOULD_NOT_LEAK"
                            }),
                        ),
                    ]),
                ),
            ],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "use deferred tools"})],
        &registry,
        &client,
        "test-model",
    );
    spec.tool_search = child_tool_search_runtime().config;
    spec.max_iterations = 3;

    let result = AgentRunner::new().run(spec)?;
    let tool_events = result
        .tool_events
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let progress = SubagentProgressUpdate {
        phase: "awaiting_tools".to_owned(),
        iteration: 1,
        tool_events,
        usage: Value::Null,
        error: None,
    };
    let serialized = serde_json::to_string(&progress)?;
    for forbidden in [
        "RAW_BRIDGE_SECRET",
        "RAW_SCHEMA_SHOULD_NOT_LEAK",
        "raw_schema",
        "Parent-only MCP test tool",
        "schema",
    ] {
        if serialized.contains(forbidden) {
            return Err(
                format!("subagent progress ToolEvent leaked {forbidden}: {serialized}").into(),
            );
        }
    }
    if !serialized.contains("tool_search")
        || !serialized.contains("tool_describe")
        || !serialized.contains("tool_call")
    {
        return Err(format!("missing bridge ToolEvents in progress payload: {serialized}").into());
    }
    Ok(())
}

struct ParentOnlyMcpTool {
    calls: Arc<Mutex<usize>>,
}

impl Tool for ParentOnlyMcpTool {
    fn name(&self) -> &str {
        "mcp_parent_only"
    }

    fn description(&self) -> &str {
        "Parent-only MCP test tool."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        if let Ok(mut calls) = self.calls.lock() {
            *calls += 1;
        }
        "parent-only executed".into()
    }
}

fn child_tool_search_runtime() -> ToolSearchRuntimeInput {
    ToolSearchRuntimeInput {
        config: ToolSearchConfig {
            enabled: ToolSearchMode::On,
            threshold_pct: 10,
            search_default_limit: 5,
            max_search_limit: 20,
        },
        context_window_tokens: None,
    }
}

fn tool_definition_names(definitions: &[Value]) -> Result<Vec<String>, Box<dyn Error>> {
    definitions
        .iter()
        .map(|definition| {
            definition
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .or_else(|| definition.get("name").and_then(Value::as_str))
                .map(str::to_owned)
                .ok_or_else(|| "missing tool definition name".into())
        })
        .collect()
}

#[test]
fn subagent_partial_progress_formats_completed_steps_and_failure() -> Result<(), Box<dyn Error>> {
    let events = vec![
        ToolEvent {
            name: "read".to_owned(),
            status: ToolStatus::Ok,
            detail: "read docs".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "grep".to_owned(),
            status: ToolStatus::Ok,
            detail: "found patterns".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "write".to_owned(),
            status: ToolStatus::Ok,
            detail: "drafted patch".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "clippy".to_owned(),
            status: ToolStatus::Ok,
            detail: "checked".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "test".to_owned(),
            status: ToolStatus::Error,
            detail: "failed assertion".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
    ];
    let progress = format_partial_progress_from_tool_events(&events, None);
    if progress.contains("read docs")
        || !progress.contains("Completed steps:")
        || !progress.contains("- grep: found patterns")
        || !progress.contains("- write: drafted patch")
        || !progress.contains("- clippy: checked")
        || !progress.contains("Failure:")
        || !progress.contains("- test: failed assertion")
    {
        return Err(format!("subagent partial progress drifted: {progress}").into());
    }
    Ok(())
}

#[test]
fn subagent_stale_result_does_not_publish_or_close_active_child() -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: None,
        origin_channel: "slack".to_owned(),
        origin_chat_id: "thread-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let mut stale = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Wrong turn summary",
    );
    stale.parent_turn_id = "turn:stale".to_owned();

    let decision = runtime.publish_child_result(stale);
    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("parent turn mismatch"))
        || runtime.running_count() != 1
        || bus.try_consume_inbound().is_some()
    {
        return Err(
            format!("stale result should not publish or close active child: {decision:?}").into(),
        );
    }
    let active = runtime
        .snapshot(&outcome.envelope.child_task_id)
        .ok_or("stale result should leave active child available")?;
    if active.state != shacs_core::runtime::SubagentState::Spawned {
        return Err(format!("stale result should not mutate active state: {active:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_stale_inbound_is_not_persisted_as_session_content() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect stale".to_owned(),
        label: None,
        origin_channel: "slack".to_owned(),
        origin_chat_id: "thread-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let mut stale = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "SHOULD_NOT_PERSIST",
    );
    stale.spawn_effect_id = "spawn:stale".to_owned();
    let decision = runtime.publish_child_result(stale);
    if !matches!(decision, MergeDecision::DiscardAsStale { .. })
        || bus.try_consume_inbound().is_some()
    {
        return Err("stale result should stay off the AgentLoop inbound path".into());
    }

    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("normal reply".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let normal = InboundMessage::new("cli", "user", "direct", "hello");
    loop_runtime.process_message(normal)?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:direct")
        .ok_or("missing session")?;
    if raw.to_string().contains("SHOULD_NOT_PERSIST") {
        return Err(format!("stale subagent result leaked into session: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_parallelism_limit_rejects_excess_children() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::with_config(SubagentRuntimeConfig { max_parallelism: 1 });
    runtime.spawn_from_request(SpawnRequest {
        task: "first".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let second = runtime.spawn_from_request(SpawnRequest {
        task: "second".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    });
    if !matches!(second, Err(ref error) if error.contains("parallelism limit")) {
        return Err(format!("parallelism limit should reject second child: {second:?}").into());
    }
    Ok(())
}

#[test]
fn spawn_tool_can_delegate_to_subagent_runtime() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let tool = SpawnTool::new(Arc::new(runtime.clone()));
    tool.set_context("telegram", "chat-1", Some("session-1".to_owned()));
    let result = shacs_core::tools::Tool::execute(
        &tool,
        Map::from_iter([("task".to_owned(), json!("Inspect workspace"))]),
    )
    .into_text();
    if !result.contains("Subagent [Inspect workspace] started")
        || runtime.running_count_by_session("session-1") != 1
    {
        return Err(format!("spawn tool/runtime integration drifted: {result}").into());
    }
    Ok(())
}

#[test]
fn loop_lifecycle_reports_structured_status() -> Result<(), Box<dyn Error>> {
    let reports = [McpLifecycle::new().status(), DreamLifecycle::new().status()];
    let components = reports
        .iter()
        .map(|report| report.component.as_str())
        .collect::<Vec<_>>();
    if components != ["mcp_lifecycle", "dream_lifecycle"] {
        return Err(format!("lifecycle component names drifted: {reports:?}").into());
    }
    if reports[0].status != RuntimeCapabilityStatus::Unavailable
        || reports[1].status != RuntimeCapabilityStatus::Unavailable
        || reports.iter().any(|report| report.reason.trim().is_empty())
        || McpLifecycle::from_counts(2, 1, 1).status().status != RuntimeCapabilityStatus::Available
        || DreamLifecycle::configured().status().status != RuntimeCapabilityStatus::Available
    {
        return Err(format!("lifecycle status reports drifted: {reports:?}").into());
    }
    let subagent_status = SubagentRuntime::new().status();
    if subagent_status.component != "subagent_runtime"
        || subagent_status.status != RuntimeCapabilityStatus::Available
        || !subagent_status.reason.contains("synthetic reentry")
    {
        return Err(format!("subagent runtime status drifted: {subagent_status:?}").into());
    }
    Ok(())
}

#[test]
fn static_provider_selector_rejects_hot_swap_without_mutating_current_turn(
) -> Result<(), Box<dyn Error>> {
    let current = ProviderSelectionSnapshot::new("openai", "gpt-5");
    let mut selector = StaticProviderSelector::new(current.clone());
    let result = selector.request_hot_swap(ProviderSelectionSnapshot::new("anthropic", "claude"));

    if result
        != (ProviderHotSwapResult::Unsupported {
            current: current.clone(),
        })
        || selector.select_snapshot() != current
    {
        return Err(format!("provider hot-swap contract drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_ask_user_interrupt_publishes_buttons_and_resumes_as_tool_result(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "ask-1",
                "ask_user",
                Map::from_iter([
                    ("question".to_owned(), json!("Continue?")),
                    ("options".to_owned(), json!(["Yes", "No"])),
                ]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let first = loop_runtime.process_direct("start", Some("cli:ask"))?;
    if first.stop_reason != "ask_user" || first.ask_user_options != ["Yes", "No"] {
        return Err(format!("ask interrupt result drifted: {first:?}").into());
    }
    let ask_outbound = bus.consume_outbound().ok_or("missing ask outbound")?;
    if !ask_outbound.content.contains("1. Yes") || !ask_outbound.buttons.is_empty() {
        return Err(format!("ask outbound should render plain options: {ask_outbound:?}").into());
    }

    let second = loop_runtime.process_direct("Yes", Some("cli:ask"))?;
    if second.final_content.as_deref() != Some("resumed") {
        return Err(format!("ask resume result drifted: {second:?}").into());
    }
    let _final_outbound = bus.consume_outbound().ok_or("missing resumed outbound")?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:ask")
        .ok_or("missing ask session")?;
    if !raw["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|message| {
            message["role"] == "tool"
                && message["name"] == "ask_user"
                && message["tool_call_id"] == "ask-1"
                && message["content"] == "Yes"
        })
    {
        return Err(format!("ask answer was not persisted as tool result: {raw:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if !requests
        .get(1)
        .into_iter()
        .flat_map(|request| &request.messages)
        .any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "ask-1"
                && message["content"] == "Yes"
        })
    {
        return Err(
            format!("ask answer was not sent to provider as tool result: {requests:?}").into(),
        );
    }
    let resume_request = requests.get(1).ok_or("missing resume request")?;
    let last_message = resume_request
        .messages
        .last()
        .ok_or("resume request should include messages")?;
    if last_message["role"] != "tool"
        || last_message["tool_call_id"] != "ask-1"
        || last_message["content"] != "Yes"
    {
        return Err(format!("ask resume request suffix drifted: {resume_request:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_executes_pending_tool_and_resumes() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-1",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!(
            "permission approval did not pause before exec: {first:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    if !approval_outbound
        .content
        .contains("Permission approval required")
        || !approval_outbound.content.contains("1. approve")
        || !approval_outbound.content.contains("3. approve_session")
    {
        return Err(format!(
            "approval outbound was not rendered for chat approval: {approval_outbound:?}"
        )
        .into());
    }

    let second = loop_runtime.process_direct("1", Some("discord:approval"))?;
    if calls.load(Ordering::SeqCst) != 1
        || second.final_content.as_deref() != Some("resumed after exec")
    {
        return Err(format!(
            "approval did not execute pending exec and resume: {second:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval")
        .ok_or("missing approval session")?;
    if !raw["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|message| {
            message["role"] == "tool"
                && message["name"] == "exec"
                && message["tool_call_id"] == "exec-1"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("exec-output"))
        })
    {
        return Err(format!("approved exec result was not persisted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_session_option_reuses_same_session_match() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-1",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("approved for session".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-2",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("reused session approval".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-3",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo clippy"))]),
            )],
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval-session"))?;
    assert_eq!(first.stop_reason, "ask_user");
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    let approved = loop_runtime.process_direct("3", Some("discord:approval-session"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("approved for session")
    {
        return Err(format!(
            "session approval did not approve the pending tool: {approved:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let _approved_outbound = bus.consume_outbound().ok_or("missing approved outbound")?;

    let reused = loop_runtime.process_direct("again", Some("discord:approval-session"))?;
    if calls.load(Ordering::SeqCst) != 2
        || reused.final_content.as_deref() != Some("reused session approval")
        || reused.stop_reason == "ask_user"
    {
        return Err(format!(
            "matching session approval was not reused: {reused:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval-session")
        .ok_or("missing session approval session")?;
    if raw["metadata"]["session_permission_approvals"]
        .as_array()
        .map(Vec::len)
        != Some(1)
        || raw["metadata"]["session_permission_approvals"][0]["session_key"]
            != "discord:approval-session"
        || !raw["metadata"]["session_permission_approvals"][0]["approval_context_digest"]
            .is_string()
        || raw["metadata"]["session_permission_approvals"][0]["approval"]["decision"]["decision"]
            != "approved_for_session"
    {
        return Err(format!("session approval metadata drifted: {raw:?}").into());
    }

    let _reused_outbound = bus.consume_outbound().ok_or("missing reused outbound")?;
    let different_action =
        loop_runtime.process_direct("different action", Some("discord:approval-session"))?;
    if different_action.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 2 {
        return Err(format!(
            "session approval reused a different action: {different_action:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_session_option_reuses_channel_message_id_change(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-channel-1",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("approved channel session".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-channel-2",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("reused channel session approval".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-session",
        "start",
        "channel-msg-1",
    ))?;
    assert_eq!(first.stop_reason, "ask_user");
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    let approved = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-session",
        "approve_session",
        "channel-msg-2",
    ))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("approved channel session")
    {
        return Err(format!("channel session approval failed: {approved:?}").into());
    }
    let _approved_outbound = bus.consume_outbound().ok_or("missing approved outbound")?;

    let reused = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-session",
        "again",
        "channel-msg-3",
    ))?;
    if calls.load(Ordering::SeqCst) != 2
        || reused.final_content.as_deref() != Some("reused channel session approval")
        || reused.stop_reason == "ask_user"
    {
        return Err(format!(
            "channel message id change prevented session approval reuse: {reused:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_session_option_does_not_cross_sessions() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-a",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("approved in first session".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-b",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval-a"))?;
    assert_eq!(first.stop_reason, "ask_user");
    let _approval_outbound = bus
        .consume_outbound()
        .ok_or("missing first approval outbound")?;
    let approved = loop_runtime.process_direct("approve_session", Some("discord:approval-a"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("approved in first session")
    {
        return Err(format!("first session approval failed: {approved:?}").into());
    }
    let _approved_outbound = bus.consume_outbound().ok_or("missing approved outbound")?;

    let second_session = loop_runtime.process_direct("same action", Some("discord:approval-b"))?;
    if second_session.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 1 {
        return Err(format!(
            "session approval crossed sessions: {second_session:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let outbound = bus
        .consume_outbound()
        .ok_or("missing second session approval outbound")?;
    if !outbound.content.contains("Permission approval required") {
        return Err(format!("second session did not ask for approval: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_session_option_clears_on_new_session() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-before-new",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("approved before new".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-after-new",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval-new"))?;
    assert_eq!(first.stop_reason, "ask_user");
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    let approved = loop_runtime.process_direct("approve_session", Some("discord:approval-new"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("approved before new")
    {
        return Err(format!("session approval before /new failed: {approved:?}").into());
    }
    let _approved_outbound = bus.consume_outbound().ok_or("missing approved outbound")?;

    let reset = loop_runtime.process_direct("/new", Some("discord:approval-new"))?;
    if reset.command != Some(AgentLoopCommandResult::NewSession)
        || reset.final_content.as_deref() != Some("Started a new session.")
    {
        return Err(format!("/new did not reset the session: {reset:?}").into());
    }
    let _reset_outbound = bus.consume_outbound().ok_or("missing reset outbound")?;

    let after_new = loop_runtime.process_direct("after new", Some("discord:approval-new"))?;
    if after_new.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 1 {
        return Err(format!(
            "session approval survived /new: {after_new:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval-new")
        .ok_or("missing reset approval session")?;
    if raw["metadata"]
        .get("session_permission_approvals")
        .is_some()
    {
        return Err(format!("/new left session approvals in metadata: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_persists_executing_state_before_tool_runs() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_status = Arc::new(Mutex::new(None));
    let mut registry = ToolRegistry::new();
    registry.register(ApprovalMetadataProbeTool {
        workspace: workspace.path().to_path_buf(),
        session_key: "discord:approval-executing",
        calls: calls.clone(),
        observed_status: observed_status.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-executing",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after executing marker".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval-executing"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("permission approval did not pause: {first:?}").into());
    }
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    let second = loop_runtime.process_direct("approve", Some("discord:approval-executing"))?;
    let observed = observed_status
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    if calls.load(Ordering::SeqCst) != 1
        || observed.as_deref() != Some("executing")
        || second.final_content.as_deref() != Some("resumed after executing marker")
    {
        return Err(format!(
            "approval execution marker was not durable before tool run: {second:?} observed={observed:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval-executing")
        .ok_or("missing approval session")?;
    if raw["metadata"].get("pending_permission_approval").is_some() {
        return Err(format!("completed approval left pending metadata: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_uses_original_context_when_reply_message_id_changes(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-channel",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after channel approval".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-1",
        "start",
        "discord-msg-1",
    ))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("channel approval did not pause: {first:?}").into());
    }
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    let second = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-1",
        "approve",
        "discord-msg-2",
    ))?;
    if calls.load(Ordering::SeqCst) != 1
        || second.final_content.as_deref() != Some("resumed after channel approval")
    {
        return Err(format!(
            "approval reply with new message_id did not execute original action: {second:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_rejects_different_sender_in_same_session() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "exec-sender",
            "exec",
            Map::from_iter([("command".to_owned(), json!("cargo test"))]),
        )],
        ..LlmResponse::default()
    }]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-1",
        "start",
        "sender-msg-1",
    ))?;
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    let reply = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-2",
        "chat-1",
        "approve",
        "sender-msg-2",
    ))?;
    if calls.load(Ordering::SeqCst) != 0
        || reply.stop_reason != "permission_approval_pending"
        || !reply
            .final_content
            .as_deref()
            .is_some_and(|content| content.contains("Approval pending"))
    {
        return Err(format!(
            "different sender should not approve pending tool: {reply:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:chat-1")
        .ok_or("missing sender approval session")?;
    if raw["metadata"].get("pending_permission_approval").is_none() {
        return Err(format!("sender mismatch cleared pending approval: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_resumes_deferred_tool_search_bridge_with_bridge_mapping(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(NamedProcExecCountingTool {
        name: "mcp_exec",
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "bridge-exec",
                "tool_call",
                Map::from_iter([
                    ("name".to_owned(), json!("mcp_exec")),
                    ("arguments".to_owned(), json!({ "command": "cargo test" })),
                ]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after bridge exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    config.tool_search = ToolSearchConfig {
        enabled: ToolSearchMode::On,
        threshold_pct: 10,
        search_default_limit: 5,
        max_search_limit: 20,
    };
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-bridge",
        "start",
        "bridge-msg-1",
    ))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("bridge approval did not pause: {first:?}").into());
    }
    let _approval_outbound = bus
        .consume_outbound()
        .ok_or("missing bridge approval outbound")?;

    let second = loop_runtime.process_message(inbound_with_message_id(
        "discord",
        "user-1",
        "chat-bridge",
        "approve",
        "bridge-msg-2",
    ))?;
    let raw_after_approval = loop_runtime
        .session_manager()
        .read_session_file("discord:chat-bridge")
        .ok_or("missing bridge session after approval")?;
    if calls.load(Ordering::SeqCst) != 1
        || second.final_content.as_deref() != Some("resumed after bridge exec")
    {
        return Err(format!(
            "bridge approval did not execute and resume: {second:?} calls={} raw={raw_after_approval:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = raw_after_approval;
    if !raw["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "bridge-exec"
                && message["name"] == "tool_call"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("exec-output"))
        })
    {
        return Err(format!("bridge result mapping was not persisted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_denial_cancels_without_execution() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "exec-deny",
            "exec",
            Map::from_iter([("command".to_owned(), json!("cargo test"))]),
        )],
        ..LlmResponse::default()
    }]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval-deny"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("permission approval did not pause before deny: {first:?}").into());
    }
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    let second = loop_runtime.process_direct("2", Some("discord:approval-deny"))?;
    if calls.load(Ordering::SeqCst) != 0
        || second.stop_reason != "permission_denied_by_user"
        || second.final_content.as_deref() != Some("Tool execution cancelled.")
    {
        return Err(format!(
            "denial should cancel without exec: {second:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if requests.len() != 1 {
        return Err(format!("denial should not call provider again: {requests:?}").into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval-deny")
        .ok_or("missing deny session")?;
    if raw["metadata"].get("pending_permission_approval").is_some()
        || !raw["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| {
                message["role"] == "tool"
                    && message["name"] == "exec"
                    && message["tool_call_id"] == "exec-deny"
                    && message["content"] == "Permission denied by user."
            })
    {
        return Err(format!("denial did not persist cancellation cleanly: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_unknown_reply_keeps_pending() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-unknown",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after unknown".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("discord:approval-unknown"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("permission approval did not pause before unknown: {first:?}").into());
    }
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;

    let unknown = loop_runtime.process_direct("maybe", Some("discord:approval-unknown"))?;
    if calls.load(Ordering::SeqCst) != 0
        || unknown.stop_reason != "permission_approval_pending"
        || !unknown
            .final_content
            .as_deref()
            .is_some_and(|content| content.contains("Approval pending"))
    {
        return Err(format!(
            "unknown reply should keep approval pending: {unknown:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let _pending_outbound = bus.consume_outbound().ok_or("missing pending outbound")?;
    let pending_raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval-unknown")
        .ok_or("missing unknown session")?;
    if pending_raw["metadata"]
        .get("pending_permission_approval")
        .is_none()
    {
        return Err(format!("unknown reply cleared pending approval: {pending_raw:?}").into());
    }

    let approved = loop_runtime.process_direct("approve", Some("discord:approval-unknown"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("resumed after unknown")
    {
        return Err(format!(
            "approval after unknown did not execute and resume: {approved:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_pending_user_turn_recovery_closes_interrupted_prior_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("new reply".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:recover");
    session.add_message("user", "unfinished", Map::new());
    session
        .metadata
        .insert("pending_user_turn".to_owned(), Value::Bool(true));
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("next", Some("cli:recover"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:recover")
        .ok_or("missing recovered session")?;
    if raw["metadata"].get("pending_user_turn").is_some()
        || !raw["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| {
                message["role"] == "assistant"
                    && message["_interrupted"] == true
                    && message["content"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("interrupted")
            })
    {
        return Err(format!("pending turn was not recovered: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:checkpoint");
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({
            "phase": "awaiting_tools",
            "assistant_message": {
                "role": "assistant",
                "content": "using tools",
                "tool_calls": [
                    {"id": "done", "type": "function", "function": {"name": "done_tool", "arguments": "{}"}},
                    {"id": "pending", "type": "function", "function": {"name": "pending_tool", "arguments": "{}"}}
                ]
            },
            "completed_tool_results": [
                {"tool_call_id": "done", "name": "done_tool", "content": "ok"}
            ],
            "pending_tool_calls": [
                {"id": "pending", "type": "function", "function": {"name": "pending_tool", "arguments": "{}"}}
            ]
        }),
    );
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/status", Some("cli:checkpoint"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:checkpoint")
        .ok_or("missing checkpoint session")?;
    let messages = raw["messages"]
        .as_array()
        .ok_or("messages should be array")?;
    if raw["metadata"].get("runtime_checkpoint").is_some()
        || messages.len() != 3
        || messages[0]["tool_calls"].as_array().map(Vec::len) != Some(2)
        || messages[1]["tool_call_id"] != "done"
        || messages[2]["tool_call_id"] != "pending"
        || !messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("interrupted or lost")
    {
        return Err(format!("checkpoint materialization drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn auto_compact_skips_active_sessions_and_preserves_checkpointed_agent_config(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:compact");
    session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(10)).to_rfc3339();
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({"phase": "awaiting_tools", "assistant_message": {"content": "using tools"}}),
    );
    session.metadata.insert(
        "agent_configuration".to_owned(),
        json!({"model": "test-model", "provider": "mock"}),
    );
    for index in 0..12 {
        session.add_message("user", format!("message {index}"), Map::new());
    }
    session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(10)).to_rfc3339();
    manager.save(&session)?;

    let mut compact = AutoCompact::new(1);
    let skipped = compact.mark_expired_sessions(&manager, ["cli:compact".to_owned()])?;
    if !skipped.is_empty() || compact.is_archiving("cli:compact") {
        return Err(format!("active compact session should be skipped: {skipped:?}").into());
    }

    let expired = compact.mark_expired_sessions(&manager, Vec::<String>::new())?;
    if expired != vec!["cli:compact".to_owned()] || !compact.is_archiving("cli:compact") {
        return Err(format!("expired compact session not marked: {expired:?}").into());
    }

    let outcome =
        compact.archive_session_with_summary(&mut manager, "cli:compact", Some("summary"))?;
    let raw = manager
        .read_session_file("cli:compact")
        .ok_or("missing compacted session")?;
    if outcome.archived_messages.len() != 4
        || outcome.kept_messages.len() != 8
        || raw["messages"].as_array().map(Vec::len) != Some(8)
        || raw["metadata"]["runtime_checkpoint"]["phase"] != "awaiting_tools"
        || raw["metadata"]["agent_configuration"]["model"] != "test-model"
        || raw["metadata"].get("_last_summary").is_none()
        || raw["last_consolidated"].as_u64().unwrap_or_default() != 0
    {
        return Err(
            format!("auto compact archive drifted: outcome={outcome:?} raw={raw:?}").into(),
        );
    }

    let loaded = manager.get_or_create("cli:compact");
    let (prepared, summary) = compact.prepare_session(&mut manager, loaded, "cli:compact")?;
    if !summary
        .as_deref()
        .unwrap_or_default()
        .contains("Previous conversation summary: summary")
        || prepared.metadata.get("_last_summary").is_some()
        || prepared.metadata["runtime_checkpoint"]["phase"] != "awaiting_tools"
        || prepared.metadata["agent_configuration"]["provider"] != "mock"
    {
        return Err(format!(
            "auto compact prepare lost metadata: summary={summary:?} prepared={prepared:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_consumes_auto_compact_summary_when_building_context() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("fresh answer".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:compact-summary");
    session.add_message("user", "old", Map::new());
    session.metadata.insert(
        "_last_summary".to_owned(),
        json!({"text": "archived facts", "last_active": chrono::Local::now().to_rfc3339()}),
    );
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_auto_compact(AutoCompact::new(60));

    let result = loop_runtime.process_direct("fresh", Some("cli:compact-summary"))?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let prompt = requests
        .first()
        .and_then(|request| request.messages.last())
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:compact-summary")
        .ok_or("missing prepared session")?;
    if result.final_content.as_deref() != Some("fresh answer")
        || !prompt.contains("Previous conversation summary: archived facts")
        || raw["metadata"].get("_last_summary").is_some()
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
    {
        return Err(format!(
            "loop autocompact summary drifted: result={result:?} prompt={prompt:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_idle_auto_compact_archives_expired_sessions_with_provider_summary(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("idle summary".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:idle");
    for index in 0..12 {
        session.add_message("user", format!("idle message {index}"), Map::new());
    }
    session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(5)).to_rfc3339();
    session
        .metadata
        .insert("runtime_checkpoint".to_owned(), json!({"phase": "kept"}));
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_auto_compact(AutoCompact::new(1));

    let outcomes = loop_runtime.run_idle_auto_compact()?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:idle")
        .ok_or("missing idle compact session")?;
    let history = shacs_core::runtime::MemoryStore::new(workspace.path())?.read_entries();
    if outcomes.len() != 1
        || outcomes[0].archived_messages.len() != 4
        || raw["messages"].as_array().map(Vec::len) != Some(8)
        || raw["metadata"]["_last_summary"]["text"] != "idle summary"
        || raw["metadata"]["runtime_checkpoint"]["phase"] != "kept"
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
        || history.first().map(|entry| entry.content.as_str()) != Some("idle summary")
    {
        return Err(format!(
            "idle autocompact drifted: outcomes={outcomes:?} raw={raw:?} history={history:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_idle_auto_compact_releases_all_markers_on_batch_failure() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    for key in ["cli:first", "cli:second"] {
        let mut session = Session::new(key);
        for index in 0..12 {
            session.add_message("user", format!("old {index}"), Map::new());
        }
        session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(5)).to_rfc3339();
        manager.save(&session)?;
    }
    std::fs::create_dir_all(workspace.path().join("memory/history.jsonl"))?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_auto_compact(AutoCompact::new(1));

    let result = loop_runtime.run_idle_auto_compact();
    if result.is_ok() {
        return Err(format!(
            "first idle compaction should fail without mock responses: result={result:?}"
        )
        .into());
    }
    std::fs::remove_dir(workspace.path().join("memory/history.jsonl"))?;
    client.push_response(LlmResponse {
        content: Some("first summary".to_owned()),
        ..LlmResponse::default()
    })?;
    client.push_response(LlmResponse {
        content: Some("second summary".to_owned()),
        ..LlmResponse::default()
    })?;
    let retried = loop_runtime.run_idle_auto_compact()?;
    if retried.len() != 2 {
        return Err(format!(
            "failed batch should release all archiving markers for retry: retried={retried:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_rejects_duplicate_active_turn_for_same_session() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("cli:direct")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result = loop_runtime.process_direct("hello", Some("cli:direct"));
    if !matches!(
        result,
        Err(AgentLoopError::DuplicateActiveTurn { ref session_key }) if session_key == "cli:direct"
    ) {
        return Err(format!("duplicate active turn should fail: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_priority_status_bypasses_active_session_lock() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let mut sessions = SessionManager::new(workspace.path())?;
    let mut active_session = Session::new("telegram:chat-1");
    active_session
        .metadata
        .insert("pending_user_turn".to_owned(), json!(true));
    sessions.save(&active_session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        sessions,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result = loop_runtime.process_message(InboundMessage::new(
        "telegram", "user-1", "chat-1", "/status",
    ))?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::Status));
    assert_eq!(result.stop_reason, "status");
    let raw = loop_runtime
        .session_manager()
        .read_session_file("telegram:chat-1")
        .ok_or("missing active session")?;
    assert_eq!(raw["metadata"]["pending_user_turn"], true);
    assert_eq!(raw["messages"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn loop_new_command_recovers_from_stopped_state() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    let _ = bus.consume_outbound().ok_or("missing stop outbound")?;
    let result = loop_runtime.process_direct("/new", Some("cli:direct"))?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::NewSession));
    assert_eq!(result.stop_reason, "new_session");
    Ok(())
}

#[test]
fn loop_exact_commands_do_not_bypass_active_session_lock() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result =
        loop_runtime.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "/new"));
    assert!(matches!(
        result,
        Err(AgentLoopError::DuplicateActiveTurn { ref session_key }) if session_key == "telegram:chat-1"
    ));
    Ok(())
}

#[test]
fn loop_observes_registered_cancellation_token_before_provider_call() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let register_result =
        loop_task_registry.register(ActiveLoopTask::new("cli:cancelled", "task-1", cancellation));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    let result = loop_runtime.process_direct("hello", Some("cli:cancelled"))?;
    if result.stop_reason != "cancelled"
        || result.final_content.as_deref() != Some("Turn cancelled before completion.")
        || !client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    {
        return Err(format!("cancelled turn drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_preserves_channel_chat_and_session_key_in_tool_context() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("inspect"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        safe_bypass_agent_loop_config(workspace.path()),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    let mut inbound = InboundMessage::new("telegram", "user-1", "chat-1", "go");
    inbound.session_key_override = Some("thread-42".to_owned());

    loop_runtime.process_message(inbound)?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let request = captured.first().ok_or("spawn was not called")?;
    if request.origin_channel != "telegram"
        || request.origin_chat_id != "chat-1"
        || request.session_key != "thread-42"
    {
        return Err(format!("tool context drifted: {request:?}").into());
    }
    Ok(())
}

#[test]
fn loop_explicit_session_override_wins_over_unified_session() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("ok".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.unified_session_key = Some("unified".to_owned());
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );
    let mut inbound = InboundMessage::new("slack", "user-1", "chat-1", "hello");
    inbound.session_key_override = Some("slack:thread-1".to_owned());

    let result = loop_runtime.process_message(inbound)?;
    if result.session_key != "slack:thread-1"
        || loop_runtime
            .session_manager()
            .read_session_file("unified")
            .is_some()
    {
        return Err(format!("session override precedence drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_deserialized_session_override_is_ignored() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("ok".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let inbound: InboundMessage = serde_json::from_value(json!({
        "channel": "slack",
        "sender_id": "user-1",
        "chat_id": "chat-1",
        "content": "hello",
        "session_key_override": "attacker:chosen"
    }))?;

    let result = loop_runtime.process_message(inbound)?;
    if result.session_key != "slack:chat-1"
        || loop_runtime
            .session_manager()
            .read_session_file("attacker:chosen")
            .is_some()
    {
        return Err(format!("deserialized override was trusted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_message_tool_delivery_suppresses_final_and_blocks_cross_target(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let message_tool = MessageTool::new(workspace.path());
    let mut registry = ToolRegistry::new();
    registry.register(message_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "msg-1",
                "message",
                Map::from_iter([("content".to_owned(), json!("tool says hi"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("final should be suppressed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        safe_bypass_agent_loop_config(workspace.path()),
    )
    .with_message_tool_delivery(message_tool);

    let result =
        loop_runtime.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
    if result.outbound_count != 0 {
        return Err(format!("final outbound should be suppressed: {result:?}").into());
    }
    let outbound = bus
        .consume_outbound()
        .ok_or("missing message tool outbound")?;
    if outbound.content != "tool says hi" || bus.consume_outbound().is_some() {
        return Err(format!("message tool outbound drifted: {outbound:?}").into());
    }

    let multi_bus = MessageBus::new();
    let multi_tool = MessageTool::new(workspace.path());
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut multi_registry = ToolRegistry::new();
    multi_registry.register(multi_tool.clone());
    multi_registry.register(spawn_tool.clone());
    let multi_client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "msg-3",
                "message",
                Map::from_iter([("content".to_owned(), json!("first iteration message"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-after-message",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("continue"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("multi-iteration final should be suppressed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut multi_loop = AgentLoop::new(
        multi_bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &multi_registry,
        &multi_client,
        safe_bypass_agent_loop_config(workspace.path()),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool))
    .with_message_tool_delivery(multi_tool);
    let multi_result =
        multi_loop.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
    if multi_result.outbound_count != 0 {
        return Err(format!(
            "multi-iteration final outbound should be suppressed: {multi_result:?}"
        )
        .into());
    }
    let multi_outbound = multi_bus
        .consume_outbound()
        .ok_or("missing multi-iteration message outbound")?;
    if multi_outbound.content != "first iteration message" || multi_bus.consume_outbound().is_some()
    {
        return Err(
            format!("multi-iteration message suppression drifted: {multi_outbound:?}").into(),
        );
    }

    let guarded_bus = MessageBus::new();
    let guarded_tool = MessageTool::new(workspace.path());
    let mut guarded_registry = ToolRegistry::new();
    guarded_registry.register(guarded_tool.clone());
    let guarded_client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "msg-2",
                "message",
                Map::from_iter([
                    ("content".to_owned(), json!("wrong target")),
                    ("chat_id".to_owned(), json!("other-chat")),
                ]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("guarded final".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut guarded_loop = AgentLoop::new(
        guarded_bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &guarded_registry,
        &guarded_client,
        safe_bypass_agent_loop_config(workspace.path()),
    )
    .with_message_tool_delivery(guarded_tool);
    guarded_loop.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
    let guarded_outbound = guarded_bus
        .consume_outbound()
        .ok_or("missing guarded final outbound")?;
    if guarded_outbound.content != "guarded final" || guarded_bus.consume_outbound().is_some() {
        return Err(format!("cross-target guard drifted: {guarded_outbound:?}").into());
    }
    Ok(())
}

#[test]
fn loop_message_tool_delivery_media_validation_allows_media_roots_and_rejects_outside_paths(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let outside_root = tempfile::tempdir()?;
    let allowed_file = media_root.path().join("allowed.txt");
    let outside_file = outside_root.path().join("outside.txt");
    std::fs::write(&allowed_file, b"allowed")?;
    std::fs::write(&outside_file, b"outside")?;

    let run_case = |media: Vec<String>,
                    final_content: &'static str|
     -> Result<(String, Vec<String>), Box<dyn Error>> {
        let bus = MessageBus::new();
        let message_tool = MessageTool::new(workspace.path());
        let mut registry = ToolRegistry::new();
        registry.register(message_tool.clone());
        let client = MockProvider::new(vec![
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "msg-media",
                    "message",
                    Map::from_iter([
                        ("content".to_owned(), json!("tool delivery")),
                        ("media".to_owned(), json!(media)),
                    ]),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some(final_content.to_owned()),
                ..LlmResponse::default()
            },
        ]);
        let mut config = safe_bypass_agent_loop_config(workspace.path());
        config.media_roots = vec![media_root.path().to_path_buf()];
        let mut loop_runtime = AgentLoop::new(
            bus.clone(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &registry,
            &client,
            config,
        )
        .with_message_tool_delivery(message_tool);

        loop_runtime.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
        let outbound = bus.consume_outbound().ok_or("missing outbound")?;
        if bus.consume_outbound().is_some() {
            return Err("unexpected extra outbound".into());
        }
        Ok((outbound.content, outbound.media))
    };

    let allowed_media = allowed_file.canonicalize()?.to_string_lossy().into_owned();
    let (allowed_content, allowed_files) = run_case(vec![allowed_media.clone()], "allowed final")?;
    if allowed_content != "tool delivery" || allowed_files != vec![allowed_media] {
        return Err(format!(
            "allowed media delivery drifted: {allowed_content:?} {allowed_files:?}"
        )
        .into());
    }

    let remote = run_case(
        vec!["https://example.invalid/media.png".to_owned()],
        "remote final",
    )?;
    if remote.0 != "remote final" || !remote.1.is_empty() {
        return Err(format!("remote media was not rejected: {remote:?}").into());
    }

    let outside_media = outside_file.canonicalize()?.to_string_lossy().into_owned();
    let outside = run_case(vec![outside_media], "outside final")?;
    if outside.0 != "outside final" || !outside.1.is_empty() {
        return Err(format!("outside media was not rejected: {outside:?}").into());
    }

    #[cfg(unix)]
    {
        let symlink_path = media_root.path().join("symlink.txt");
        std::os::unix::fs::symlink(&allowed_file, &symlink_path)?;
        let symlink_media = symlink_path.to_string_lossy().into_owned();
        let symlink = run_case(vec![symlink_media], "symlink final")?;
        if symlink.0 != "symlink final" || !symlink.1.is_empty() {
            return Err(format!("symlink media was not rejected: {symlink:?}").into());
        }

        let real_parent = media_root.path().join("real-parent");
        std::fs::create_dir_all(&real_parent)?;
        let nested_file = real_parent.join("nested.txt");
        std::fs::write(&nested_file, b"nested")?;
        let parent_symlink = media_root.path().join("parent-symlink");
        std::os::unix::fs::symlink(&real_parent, &parent_symlink)?;
        let parent_symlink_media = parent_symlink
            .join("nested.txt")
            .to_string_lossy()
            .into_owned();
        let parent_symlink = run_case(vec![parent_symlink_media], "parent symlink final")?;
        if parent_symlink.0 != "parent symlink final" || !parent_symlink.1.is_empty() {
            return Err(
                format!("parent symlink media was not rejected: {parent_symlink:?}").into(),
            );
        }
    }

    Ok(())
}

#[test]
fn loop_checkpoint_callback_persists_during_tool_execution_and_success_clears(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-checkpoint",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("checkpoint"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));

    loop_runtime.process_direct("go", Some("cli:checkpoint-callback"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:checkpoint-callback")
        .ok_or("missing checkpoint callback session")?;
    if raw["metadata"].get("runtime_checkpoint").is_some()
        || raw["metadata"].get("pending_user_turn").is_some()
        || !raw["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| {
                message["role"] == "tool" && message["tool_call_id"] == "spawn-checkpoint"
            })
    {
        return Err(format!("successful run did not clear checkpoint markers: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_ask_user_interrupt_checkpoint_materializes_pending_placeholder(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:ask-checkpoint");
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({
            "phase": "awaiting_tools",
            "assistant_message": {
                "role": "assistant",
                "content": "need input",
                "tool_calls": [
                    {"id": "ask-crash", "type": "function", "function": {"name": "ask_user", "arguments": "{\"question\":\"Continue?\"}"}}
                ]
            },
            "completed_tool_results": [],
            "pending_tool_calls": [
                {"id": "ask-crash", "type": "function", "function": {"name": "ask_user", "arguments": "{\"question\":\"Continue?\"}"}}
            ]
        }),
    );
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/status", Some("cli:ask-checkpoint"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:ask-checkpoint")
        .ok_or("missing ask checkpoint session")?;
    let messages = raw["messages"]
        .as_array()
        .ok_or("messages should be array")?;
    if messages.len() != 2
        || messages[0]["tool_calls"][0]["function"]["name"] != "ask_user"
        || messages[1]["role"] != "tool"
        || messages[1]["tool_call_id"] != "ask-crash"
        || !messages[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("interrupted or lost")
    {
        return Err(format!("ask checkpoint placeholder drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_run_until_idle_dispatches_bus_messages_and_drains_same_session_injection(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("first"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("after injection".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("other session".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-1", "start"));
    bus.publish_inbound(InboundMessage::new(
        "telegram",
        "user-1",
        "chat-1",
        "follow-up",
    ));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-2", "other"));

    let summary = loop_runtime.run_until_idle(1)?;
    if summary.processed != 1
        || summary.results.first().map(|result| result.had_injections) != Some(true)
        || bus.inbound_size() != 1
        || !loop_runtime.active_session_keys().is_empty()
    {
        return Err(format!("dispatcher/injection summary drifted: {summary:?}").into());
    }
    let first_outbound = bus.consume_outbound().ok_or("missing first outbound")?;
    if first_outbound.content != "after injection" {
        return Err(format!("injected turn final drifted: {first_outbound:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if !requests
        .get(1)
        .into_iter()
        .flat_map(|request| &request.messages)
        .any(|message| {
            message["role"] == "user" && message["content"].to_string().contains("follow-up")
        })
    {
        return Err(format!("same-session follow-up was not injected: {requests:?}").into());
    }
    drop(requests);

    let second = loop_runtime
        .process_next_inbound()?
        .ok_or("missing deferred other session")?;
    if second.session_key != "telegram:chat-2"
        || second.final_content.as_deref() != Some("other session")
    {
        return Err(format!("deferred other session drifted: {second:?}").into());
    }
    Ok(())
}

#[test]
fn loop_mid_turn_injection_preserves_bus_fifo_after_limit() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("first"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("after injections".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-1", "start"));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "follow-1",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-2", "other-a",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "follow-2",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-3", "other-b",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "follow-3",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "/status",
    ));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-4", "tail"));

    let summary = loop_runtime.run_until_idle(1)?;
    if summary.results.first().map(|result| result.had_injections) != Some(true) {
        return Err(format!("expected injection summary: {summary:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let second_request = requests.get(1).ok_or("missing second provider request")?;
    for follow_up in ["follow-1", "follow-2", "follow-3"] {
        if !second_request.messages.iter().any(|message| {
            message["role"] == "user" && message["content"].to_string().contains(follow_up)
        }) {
            return Err(format!("missing injected follow-up {follow_up}: {requests:?}").into());
        }
    }
    drop(requests);

    let mut retained = Vec::new();
    while let Some(message) = bus.try_consume_inbound() {
        retained.push(message.content);
    }
    if retained != ["other-a", "other-b", "/status", "tail"] {
        return Err(format!("deferred bus FIFO order drifted: {retained:?}").into());
    }
    Ok(())
}

#[test]
fn loop_mid_turn_injection_uses_explicit_override_before_unified_key() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("first"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("after explicit injection".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.unified_session_key = Some("unified".to_owned());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    let mut current = InboundMessage::new("telegram", "user-1", "chat-1", "start");
    current.session_key_override = Some("explicit".to_owned());
    let mut explicit_follow_up =
        InboundMessage::new("telegram", "user-1", "chat-2", "explicit follow-up");
    explicit_follow_up.session_key_override = Some("explicit".to_owned());
    bus.publish_inbound(current);
    bus.publish_inbound(explicit_follow_up);
    bus.publish_inbound(InboundMessage::new(
        "telegram",
        "user-1",
        "chat-3",
        "unified follow-up",
    ));

    let summary = loop_runtime.run_until_idle(1)?;
    if summary
        .results
        .first()
        .map(|result| result.session_key.as_str())
        != Some("explicit")
        || summary.results.first().map(|result| result.had_injections) != Some(true)
    {
        return Err(format!("explicit override summary drifted: {summary:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let second_request = requests.get(1).ok_or("missing second provider request")?;
    if !second_request.messages.iter().any(|message| {
        message["content"]
            .to_string()
            .contains("explicit follow-up")
    }) {
        return Err(format!("explicit follow-up was not injected: {requests:?}").into());
    }
    drop(requests);
    let retained = bus
        .try_consume_inbound()
        .ok_or("missing unified follow-up")?;
    if retained.content != "unified follow-up" || bus.inbound_size() != 0 {
        return Err(format!("unified message should remain deferred: {retained:?}").into());
    }
    Ok(())
}

#[test]
fn loop_forwards_tool_and_provider_progress_callbacks() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    let client = StreamMockProvider::new(
        vec![LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "ask-progress",
                "ask_user",
                Map::from_iter([("question".to_owned(), json!("Continue?"))]),
            )],
            ..LlmResponse::default()
        }],
        vec![ProviderEvent::TextDelta {
            text: "thinking".to_owned(),
        }],
    );
    let provider_events = Arc::new(Mutex::new(Vec::<ProviderEvent>::new()));
    let provider_events_clone = provider_events.clone();
    let tool_events = Arc::new(Mutex::new(Vec::<ToolStatus>::new()));
    let tool_events_clone = tool_events.clone();
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = provider_events_clone.lock() {
            events.push(event.clone());
        }
    }))
    .with_tool_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = tool_events_clone.lock() {
            events.push(event.status.clone());
        }
    }));

    let result = loop_runtime.process_direct("start", Some("cli:progress"))?;
    let provider_events = provider_events.lock().map_err(|error| error.to_string())?;
    let tool_events = tool_events.lock().map_err(|error| error.to_string())?;
    if result.stop_reason != "ask_user"
        || provider_events.first()
            != Some(&ProviderEvent::TextDelta {
                text: "thinking".to_owned(),
            })
        || !tool_events.contains(&ToolStatus::Waiting)
    {
        return Err(format!(
            "progress callbacks drifted: result={result:?} provider={provider_events:?} tool={tool_events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_does_not_persist_provider_stream_delta_as_session_content() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = StreamMockProvider::new(
        vec![LlmResponse {
            content: Some("FINAL_PROVIDER_OUTPUT".to_owned()),
            ..LlmResponse::default()
        }],
        vec![ProviderEvent::TextDelta {
            text: "STREAM_DELTA_SHOULD_NOT_PERSIST".to_owned(),
        }],
    );
    let provider_events = Arc::new(Mutex::new(Vec::<ProviderEvent>::new()));
    let provider_events_clone = provider_events.clone();
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = provider_events_clone.lock() {
            events.push(event.clone());
        }
    }));

    let result = loop_runtime.process_direct("stream please", Some("cli:provider-stream"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:provider-stream")
        .ok_or("missing session")?;
    let raw_text = raw.to_string();
    let provider_events = provider_events.lock().map_err(|error| error.to_string())?;
    if result.final_content.as_deref() != Some("FINAL_PROVIDER_OUTPUT")
        || provider_events.first()
            != Some(&ProviderEvent::TextDelta {
                text: "STREAM_DELTA_SHOULD_NOT_PERSIST".to_owned(),
            })
        || raw_text.contains("STREAM_DELTA_SHOULD_NOT_PERSIST")
        || !raw_text.contains("FINAL_PROVIDER_OUTPUT")
    {
        return Err(format!(
            "provider stream delta should stay observational: result={result:?} events={provider_events:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_provider_error_publishes_error_and_clears_runtime_markers() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("fail provider", Some("cli:provider-error"))?;
    let outbound = bus
        .consume_outbound()
        .ok_or("missing provider error outbound")?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:provider-error")
        .ok_or("missing session")?;
    if result.stop_reason != "error"
        || !result
            .final_content
            .as_deref()
            .unwrap_or_default()
            .contains("no mock response")
        || outbound.metadata["stop_reason"] != "error"
        || !outbound.content.contains("no mock response")
        || raw["metadata"].get("pending_user_turn").is_some()
        || raw["metadata"].get("runtime_checkpoint").is_some()
        || raw["messages"].as_array().map(Vec::len) != Some(2)
        || raw["messages"][1]["role"] != "assistant"
        || !raw["messages"][1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("no mock response")
    {
        return Err(format!(
            "provider error should be session-visible only through runtime boundary: result={result:?} outbound={outbound:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn prd010_frozen_session_snapshot_digest_survives_later_session_changes(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("session-prd010");
    session.add_message("user", "approval evidence is here", Map::new());
    manager.save(&session)?;

    let snapshot =
        freeze_session_search_snapshot(&manager, "session-prd010", "approval", "snapshot-prd010")?;

    session.add_message("assistant", "approval evidence changed later", Map::new());
    manager.save(&session)?;

    if snapshot.matched_event_refs.len() != 1 || snapshot.result_digest.len() != 64 {
        return Err(format!("frozen snapshot changed unexpectedly: {snapshot:?}").into());
    }
    Ok(())
}

#[test]
fn prd010_skill_list_view_and_reference_use_progressive_disclosure() -> Result<(), Box<dyn Error>> {
    let registry = test_skill_registry(SkillSourceKind::WorkspaceLocal, Some("api_key=sk-secret"));

    let list = runtime_skill_list_disclosure(&registry);
    let list_json = serde_json::to_value(&list)?;
    if list.len() != 1
        || list_json.to_string().contains("sk-secret")
        || list_json.to_string().contains("redacted_body")
    {
        return Err(format!("skill list leaked raw body: {list_json}").into());
    }

    if runtime_skill_view_disclosure(&registry, "prd010-skill", false).is_ok() {
        return Err("skill view should require an explicit request".into());
    }
    let view = runtime_skill_view_disclosure(&registry, "prd010-skill", true)?;
    if view
        .redacted_body
        .as_deref()
        .unwrap_or_default()
        .contains("sk-secret")
        || view.body_digest.is_none()
    {
        return Err(format!("skill view did not redact body: {view:?}").into());
    }

    let reference = runtime_skill_reference_evidence(&registry, "prd010-skill")?;
    if reference.redacted_body.is_some()
        || reference
            .evidence_ref
            .as_ref()
            .map(|evidence| evidence.digest.as_str())
            != Some("skill-digest-prd010")
    {
        return Err(format!("skill reference carried wrong evidence: {reference:?}").into());
    }
    Ok(())
}

#[test]
fn prd010_authored_skill_requires_dry_run_and_approval_before_active() -> Result<(), Box<dyn Error>>
{
    let mut lifecycle = shacs_eval::evaluator::authored_skill_lifecycle_draft(
        "authored-prd010",
        vec![runtime_eval_evidence()],
    );

    lifecycle.state = AuthoredSkillLifecycleState::DryRunPending;
    lifecycle.dry_run_passed = true;
    if authored_skill_ready_for_active_registry(&lifecycle) {
        return Err("dry run alone should not allow active registry promotion".into());
    }

    lifecycle.state = AuthoredSkillLifecycleState::ApprovalPending;
    lifecycle.approval_granted = true;
    if authored_skill_ready_for_active_registry(&lifecycle) {
        return Err("approval pending should not be active before active_candidate".into());
    }

    lifecycle.state = AuthoredSkillLifecycleState::ActiveCandidate;
    if !authored_skill_ready_for_active_registry(&lifecycle) {
        return Err("active_candidate with dry run and approval should be promotable".into());
    }
    Ok(())
}

#[test]
fn prd010_curator_proposal_records_without_owner_mutation() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = shacs_core::runtime::MemoryStore::new(workspace.path())?;
    store.append_history("duplicate memory", None)?;
    let registry = test_skill_registry(SkillSourceKind::WorkspaceLocal, Some("body"));

    let before_memory_entries = store.read_entries().len();
    let before_skill_status = registry.find("prd010-skill").ok_or("missing skill")?.status;
    let proposal = runtime_curator_proposal_record(
        "proposal-prd010",
        CuratorTargetKind::Memory,
        vec![runtime_eval_evidence()],
        "duplicate memory",
        vec![runtime_eval_evidence()],
        CuratorActionProposed::DeleteMemory,
        Some(runtime_eval_evidence()),
    );

    if proposal.final_status != CuratorProposalFinalStatus::ApprovalPending
        || store.read_entries().len() != before_memory_entries
        || registry.find("prd010-skill").ok_or("missing skill")?.status != before_skill_status
    {
        return Err(format!("curator proposal mutated owners: {proposal:?}").into());
    }
    Ok(())
}

#[test]
fn prd010_memory_evidence_is_bounded_and_lineaged() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let store = shacs_core::runtime::MemoryStore::new(workspace.path())?;
    store.append_history("alpha relevant memory", None)?;
    store.append_history("alpha second memory", None)?;

    let request = runtime_memory_evidence_request(RuntimeMemoryEvidenceRequestInput {
        request_id: "request-prd010".to_owned(),
        session_id: "session-prd010".to_owned(),
        query: "alpha".to_owned(),
        evaluator_kind: EvaluatorKind::GoalCompletion,
        max_result_refs: 1,
        cutoff: "9999-12-31T23:59:59Z".to_owned(),
        redaction_profile: "default".to_owned(),
        caller_reason: "goal completion evidence".to_owned(),
    });
    let evidence = build_runtime_memory_evidence(&store, &request)?;

    if evidence.request_id != "request-prd010"
        || evidence.query != "[redacted]"
        || evidence.result_count != 1
        || evidence.omitted_count != 1
        || evidence.omitted_reason != Some(MemoryEvidenceOmittedReason::OmittedByBudget)
        || evidence.result_digest.len() != 64
    {
        return Err(format!("memory evidence was not bounded/lineaged: {evidence:?}").into());
    }
    Ok(())
}

#[test]
fn prd010_memory_evidence_uses_cutoff_and_relevance_omission_reasons() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let store = shacs_core::runtime::MemoryStore::new(workspace.path())?;
    store.append_history("alpha relevant memory", None)?;
    store.append_history("beta unrelated memory", None)?;

    let cutoff_request = runtime_memory_evidence_request(RuntimeMemoryEvidenceRequestInput {
        request_id: "request-cutoff".to_owned(),
        session_id: "session-prd010".to_owned(),
        query: "alpha".to_owned(),
        evaluator_kind: EvaluatorKind::GoalCompletion,
        max_result_refs: 10,
        cutoff: "0000".to_owned(),
        redaction_profile: "default".to_owned(),
        caller_reason: "contains raw caller secret sk-test".to_owned(),
    });
    let cutoff = build_runtime_memory_evidence(&store, &cutoff_request)?;
    assert_eq!(
        cutoff.omitted_reason,
        Some(MemoryEvidenceOmittedReason::OmittedByCutoff)
    );
    assert_eq!(cutoff.result_count, 0);

    let relevance_request = runtime_memory_evidence_request(RuntimeMemoryEvidenceRequestInput {
        request_id: "request-relevance".to_owned(),
        session_id: "session-prd010".to_owned(),
        query: "alpha".to_owned(),
        evaluator_kind: EvaluatorKind::GoalCompletion,
        max_result_refs: 10,
        cutoff: "9999-12-31T23:59:59Z".to_owned(),
        redaction_profile: "default".to_owned(),
        caller_reason: "contains raw caller secret sk-test".to_owned(),
    });
    let relevance = build_runtime_memory_evidence(&store, &relevance_request)?;
    let serialized = serde_json::to_string(&relevance)?;
    assert_eq!(
        relevance.omitted_reason,
        Some(MemoryEvidenceOmittedReason::OmittedByRelevance)
    );
    assert!(!serialized.contains("sk-test"));
    assert!(!serialized.contains("alpha"));

    Ok(())
}

#[test]
fn prd010_app_provided_skill_reference_requires_manifest_and_task_boundary(
) -> Result<(), Box<dyn Error>> {
    let registry = test_skill_registry(SkillSourceKind::PluginProvided, Some("plugin body"));

    if app_provided_skill_reference_evidence(&registry, "prd010-skill", None, None).is_ok() {
        return Err("app provided skill ref should require manifest evidence".into());
    }
    if app_provided_skill_reference_evidence(
        &registry,
        "prd010-skill",
        Some(runtime_eval_evidence()),
        None,
    )
    .is_ok()
    {
        return Err("app provided skill ref should require task boundary evidence".into());
    }
    let manifest_ref = EvidenceRef {
        id: "app-manifest-1".to_owned(),
        digest: "app-manifest-digest-1".to_owned(),
        summary: "app manifest evidence".to_owned(),
        ..runtime_eval_evidence()
    };
    let task_boundary_ref = EvidenceRef {
        id: "app-task-boundary-1".to_owned(),
        digest: "app-task-boundary-digest-1".to_owned(),
        summary: "app task boundary evidence".to_owned(),
        ..runtime_eval_evidence()
    };
    let reference = app_provided_skill_reference_evidence(
        &registry,
        "prd010-skill",
        Some(manifest_ref.clone()),
        Some(task_boundary_ref.clone()),
    )?;

    if reference.evidence_ref.is_none()
        || reference.redacted_body.is_some()
        || reference.app_manifest_ref != Some(manifest_ref)
        || reference.app_task_boundary_ref != Some(task_boundary_ref)
    {
        return Err(
            format!("app skill reference lost lineage or leaked body: {reference:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn prd011_pre_approval_proposal_is_runtime_inert() -> Result<(), Box<dyn Error>> {
    let proposed = prd011_improvement_proposal(ImprovementProposalStatus::Proposed);
    let approval_pending = prd011_improvement_proposal(ImprovementProposalStatus::ApprovalPending);

    if !runtime_improvement_proposal_behavior_inert(&proposed)
        || !runtime_improvement_proposal_behavior_inert(&approval_pending)
    {
        return Err("pre-approval proposals must not affect runtime behavior".into());
    }
    Ok(())
}

#[test]
fn prd011_missing_checkpoint_blocks_apply_without_apply_record() -> Result<(), Box<dyn Error>> {
    let proposal = prd011_improvement_proposal(ImprovementProposalStatus::Checkpointed);
    let approval = prd011_improvement_approval(vec!["tool:memory.search".to_owned()]);

    let readiness = runtime_improvement_apply_readiness(
        &proposal,
        Some(&approval),
        None,
        None,
        20,
        Some(runtime_eval_evidence()),
    );
    let apply_record: Option<ImprovementApplyRecord> = None;

    if readiness.ready
        || readiness.status != ImprovementProposalStatus::BlockedCheckpointUnavailable
        || apply_record.is_some()
    {
        return Err(format!("missing checkpoint should block apply: {readiness:?}").into());
    }
    Ok(())
}

#[test]
fn prd011_expired_approval_blocks_apply_readiness() -> Result<(), Box<dyn Error>> {
    let proposal = prd011_improvement_proposal(ImprovementProposalStatus::Checkpointed);
    let approval = prd011_improvement_approval(vec!["tool:memory.search".to_owned()]);
    let checkpoint = prd011_improvement_checkpoint(true);
    let gate = prd011_checkpoint_gate();

    let readiness = runtime_improvement_apply_readiness(
        &proposal,
        Some(&approval),
        Some(&checkpoint),
        Some(&gate),
        101,
        Some(runtime_eval_evidence()),
    );

    if readiness.ready || readiness.status != ImprovementProposalStatus::BlockedApprovalRequired {
        return Err(format!("expired approval should block apply: {readiness:?}").into());
    }
    Ok(())
}

#[test]
fn prd011_app_task_can_create_but_not_approve_apply_or_rollback() -> Result<(), Box<dyn Error>> {
    if !shacs_eval::evaluator::app_task_improvement_authority(
        &ImprovementActorAuthority::AppTask,
        &ImprovementAuthorityAction::CreateProposal,
    ) || shacs_eval::evaluator::app_task_improvement_authority(
        &ImprovementActorAuthority::AppTask,
        &ImprovementAuthorityAction::Approve,
    ) || shacs_eval::evaluator::app_task_improvement_authority(
        &ImprovementActorAuthority::AppTask,
        &ImprovementAuthorityAction::Apply,
    ) || shacs_eval::evaluator::app_task_improvement_authority(
        &ImprovementActorAuthority::AppTask,
        &ImprovementAuthorityAction::Rollback,
    ) {
        return Err("app task authority must stop at proposal creation".into());
    }
    Ok(())
}

#[test]
fn prd011_mcp_exposure_default_deny_and_scope_only_widening() -> Result<(), Box<dyn Error>> {
    let proposal = prd011_improvement_proposal(ImprovementProposalStatus::Checkpointed);
    let no_approval = runtime_mcp_exposure_projection(
        "tool:memory.search",
        "session",
        "deny",
        Some(&proposal),
        None,
        20,
    );
    if shacs_eval::evaluator::mcp_exposure_can_widen(&no_approval) {
        return Err("mcp exposure widened without approval".into());
    }

    let wildcard_approval = prd011_improvement_approval(vec!["*".to_owned()]);
    let wildcard = runtime_mcp_exposure_projection(
        "tool:memory.search",
        "session",
        "deny",
        Some(&proposal),
        Some(&wildcard_approval),
        20,
    );
    if shacs_eval::evaluator::mcp_exposure_can_widen(&wildcard) {
        return Err("wildcard scope must not widen without explicit matching approval".into());
    }

    let broad_kind_approval = prd011_improvement_approval(vec!["tool_exposure".to_owned()]);
    let broad = runtime_mcp_exposure_projection(
        "tool:memory.search",
        "session",
        "deny",
        Some(&proposal),
        Some(&broad_kind_approval),
        20,
    );
    if shacs_eval::evaluator::mcp_exposure_can_widen(&broad) || broad.approval_ref.is_some() {
        return Err("target kind approval must not widen a specific MCP target_ref".into());
    }

    let exact_approval = prd011_improvement_approval(vec!["tool:memory.search".to_owned()]);
    let exact = runtime_mcp_exposure_projection(
        "tool:memory.search",
        "session",
        "deny",
        Some(&proposal),
        Some(&exact_approval),
        20,
    );
    if !shacs_eval::evaluator::mcp_exposure_can_widen(&exact)
        || exact.approval_ref.as_ref() != Some(&exact_approval.decision_ref)
    {
        return Err(format!("exact approved scope should widen only itself: {exact:?}").into());
    }

    let expired = runtime_mcp_exposure_projection(
        "tool:memory.search",
        "session",
        "deny",
        Some(&proposal),
        Some(&exact_approval),
        101,
    );
    if shacs_eval::evaluator::mcp_exposure_can_widen(&expired) || expired.approval_ref.is_some() {
        return Err("expired exact approval must not widen MCP exposure".into());
    }
    Ok(())
}

#[test]
fn prd011_apply_verify_and_rollback_records_preserve_lineage() -> Result<(), Box<dyn Error>> {
    let proposal = prd011_improvement_proposal(ImprovementProposalStatus::Checkpointed);
    let approval = prd011_improvement_approval(vec!["tool:memory.search".to_owned()]);
    let checkpoint = prd011_improvement_checkpoint(true);
    let gate = prd011_checkpoint_gate();

    let readiness = runtime_improvement_apply_readiness(
        &proposal,
        Some(&approval),
        Some(&checkpoint),
        Some(&gate),
        20,
        Some(runtime_eval_evidence()),
    );
    if !readiness.ready {
        return Err(
            format!("approved checkpointed proposal should be ready: {readiness:?}").into(),
        );
    }

    let owner_apply_ref = OwnerPrimitiveRef {
        owner_spec: "010-host-safety-permissions".to_owned(),
        primitive_ref: "owner-primitive://tool-exposure/apply".to_owned(),
    };
    let apply_input = json!({
        "approval_ref": approval.decision_ref.decision_id.clone(),
        "proposal_id": proposal.proposal_id.clone(),
        "target": proposal.target_kind.clone(),
    });
    let apply_record = runtime_improvement_apply_record(
        "apply-prd011",
        &proposal,
        owner_apply_ref.clone(),
        &apply_input,
        prd011_evidence(EvidenceKind::ImprovementApplyRecord, "apply-outcome"),
    )?;
    if apply_record.action_ref != owner_apply_ref
        || apply_record.input_digest != shacs_eval::evaluator::stable_sha256_digest(&apply_input)?
        || runtime_improvement_status_after_apply_record()
            != ImprovementProposalStatus::AppliedUnverified
    {
        return Err(format!("apply record lost owner/input lineage: {apply_record:?}").into());
    }

    let verification = runtime_improvement_verification_record(
        "verify-prd011",
        &proposal,
        "tool exposure remains scoped to the approved session resource",
        prd011_evidence(EvidenceKind::ImprovementVerification, "verify-failure"),
        false,
        Some(&checkpoint),
        true,
    );
    if verification.next_action != ImprovementVerificationNextAction::Rollback
        || verification.correlation_id != proposal.correlation_id
    {
        return Err(
            format!("verification did not preserve rollback lineage: {verification:?}").into(),
        );
    }

    let owner_rollback_ref = checkpoint
        .rollback_capability
        .clone()
        .ok_or("missing rollback capability")?;
    let rollback = runtime_improvement_rollback_projection(
        "rollback-prd011",
        &proposal,
        Some(&checkpoint),
        &verification,
        Some(owner_rollback_ref.clone()),
        "restore the checkpoint through the owner primitive",
    );
    let rollback_record = rollback
        .rollback_record
        .as_ref()
        .ok_or("missing rollback record")?;
    if rollback.status != ImprovementProposalStatus::RolledBack
        || rollback_record.owner_rollback_ref != Some(owner_rollback_ref)
        || rollback_record.verify_failure_ref != verification.observed_result_ref
        || rollback_record.checkpoint_ref != checkpoint.checkpoint_ref
        || rollback_record.result != ImprovementRollbackResult::RolledBack
        || approval.decision_ref.decision != ApprovalDecisionKind::Approved
    {
        return Err(
            format!("rollback record lost approval/owner lineage: {rollback_record:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn prd011_rollback_unavailable_projects_manual_recovery() -> Result<(), Box<dyn Error>> {
    let proposal = prd011_improvement_proposal(ImprovementProposalStatus::Checkpointed);
    let checkpoint = prd011_improvement_checkpoint(false);
    let verification = runtime_improvement_verification_record(
        "verify-prd011",
        &proposal,
        "tool exposure remains scoped",
        prd011_evidence(EvidenceKind::ImprovementVerification, "verify-failure"),
        false,
        Some(&checkpoint),
        false,
    );

    let rollback = runtime_improvement_rollback_projection(
        "rollback-prd011",
        &proposal,
        Some(&checkpoint),
        &verification,
        None,
        "manually restore checkpoint-prd011 via the tool exposure owner",
    );
    let rollback_record = rollback
        .rollback_record
        .as_ref()
        .ok_or("missing rollback record")?;

    if rollback.status != ImprovementProposalStatus::BlockedRollbackUnavailable
        || rollback.manual_recovery_hint.as_deref()
            != Some("manually restore checkpoint-prd011 via the tool exposure owner")
        || rollback_record.result != ImprovementRollbackResult::BlockedManualRecoveryRequired
        || rollback_record.owner_rollback_ref.is_some()
    {
        return Err(
            format!("rollback unavailable should carry manual recovery: {rollback:?}").into(),
        );
    }
    Ok(())
}

fn prd011_improvement_proposal(status: ImprovementProposalStatus) -> ImprovementProposal {
    ImprovementProposal {
        proposal_id: "proposal-prd011".to_owned(),
        target_kind: "tool_exposure".to_owned(),
        target_ref: Some("tool:memory.search".to_owned()),
        target_source: Some("app_task".to_owned()),
        proposed_diff_summary_ref: prd011_evidence(EvidenceKind::ImprovementProposal, "diff"),
        risk_summary: "widens one MCP tool exposure scope".to_owned(),
        evidence_refs: vec![runtime_eval_evidence()],
        expected_benefit: "allows approved local tool use".to_owned(),
        rollback_plan: "restore owner checkpoint through rollback primitive".to_owned(),
        status,
        correlation_id: "corr-prd011".to_owned(),
    }
}

fn prd011_improvement_approval(approved_scope: Vec<String>) -> ImprovementApproval {
    ImprovementApproval {
        proposal_id: "proposal-prd011".to_owned(),
        request_ref: ApprovalRequestRef {
            request_id: "approval-request-prd011".to_owned(),
            action_digest: "action-digest-prd011".to_owned(),
            snapshot_digest: "snapshot-digest-prd011".to_owned(),
            created_at_ms: 10,
            expires_at_ms: 100,
            displayed_risk_summary: "widens one MCP tool exposure scope".to_owned(),
            status: ApprovalRequestStatus::Approved,
            correlation_id: "corr-prd011".to_owned(),
        },
        decision_ref: ApprovalDecisionRef {
            decision_id: "approval-decision-prd011".to_owned(),
            request_id: "approval-request-prd011".to_owned(),
            action_digest: "action-digest-prd011".to_owned(),
            snapshot_digest: "snapshot-digest-prd011".to_owned(),
            decision: ApprovalDecisionKind::Approved,
            decided_at_ms: 20,
            actor_local_user: "local-user".to_owned(),
            correlation_id: "corr-prd011".to_owned(),
        },
        approved_scope,
        expires_at_ms: 100,
        actor_local_user: "local-user".to_owned(),
        correlation_id: "corr-prd011".to_owned(),
    }
}

fn prd011_improvement_checkpoint(with_rollback: bool) -> ImprovementCheckpoint {
    ImprovementCheckpoint {
        checkpoint_ref: "checkpoint-prd011".to_owned(),
        target_digest_before: "target-digest-before-prd011".to_owned(),
        inspect_ref: prd011_evidence(EvidenceKind::ImprovementCheckpoint, "checkpoint"),
        rollback_capability: with_rollback.then(|| OwnerPrimitiveRef {
            owner_spec: "010-host-safety-permissions".to_owned(),
            primitive_ref: "owner-primitive://tool-exposure/rollback".to_owned(),
        }),
        proposal_id: "proposal-prd011".to_owned(),
        correlation_id: "corr-prd011".to_owned(),
    }
}

fn prd011_checkpoint_gate() -> CheckpointGateDecision {
    CheckpointGateDecision {
        status: CheckpointGateStatus::Required,
        required: true,
        reason: "owner checkpoint is available".to_owned(),
        checkpoint_ref: Some("checkpoint-prd011".to_owned()),
    }
}

fn prd011_evidence(kind: EvidenceKind, id: &str) -> EvidenceRef {
    EvidenceRef {
        kind,
        id: format!("{id}-prd011"),
        digest: format!("{id}-digest-prd011"),
        summary: "prd011 runtime evidence".to_owned(),
        redaction_status: RedactionStatus::AlreadySafe,
        owner_spec: Some("018".to_owned()),
        locator: Some(format!("diagnostics://prd011/{id}")),
        retention_hint: Some("local".to_owned()),
    }
}

fn test_skill_registry(source_kind: SkillSourceKind, raw: Option<&str>) -> SkillRegistry {
    SkillRegistry {
        entries: vec![SkillRegistryEntry {
            descriptor: SkillDescriptor {
                name: "prd010-skill".to_owned(),
                description: Some("PRD010 skill".to_owned()),
                source_kind,
                source_path: None,
                body_hash: "skill-digest-prd010".to_owned(),
                requirements: Vec::new(),
                install_metadata: None,
            },
            status: SkillRegistryStatus::Active,
            diagnostics: Vec::new(),
            raw: raw.map(str::to_owned),
        }],
    }
}

struct StreamMockProvider {
    inner: MockProvider,
    events: Vec<ProviderEvent>,
}

impl StreamMockProvider {
    fn new(responses: Vec<LlmResponse>, events: Vec<ProviderEvent>) -> Self {
        Self {
            inner: MockProvider::new(responses),
            events,
        }
    }
}

impl ProviderClient for StreamMockProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.inner.chat(request)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        for event in &self.events {
            on_event(event.clone());
        }
        self.inner.chat(request)
    }
}

struct MockProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl MockProvider {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn push_response(&self, response: LlmResponse) -> Result<(), Box<dyn Error>> {
        self.responses
            .lock()
            .map_err(|error| error.to_string())?
            .push_back(response);
        Ok(())
    }
}

impl ProviderClient for MockProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .push(request);
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn provider_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
