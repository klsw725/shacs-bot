use serde_json::{json, Map, Value};
use sha2::Digest;
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
    runtime_spec018_local_api_projection, ActiveLoopTask, AgentHook, AgentHookContext, AgentLoop,
    AgentLoopCommandResult, AgentLoopConfig, AgentLoopError, AgentRunSpec, AgentRunner,
    AutoCompact, AutoEvaluatorVerdictKind, AutomationExecutionControl, AutomationSourceEvent,
    AutomationSourceEventKind, BridgeUnderlyingMappingEvidence, CancellationToken,
    ChildResultEnvelope, ChildResultStatus, ContainerNetworkMode, ContainerRuntimeKind,
    ContainmentSnapshotRef, ContextBuilder, DockerContainmentSnapshot, DreamLifecycle,
    DurableWorkDispatcher, EvaluatorConfidence, EvaluatorDecisionInput, EvaluatorScopeMatch,
    ExecutionOutcome, GoalCompletionVerdict, GoalEvaluatorOutcome, InboundMessage,
    LateResultDecision, LedgerConsumptionStatus, LoopTaskRegisterResult, McpLifecycle,
    MergeDecision, MessageBus, PermissionMode, PermissionModeSnapshot, PermissionPolicyReason,
    PermissionRuleInput, PermissionSecretRefEvidence, PermissionSecretRefStatus,
    PermissionedActionOrigin, PersistentGoal, PersistentGoalStatus, PolicySafetyDigest,
    PolicySafetySnapshotId, PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId, ProcExecSummary,
    ProviderHotSwapResult, ProviderSelectionSnapshot, RecentAutoModeDenial,
    RecentAutoModeDenialStore, RecentAutoModeRetryToken, RecentAutoModeRetryTokenMatch,
    RecentAutoModeRetryTokenStore, RedactedPolicySafetySummary, RuntimeCapabilityStatus,
    RuntimeContextTools, RuntimeDecisionKind, RuntimeInterrupt, RuntimeMemoryEvidenceRequestInput,
    RuntimePolicyGateResults, RuntimeReplayInput, RuntimeSelectedAction,
    RuntimeSpec018ProjectionInput, RuntimeToolCall, RuntimeToolExecutor, Session, SessionManager,
    SessionTurnAcquireError, SessionTurnLock, StaticProviderSelector, SubagentExecutionConfig,
    SubagentMergeState, SubagentOutcomeKind, SubagentProgressUpdate, SubagentRuntime,
    SubagentRuntimeConfig, SurfaceActionOutcomeKind, ToolEvent, ToolExecutionContext,
    ToolSearchConfig, ToolSearchMode, ToolSearchRuntimeInput, ToolStatus,
    GOAL_EVALUATOR_BOUNDARY_METADATA_KEY, PERSISTENT_GOAL_METADATA_KEY,
    RECENT_AUTO_MODE_DENIAL_LIMIT,
};
use shacs_core::tools::{
    assemble_tool_surface, ActivationState, AskUserTool, JsonMap, MessageTool, SchemaFragment,
    SpawnRequest, SpawnTool, Tool, ToolParameters, ToolRegistry, ToolResult,
    ToolSurfaceAssemblyInput,
};
use shacs_eval::completion_boundary::{EvaluatorRoute, OwnerResultLocator};
use shacs_eval::evaluator::{
    ApprovalDecisionKind, ApprovalDecisionRef, ApprovalRequestRef, ApprovalRequestStatus,
    AuthoredSkillLifecycleState, AutomationExecutionMode, AutomationRecursionGuard,
    AuxiliaryJudgeRole, AuxiliaryJudgeRoute, AuxiliaryJudgeRouteFinalStatus,
    CheckpointGateDecision, CheckpointGateStatus, ConfidenceBand, CuratorActionProposed,
    CuratorProposalFinalStatus, CuratorTargetKind, DeliverySeverity, EvaluatorKind,
    EvaluatorVerdictEnvelope, EvidenceKind, EvidenceRef, ImprovementActorAuthority,
    ImprovementApplyRecord, ImprovementApproval, ImprovementAuthorityAction, ImprovementCheckpoint,
    ImprovementProposal, ImprovementProposalStatus, ImprovementRollbackResult,
    ImprovementVerificationNextAction, JudgeFallbackReason, MemoryEvidenceOmittedReason,
    OwnerPrimitiveRef, ProjectionStatus, ProjectionSurface, ProviderFallbackStep,
    ProviderModelSnapshot, ProviderRouteRole, RedactionStatus, ReplayComparisonSeverity,
    ReplayComparisonStatus, ReplayDatasetItem, ReplayRunStatus, ReplaySafeMockOutcome,
    ReplayToolOutcomePolicy, SuggestedNextAction, TaskOutcomeClass, TrajectoryRecord,
    TrajectoryStats, VerdictKind,
};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest,
    ToolCallRequest,
};
use shacs_redaction::{
    RedactionEvidence, RedactionEvidenceRef, SafeSecretSummary, SecretLocator, SecretRef,
    SecretRefId, SecretRefKind, SecretSourceKind,
};
use shacs_session::durable_event::{
    DurableEventStore, SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::evaluate_durable_work_recovery;
use shacs_skills::{
    SkillDescriptor, SkillRegistry, SkillRegistryEntry, SkillRegistryStatus, SkillSourceKind,
};
use shacs_utils::gitstore::{GitCliStore, GitStore};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct ProcExecCountingTool {
    calls: Arc<AtomicUsize>,
}

struct ProcExecLargeOutputTool;

struct ProcExecMcpFailureTool {
    calls: Arc<AtomicUsize>,
}

struct NamedProcExecCountingTool {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

struct WriteFileCountingTool {
    calls: Arc<AtomicUsize>,
}

struct MessageCountingTool {
    calls: Arc<AtomicUsize>,
}

struct ApprovalMetadataProbeTool {
    workspace: PathBuf,
    session_key: &'static str,
    calls: Arc<AtomicUsize>,
    observed_status: Arc<Mutex<Option<String>>>,
}

struct CheckpointMetadataProbeTool {
    workspace: PathBuf,
    session_key: &'static str,
    calls: Arc<AtomicUsize>,
    observed_checkpoint: Arc<Mutex<Option<String>>>,
}

struct ToolObservabilityCaptureHook {
    observed: Arc<Mutex<Vec<String>>>,
}

impl AgentHook for ToolObservabilityCaptureHook {
    fn after_response(&self, _context: &AgentHookContext, response: &LlmResponse) {
        if let Ok(mut observed) = self.observed.lock() {
            observed.push(serde_json::to_string(response).unwrap_or_default());
        }
    }

    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        if let Ok(mut observed) = self.observed.lock() {
            observed.push(serde_json::to_string(&context.messages).unwrap_or_default());
            observed.extend(calls.iter().map(|call| call.arguments.to_string()));
        }
    }
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

impl Tool for ProcExecLargeOutputTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Return a large proc exec result."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        "sensitive-output".repeat(128).into()
    }
}

impl Tool for ProcExecMcpFailureTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Return an MCP-style failure from a permission-gated exec."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", shacs_core::tools::StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "(MCP tool call failed: TimeoutError)".into()
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

impl Tool for WriteFileCountingTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Count workspace file writes."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("path", shacs_core::tools::StringSchema::new("Path"))
            .property("content", shacs_core::tools::StringSchema::new("Content"))
            .required(["path", "content"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "written".into()
    }
}

impl Tool for MessageCountingTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Count external message deliveries."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("target", shacs_core::tools::StringSchema::new("Target"))
            .property("content", shacs_core::tools::StringSchema::new("Content"))
            .required(["target", "content"])
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "sent".into()
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

impl Tool for CheckpointMetadataProbeTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Probe checkpoint metadata while counting proc exec attempts."
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
            .and_then(|raw| raw["metadata"].get("runtime_checkpoint").cloned())
            .map(|checkpoint| checkpoint.to_string());
        if let Ok(mut checkpoint) = self.observed_checkpoint.lock() {
            *checkpoint = observed;
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
        backend: None,
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
fn replay_runner_blocks_without_live_tool_dispatch_for_failure_cases() {
    let missing_outcome_dataset =
        vec![replay_item("case-1", vec![replay_tool_policy(false, None)])];
    let selected = vec!["case-1".to_owned()];

    let missing_outcome = run_local_replay(replay_input(&missing_outcome_dataset, &selected));

    assert_eq!(missing_outcome.live_tool_dispatch_count, 0);
    assert_eq!(missing_outcome.replayed_tool_policy_count, 1);
    assert_eq!(missing_outcome.run_record.status, ReplayRunStatus::Blocked);
    assert_eq!(
        missing_outcome.run_record.case_results[0]
            .blocked_reason
            .as_deref(),
        Some("blocked_missing_replay_outcome")
    );

    let schema_mismatch_dataset = vec![replay_item(
        "case-1",
        vec![replay_tool_policy(false, Some("schema-b"))],
    )];
    let schema_mismatch = run_local_replay(replay_input(&schema_mismatch_dataset, &selected));

    assert_eq!(schema_mismatch.live_tool_dispatch_count, 0);
    assert_eq!(schema_mismatch.replayed_tool_policy_count, 1);
    assert_eq!(schema_mismatch.run_record.status, ReplayRunStatus::Blocked);
    assert_eq!(
        schema_mismatch.run_record.case_results[0].comparison_status,
        ReplayComparisonStatus::SchemaMismatch
    );
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
    let durable_event_root = workspace.path().join("runtime").join("durable-events");
    let plugin_skill_root = workspace.path().join("plugin-skills");
    std::fs::create_dir_all(plugin_skill_root.join("plugin-skill"))?;
    std::fs::create_dir_all(plugin_skill_root.join("disabled-skill"))?;
    std::fs::write(
        plugin_skill_root.join("plugin-skill").join("SKILL.md"),
        "---\ndescription: Plugin skill\n---\nPlugin body",
    )?;
    std::fs::write(
        plugin_skill_root.join("disabled-skill").join("SKILL.md"),
        "---\ndescription: Disabled skill\n---\nDisabled body",
    )?;
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(durable_event_root.clone());
    let context_builder = ContextBuilder::new(workspace.path())
        .with_skill_roots([plugin_skill_root])
        .with_disabled_skills(["disabled-skill".to_owned()]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        context_builder,
        &registry,
        &client,
        config,
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
    let runtime_execution = &raw["metadata"]["runtime_execution"];
    if runtime_execution["pending"].as_array().map(Vec::len) != Some(0)
        || runtime_execution["outcomes"].as_array().map(Vec::len) != Some(1)
        || runtime_execution["outcomes"][0]["fact"]["outcome"]["domain"] != "provider"
        || runtime_execution["outcomes"][0]["fact"]["outcome"]["outcome"] != "completed"
        || runtime_execution["outcomes"][0]["decision"]["kind"] != "accepted"
    {
        return Err(format!("runtime execution ledger did not persist: {raw:?}").into());
    }
    let events = DurableEventStore::open(durable_event_root)?
        .scan(10)?
        .records;
    if events.len() != 2
        || events[0].kind != SESSION_TURN_ACCEPTED
        || events[1].kind != SESSION_TURN_COMPLETED
        || events[0].session_id != "cli:thread-1"
        || events[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.skill_registry_hash.as_ref())
            .is_none()
        || !events[0].provenance.as_ref().is_some_and(|provenance| {
            provenance.skill_body_hashes.contains_key("plugin-skill")
                && !provenance.skill_body_hashes.contains_key("disabled-skill")
        })
        || events[0].payload
            != shacs_session::durable_event::DurableEventPayload::inline(
                "orchestrator_fact",
                json!({
                    "channel": "direct",
                    "content_hash": format!("sha256:{:x}", sha2::Sha256::digest(b"hello")),
                    "media_count": 0,
                }),
            )
        || events[1]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.execution_identity.as_ref())
            .is_none()
    {
        return Err(format!("durable accepted facts drifted: {events:?}").into());
    }
    Ok(())
}

#[test]
fn loop_command_response_records_a_durable_accepted_fact() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let durable_event_root = workspace.path().join("runtime").join("durable-events");
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(durable_event_root.clone());
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let result = loop_runtime.process_direct("/status", Some("cli:command-event"))?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::Status));
    assert!(client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());
    let events = DurableEventStore::open(durable_event_root)?
        .scan(10)?
        .records;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, SESSION_TURN_COMPLETED);
    assert_eq!(events[0].session_id, "cli:command-event");
    match &events[0].payload {
        shacs_session::durable_event::DurableEventPayload::Inline { data, .. } => {
            assert_eq!(data["command"], "status");
            assert_eq!(data["stop_reason"], "status");
        }
        payload => return Err(format!("unexpected command event payload: {payload:?}").into()),
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
    assert_eq!(
        raw["metadata"]["goal_transition_history"]
            .as_array()
            .map(Vec::len),
        Some(8)
    );
    assert_ne!(
        raw["metadata"]["goal_transition_history"][0]["goal_id"],
        raw["metadata"]["goal_transition_history"][6]["goal_id"]
    );
    assert!(client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());
    Ok(())
}

#[test]
fn conservative_production_evaluator_returns_unknown_without_provider() -> Result<(), Box<dyn Error>>
{
    let goal = create_persistent_goal("cli:conservative", "ship it", "1", 8);
    let request = shacs_core::runtime::build_goal_completion_evaluation_request(
        &goal,
        shacs_eval::evaluator::EvaluationTriggerSource::SessionTurn,
        1,
    )?;
    let evaluator = shacs_core::runtime::ConservativeGoalCompletionEvaluator;

    let outcome = shacs_core::runtime::GoalCompletionEvaluator::evaluate(&evaluator, &request)?;

    assert_eq!(outcome.output.verdict_kind, VerdictKind::LowConfidence);
    assert_eq!(outcome.output.confidence, 0.0);
    assert_eq!(outcome.advisory_verdict, None);
    assert_eq!(outcome.requested_route, EvaluatorRoute::Notify);
    Ok(())
}

#[test]
fn loop_turn_end_records_advisory_evaluator_once_without_implicit_goal_mutation(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("turn complete".to_owned()),
        ..LlmResponse::default()
    }]);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_capture = calls.clone();
    let evaluator = Arc::new(
        move |_request: &shacs_core::runtime::GoalEvaluationRequest| {
            calls_capture.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(GoalEvaluatorOutcome {
                output: EvaluatorVerdictEnvelope {
                    verdict_kind: VerdictKind::Pass,
                    reason: "complete".to_owned(),
                    confidence: 1.0,
                    evidence_refs: Vec::new(),
                    suggested_next_action: SuggestedNextAction::None,
                    expires_at_ms: None,
                    redaction_status: RedactionStatus::AlreadySafe,
                    evaluator_version: "test-v1".to_owned(),
                },
                requested_route: EvaluatorRoute::Notify,
                owner_result_locator: OwnerResultLocator::new("session:cli:goal-eval:turn-1"),
                advisory_verdict: None,
            })
        },
    );
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:goal-eval"))?;

    loop_runtime.process_direct("work", Some("cli:goal-eval"))?;

    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal-eval")
        .ok_or("missing persisted evaluator session")?;
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
        "active"
    );
    assert_eq!(
        raw["metadata"][GOAL_EVALUATOR_BOUNDARY_METADATA_KEY]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        raw["metadata"][GOAL_EVALUATOR_BOUNDARY_METADATA_KEY][0]["route"],
        "notify"
    );
    assert_eq!(
        raw["metadata"][GOAL_EVALUATOR_BOUNDARY_METADATA_KEY][0]["owner_result_locator"],
        "session:cli:goal-eval:turn-1"
    );
    Ok(())
}

#[test]
fn evaluator_failure_preserves_successful_owner_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("turn complete".to_owned()),
        ..LlmResponse::default()
    }]);
    let evaluator = Arc::new(
        |_request: &shacs_core::runtime::GoalEvaluationRequest| -> Result<
            GoalEvaluatorOutcome,
            String,
        > { Err("evaluator unavailable".to_owned()) },
    );
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:evaluator-failure"))?;

    let result = loop_runtime.process_direct("work", Some("cli:evaluator-failure"))?;

    assert_eq!(result.final_content.as_deref(), Some("turn complete"));
    assert_eq!(result.stop_reason, "completed");
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:evaluator-failure")
        .ok_or("missing persisted owner turn")?;
    assert!(raw["messages"].as_array().is_some_and(|messages| {
        messages
            .iter()
            .any(|message| message["content"] == "turn complete")
    }));
    assert_eq!(
        raw["metadata"]["goal_evaluator_last_failure"]["status"],
        "advisory_failed"
    );
    assert_eq!(
        raw["metadata"]["goal_evaluator_last_failure"]["owner_turn_preserved"],
        true
    );
    Ok(())
}

#[test]
fn loop_turn_end_routes_explicit_advisory_verdict_through_runtime_policy(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("turn complete".to_owned()),
        ..LlmResponse::default()
    }]);
    let evaluator = Arc::new(
        move |_request: &shacs_core::runtime::GoalEvaluationRequest| {
            Ok(GoalEvaluatorOutcome {
                output: EvaluatorVerdictEnvelope {
                    verdict_kind: VerdictKind::Pass,
                    reason: "complete".to_owned(),
                    confidence: 1.0,
                    evidence_refs: Vec::new(),
                    suggested_next_action: SuggestedNextAction::None,
                    expires_at_ms: None,
                    redaction_status: RedactionStatus::AlreadySafe,
                    evaluator_version: "test-v1".to_owned(),
                },
                requested_route: EvaluatorRoute::Verify,
                owner_result_locator: OwnerResultLocator::new("session:cli:goal-apply:turn-1"),
                advisory_verdict: Some(GoalCompletionVerdict::Done),
            })
        },
    );
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:goal-apply"))?;

    loop_runtime.process_direct("work", Some("cli:goal-apply"))?;

    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal-apply")
        .ok_or("missing applied evaluator session")?;
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
        "done"
    );
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["transitions"][1]["stop_reason"],
        "evaluator_completion_accepted"
    );
    assert_eq!(
        raw["metadata"]["goal_evaluator_runtime_decisions"][0]["selected_action"],
        "complete_goal"
    );
    assert_eq!(
        raw["metadata"]["goal_evaluator_consumptions"][0]["status"],
        "consumed"
    );
    Ok(())
}

#[test]
fn loop_turn_end_dispatches_goal_continuation_as_next_durable_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let event_root = workspace.path().join("runtime/durable-events");
    let checkpoint_root = workspace.path().join("runtime/durable-checkpoints");
    let payload_root = workspace.path().join("runtime/work-payloads");
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("first turn".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("continued turn".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let evaluations = Arc::new(AtomicUsize::new(0));
    let evaluations_capture = Arc::clone(&evaluations);
    let evaluator = Arc::new(
        move |_request: &shacs_core::runtime::GoalEvaluationRequest| {
            let verdict = if evaluations_capture.fetch_add(1, Ordering::SeqCst) == 0 {
                GoalCompletionVerdict::Continue
            } else {
                GoalCompletionVerdict::Done
            };
            Ok(GoalEvaluatorOutcome {
                output: EvaluatorVerdictEnvelope {
                    verdict_kind: VerdictKind::Pass,
                    reason: "test decision".to_owned(),
                    confidence: 1.0,
                    evidence_refs: Vec::new(),
                    suggested_next_action: SuggestedNextAction::None,
                    expires_at_ms: None,
                    redaction_status: RedactionStatus::AlreadySafe,
                    evaluator_version: "test-v1".to_owned(),
                },
                requested_route: EvaluatorRoute::Verify,
                owner_result_locator: OwnerResultLocator::new(format!(
                    "session:cli:goal-continue:turn-{}",
                    evaluations_capture.load(Ordering::SeqCst)
                )),
                advisory_verdict: Some(verdict),
            })
        },
    );
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(event_root.clone());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:goal-continue"))?;

    loop_runtime.process_direct("start work", Some("cli:goal-continue"))?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing durable continuation state")?;
    let admission = evaluate_durable_work_recovery(&pending.work, &payload_root, 1);
    assert_eq!(admission.due_work_ids.len(), 1);
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "test-owner", 100)?;
    dispatcher.dispatch_due(&pending.work, &admission, 1)?;
    let follow_up = bus.consume_inbound().ok_or("missing durable follow-up")?;
    assert_eq!(follow_up.session_key(), "cli:goal-continue");
    assert_eq!(
        follow_up.metadata["goal_continuation"]["remaining_turns"],
        8
    );

    loop_runtime.process_message(follow_up)?;

    assert_eq!(evaluations.load(Ordering::SeqCst), 2);
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:goal-continue")
        .ok_or("missing continued goal session")?;
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["status"],
        "done"
    );
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    assert_eq!(requests.len(), 2);
    Ok(())
}

#[test]
fn continuation_enqueue_failure_does_not_persist_evaluator_accounting() -> Result<(), Box<dyn Error>>
{
    // Given
    let workspace = tempfile::tempdir()?;
    let event_root = workspace.path().join("runtime/durable-events");
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("owner turn complete".to_owned()),
        ..LlmResponse::default()
    }]);
    let evaluator = Arc::new(|_request: &shacs_core::runtime::GoalEvaluationRequest| {
        Ok(GoalEvaluatorOutcome {
            output: EvaluatorVerdictEnvelope {
                verdict_kind: VerdictKind::Pass,
                reason: "continue".to_owned(),
                confidence: 1.0,
                evidence_refs: Vec::new(),
                suggested_next_action: SuggestedNextAction::None,
                expires_at_ms: None,
                redaction_status: RedactionStatus::AlreadySafe,
                evaluator_version: "test-v1".to_owned(),
            },
            requested_route: EvaluatorRoute::Continue,
            owner_result_locator: OwnerResultLocator::new("session:cli:enqueue-failure:turn-1"),
            advisory_verdict: Some(GoalCompletionVerdict::Continue),
        })
    });
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(event_root.clone());
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:enqueue-failure"))?;
    std::fs::write(workspace.path().join("runtime/work-payloads"), b"blocked")?;

    // When
    let result = loop_runtime.process_direct("work", Some("cli:enqueue-failure"))?;

    // Then
    assert_eq!(result.final_content.as_deref(), Some("owner turn complete"));
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:enqueue-failure")
        .ok_or("missing persisted owner turn")?;
    assert!(raw["metadata"]
        .get(GOAL_EVALUATOR_BOUNDARY_METADATA_KEY)
        .is_none());
    assert!(raw["metadata"]
        .get("goal_evaluator_runtime_decisions")
        .is_none());
    assert!(raw["metadata"].get("goal_evaluator_consumptions").is_none());
    assert_eq!(
        raw["metadata"][PERSISTENT_GOAL_METADATA_KEY]["turns_used"],
        0
    );
    assert_eq!(
        raw["metadata"]["goal_evaluator_last_failure"]["status"],
        "advisory_failed"
    );
    Ok(())
}

#[test]
fn paused_goal_suppresses_already_leased_continuation_before_provider_turn(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let event_root = workspace.path().join("runtime/durable-events");
    let checkpoint_root = workspace.path().join("runtime/durable-checkpoints");
    let payload_root = workspace.path().join("runtime/work-payloads");
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("first turn".to_owned()),
        ..LlmResponse::default()
    }]);
    let evaluations = Arc::new(AtomicUsize::new(0));
    let evaluations_capture = Arc::clone(&evaluations);
    let evaluator = Arc::new(
        move |_request: &shacs_core::runtime::GoalEvaluationRequest| {
            evaluations_capture.fetch_add(1, Ordering::SeqCst);
            Ok(GoalEvaluatorOutcome {
                output: EvaluatorVerdictEnvelope {
                    verdict_kind: VerdictKind::Pass,
                    reason: "continue".to_owned(),
                    confidence: 1.0,
                    evidence_refs: Vec::new(),
                    suggested_next_action: SuggestedNextAction::None,
                    expires_at_ms: None,
                    redaction_status: RedactionStatus::AlreadySafe,
                    evaluator_version: "test-v1".to_owned(),
                },
                requested_route: EvaluatorRoute::Verify,
                owner_result_locator: OwnerResultLocator::new("session:cli:stale:turn-1"),
                advisory_verdict: Some(GoalCompletionVerdict::Continue),
            })
        },
    );
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(event_root.clone());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:stale"))?;
    loop_runtime.process_direct("start work", Some("cli:stale"))?;
    let pending = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing continuation")?;
    let admission = evaluate_durable_work_recovery(&pending.work, &payload_root, 1);
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner", 100)?;
    dispatcher.dispatch_due(&pending.work, &admission, 1)?;
    let follow_up = bus.consume_inbound().ok_or("missing follow-up")?;
    shacs_core::runtime::apply_goal_surface_action(
        workspace.path(),
        "cli:stale",
        shacs_core::runtime::GoalSurfaceAction::Pause,
        "2",
    )?;

    let result = loop_runtime.process_message(follow_up)?;

    assert_eq!(result.stop_reason, "stale_goal_continuation");
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    let terminal = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing terminal state")?;
    assert_eq!(
        terminal
            .work
            .items
            .values()
            .find(|item| item.work_id.starts_with("goal-continuation-"))
            .ok_or("missing continuation work")?
            .state,
        shacs_session::durable_work::ReplayWorkState::Cancelled
    );
    Ok(())
}

#[test]
fn interrupted_goal_suppresses_already_leased_continuation_before_provider_turn(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let event_root = workspace.path().join("runtime/durable-events");
    let checkpoint_root = workspace.path().join("runtime/durable-checkpoints");
    let payload_root = workspace.path().join("runtime/work-payloads");
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("first turn".to_owned()),
        ..LlmResponse::default()
    }]);
    let evaluator = Arc::new(|_request: &shacs_core::runtime::GoalEvaluationRequest| {
        Ok(GoalEvaluatorOutcome {
            output: EvaluatorVerdictEnvelope {
                verdict_kind: VerdictKind::Pass,
                reason: "continue".to_owned(),
                confidence: 1.0,
                evidence_refs: Vec::new(),
                suggested_next_action: SuggestedNextAction::None,
                expires_at_ms: None,
                redaction_status: RedactionStatus::AlreadySafe,
                evaluator_version: "test-v1".to_owned(),
            },
            requested_route: EvaluatorRoute::Verify,
            owner_result_locator: OwnerResultLocator::new("session:cli:interrupted:turn-1"),
            advisory_verdict: Some(GoalCompletionVerdict::Continue),
        })
    });
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(event_root.clone());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_goal_completion_evaluator(evaluator);
    loop_runtime.process_direct("/goal ship it", Some("cli:interrupted"))?;
    loop_runtime.process_direct("start work", Some("cli:interrupted"))?;
    let pending = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing continuation")?;
    let admission = evaluate_durable_work_recovery(&pending.work, &payload_root, 1);
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner", 100)?;
    dispatcher.dispatch_due(&pending.work, &admission, 1)?;
    let follow_up = bus.consume_inbound().ok_or("missing follow-up")?;
    let mut session = loop_runtime
        .session_manager()
        .load_existing("cli:interrupted")
        .ok_or("missing goal session")?;
    let goal = shacs_core::runtime::persistent_goal_from_session(&session)
        .ok_or("missing persistent goal")?;
    let interrupted = shacs_core::runtime::record_goal_stop(
        &goal,
        shacs_core::runtime::GoalStopReason::UserInterrupted,
        true,
        "2",
    )?;
    shacs_core::runtime::store_persistent_goal(&mut session, &interrupted)?;
    loop_runtime.session_manager_mut().save(&session)?;

    let result = loop_runtime.process_message(follow_up)?;

    assert_eq!(result.stop_reason, "stale_goal_continuation");
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    let terminal = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing terminal state")?;
    assert_eq!(
        terminal
            .work
            .items
            .values()
            .find(|item| item.work_id.starts_with("goal-continuation-"))
            .ok_or("missing continuation work")?
            .state,
        shacs_session::durable_work::ReplayWorkState::Cancelled
    );
    Ok(())
}

#[test]
fn goal_surface_mutation_waits_for_agent_turn_and_preserves_both_updates(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    manager.save(&Session::new("cli:goal-race"))?;
    shacs_core::runtime::apply_goal_surface_action(
        workspace.path(),
        "cli:goal-race",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "ship it".to_owned(),
            turn_budget: 8,
        },
        "1",
    )?;
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let entered_provider = entered.clone();
    let release_provider = release.clone();
    let client = BlockingProvider {
        entered: entered_provider,
        release: release_provider,
    };
    let registry = ToolRegistry::new();
    let workspace_path = workspace.path().to_path_buf();

    std::thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        let turn = scope.spawn(|| -> Result<(), AgentLoopError> {
            let mut runtime = AgentLoop::new(
                MessageBus::new(),
                SessionManager::new(&workspace_path)?,
                ContextBuilder::new(&workspace_path),
                &registry,
                &client,
                AgentLoopConfig::new(&workspace_path, "test-model"),
            );
            runtime.process_direct("work", Some("cli:goal-race"))?;
            Ok(())
        });
        entered.wait();
        let mutation = scope.spawn(|| {
            shacs_core::runtime::apply_goal_surface_action(
                &workspace_path,
                "cli:goal-race",
                shacs_core::runtime::GoalSurfaceAction::Pause,
                "2",
            )
        });
        release.wait();
        turn.join().map_err(|_| "turn panicked")??;
        mutation.join().map_err(|_| "mutation panicked")??;
        Ok(())
    })?;

    let session = SessionManager::open_existing(workspace.path())?
        .and_then(|manager| manager.load_existing("cli:goal-race"))
        .ok_or("missing raced session")?;
    assert_eq!(session.messages.len(), 2);
    assert_eq!(
        shacs_core::runtime::persistent_goal_from_session(&session)
            .ok_or("missing raced goal")?
            .status,
        PersistentGoalStatus::Paused
    );
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
        || outcome.envelope.correlation_id.is_none()
        || outcome.envelope.attempt_id.as_deref() != Some("attempt:1")
        || outcome.envelope.idempotency_key.is_none()
    {
        return Err(format!("subagent spawn snapshots drifted: {outcome:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_registration_begins_pending_execution_fact() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect docs".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "cli:direct".to_owned(),
    })?;
    let ledger = runtime.execution_ledger_snapshot();

    if ledger.pending.len() != 1
        || ledger.pending[0].domain != shacs_core::runtime::ExecutionDomain::Subagent
        || ledger.pending[0].identity.scope.session_id != "cli:direct"
        || ledger.pending[0].identity.scope.turn_id != "turn:cli:direct"
        || ledger.pending[0].identity.effect_id != outcome.envelope.spawn_effect_id
    {
        return Err(format!("subagent pending execution drifted: {ledger:?}").into());
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
        backend: None,
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
        permission_session_remembered_rules: Vec::new(),
        project_permission_store: None,
        active_workspace: None,
        in_cron_context: false,
        record_channel_delivery: false,
        cancellation_token: None,
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
    let ledger = runtime.execution_ledger_snapshot();
    if !ledger.pending.is_empty()
        || ledger.outcomes.len() != 1
        || ledger.outcomes[0].decision != LateResultDecision::Accepted
        || ledger.outcomes[0].fact.outcome
            != ExecutionOutcome::Subagent(SubagentOutcomeKind::Completed)
    {
        return Err(format!("completed subagent ledger drifted: {ledger:?}").into());
    }
    let inbound = bus
        .consume_inbound()
        .ok_or("missing synthetic subagent inbound")?;
    if inbound.channel != "system"
        || inbound.sender_id != "subagent"
        || inbound.session_key_override.as_deref() != Some("session-1")
        || inbound.metadata["injected_event"] != "subagent_result"
        || inbound.metadata["subagent_task_id"] != outcome.envelope.child_task_id
        || inbound.metadata["subagent_outcome"]["outcome"] != "completed"
        || !matches!(
            inbound.owner_accepted_automation_result(),
            Some(shacs_core::runtime::OwnerAcceptedAutomationResult::SubagentTerminal { result_ref })
                if result_ref == &outcome.envelope.child_task_id
        )
        || inbound.metadata["late_result_decision"]["kind"] != "accepted"
        || inbound.metadata["execution_fact"]["outcome"]["domain"] != "subagent"
        || inbound.metadata["execution_fact"]["outcome"]["outcome"] != "completed"
        || inbound.metadata.get("structured_result").is_some()
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
fn subagent_reentry_persists_outcome_in_session_diagnostics() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    runtime.publish_child_result(ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Child summary",
    ));

    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("Parent incorporated child summary".to_owned()),
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
    loop_runtime
        .process_next_inbound()?
        .ok_or("missing subagent reentry turn")?;

    let diagnostics = loop_runtime
        .session_manager()
        .session_ux_diagnostics("session-1");
    let execution = diagnostics
        .runtime_execution
        .ok_or("missing runtime execution projection")?;
    if execution.outcomes_by_domain.subagent != 1
        || execution.outcomes_by_domain.provider != 1
        || execution.decisions.accepted != 2
    {
        return Err(format!("subagent session projection drifted: {execution:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_failed_timedout_and_cancelled_finishes_record_formal_outcomes(
) -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            ChildResultStatus::Failed,
            MergeDecision::AcceptFailureFact,
            SubagentOutcomeKind::Failed,
            "subagent_failed",
        ),
        (
            ChildResultStatus::TimedOut,
            MergeDecision::RetryChild,
            SubagentOutcomeKind::TimedOut,
            "subagent_timed_out",
        ),
        (
            ChildResultStatus::Cancelled,
            MergeDecision::AcceptCancellationFact,
            SubagentOutcomeKind::Cancelled,
            "subagent_cancelled",
        ),
    ];

    for (status, expected_decision, expected_outcome, expected_command) in cases {
        let bus = MessageBus::new();
        let runtime = SubagentRuntime::with_bus(bus.clone());
        let outcome = runtime.spawn_from_request(SpawnRequest {
            task: format!("case {status:?}"),
            label: None,
            origin_channel: "cli".to_owned(),
            origin_chat_id: "direct".to_owned(),
            session_key: format!("session-{status:?}"),
        })?;
        let result =
            ChildResultEnvelope::from_spawn(&outcome.envelope, status.clone(), "case result");

        let decision = runtime.publish_child_result(result);
        let inbound = bus.consume_inbound().ok_or("missing typed reentry")?;
        let ledger = runtime.execution_ledger_snapshot();

        if decision != expected_decision
            || runtime.running_count() != 0
            || ledger.outcomes.last().map(|record| &record.decision)
                != Some(&LateResultDecision::Accepted)
            || ledger.outcomes.last().map(|record| &record.fact.outcome)
                != Some(&ExecutionOutcome::Subagent(expected_outcome))
            || inbound.metadata["subagent_command"] != expected_command
            || inbound.metadata["late_result_decision"]["kind"] != "accepted"
            || (status == ChildResultStatus::TimedOut
                && inbound.owner_accepted_automation_result().is_some())
            || (status != ChildResultStatus::TimedOut
                && inbound.owner_accepted_automation_result().is_none())
        {
            return Err(format!(
                "subagent terminal outcome drifted: decision={decision:?} inbound={inbound:?} ledger={ledger:?}"
            )
            .into());
        }
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
    let ledger = runtime.execution_ledger_snapshot();
    if ledger.outcomes.last().map(|record| &record.fact.outcome)
        != Some(&ExecutionOutcome::Subagent(SubagentOutcomeKind::Cancelled))
    {
        return Err(format!("cancelled subagent ledger drifted: {ledger:?}").into());
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

#[test]
fn agent_runner_auto_classifier_preserves_local_workspace_edit_fast_path_without_classifier_request(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "write-local-fast-path",
            "write_file",
            json!({ "path": "src/lib.rs", "content": "ok" }),
        ))?,
        LlmResponse {
            content: Some("local fast path completed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(Vec::new());
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_workspace_edits: true,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let main_requests = client.requests.lock().map_err(|error| error.to_string())?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("local fast path completed")
        || main_requests.len() != 2
        || !classifier_requests.is_empty()
    {
        return Err(format!(
            "local auto-approval should execute without classifier: result={result:?} calls={} main_requests={main_requests:?} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_allows_unresolved_static_allow_candidate(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-classified-allow",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        LlmResponse {
            content: Some("classifier allowed exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.session_key = Some("/Users/example/.shacs-bot/sessions/raw-session.json".to_owned());
    context.message_id = Some("/tmp/shacs/raw-turn.json".to_owned());
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("classifier allowed exec")
        || classifier_requests.len() != 1
        || classifier_requests[0].model != "test-model"
        || !classifier_requests[0]
            .messages
            .iter()
            .any(|message| message.to_string().contains("exec"))
    {
        return Err(format!(
            "classifier allow verdict should stage evaluator approval before execution: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_classifier_records_deterministic_accounting_evidence() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-classifier-accounting",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        LlmResponse {
            content: Some("classifier accounting allowed exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut classifier_response =
        classifier_verdict_response("allow_candidate", "high", "requested");
    classifier_response.usage = BTreeMap::from([
        ("prompt_tokens".to_owned(), 17),
        ("completion_tokens".to_owned(), 3),
    ]);
    let classifier = MockProvider::new(vec![classifier_response]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_events_capture = observed_events.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut observed) = observed_events_capture.lock() {
                observed.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let diagnostic_text = observed_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");

    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("classifier accounting allowed exec")
        || !diagnostic_text.contains("\"classifier_evidence_id\"")
        || !diagnostic_text.contains("\"disposition\":\"allow_candidate_consumed\"")
        || !diagnostic_text.contains("\"precedence\":\"classifier_reviewable\"")
        || !diagnostic_text.contains("\"token_accounting\"")
        || !diagnostic_text.contains("\"value\":17")
        || !diagnostic_text.contains("\"value\":3")
        || !diagnostic_text.contains("\"latency\"")
        || !diagnostic_text.contains("\"cost\"")
        || diagnostic_text.contains("RAW_PROVIDER_RESPONSE_SECRET")
    {
        return Err(format!(
            "classifier accounting evidence missing or unsafe: result={result:?} calls={} diagnostics={diagnostic_text}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_classifier_serializes_deterministic_evidence_with_injected_clock(
) -> Result<(), Box<dyn Error>> {
    fn run_once() -> Result<String, Box<dyn Error>> {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(ProcExecCountingTool {
            calls: calls.clone(),
        });
        let client = MockProvider::new(vec![
            response_with_runtime_tool_call(RuntimeToolCall::new(
                "exec-classifier-deterministic",
                "exec",
                json!({ "command": "cargo test" }),
            ))?,
            LlmResponse {
                content: Some("classifier deterministic evidence".to_owned()),
                ..LlmResponse::default()
            },
        ]);
        let mut classifier_response =
            classifier_verdict_response("allow_candidate", "high", "requested");
        classifier_response.usage = BTreeMap::from([
            ("prompt_tokens".to_owned(), 17),
            ("completion_tokens".to_owned(), 3),
        ]);
        let classifier = MockProvider::new(vec![classifier_response]);
        let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
        spec.tool_context = interactive_auto_context(AutoApprovalConfig {
            enabled: true,
            allow_proc_exec_verification: false,
            ..AutoApprovalConfig::default()
        });
        let times = Arc::new(Mutex::new(VecDeque::from([1_000_u64, 1_007, 1_307])));
        let times_capture = times.clone();
        spec.classifier_time_source = Some(Arc::new(move || {
            times_capture
                .lock()
                .ok()
                .and_then(|mut values| values.pop_front())
                .unwrap_or(1_307)
        }));
        let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
        let observed_events_capture = observed_events.clone();
        spec.tool_event_callback = Some(Arc::new(move |event| {
            if event.name == "permission_auto_approval" {
                if let Ok(mut observed) = observed_events_capture.lock() {
                    observed.push(event.detail.clone());
                }
            }
        }));

        let result = AgentRunner::new().run(spec)?;
        if calls.load(Ordering::SeqCst) != 1
            || result.final_content.as_deref() != Some("classifier deterministic evidence")
        {
            return Err(format!(
                "deterministic classifier run did not execute once: result={result:?} calls={}",
                calls.load(Ordering::SeqCst)
            )
            .into());
        }
        let diagnostic = observed_events
            .lock()
            .map_err(|error| error.to_string())?
            .iter()
            .find(|detail| detail.contains("\"classifier_evidence_id\""))
            .cloned();
        let Some(diagnostic) = diagnostic else {
            return Err("missing classifier evidence diagnostic".into());
        };
        Ok(diagnostic)
    }

    let first = run_once()?;
    let second = run_once()?;

    assert_eq!(first, second);
    assert!(first.contains("\"created_at_unix_ms\":1000"));
    assert!(first.contains("\"value\":7"));
    Ok(())
}

#[test]
fn agent_runner_classifier_missing_usage_is_unavailable_not_zero() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-classifier-missing-usage",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        LlmResponse {
            content: Some("classifier missing usage allowed exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_events_capture = observed_events.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut observed) = observed_events_capture.lock() {
                observed.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let diagnostic_text = observed_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");

    if calls.load(Ordering::SeqCst) != 1
        || result.stop_reason != "completed"
        || !diagnostic_text.contains("\"unavailable_reason\":\"provider_omitted_usage\"")
        || diagnostic_text.contains("\"input_tokens\":0")
        || diagnostic_text.contains("\"output_tokens\":0")
        || diagnostic_text.contains("\"cost_amount\":0")
    {
        return Err(format!(
            "missing classifier usage should preserve allow behavior with unavailable accounting, not zeros: result={result:?} calls={} diagnostics={diagnostic_text}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_classifier_provider_error_records_fallback_and_fails_closed(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-provider-error",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(Vec::new());
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_events_capture = observed_events.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut observed) = observed_events_capture.lock() {
                observed.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let diagnostic_text = observed_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");

    if calls.load(Ordering::SeqCst) != 0
        || result.stop_reason != "ask_user"
        || !diagnostic_text.contains("\"fallback_cause\":\"provider_error\"")
        || !diagnostic_text.contains("\"disposition\":\"failed_closed\"")
        || diagnostic_text.contains("\"can_handoff_to_tool_runtime\":true")
    {
        return Err(format!(
            "classifier provider error should fail closed with fallback evidence: result={result:?} calls={} diagnostics={diagnostic_text}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_classifier_provider_timeout_records_timeout_fallback_without_sleep(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-provider-timeout",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = ErrorProvider {
        message: "deterministic provider timeout".to_owned(),
    };
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    let times = Arc::new(Mutex::new(VecDeque::from([1_000_u64, 1_125])));
    let times_capture = times.clone();
    spec.classifier_time_source = Some(Arc::new(move || {
        times_capture
            .lock()
            .ok()
            .and_then(|mut values| values.pop_front())
            .unwrap_or(1_125)
    }));
    let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_events_capture = observed_events.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut observed) = observed_events_capture.lock() {
                observed.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let diagnostic_text = observed_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");

    if calls.load(Ordering::SeqCst) != 0
        || result.stop_reason != "ask_user"
        || !diagnostic_text.contains("\"fallback_cause\":\"provider_timeout\"")
        || !diagnostic_text.contains("\"disposition\":\"failed_closed\"")
        || diagnostic_text.contains("\"value\":0")
        || diagnostic_text.contains("\"can_handoff_to_tool_runtime\":true")
    {
        return Err(format!(
            "classifier provider timeout should fail closed without sleeps: result={result:?} calls={} diagnostics={diagnostic_text}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_static_deny_records_classifier_not_invoked_precedence() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "write-static-deny-classifier-evidence",
            "write_file",
            json!({ "path": ".git/config", "content": "blocked" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_workspace_edits: true,
        ..AutoApprovalConfig::default()
    });
    let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_events_capture = observed_events.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut observed) = observed_events_capture.lock() {
                observed.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    let diagnostic_text = observed_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");

    if calls.load(Ordering::SeqCst) != 0
        || result.stop_reason != "ask_user"
        || !classifier_requests.is_empty()
        || !diagnostic_text.contains("\"precedence\":\"static_deny_wins\"")
        || !diagnostic_text.contains("\"disposition\":\"not_invoked_static_policy\"")
        || diagnostic_text.contains("allow_candidate_consumed")
    {
        return Err(format!(
            "static deny should skip classifier with precedence evidence: result={result:?} calls={} classifier_requests={classifier_requests:?} diagnostics={diagnostic_text}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_records_recent_denial_without_raw_command(
) -> Result<(), Box<dyn Error>> {
    const RAW_COMMAND: &str = "RAW_EVENT_COMMAND_SECRET";
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = StreamMockProvider::new(
        vec![
            response_with_runtime_tool_call(RuntimeToolCall::new(
                "exec-classified-deny-record",
                "exec",
                json!({ "command": RAW_COMMAND }),
            ))?,
            LlmResponse {
                content: Some("classifier denied exec".to_owned()),
                ..LlmResponse::default()
            },
        ],
        vec![
            ProviderEvent::ToolCallStart {
                id: "exec-classified-deny-record".to_owned(),
                name: "exec".to_owned(),
            },
            ProviderEvent::ToolCallDelta {
                id: "exec-classified-deny-record".to_owned(),
                delta: format!(r#"{{"command":"{RAW_COMMAND}"}}"#),
            },
            ProviderEvent::ToolCallReady {
                id: "exec-classified-deny-record".to_owned(),
                name: "exec".to_owned(),
                input: json!({ "command": RAW_COMMAND }),
            },
        ],
    );
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "deny_candidate",
        "high",
        "unrelated",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.session_key = Some("/Users/example/.shacs-bot/sessions/raw-session.json".to_owned());
    context.message_id = Some("/tmp/shacs/raw-turn.json".to_owned());
    spec.tool_context = context;
    let observed_hook_arguments = Arc::new(Mutex::new(Vec::<String>::new()));
    spec.agent_hook = Some(Arc::new(ToolObservabilityCaptureHook {
        observed: observed_hook_arguments.clone(),
    }));
    let observed_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_events_capture = observed_events.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if let Ok(mut observed) = observed_events_capture.lock() {
            observed.push(serde_json::to_string(event).unwrap_or_default());
        }
    }));
    let observed_provider_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_provider_events_capture = observed_provider_events.clone();
    spec.provider_event_callback = Some(Arc::new(move |event| {
        if let Ok(mut observed) = observed_provider_events_capture.lock() {
            observed.push(format!("{event:?}"));
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let hook_text = observed_hook_arguments
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");
    let event_text = observed_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");
    let provider_event_text = observed_provider_events
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");
    if calls.load(Ordering::SeqCst) != 0 || result.recent_auto_mode_denials.len() != 1 {
        return Err(format!(
            "classifier deny should record one recent denial without executing: result={result:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let denial = &result.recent_auto_mode_denials[0];
    let serialized = serde_json::to_string(denial)?;
    if denial.tool_name != "exec"
        || denial.retryable
        || !result.recent_auto_mode_retry_tokens.is_empty()
        || !denial.denial_id.starts_with("auto_denial_")
        || serialized.contains(RAW_COMMAND)
        || serialized.contains("command")
        || serialized.contains("classifier:test")
        || serialized.contains("raw-session")
        || serialized.contains("raw-turn")
        || serialized.contains("/Users/example")
        || serialized.contains("/tmp/shacs")
        || hook_text.contains(RAW_COMMAND)
        || hook_text.contains("command")
        || event_text.contains(RAW_COMMAND)
        || event_text.contains("command")
        || provider_event_text.contains(RAW_COMMAND)
        || provider_event_text.contains("command")
        || !hook_text.contains("redacted")
        || !event_text.contains("redacted")
        || !provider_event_text.contains("redacted")
    {
        return Err(format!(
            "recent denial observability was not sanitized: denial={serialized} hook={hook_text} events={event_text} provider_events={provider_event_text}"
        )
        .into());
    }
    if denial.session_digest.len() != 64 || denial.turn_digest.len() != 64 {
        return Err(
            format!("recent denial id digests should be full sha256 hex: {serialized}").into(),
        );
    }
    Ok(())
}

#[test]
fn agent_runner_bridge_classifier_denial_records_recent_visibility() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(NamedProcExecCountingTool {
        name: "mcp_exec",
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "bridge-classified-deny-record",
            "tool_call",
            json!({ "name": "mcp_exec", "arguments": { "command": "cargo test" } }),
        ))?,
        LlmResponse {
            content: Some("bridge classifier denied exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "deny_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_search = ToolSearchConfig {
        enabled: ToolSearchMode::On,
        threshold_pct: 10,
        search_default_limit: 5,
        max_search_limit: 20,
    };
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    if calls.load(Ordering::SeqCst) != 0
        || result.recent_auto_mode_denials.len() != 1
        || result.recent_auto_mode_retry_tokens.len() != 1
        || result.recent_auto_mode_denials[0].tool_name != "mcp_exec"
    {
        return Err(format!(
            "bridge classifier denial did not record visibility/token without executing: result={result:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_classifier_denial_asks_and_preserves_recent_denials() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classified-deny-then-cancel",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "deny_candidate",
        "high",
        "requested",
    )]);
    let cancellation = CancellationToken::new();
    let cancellation_for_event = cancellation.clone();
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    spec.cancellation_token = Some(cancellation);
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "exec" {
            cancellation_for_event.cancel();
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    if result.stop_reason != "ask_user"
        || calls.load(Ordering::SeqCst) != 0
        || result.recent_auto_mode_denials.len() != 1
    {
        return Err(format!(
            "classifier denial should ask and preserve recent visibility without executing: result={result:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

fn decline_initial_auto_permission(
    loop_runtime: &mut AgentLoop<'_>,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let pending = loop_runtime.process_direct("run the tests", Some(session_key))?;
    if pending.stop_reason != "ask_user" {
        return Err(format!("classifier denial did not ask the user: {pending:?}").into());
    }

    let denied = loop_runtime.process_direct("deny", Some(session_key))?;
    if denied.stop_reason != "permission_denied_by_user" {
        return Err(
            format!("permission denial did not close the pending action: {denied:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn loop_permission_recent_shows_sanitized_classifier_denials() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-loop-classified-deny-record",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        classifier_verdict_response("deny_candidate", "high", "unrelated"),
        LlmResponse {
            content: Some("classifier denied loop exec".to_owned()),
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
    config.permission_rule_input = confirmed_non_privileged_permission_input();
    config.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(true),
        backend: Some("official-container".to_owned()),
        digest: Some("test-contained".to_owned()),
        summary: Some("non-privileged test containment".to_owned()),
    });
    config.permission_auto_approval = AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    };
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-denials")?;
    if calls.load(Ordering::SeqCst) != 0 {
        return Err(format!(
            "declined classifier action should not execute: calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }

    let recent = loop_runtime.process_direct("/permission recent", Some("cli:recent-denials"))?;
    let content = recent.final_content.unwrap_or_default();
    if !content.contains("Recent auto-mode classifier denials")
        || !content.contains("auto_denial_")
        || !content.contains("tool=exec")
        || !content.contains("retry_state=unavailable")
        || content.contains("cargo test")
        || content.contains("command")
        || content.contains("classifier:test")
    {
        return Err(format!("recent denial output was not sanitized: {content}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_shows_token_availability_not_blanket_retryable(
) -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, calls, _workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-state")?;

    let recent =
        loop_runtime.process_direct("/permission recent", Some("cli:recent-retry-state"))?;
    let content = recent.final_content.unwrap_or_default();
    if calls.load(Ordering::SeqCst) != 0
        || !content.contains("retry_state=available")
        || content.contains("retryable=true")
    {
        return Err(format!("recent output did not show token availability: {content}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_creates_formal_approval_without_raw_payload_metadata(
) -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, _calls, workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-meta")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:recent-retry-meta"))?
            .final_content
            .unwrap_or_default(),
    )?;

    let retry = loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:recent-retry-meta"),
    )?;
    let raw = SessionManager::new(workspace.path())?
        .read_session_file("cli:recent-retry-meta")
        .ok_or("missing recent retry metadata session")?;
    let metadata = raw["metadata"].to_string();
    if retry.stop_reason != "permission_recent_retry_pending"
        || !metadata.contains("pending_recent_retry_approval")
        || metadata.contains("cargo test")
        || metadata.contains("command")
        || metadata.contains("tool_call")
        || metadata.contains("tool_context")
    {
        return Err(format!("recent retry persisted unsafe metadata: {metadata}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_approval_executes_once_through_existing_permission_path(
) -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, calls, _workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-approve")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:recent-retry-approve"))?
            .final_content
            .unwrap_or_default(),
    )?;
    loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:recent-retry-approve"),
    )?;

    let approved = loop_runtime.process_direct("approve", Some("cli:recent-retry-approve"))?;
    if calls.load(Ordering::SeqCst) != 1 || approved.stop_reason == "permission_recent_retry_closed"
    {
        return Err(format!(
            "recent retry approval did not execute once: {approved:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_stops_after_fatal_tool_outcome() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecMcpFailureTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-recent-retry-fatal",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        classifier_verdict_response("deny_candidate", "high", "requested"),
        LlmResponse {
            content: Some("provider must not resume".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = recent_retry_config(workspace.path(), true);
    config.fail_on_tool_error = true;
    let events = Arc::new(Mutex::new(Vec::<ToolEvent>::new()));
    let event_capture = events.clone();
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_tool_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = event_capture.lock() {
            events.push(event.clone());
        }
    }));
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-fatal")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:recent-retry-fatal"))?
            .final_content
            .unwrap_or_default(),
    )?;
    loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:recent-retry-fatal"),
    )?;

    let approved = loop_runtime.process_direct("approve", Some("cli:recent-retry-fatal"))?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let events = events.lock().map_err(|error| error.to_string())?;
    if approved.stop_reason != "tool_error"
        || approved
            .final_content
            .as_deref()
            .is_none_or(|content| !content.contains("MCP tool call failed"))
        || calls.load(Ordering::SeqCst) != 1
        || requests.len() != 2
        || !events
            .iter()
            .any(|event| event.name == "exec" && event.status == ToolStatus::Error)
    {
        return Err(format!(
            "recent retry fatal tool resumed provider: approved={approved:?} requests={requests:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_rejects_approve_session_without_execution(
) -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, calls, _workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-session")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:recent-retry-session"))?
            .final_content
            .unwrap_or_default(),
    )?;
    loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:recent-retry-session"),
    )?;
    let reply = loop_runtime.process_direct("approve_session", Some("cli:recent-retry-session"))?;
    if calls.load(Ordering::SeqCst) != 0 || reply.stop_reason != "permission_recent_retry_rejected"
    {
        return Err(format!("approve_session did not fail closed: {reply:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_missing_token_fails_closed() -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, calls, workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-missing")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:recent-retry-missing"))?
            .final_content
            .unwrap_or_default(),
    )?;
    drop(loop_runtime);
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(Vec::new());
    let mut config = recent_retry_config(workspace.path(), true);
    config.permission_auto_approval.enabled = true;
    let mut restarted = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );
    let retry = restarted.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:recent-retry-missing"),
    )?;
    if calls.load(Ordering::SeqCst) != 0 || retry.stop_reason != "permission_recent_retry_closed" {
        return Err(format!("missing token did not fail closed: {retry:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_non_interactive_fails_closed_without_pending_approval(
) -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, calls, workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:recent-retry-noninteractive")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct(
                "/permission recent",
                Some("cli:recent-retry-noninteractive"),
            )?
            .final_content
            .unwrap_or_default(),
    )?;
    drop(loop_runtime);
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(Vec::new());
    let config = recent_retry_config(workspace.path(), false);
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );
    let retry = loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:recent-retry-noninteractive"),
    )?;
    let raw = SessionManager::new(workspace.path())?
        .read_session_file("cli:recent-retry-noninteractive")
        .ok_or("missing non-interactive retry session")?;
    if calls.load(Ordering::SeqCst) != 0
        || retry.stop_reason != "permission_recent_retry_non_interactive"
        || raw["metadata"]
            .get("pending_recent_retry_approval")
            .is_some()
    {
        return Err(format!("non-interactive retry did not fail closed: {retry:?}").into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_does_not_overwrite_pending_permission_approval(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-retry-overwrite-denial",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        classifier_verdict_response("deny_candidate", "high", "requested"),
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-existing-approval",
            "exec",
            json!({ "command": "pwd; true" }),
        ))?,
        classifier_verdict_response("ask_user", "medium", "adjacent"),
        LlmResponse {
            content: Some("resumed existing approval".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        recent_retry_config(workspace.path(), true),
    );

    decline_initial_auto_permission(&mut loop_runtime, "cli:retry-overwrite-permission")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:retry-overwrite-permission"))?
            .final_content
            .unwrap_or_default(),
    )?;
    let pending =
        loop_runtime.process_direct("needs approval", Some("cli:retry-overwrite-permission"))?;
    if pending.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("fixture did not create pending approval: {pending:?}").into());
    }

    let retry = loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:retry-overwrite-permission"),
    )?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:retry-overwrite-permission")
        .ok_or("missing retry overwrite session")?;
    if retry.stop_reason != "permission_approval_pending"
        || raw["metadata"].get("pending_permission_approval").is_none()
        || raw["metadata"]
            .get("pending_recent_retry_approval")
            .is_some()
    {
        return Err(format!(
            "recent retry overwrote or disturbed pending permission approval: retry={retry:?} raw={raw:?}"
        )
        .into());
    }

    let approved =
        loop_runtime.process_direct("approve", Some("cli:retry-overwrite-permission"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("resumed existing approval")
    {
        return Err(format!(
            "original pending approval was not preserved: approved={approved:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_recent_retry_does_not_overwrite_pending_recent_retry_approval(
) -> Result<(), Box<dyn Error>> {
    let (mut loop_runtime, calls, _workspace) = recent_retry_loop(true)?;
    decline_initial_auto_permission(&mut loop_runtime, "cli:retry-overwrite-recent")?;
    let denial_id = recent_denial_id_from_output(
        &loop_runtime
            .process_direct("/permission recent", Some("cli:retry-overwrite-recent"))?
            .final_content
            .unwrap_or_default(),
    )?;
    let first = loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:retry-overwrite-recent"),
    )?;
    let second = loop_runtime.process_direct(
        format!("/permission recent retry {denial_id}"),
        Some("cli:retry-overwrite-recent"),
    )?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:retry-overwrite-recent")
        .ok_or("missing pending recent retry session")?;
    if first.stop_reason != "permission_recent_retry_pending"
        || second.stop_reason != "permission_recent_retry_pending"
        || calls.load(Ordering::SeqCst) != 0
        || raw["metadata"]
            .get("pending_recent_retry_approval")
            .is_none()
        || raw["metadata"].get("pending_permission_approval").is_some()
    {
        return Err(format!(
            "recent retry command disturbed existing recent retry approval: first={first:?} second={second:?} raw={raw:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }

    let approved = loop_runtime.process_direct("approve", Some("cli:retry-overwrite-recent"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("recent retry completed")
    {
        return Err(format!(
            "existing recent retry approval was not preserved: approved={approved:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn recent_auto_mode_denial_store_caps_at_twenty_newest_records() -> Result<(), Box<dyn Error>> {
    let mut store = RecentAutoModeDenialStore::default();
    for index in 0..25 {
        store.push_front(RecentAutoModeDenial {
            denial_id: format!("auto_denial_{index}"),
            created_at_unix_ms: index,
            session_digest: "session-digest".to_owned(),
            turn_digest: format!("turn-digest-{index}"),
            tool_name: "exec".to_owned(),
            capabilities: vec![shacs_config::SafetyCapability::ProcExec],
            target_summary: vec![format!("target:{index}")],
            action_digest: format!("action-{index}"),
            argument_digest: format!("argument-{index}"),
            snapshot_digest: format!("snapshot-{index}"),
            policy_safety_snapshot_ref: None,
            secret_ref_evidence: Vec::new(),
            decision_reason: PermissionPolicyReason::EvaluatorUncertain,
            classifier_verdict: AutoEvaluatorVerdictKind::DenyCandidate,
            classifier_confidence: EvaluatorConfidence::High,
            classifier_scope_match: EvaluatorScopeMatch::Unrelated,
            retryable: true,
        });
    }

    if store.as_slice().len() != RECENT_AUTO_MODE_DENIAL_LIMIT
        || store
            .as_slice()
            .first()
            .map(|denial| denial.denial_id.as_str())
            != Some("auto_denial_24")
        || store
            .as_slice()
            .last()
            .map(|denial| denial.denial_id.as_str())
            != Some("auto_denial_5")
    {
        return Err(format!("recent denial store did not keep newest 20: {store:?}").into());
    }
    Ok(())
}

#[test]
fn recent_auto_mode_denial_store_extend_keeps_newest_first_input_order(
) -> Result<(), Box<dyn Error>> {
    let mut store = RecentAutoModeDenialStore::default();
    store.extend_newest_first(
        ["newest", "older"]
            .into_iter()
            .map(|id| RecentAutoModeDenial {
                denial_id: format!("auto_denial_{id}"),
                created_at_unix_ms: 1,
                session_digest: "session-digest".to_owned(),
                turn_digest: format!("turn-digest-{id}"),
                tool_name: "exec".to_owned(),
                capabilities: vec![shacs_config::SafetyCapability::ProcExec],
                target_summary: vec!["target:test".to_owned()],
                action_digest: format!("action-{id}"),
                argument_digest: format!("argument-{id}"),
                snapshot_digest: format!("snapshot-{id}"),
                policy_safety_snapshot_ref: None,
                secret_ref_evidence: Vec::new(),
                decision_reason: PermissionPolicyReason::EvaluatorUncertain,
                classifier_verdict: AutoEvaluatorVerdictKind::DenyCandidate,
                classifier_confidence: EvaluatorConfidence::High,
                classifier_scope_match: EvaluatorScopeMatch::Unrelated,
                retryable: true,
            }),
    );

    let ids = store
        .as_slice()
        .iter()
        .map(|denial| denial.denial_id.as_str())
        .collect::<Vec<_>>();
    if ids != ["auto_denial_newest", "auto_denial_older"] {
        return Err(format!("extend_newest_first reordered input: {ids:?}").into());
    }
    Ok(())
}

#[test]
fn recent_auto_mode_retry_token_store_consumes_one_shot() -> Result<(), Box<dyn Error>> {
    let denial = sample_recent_denial("one-shot");
    let token = RecentAutoModeRetryToken::new(
        &denial,
        RuntimeToolCall::new("call-1", "exec", json!({ "command": "cargo test" })),
        interactive_auto_context(AutoApprovalConfig::default()),
        10,
    );
    let mut store = RecentAutoModeRetryTokenStore::default();
    store.insert(token);

    let consumed = store
        .consume(
            &denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&denial),
            5,
        )
        .map_err(|error| format!("unexpected consume error: {error:?}"))?;
    if consumed.denial_id() != denial.denial_id || store.is_available(&denial.denial_id, 5) {
        return Err("retry token was not consumed exactly once".into());
    }
    if store
        .consume(
            &denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&denial),
            5,
        )
        .is_ok()
    {
        return Err("consumed retry token was reusable".into());
    }
    Ok(())
}

#[test]
fn recent_auto_mode_retry_token_store_rejects_expired_and_mismatched_once(
) -> Result<(), Box<dyn Error>> {
    let expired_denial = sample_recent_denial("expired");
    let expired_token = RecentAutoModeRetryToken::new(
        &expired_denial,
        RuntimeToolCall::new("call-expired", "exec", json!({ "command": "cargo test" })),
        interactive_auto_context(AutoApprovalConfig::default()),
        10,
    );
    let mismatched_denial = sample_recent_denial("mismatched");
    let mismatched_token = RecentAutoModeRetryToken::new(
        &mismatched_denial,
        RuntimeToolCall::new(
            "call-mismatched",
            "exec",
            json!({ "command": "cargo test" }),
        ),
        interactive_auto_context(AutoApprovalConfig::default()),
        100,
    );
    let mut store = RecentAutoModeRetryTokenStore::default();
    store.insert(expired_token);
    store.insert(mismatched_token);

    if store.consume(
        &expired_denial.denial_id,
        RecentAutoModeRetryTokenMatch::from_denial(&expired_denial),
        11,
    ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Expired)
        || store.consume(
            &expired_denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&expired_denial),
            11,
        ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Consumed)
    {
        return Err("expired token was not terminally consumed".into());
    }

    if store.consume(
        &mismatched_denial.denial_id,
        RecentAutoModeRetryTokenMatch {
            action_digest: "different-action",
            ..RecentAutoModeRetryTokenMatch::from_denial(&mismatched_denial)
        },
        20,
    ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Mismatched)
        || store.consume(
            &mismatched_denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&mismatched_denial),
            20,
        ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Consumed)
    {
        return Err("mismatched token was not terminally consumed".into());
    }
    Ok(())
}

#[test]
fn recent_auto_mode_retry_token_store_rejects_policy_safety_ref_mismatch_once(
) -> Result<(), Box<dyn Error>> {
    let mut denial = sample_recent_denial("policy-ref");
    denial.policy_safety_snapshot_ref = Some(policy_safety_ref("original"));
    let token = RecentAutoModeRetryToken::new(
        &denial,
        RuntimeToolCall::new(
            "call-policy-ref",
            "exec",
            json!({ "command": "cargo test" }),
        ),
        interactive_auto_context(AutoApprovalConfig::default()),
        100,
    );
    let mut store = RecentAutoModeRetryTokenStore::default();
    store.insert(token);

    if store.consume(
        &denial.denial_id,
        RecentAutoModeRetryTokenMatch {
            policy_safety_snapshot_ref: Some(&policy_safety_ref("changed")),
            ..RecentAutoModeRetryTokenMatch::from_denial(&denial)
        },
        20,
    ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Mismatched)
        || store.consume(
            &denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&denial),
            20,
        ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Consumed)
    {
        return Err("policy safety ref mismatch was not terminally consumed".into());
    }
    Ok(())
}

#[test]
fn spec030_recent_retry_token_preserves_secret_ref_evidence_and_rejects_token_change(
) -> Result<(), Box<dyn Error>> {
    let mut denial = sample_recent_denial("secret-ref");
    denial.secret_ref_evidence = vec![sample_secret_ref_evidence("opaque-owner-state-a")];
    let token = RecentAutoModeRetryToken::new(
        &denial,
        RuntimeToolCall::new(
            "call-secret-ref",
            "exec",
            json!({ "command": "cargo test" }),
        ),
        interactive_auto_context(AutoApprovalConfig::default()),
        100,
    );
    let mut store = RecentAutoModeRetryTokenStore::default();
    store.insert(token);

    let mut changed = denial.secret_ref_evidence.clone();
    changed[0].secret_ref.staleness_token = "opaque-owner-state-b".to_owned();
    if store.consume(
        &denial.denial_id,
        RecentAutoModeRetryTokenMatch {
            secret_ref_evidence: &changed,
            ..RecentAutoModeRetryTokenMatch::from_denial(&denial)
        },
        20,
    ) != Err(shacs_core::runtime::RecentAutoModeRetryTokenConsumeError::Mismatched)
    {
        return Err("retry token accepted changed secret ref staleness token".into());
    }

    let mut store = RecentAutoModeRetryTokenStore::default();
    store.insert(RecentAutoModeRetryToken::new(
        &denial,
        RuntimeToolCall::new(
            "call-secret-ref",
            "exec",
            json!({ "command": "cargo test" }),
        ),
        interactive_auto_context(AutoApprovalConfig::default()),
        100,
    ));
    let consumed = store
        .consume(
            &denial.denial_id,
            RecentAutoModeRetryTokenMatch::from_denial(&denial),
            20,
        )
        .map_err(|error| format!("retry token should be consumable: {error:?}"))?;
    if consumed.secret_ref_evidence() != denial.secret_ref_evidence.as_slice() {
        return Err("retry token did not preserve denied action secret evidence".into());
    }
    Ok(())
}

#[test]
fn recent_auto_mode_retry_token_debug_redacts_raw_payload() -> Result<(), Box<dyn Error>> {
    let denial = sample_recent_denial("debug-redacts");
    let mut context = interactive_auto_context(AutoApprovalConfig::default());
    context.metadata = json!({ "secret": "RAW_CONTEXT_SECRET" });
    let token = RecentAutoModeRetryToken::new(
        &denial,
        RuntimeToolCall::new("call-1", "exec", json!({ "command": "RAW_COMMAND_SECRET" })),
        context,
        10,
    );
    let debug = format!("{token:?}");
    if debug.contains("RAW_COMMAND_SECRET")
        || debug.contains("RAW_CONTEXT_SECRET")
        || debug.contains("command")
    {
        return Err(format!("retry token debug leaked raw payload: {debug}").into());
    }
    Ok(())
}

fn sample_recent_denial(label: &str) -> RecentAutoModeDenial {
    RecentAutoModeDenial {
        denial_id: format!("auto_denial_{label}"),
        created_at_unix_ms: 1,
        session_digest: "session-digest".to_owned(),
        turn_digest: "turn-digest".to_owned(),
        tool_name: "exec".to_owned(),
        capabilities: vec![shacs_config::SafetyCapability::ProcExec],
        target_summary: vec!["target:test".to_owned()],
        action_digest: format!("action-{label}"),
        argument_digest: format!("argument-{label}"),
        snapshot_digest: format!("snapshot-{label}"),
        policy_safety_snapshot_ref: None,
        secret_ref_evidence: Vec::new(),
        decision_reason: PermissionPolicyReason::EvaluatorUncertain,
        classifier_verdict: AutoEvaluatorVerdictKind::DenyCandidate,
        classifier_confidence: EvaluatorConfidence::High,
        classifier_scope_match: EvaluatorScopeMatch::Unrelated,
        retryable: true,
    }
}

fn sample_secret_ref_evidence(token: &str) -> PermissionSecretRefEvidence {
    let secret_ref = SecretRef {
        kind: SecretRefKind::SecretRef,
        schema_version: 1,
        ref_id: SecretRefId::new("sec_spec030_recent_retry"),
        source_kind: SecretSourceKind::Env,
        locator: SecretLocator::EnvVar {
            name: "SPEC030_API_KEY".to_owned(),
        },
        owner: "spec035-config-profile".to_owned(),
        scope: "provider-auth".to_owned(),
        created_by: Some("config-profile".to_owned()),
        created_at_ms: Some(0),
        locator_digest: "sha256:recent-retry-locator".to_owned(),
        staleness_token: token.to_owned(),
        safe_summary: SafeSecretSummary {
            label: "env:SPEC030_API_KEY".to_owned(),
            required: true,
        },
    };
    PermissionSecretRefEvidence {
        secret_ref: secret_ref.clone(),
        redaction_evidence: RedactionEvidence::for_secret_ref(
            RedactionEvidenceRef::new("red_spec030_recent_retry"),
            secret_ref.ref_id,
            "recent_retry",
            "sha256:safe-summary",
        ),
        status: PermissionSecretRefStatus::Unresolved,
        requested_consumer: "tool:exec".to_owned(),
    }
}

fn policy_safety_ref(label: &str) -> PolicySafetySnapshotRef {
    PolicySafetySnapshotRef {
        schema_id: PolicySafetySnapshotSchemaId::V1,
        snapshot_id: PolicySafetySnapshotId(format!("snapshot-{label}")),
        policy_safety_digest: PolicySafetyDigest(
            "1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
        ),
        created_at_unix_ms: 500,
        expires_at_unix_ms: None,
        redacted_summary: RedactedPolicySafetySummary {
            permission_mode: "auto".to_owned(),
            capability_count: 1,
            containment_digest: Some(format!("containment-{label}")),
            source_ref_count: 2,
            provenance_ref_count: 1,
        },
    }
}

fn recent_retry_loop(
    interactive: bool,
) -> Result<(AgentLoop<'static>, Arc<AtomicUsize>, tempfile::TempDir), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let registry = Box::leak(Box::new(registry));
    let client = Box::leak(Box::new(MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-loop-recent-retry",
            "exec",
            json!({ "command": "cargo test" }),
        ))?,
        classifier_verdict_response("deny_candidate", "high", "requested"),
        LlmResponse {
            content: Some("recent retry completed".to_owned()),
            ..LlmResponse::default()
        },
    ])));
    let config = recent_retry_config(workspace.path(), interactive);
    let loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        registry,
        client,
        config,
    );
    Ok((loop_runtime, calls, workspace))
}

fn recent_retry_config(workspace: &std::path::Path, interactive: bool) -> AgentLoopConfig {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = interactive;
    config.permission_rule_input = confirmed_non_privileged_permission_input();
    config.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(true),
        backend: Some("official-container".to_owned()),
        digest: Some("test-contained".to_owned()),
        summary: Some("non-privileged test containment".to_owned()),
    });
    config.permission_auto_approval = AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    };
    config
}

fn recent_denial_id_from_output(content: &str) -> Result<String, Box<dyn Error>> {
    content
        .split_whitespace()
        .find_map(|part| part.strip_prefix("id=").map(str::to_owned))
        .ok_or_else(|| format!("missing denial id in recent output: {content}").into())
}

#[test]
fn agent_runner_auto_classifier_summarizes_simple_contained_exec_before_classifier(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-classified-pwd",
            "exec",
            json!({ "command": "pwd" }),
        ))?,
        LlmResponse {
            content: Some("classifier allowed pwd".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;
    let diagnostic_details = Arc::new(Mutex::new(Vec::<String>::new()));
    let diagnostic_details_capture = diagnostic_details.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut details) = diagnostic_details_capture.lock() {
                details.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    let classifier_prompt = classifier_requests
        .first()
        .map(|request| {
            request
                .messages
                .iter()
                .filter_map(|message| message.get("content").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let diagnostic_details = diagnostic_details
        .lock()
        .map_err(|error| error.to_string())?;
    let diagnostic_text = diagnostic_details.join("\n");
    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("classifier allowed pwd")
        || classifier_requests.len() != 1
        || !classifier_prompt.contains("confidence must be \"high\"")
        || !classifier_prompt.contains("scope_match must be \"requested\"")
        || !diagnostic_text.contains("\"evaluator_source\":\"permission_classifier\"")
        || diagnostic_text.contains("classifier:test")
        || diagnostic_text.contains("evaluator_ref")
    {
        return Err(format!(
            "simple contained exec should reach classifier with redacted diagnostics despite missing proc exec summary: result={result:?} calls={} classifier_requests={classifier_requests:?} diagnostic_details={diagnostic_details:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_diagnostic_source_ignores_spoofed_evaluator_ref(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-classifier-spoof-source",
            "exec",
            json!({ "command": "pwd" }),
        ))?,
        LlmResponse {
            content: Some("classifier allowed spoof source test".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some(
            json!({
                "verdict": "allow_candidate",
                "confidence": "high",
                "scope_match": "requested",
                "risk_summary": "test spoofed evaluator source",
                "evidence_refs": ["classifier:test"],
                "evaluator_ref": "local-auto-approval"
            })
            .to_string(),
        ),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;
    let diagnostic_details = Arc::new(Mutex::new(Vec::<String>::new()));
    let diagnostic_details_capture = diagnostic_details.clone();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        if event.name == "permission_auto_approval" {
            if let Ok(mut details) = diagnostic_details_capture.lock() {
                details.push(event.detail.clone());
            }
        }
    }));

    let result = AgentRunner::new().run(spec)?;
    let diagnostic_text = diagnostic_details
        .lock()
        .map_err(|error| error.to_string())?
        .join("\n");
    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("classifier allowed spoof source test")
        || !diagnostic_text.contains("\"evaluator_source\":\"permission_classifier\"")
        || diagnostic_text.contains("local_auto_approval")
        || diagnostic_text.contains("local-auto-approval")
    {
        return Err(format!(
            "classifier-controlled evaluator_ref should not spoof diagnostic source: result={result:?} calls={} diagnostics={diagnostic_text}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_accepts_fenced_json_verdict() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "exec-classified-fenced",
            "exec",
            json!({ "command": "pwd" }),
        ))?,
        LlmResponse {
            content: Some("classifier allowed fenced json".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some(format!(
            "```json\n{}\n```",
            json!({
                "verdict": "allow_candidate",
                "confidence": "high",
                "scope_match": "requested",
                "risk_summary": "test fenced classifier verdict",
                "evidence_refs": ["classifier:test"],
                "evaluator_ref": "classifier:test"
            })
        )),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("classifier allowed fenced json")
        || classifier_requests.len() != 1
    {
        return Err(format!(
            "fenced classifier JSON should parse and allow requested exec: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_keeps_prose_wrapped_json_verdict_approval_gated(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classified-prose-json",
            "exec",
            json!({ "command": "pwd" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some(format!(
            "The action is in scope.\n{}\nNo further concerns.",
            json!({
                "verdict": "allow_candidate",
                "confidence": "high",
                "scope_match": "requested",
                "risk_summary": "test prose wrapped classifier verdict",
                "evidence_refs": ["classifier:test"],
                "evaluator_ref": "classifier:test"
            })
        )),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "prose-wrapped classifier JSON should fail closed instead of allowing exec: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_keeps_nested_json_verdict_approval_gated(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classified-nested",
            "exec",
            json!({ "command": "pwd" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some(
            json!({
                "classification": {
                    "verdict": "allow_candidate",
                    "confidence": "high",
                    "scope_match": "requested",
                    "risk_summary": "test nested classifier verdict",
                    "evidence_refs": ["classifier:test"],
                    "evaluator_ref": "classifier:test"
                }
            })
            .to_string(),
        ),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "nested classifier JSON should fail closed instead of allowing exec: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_keeps_alias_json_verdict_approval_gated(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new("exec-classified-alias", "exec", json!({ "command": "pwd" })),
    )?]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some(
            json!({
                "allow": true,
                "confidence": "high",
                "scope": "in_scope",
                "reason": "test alias classifier verdict",
                "evidence": ["classifier:test"],
                "evaluator": "classifier:test"
            })
            .to_string(),
        ),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "alias classifier JSON should fail closed instead of allowing exec: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_allows_only_exact_proc_exec_verification_commands(
) -> Result<(), Box<dyn Error>> {
    for command in [
        "pwd",
        "cargo check",
        "cargo test",
        "cargo clippy",
        "cargo build",
        "cargo fmt --check",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(ProcExecCountingTool {
            calls: calls.clone(),
        });
        let client = MockProvider::new(vec![
            response_with_runtime_tool_call(RuntimeToolCall::new(
                "exec-exact-verification",
                "exec",
                json!({ "command": command }),
            ))?,
            LlmResponse {
                content: Some("classifier allowed exact verification command".to_owned()),
                ..LlmResponse::default()
            },
        ]);
        let classifier = MockProvider::new(vec![classifier_verdict_response(
            "allow_candidate",
            "high",
            "requested",
        )]);
        let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
        let mut context = interactive_auto_context(AutoApprovalConfig {
            enabled: true,
            allow_proc_exec_verification: false,
            ..AutoApprovalConfig::default()
        });
        context.permission_rule_input.proc_exec_summary = None;
        spec.tool_context = context;

        let result = AgentRunner::new().run(spec)?;
        let classifier_requests = classifier
            .requests
            .lock()
            .map_err(|error| error.to_string())?;
        if calls.load(Ordering::SeqCst) != 1
            || classifier_requests.len() != 1
            || result.final_content.as_deref()
                != Some("classifier allowed exact verification command")
        {
            return Err(format!(
                "exact verification command should reach classifier and execute: command={command:?} result={result:?} calls={} classifier_requests={classifier_requests:?}",
                calls.load(Ordering::SeqCst)
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_keeps_non_exact_proc_exec_commands_approval_gated(
) -> Result<(), Box<dyn Error>> {
    let overlong_padded_pwd = format!("{}pwd", " ".repeat(201));
    for command in [
        "",
        "   ",
        "pwd; true",
        "pwd\ntrue",
        "cargo test | cat",
        overlong_padded_pwd.as_str(),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(ProcExecCountingTool {
            calls: calls.clone(),
        });
        let client = MockProvider::new(vec![
            response_with_runtime_tool_call(RuntimeToolCall::new(
                "exec-non-exact-verification",
                "exec",
                json!({ "command": command }),
            ))?,
            LlmResponse {
                content: Some("non-exact command blocked".to_owned()),
                ..LlmResponse::default()
            },
        ]);
        let classifier = MockProvider::new(Vec::new());
        let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
        let mut context = interactive_auto_context(AutoApprovalConfig {
            enabled: true,
            allow_proc_exec_verification: false,
            ..AutoApprovalConfig::default()
        });
        context.permission_rule_input.proc_exec_summary = None;
        spec.tool_context = context;

        let result = AgentRunner::new().run(spec)?;
        let classifier_requests = classifier
            .requests
            .lock()
            .map_err(|error| error.to_string())?;
        if calls.load(Ordering::SeqCst) != 0
            || !classifier_requests.is_empty()
            || (result.final_content.as_deref() != Some("non-exact command blocked")
                && !matches!(
                    result.interrupt,
                    Some(RuntimeInterrupt::PermissionApproval { .. })
                ))
        {
            return Err(format!(
                "non-exact proc exec command should stay approval-gated without classifier: command={command:?} result={result:?} calls={} classifier_requests={classifier_requests:?}",
                calls.load(Ordering::SeqCst)
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_keeps_unsummarized_exec_approval_gated(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-unsummarized",
            "exec",
            json!({ "command": "rm .git/config" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || !classifier_requests.is_empty()
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "unsummarized exec should remain approval-gated without classifier: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_keeps_cargo_with_untrusted_options_approval_gated(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-cargo-config",
            "exec",
            json!({ "command": "cargo test --config build.rustc-wrapper=/bin/false" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "ask_user", "medium", "adjacent",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    let mut context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });
    context.permission_rule_input.proc_exec_summary = None;
    spec.tool_context = context;

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || !classifier_requests.is_empty()
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
    {
        return Err(format!(
            "cargo commands with untrusted options should remain approval-gated without classifier: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_requires_user_intent_before_allowing() -> Result<(), Box<dyn Error>>
{
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-no-intent",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec =
        classifier_agent_run_spec_with_messages(Vec::new(), &registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || !classifier_requests.is_empty()
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "classifier should require user intent before allowing execution: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_unsafe_capability_skips_classifier_and_execution(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(MessageCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "message-classifier-high-allow",
            "message",
            json!({ "target": "user", "content": "hello" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "high",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || !classifier_requests.is_empty()
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "unsafe classifier capability should remain approval-gated: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_provider_error_interrupts_without_executing_in_interactive_mode(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-error",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(Vec::new());
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "classifier provider error should fall back to approval interrupt without execution: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_uncertain_verdict_interrupts_without_executing_in_interactive_mode(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-uncertain",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![classifier_verdict_response(
        "allow_candidate",
        "low",
        "requested",
    )]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "low-confidence classifier verdict should ask instead of executing: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_malformed_verdict_interrupts_without_executing_in_interactive_mode(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-malformed",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some("not json".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "malformed classifier verdict should ask instead of executing: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_classifier_prompt_injection_signal_interrupts_without_executing(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![response_with_runtime_tool_call(
        RuntimeToolCall::new(
            "exec-classifier-injection-signal",
            "exec",
            json!({ "command": "cargo test" }),
        ),
    )?]);
    let classifier = MockProvider::new(vec![LlmResponse {
        content: Some(
            json!({
                "verdict": "allow_candidate",
                "confidence": "high",
                "scope_match": "requested",
                "risk_summary": "test classifier prompt injection signal",
                "evidence_refs": ["classifier:test"],
                "evaluator_ref": "classifier:test",
                "prompt_injection_signals": [{
                    "source_ref": "message:test",
                    "reason": "attempted to widen requested command scope",
                    "confidence": "high"
                }]
            })
            .to_string(),
        ),
        ..LlmResponse::default()
    }]);
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || classifier_requests.len() != 1
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "prompt-injection classifier signal should ask instead of executing: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

#[test]
fn agent_runner_auto_static_protected_target_asks_without_classifier_or_execution(
) -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(WriteFileCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        response_with_runtime_tool_call(RuntimeToolCall::new(
            "write-protected-target",
            "write_file",
            json!({ "path": "src/lib.rs", "content": "no" }),
        ))?,
        LlmResponse {
            content: Some("protected target denied".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let classifier = MockProvider::new(Vec::new());
    let mut spec = classifier_agent_run_spec(&registry, &client, &classifier);
    spec.tool_context = interactive_auto_context(AutoApprovalConfig {
        enabled: true,
        allow_workspace_edits: true,
        protected_targets: vec!["src".to_owned()],
        ..AutoApprovalConfig::default()
    });

    let result = AgentRunner::new().run(spec)?;
    let classifier_requests = classifier
        .requests
        .lock()
        .map_err(|error| error.to_string())?;
    if calls.load(Ordering::SeqCst) != 0
        || !classifier_requests.is_empty()
        || !matches!(
            result.interrupt,
            Some(RuntimeInterrupt::PermissionApproval { .. })
        )
        || result.stop_reason != "ask_user"
    {
        return Err(format!(
            "protected static rule should ask without classifier or execution: result={result:?} calls={} classifier_requests={classifier_requests:?}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    Ok(())
}

fn classifier_agent_run_spec<'a>(
    registry: &'a ToolRegistry,
    client: &'a dyn ProviderClient,
    classifier: &'a dyn ProviderClient,
) -> AgentRunSpec<'a> {
    classifier_agent_run_spec_with_messages(
        vec![json!({ "role": "user", "content": "use a tool" })],
        registry,
        client,
        classifier,
    )
}

fn classifier_agent_run_spec_with_messages<'a>(
    initial_messages: Vec<Value>,
    registry: &'a ToolRegistry,
    client: &'a dyn ProviderClient,
    classifier: &'a dyn ProviderClient,
) -> AgentRunSpec<'a> {
    let mut spec = AgentRunSpec::new(initial_messages, registry, client, "test-model");
    spec.max_iterations = 3;
    spec.permission_classifier_client = Some(classifier);
    spec
}

fn interactive_auto_context(permission_auto_approval: AutoApprovalConfig) -> ToolExecutionContext {
    ToolExecutionContext {
        containment_snapshot: Some(ContainmentSnapshotRef {
            contained: Some(true),
            backend: None,
            digest: Some("test-contained".to_owned()),
            summary: Some("non-privileged test containment".to_owned()),
        }),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode: PermissionMode::Auto,
            source: Some("test".to_owned()),
            scope_ref: None,
        },
        permission_rule_input: confirmed_non_privileged_permission_input(),
        permission_auto_approval,
        permission_interactive: true,
        ..ToolExecutionContext::default()
    }
}

fn response_with_runtime_tool_call(call: RuntimeToolCall) -> Result<LlmResponse, Box<dyn Error>> {
    let arguments = match call.arguments {
        Value::Object(arguments) => arguments,
        other => {
            return Err(format!("test tool call arguments must be an object: {other:?}").into())
        }
    };
    Ok(LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(call.id, call.name, arguments)],
        ..LlmResponse::default()
    })
}

fn classifier_verdict_response(verdict: &str, confidence: &str, scope_match: &str) -> LlmResponse {
    LlmResponse {
        content: Some(
            json!({
                "verdict": verdict,
                "confidence": confidence,
                "scope_match": scope_match,
                "risk_summary": "test classifier verdict",
                "evidence_refs": ["classifier:test"],
                "evaluator_ref": "classifier:test"
            })
            .to_string(),
        ),
        ..LlmResponse::default()
    }
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
    let ledger = runtime.execution_ledger_snapshot();
    if ledger.outcomes.len() != 1
        || !matches!(
            ledger.outcomes[0].decision,
            LateResultDecision::DiscardedStale { .. }
        )
        || ledger.outcomes[0].fact.outcome != ExecutionOutcome::Subagent(SubagentOutcomeKind::Stale)
    {
        return Err(format!("stale subagent ledger drifted: {ledger:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_optional_ids_do_not_override_authoritative_four_field_correlation(
) -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect authoritative fields".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let mut stale = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Wrong session summary",
    );
    stale.session_id = "session-2".to_owned();
    stale.correlation_id = outcome.envelope.correlation_id.clone();
    stale.idempotency_key = outcome.envelope.idempotency_key.clone();

    let decision = runtime.publish_child_result(stale);

    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("parent session mismatch"))
        || runtime.running_count() != 1
        || bus.try_consume_inbound().is_some()
    {
        return Err(
            format!("four-field correlation should remain authoritative: {decision:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn subagent_second_delivery_is_classified_as_duplicate_without_republishing(
) -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize once".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let result = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "First summary",
    );
    let first = runtime.publish_child_result(result.clone());
    let _ = bus.consume_inbound().ok_or("missing first reentry")?;

    let second = runtime.publish_child_result(result);
    let ledger = runtime.execution_ledger_snapshot();

    if first != MergeDecision::AcceptSummaryOnly
        || !matches!(second, MergeDecision::DiscardAsDuplicate { .. })
        || bus.try_consume_inbound().is_some()
        || ledger.outcomes.len() != 2
        || !matches!(
            ledger.outcomes[1].decision,
            LateResultDecision::DuplicateIgnored { .. }
        )
    {
        return Err(format!(
            "duplicate subagent delivery drifted: second={second:?} ledger={ledger:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn subagent_stale_and_duplicate_results_never_expose_terminal_automation_metadata(
) -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Accept once".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let result = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Accepted summary",
    );
    assert_eq!(
        runtime.publish_child_result(result.clone()),
        MergeDecision::AcceptSummaryOnly
    );
    let accepted = bus.consume_inbound().ok_or("missing accepted reentry")?;
    assert!(accepted.owner_accepted_automation_result().is_some());

    assert!(matches!(
        runtime.publish_child_result(result),
        MergeDecision::DiscardAsDuplicate { .. }
    ));
    let mut stale = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Stale summary",
    );
    stale.parent_turn_id = "turn:stale".to_owned();
    assert!(matches!(
        runtime.publish_child_result(stale),
        MergeDecision::DiscardAsStale { .. }
    ));
    assert!(bus.try_consume_inbound().is_none());
    Ok(())
}

#[test]
fn subagent_later_attempt_after_timeout_is_classified_as_late_without_republishing(
) -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Timeout once".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let timed_out = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::TimedOut,
        "Timed out",
    );
    let first = runtime.publish_child_result(timed_out);
    let _ = bus.consume_inbound().ok_or("missing timeout reentry")?;
    let mut completed_late = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Late summary",
    );
    completed_late.attempt_id = Some("attempt:2".to_owned());

    let second = runtime.publish_child_result(completed_late);
    let ledger = runtime.execution_ledger_snapshot();

    if first != MergeDecision::RetryChild
        || !matches!(second, MergeDecision::DiscardAsLate { .. })
        || bus.try_consume_inbound().is_some()
        || !matches!(
            ledger.outcomes.last().map(|record| &record.decision),
            Some(LateResultDecision::DiscardedLate { .. })
        )
    {
        return Err(
            format!("late subagent delivery drifted: second={second:?} ledger={ledger:?}").into(),
        );
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    let pending_raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval")
        .ok_or("missing pending approval session")?;
    if first.ask_user_options.as_slice()
        != [
            "approve",
            "deny",
            "approve_session",
            "approve_project",
            "deny_session",
            "deny_project",
        ]
        || pending_raw["metadata"]["pending_permission_approval"]["approval_request"]
            ["allowed_decisions"]
            != json!([
                "approved",
                "denied",
                "approved_for_session",
                "approved_for_project",
                "denied_for_session",
                "denied_for_project"
            ])
    {
        return Err(format!(
            "approval interrupt did not expose structured approval options: first={first:?} raw={pending_raw:?}"
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
fn loop_permission_approval_by_lineage_executes_pending_tool_after_restart(
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
                "exec-lineage",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after lineage approval".to_owned()),
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

    let first = loop_runtime.process_direct("start", Some("discord:lineage-approval"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("permission approval did not pause: {first:?}").into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:lineage-approval")
        .ok_or("missing pending approval session")?;
    let lineage = raw["metadata"]["pending_permission_approval"]["approval_request_id"]
        .as_str()
        .ok_or("missing approval lineage")?
        .to_owned();

    drop(loop_runtime);
    let restarted_client = MockProvider::new(vec![LlmResponse {
        content: Some("resumed after lineage approval".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut restarted_config = AgentLoopConfig::new(workspace.path(), "test-model");
    restarted_config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    restarted_config.permission_interactive = true;
    let mut restarted = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &restarted_client,
        restarted_config,
    );

    let outcome = restarted.process_permission_approval_by_lineage(
        "discord:lineage-approval",
        &lineage,
        true,
    )?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Completed);
    assert!(outcome.changed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn loop_permission_approval_by_lineage_rejects_stale_lineage() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "exec-stale-lineage",
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
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_direct("start", Some("discord:stale-lineage"))?;
    let outcome = loop_runtime.process_permission_approval_by_lineage(
        "discord:stale-lineage",
        "stale-lineage",
        true,
    )?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::StaleLineage);
    assert!(!outcome.changed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn loop_permission_approval_by_lineage_revalidates_replaced_pending_under_turn_lock(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-replaced-lineage",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after replaced lineage".to_owned()),
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
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_direct("start", Some("discord:lineage-race"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:lineage-race")
        .ok_or("missing pending approval session")?;
    let stale_lineage = raw["metadata"]["pending_permission_approval"]["approval_request_id"]
        .as_str()
        .ok_or("missing approval lineage")?
        .to_owned();
    replace_pending_approval_lineage(&mut loop_runtime, "discord:lineage-race", "approval-b")?;

    let stale = loop_runtime.process_permission_approval_by_lineage(
        "discord:lineage-race",
        &stale_lineage,
        true,
    )?;

    assert_eq!(stale.kind, SurfaceActionOutcomeKind::StaleLineage);
    assert!(!stale.changed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let still_pending = loop_runtime
        .session_manager()
        .read_session_file("discord:lineage-race")
        .ok_or("missing pending approval session")?;
    assert_eq!(
        still_pending["metadata"]["pending_permission_approval"]["approval_request_id"],
        json!("approval-b")
    );

    let accepted = loop_runtime.process_permission_approval_by_lineage(
        "discord:lineage-race",
        "approval-b",
        true,
    )?;

    assert_eq!(accepted.kind, SurfaceActionOutcomeKind::Completed);
    assert!(accepted.changed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn loop_permission_approval_by_lineage_rejects_expired_pending_under_turn_lock(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecCountingTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "exec-expired-lineage",
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
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_direct("start", Some("discord:expired-lineage"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:expired-lineage")
        .ok_or("missing pending approval session")?;
    let lineage = raw["metadata"]["pending_permission_approval"]["approval_request_id"]
        .as_str()
        .ok_or("missing approval lineage")?
        .to_owned();
    expire_pending_approval(&mut loop_runtime, "discord:expired-lineage")?;

    let expired = loop_runtime.process_permission_approval_by_lineage(
        "discord:expired-lineage",
        &lineage,
        true,
    )?;

    assert_eq!(expired.kind, SurfaceActionOutcomeKind::Unavailable);
    assert!(!expired.changed);
    assert!(expired.detail.contains("expired"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

fn replace_pending_approval_lineage(
    loop_runtime: &mut AgentLoop,
    session_key: &str,
    lineage: &str,
) -> Result<(), Box<dyn Error>> {
    let manager = loop_runtime.session_manager_mut();
    let mut session = manager.get_or_create(session_key);
    let approval = session
        .metadata
        .get_mut("pending_permission_approval")
        .and_then(Value::as_object_mut)
        .ok_or("missing pending approval")?;
    approval.insert("approval_request_id".to_owned(), json!(lineage));
    if let Some(request) = approval
        .get_mut("approval_request")
        .and_then(Value::as_object_mut)
    {
        request.insert("approval_request_id".to_owned(), json!(lineage));
    }
    manager.save(&session)?;
    Ok(())
}

fn expire_pending_approval(
    loop_runtime: &mut AgentLoop,
    session_key: &str,
) -> Result<(), Box<dyn Error>> {
    let manager = loop_runtime.session_manager_mut();
    let mut session = manager.get_or_create(session_key);
    let approval = session
        .metadata
        .get_mut("pending_permission_approval")
        .and_then(Value::as_object_mut)
        .ok_or("missing pending approval")?;
    if let Some(request) = approval
        .get_mut("approval_request")
        .and_then(Value::as_object_mut)
    {
        request.insert("expires_at_unix_ms".to_owned(), json!(1));
    }
    manager.save(&session)?;
    Ok(())
}

#[test]
fn loop_permission_approval_normalizes_artifact_and_records_tool_outcome(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecLargeOutputTool);
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-large-approved",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed after large exec".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.max_tool_result_chars = 10;
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let first = loop_runtime.process_direct("start", Some("cli:approved-artifact"))?;
    if first.stop_reason != "ask_user" {
        return Err(format!("large exec should pause for approval: {first:?}").into());
    }
    let second = loop_runtime.process_direct("approve", Some("cli:approved-artifact"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:approved-artifact")
        .ok_or("missing approved artifact session")?;
    let tool_message = raw["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|message| message["tool_call_id"] == "exec-large-approved")
        .ok_or("missing approved tool message")?;
    let tool_content = tool_message["content"].as_str().unwrap_or_default();
    let outcomes = raw["metadata"]["runtime_execution"]["outcomes"]
        .as_array()
        .ok_or("missing runtime execution outcomes")?;
    if second.final_content.as_deref() != Some("resumed after large exec")
        || tool_content.len() >= "sensitive-output".len() * 128
        || !tool_content.contains(".nanobot/tool-results/")
        || !outcomes.iter().any(|record| {
            record["fact"]["outcome"]["domain"] == "tool"
                && record["fact"]["outcome"]["outcome"]["kind"] == "completed"
                && record["fact"]["artifact_ref"]["locator"]
                    .as_str()
                    .is_some_and(|locator| locator.starts_with(".nanobot/tool-results/"))
                && record["decision"]["kind"] == "accepted"
        })
    {
        return Err(format!(
            "approved tool result bypassed normalization or ledger recording: second={second:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_permission_approval_stops_after_fatal_tool_outcome() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(ProcExecMcpFailureTool {
        calls: calls.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-fatal-approved",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("provider must not resume".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.fail_on_tool_error = true;
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = true;
    let events = Arc::new(Mutex::new(Vec::<ToolEvent>::new()));
    let event_capture = events.clone();
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_tool_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = event_capture.lock() {
            events.push(event.clone());
        }
    }));

    let first = loop_runtime.process_direct("start", Some("cli:approved-fatal"))?;
    let second = loop_runtime.process_direct("approve", Some("cli:approved-fatal"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:approved-fatal")
        .ok_or("missing approved fatal session")?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let events = events.lock().map_err(|error| error.to_string())?;
    if first.stop_reason != "ask_user"
        || second.stop_reason != "tool_error"
        || second
            .final_content
            .as_deref()
            .is_none_or(|content| !content.contains("MCP tool call failed"))
        || calls.load(Ordering::SeqCst) != 1
        || requests.len() != 1
        || !events
            .iter()
            .any(|event| event.name == "exec" && event.status == ToolStatus::Error)
        || !raw["metadata"]["runtime_execution"]["outcomes"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|record| {
                record["fact"]["outcome"]["domain"] == "tool"
                    && record["fact"]["outcome"]["outcome"]["kind"] == "failed"
                    && record["fact"]["outcome"]["outcome"]["class"] == "fatal"
            })
    {
        return Err(format!(
            "approved fatal tool resumed provider or lost outcome: first={first:?} second={second:?} raw={raw:?} requests={requests:?}"
        )
        .into());
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
            "session remembered approval did not reuse matching action: {reused:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:approval-session")
        .ok_or("missing session approval session")?;
    if raw["metadata"]["session_remembered_permissions_v1"]["rules"]
        .as_array()
        .map(Vec::len)
        != Some(1)
        || raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["session_key"]
            != "discord:approval-session"
        || !raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]
            ["approval_context_digest"]
            .is_string()
        || raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["matcher"]["kind"]
            != "exec_prefix"
        || raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["matcher"]["tokens"]
            != json!(["cargo", "fmt"])
        || raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["effect"] != "allow"
    {
        return Err(format!("session approval metadata drifted: {raw:?}").into());
    }

    let _reused_outbound = bus
        .consume_outbound()
        .ok_or("missing changed-ref approval outbound")?;
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
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
    let _outbound = bus
        .consume_outbound()
        .ok_or("missing second session approval outbound")?;
    if second_session.ask_user_options.as_slice()
        != [
            "approve",
            "deny",
            "approve_session",
            "approve_project",
            "deny_session",
            "deny_project",
        ]
    {
        return Err(format!("second session did not ask for approval: {second_session:?}").into());
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
        || raw["metadata"]
            .get("session_remembered_permissions_v1")
            .and_then(|value| value.get("rules"))
            .and_then(Value::as_array)
            .is_some_and(|rules| !rules.is_empty())
    {
        return Err(format!("/new left session permission rules in metadata: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn remembered_session_permission_allow_survives_legacy_request_expiry() -> Result<(), Box<dyn Error>>
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
                "exec-remember-allow-1",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("approved remembered session".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-remember-allow-2",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo fmt --check"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("reused remembered session".to_owned()),
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

    let first = loop_runtime.process_direct("start", Some("discord:remembered-allow"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("remembered allow fixture did not pause: {first:?}").into());
    }
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    let approved =
        loop_runtime.process_direct("approve_session", Some("discord:remembered-allow"))?;
    if calls.load(Ordering::SeqCst) != 1
        || approved.final_content.as_deref() != Some("approved remembered session")
    {
        return Err(format!("remembered allow approval failed: {approved:?}").into());
    }
    let _approved_outbound = bus.consume_outbound().ok_or("missing approved outbound")?;

    let mut session = loop_runtime
        .session_manager()
        .load_existing("discord:remembered-allow")
        .ok_or("missing remembered allow session")?;
    if let Some(entry) = session
        .metadata
        .get_mut("session_permission_approvals")
        .and_then(Value::as_array_mut)
        .and_then(|entries| entries.first_mut())
    {
        entry["approval"]["request"]["expires_at_unix_ms"] = json!(0);
    }
    loop_runtime.session_manager_mut().save(&session)?;

    let reused = loop_runtime.process_direct("again", Some("discord:remembered-allow"))?;
    if reused.stop_reason == "ask_user"
        || calls.load(Ordering::SeqCst) != 2
        || reused.final_content.as_deref() != Some("reused remembered session")
    {
        return Err(format!(
            "remembered session allow did not outlive legacy expiry: {reused:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:remembered-allow")
        .ok_or("missing remembered allow raw session")?;
    if raw["metadata"]["session_remembered_permissions_v1"]["rules"]
        .as_array()
        .map(Vec::len)
        != Some(1)
    {
        return Err(format!("remembered allow rule was not persisted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn remembered_session_permission_deny_cancels_matching_action_without_prompt(
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
                "exec-remember-deny-1",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-remember-deny-2",
                "exec",
                Map::from_iter([("command".to_owned(), json!("cargo test --workspace"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("denied by remembered session".to_owned()),
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

    let first = loop_runtime.process_direct("start", Some("discord:remembered-deny"))?;
    if first.stop_reason != "ask_user" || calls.load(Ordering::SeqCst) != 0 {
        return Err(format!("remembered deny fixture did not pause: {first:?}").into());
    }
    let _approval_outbound = bus
        .consume_outbound()
        .ok_or("missing deny approval outbound")?;
    let denied = loop_runtime.process_direct("deny_session", Some("discord:remembered-deny"))?;
    if denied.final_content.as_deref() != Some("Tool execution cancelled.")
        || calls.load(Ordering::SeqCst) != 0
    {
        return Err(format!("remembered deny reply executed tool: {denied:?}").into());
    }
    let _denied_outbound = bus.consume_outbound().ok_or("missing denied outbound")?;

    let reused = loop_runtime.process_direct("again", Some("discord:remembered-deny"))?;
    if reused.stop_reason == "ask_user"
        || calls.load(Ordering::SeqCst) != 0
        || reused.final_content.as_deref() != Some("denied by remembered session")
    {
        return Err(format!(
            "remembered session deny did not cancel without prompting: {reused:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("discord:remembered-deny")
        .ok_or("missing remembered deny raw session")?;
    if raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["effect"] != "deny" {
        return Err(format!("remembered deny rule was not persisted: {raw:?}").into());
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
fn loop_runtime_checkpoint_redacts_pending_tool_arguments_while_tool_runs(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_checkpoint = Arc::new(Mutex::new(None));
    let mut registry = ToolRegistry::new();
    registry.register(CheckpointMetadataProbeTool {
        workspace: workspace.path().to_path_buf(),
        session_key: "cli:checkpoint-redaction",
        calls: calls.clone(),
        observed_checkpoint: observed_checkpoint.clone(),
    });
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "exec-checkpoint-redaction",
                "exec",
                Map::from_iter([("command".to_owned(), json!("RAW_CHECKPOINT_COMMAND_SECRET"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("checkpoint redaction complete".to_owned()),
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
    );

    let result =
        loop_runtime.process_direct("run checkpoint probe", Some("cli:checkpoint-redaction"))?;
    let observed = observed_checkpoint
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or("checkpoint probe did not observe checkpoint metadata")?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:checkpoint-redaction")
        .ok_or("missing checkpoint redaction session")?;
    if calls.load(Ordering::SeqCst) != 1
        || result.final_content.as_deref() != Some("checkpoint redaction complete")
        || raw["metadata"].get("runtime_checkpoint").is_some()
        || observed.contains("RAW_CHECKPOINT_COMMAND_SECRET")
        || observed.contains("\\\"command\\\"")
        || !observed.contains("<redacted>")
    {
        return Err(format!(
            "runtime checkpoint persisted raw tool arguments: result={result:?} observed={observed} raw={raw:?} calls={}",
            calls.load(Ordering::SeqCst)
        )
        .into());
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
fn loop_priority_restart_bypasses_active_session_lock() -> Result<(), Box<dyn Error>> {
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
        "telegram", "user-1", "chat-1", "/restart",
    ))?;
    assert_eq!(
        result.command,
        Some(AgentLoopCommandResult::RestartRequested)
    );
    assert_eq!(result.stop_reason, "restart_requested");
    let raw = loop_runtime
        .session_manager()
        .read_session_file("telegram:chat-1")
        .ok_or("missing active session")?;
    assert_eq!(raw["metadata"]["pending_user_turn"], true);
    assert_eq!(raw["messages"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn loop_priority_stop_cancels_active_session_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let cancellation = turn_lock
        .cancellation_token("telegram:chat-1")
        .ok_or("missing active turn cancellation token")?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result = loop_runtime
        .process_message(InboundMessage::new("telegram", "user-1", "chat-1", "/stop"))?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::StopRequested));
    assert!(cancellation.is_cancelled());
    Ok(())
}

#[test]
fn loop_priority_stop_cancels_reserved_session_before_bind() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let turn_lock = SessionTurnLock::new();
    let reservation = turn_lock.reserve("telegram:chat-1");
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("must not run".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock.clone());

    let stop = loop_runtime
        .process_message(InboundMessage::new("telegram", "user-1", "chat-1", "/stop"))?;
    assert_eq!(stop.command, Some(AgentLoopCommandResult::StopRequested));
    reservation.bind_to_current_thread();
    assert!(matches!(
        turn_lock.acquire("telegram:chat-1"),
        Err(SessionTurnAcquireError::Cancelled { ref session_key }) if session_key == "telegram:chat-1"
    ));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        0
    );
    Ok(())
}

#[test]
fn loop_priority_status_and_restart_do_not_cancel_reserved_session() -> Result<(), Box<dyn Error>> {
    for command in ["/status", "/restart"] {
        let workspace = tempfile::tempdir()?;
        let turn_lock = SessionTurnLock::new();
        let reservation = turn_lock.reserve("telegram:chat-1");
        let registry = ToolRegistry::new();
        let client = MockProvider::new(Vec::new());
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &registry,
            &client,
            AgentLoopConfig::new(workspace.path(), "test-model"),
        )
        .with_session_turn_lock(turn_lock.clone());

        let result = loop_runtime
            .process_message(InboundMessage::new("telegram", "user-1", "chat-1", command))?;
        assert!(matches!(
            result.command,
            Some(AgentLoopCommandResult::Status | AgentLoopCommandResult::RestartRequested)
        ));
        reservation.bind_to_current_thread();
        let _guard = turn_lock
            .acquire("telegram:chat-1")
            .map_err(|error| format!("{command} should not cancel reservation: {error:?}"))?;
    }
    Ok(())
}

#[test]
fn loop_priority_stop_does_not_cancel_reserved_other_session() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let turn_lock = SessionTurnLock::new();
    let reservation = turn_lock.reserve("telegram:chat-1");
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock.clone());

    let stop = loop_runtime
        .process_message(InboundMessage::new("telegram", "user-1", "chat-2", "/stop"))?;
    assert_eq!(stop.command, Some(AgentLoopCommandResult::StopRequested));
    reservation.bind_to_current_thread();
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("wrong-session /stop should not cancel reservation: {error:?}"))?;
    Ok(())
}

#[test]
fn session_turn_lock_applies_cancellation_requested_before_acquire() -> Result<(), Box<dyn Error>> {
    let turn_lock = SessionTurnLock::new();
    assert!(turn_lock.cancel("telegram:chat-1"));
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let cancellation = turn_lock
        .cancellation_token("telegram:chat-1")
        .ok_or("missing pending cancellation token")?;
    assert!(cancellation.is_cancelled());
    Ok(())
}

#[test]
fn priority_stop_does_not_consume_pending_cancellation_for_original_turn(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let turn_lock = SessionTurnLock::new();
    assert!(turn_lock.cancel("telegram:chat-1"));
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock.clone());

    let stop = loop_runtime
        .process_message(InboundMessage::new("telegram", "user-1", "chat-1", "/stop"))?;
    assert_eq!(stop.command, Some(AgentLoopCommandResult::StopRequested));
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("original turn lock acquire failed: {error:?}"))?;
    assert!(turn_lock
        .cancellation_token("telegram:chat-1")
        .is_some_and(|token| token.is_cancelled()));
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
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.execution_control = Some(AutomationExecutionControl::with_timeout(
        "automation-test",
        Duration::from_secs(1),
    ));
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
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

struct BlockingProvider {
    entered: Arc<std::sync::Barrier>,
    release: Arc<std::sync::Barrier>,
}

impl ProviderClient for BlockingProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.entered.wait();
        self.release.wait();
        Ok(LlmResponse {
            content: Some("turn complete".to_owned()),
            ..LlmResponse::default()
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct ErrorProvider {
    message: String,
}

impl ProviderClient for ErrorProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Err(provider_error(self.message.clone()))
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
