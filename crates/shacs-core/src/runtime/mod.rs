mod agent_loop;
mod autocompact;
mod automation;
mod context;
mod diagnostics_release;
mod goal;
mod lifecycle;
mod loop_control;
mod memory;
mod memory_skill_curator;
mod permission_action;
mod projection;
mod replay;
mod runner;
mod self_improvement;
mod subagent;
mod tool_execution;
mod tool_search;

pub use agent_loop::{
    AgentLoop, AgentLoopCommandResult, AgentLoopConfig, AgentLoopError, AgentLoopOutcome,
    AgentLoopRunSummary, AgentLoopTurnResult,
};
pub use autocompact::{AutoCompact, AutoCompactArchiveOutcome, RECENT_SUFFIX_MESSAGES};
pub use automation::{
    coordinate_automation_run, AutomationCoordinationOutcome, AutomationPrd008LinkageMetadata,
    AutomationSourceEvent, AutomationSourceEventKind, AutomationTaskOutcomeEligibility,
    SubagentMergeState,
};
pub use context::{add_assistant_message, add_tool_result, ContextBuildRequest, ContextBuilder};
pub use diagnostics_release::{
    build_spec018_diagnostics_manifest, build_spec018_ledger_inspect_result,
    evaluate_spec018_release_gate, tool_search_prd005_release_evidence_checklist,
    tool_search_prd006_release_evidence_checklist, RuntimeSpec018DiagnosticsManifestInput,
    RuntimeSpec018LedgerInspectInput, RuntimeSpec018ReleaseGateInput, ToolSearchReleaseEvidence,
    ToolSearchReleaseEvidenceBucket, ToolSearchReleaseEvidenceChecklist,
};
pub use goal::{
    apply_completion_verdict, build_goal_completion_evaluation_request, clear_goal,
    consume_evaluator_decision, continuation_decision, create_persistent_goal,
    evaluator_consumption_idempotency_key, mark_goal_blocked, mark_goal_done, pause_goal,
    persistent_goal_from_session, remove_persistent_goal, resume_goal, store_persistent_goal,
    EvaluatorDecisionInput, GoalCompletionVerdict, GoalContinuationDecision,
    GoalContinuationStopReason, GoalEvaluationRequest, GoalMetadataError, LedgerConsumptionRecord,
    LedgerConsumptionStatus, PersistentGoal, PersistentGoalStatus, RuntimeContinuationDecision,
    RuntimeDecisionKind, RuntimeDecisionRecord, RuntimePolicyGateResults, RuntimeSelectedAction,
    StaleVerdictRecord, DEFAULT_GOAL_TURN_BUDGET, PERSISTENT_GOAL_METADATA_KEY,
};
pub use lifecycle::{
    DreamLifecycle, McpLifecycle, ProviderHotSwapResult, ProviderSelectionSnapshot,
    RuntimeCapabilityReport, RuntimeCapabilityStatus, StaticProviderSelector,
};
pub use loop_control::{
    ActiveLoopTask, ActiveLoopTaskSnapshot, CancellationToken, LoopTaskCancelResult,
    LoopTaskRegisterResult, LoopTaskRegistry, LoopTaskStatus, SessionTurnAcquireError,
    SessionTurnGuard, SessionTurnLock, StreamDeltaBatch, StreamDeltaCoalescer,
};
pub use memory::{
    estimate_message_tokens, estimate_session_prompt_tokens, pick_consolidation_boundary,
    DreamProcessor, DreamRunOutcome, MemoryArchiveOutcome, MemoryConsolidationError,
    MemoryConsolidationOutcome, MemoryConsolidationRequest, MemoryConsolidator, MemoryGitBoundary,
    MemoryHistoryEntry, MemoryLineAge, MemoryStore, NoGitBoundary, ProviderArchiveConsolidator,
    ProviderMemoryConsolidator, SessionConsolidationLocks, SessionTokenConsolidationOutcome,
    TokenConsolidationConfig, ARCHIVE_SUMMARY_MAX_CHARS, DEFAULT_CONSOLIDATION_SAFETY_BUFFER,
    DEFAULT_MAX_CONSOLIDATION_ROUNDS, DEFAULT_MAX_HISTORY_ENTRIES,
    DREAM_HISTORY_ENTRY_PREVIEW_MAX_CHARS, DREAM_MEMORY_FILE_MAX_CHARS, DREAM_SOUL_FILE_MAX_CHARS,
    DREAM_STALE_THRESHOLD_DAYS, DREAM_USER_FILE_MAX_CHARS, HISTORY_ENTRY_HARD_CAP,
    RAW_ARCHIVE_MAX_CHARS,
};
pub use memory_skill_curator::{
    app_provided_skill_reference_evidence, authored_skill_ready_for_active_registry,
    build_runtime_memory_evidence, freeze_session_search_snapshot,
    freeze_session_search_snapshot_from_session, runtime_curator_proposal_record,
    runtime_memory_evidence_request, runtime_skill_list_disclosure,
    runtime_skill_reference_evidence, runtime_skill_view_disclosure,
    MemorySkillCuratorRuntimeError, RuntimeMemoryEvidenceRequestInput,
};
pub use permission_action::{
    normalize_resolved_deferred_tool_call, normalize_runtime_tool_call, ActionNormalizationError,
    ActionNormalizationState, ContainmentSnapshotRef, IntentSnapshotRef, PermissionDecisionInput,
    PermissionMode, PermissionModeSnapshot, PermissionedAction, PermissionedActionInput,
    PermissionedActionOrigin, SafetyCapability, TargetRef,
};
pub use projection::{
    build_spec018_projection, runtime_spec018_channel_projection,
    runtime_spec018_local_api_projection, RuntimeSpec018ProjectionInput,
};
pub use replay::{run_local_replay, RuntimeReplayInput, RuntimeReplayOutcome};
pub use runner::{
    AgentHook, AgentHookContext, AgentRunResult, AgentRunSpec, AgentRunner, CompositeHook,
    MidTurnInjectionCallback, NoopAgentHook, ProviderEventCallback, RetryWaitCallback, ToolEvent,
    ToolEventCallback, ToolSearchConfig, ToolSearchMode, ToolSearchRuntimeInput, ToolStatus,
};
pub use self_improvement::{
    runtime_improvement_apply_readiness, runtime_improvement_apply_record,
    runtime_improvement_approved_scope_matches, runtime_improvement_proposal_behavior_inert,
    runtime_improvement_rollback_projection, runtime_improvement_status_after_apply_record,
    runtime_improvement_verification_record, runtime_mcp_exposure_projection,
    SelfImprovementApplyReadiness, SelfImprovementRollbackProjection,
};
pub use shacs_bus::{InboundMessage, MessageBus, MessageBusError, OutboundMessage};
pub use shacs_heartbeat::{
    build_decision_request, current_time_str, heartbeat_tool_schema, is_deliverable,
    parse_decision_response, read_heartbeat_file, HeartbeatAction, HeartbeatDecision,
    HeartbeatError, HeartbeatNotifier, HeartbeatResponseEvaluator, HeartbeatService,
    HeartbeatStartResult, HeartbeatTaskExecutor, HeartbeatTickOutcome, HeartbeatWorker,
    ProviderNotificationEvaluator, HEARTBEAT_FILE_NAME, HEARTBEAT_TOOL_NAME,
};
pub use shacs_providers::{GenerationSettings, ProviderClient, ProviderRetryMode};
pub use shacs_session::{
    find_legal_message_start, Session, SessionHistoryOptions, SessionManager, SessionSummary,
    FILE_MAX_MESSAGES,
};
pub use subagent::{
    build_subagent_tool_registry, format_partial_progress,
    format_partial_progress_from_tool_events, ChildResultEnvelope, ChildResultStatus,
    MergeDecision, SpawnEnvelope, SubagentExecutionConfig, SubagentProgressUpdate, SubagentRuntime,
    SubagentRuntimeConfig, SubagentSpawnOutcome, SubagentState, SubagentStatus,
    SyntheticSubagentCommand,
};
pub use tool_execution::{
    RuntimeAssistantToolCallMessage, RuntimeContextTools, RuntimeInterrupt, RuntimeToolCall,
    RuntimeToolExecutionReport, RuntimeToolExecutor, RuntimeToolMessage, ToolExecutionContext,
};
pub use tool_search::{
    bridge_underlying_mapping_evidence_ref, dispatch_bridge_tool_call, dispatch_bridge_tool_calls,
    BridgeToolCall, BridgeToolExecutionReport, BridgeToolResult, BridgeUnderlyingMappingEvidence,
    ResolvedDeferredToolCall, ToolCallScopeError, ToolDescribeEvidence, ToolSearchActivationReason,
    ToolSearchDiagnosticsSummary, ToolSearchQueryEvidence,
};

pub use shacs_workflow::{
    admit_workflow_plan, build_workflow_checkpoint, decide_workflow_admission,
    workflow_barrier_decision, workflow_budget_decision, workflow_diagnostics_manifest,
    workflow_harness_plan_digest, workflow_model_route_snapshot,
    workflow_permission_ceiling_decision, workflow_prd000_release_evidence_checklist,
    workflow_projection, workflow_quarantine_decision, workflow_ready_step_ids,
    workflow_recipe_readiness, workflow_resume_decision,
    workflow_spec024_release_evidence_checklist, workflow_synthesis_outcome,
    workflow_verification_gate, workflow_worktree_decision, WorkflowAdmissionDecision,
    WorkflowAdmissionInput, WorkflowBarrierDecision, WorkflowBudgetDecision, WorkflowBudgetPolicy,
    WorkflowBudgetSlice, WorkflowBudgetUsage, WorkflowCheckpoint, WorkflowCheckpointInput,
    WorkflowCheckpointPolicy, WorkflowChildResult, WorkflowChildRunStatus, WorkflowChildSpec,
    WorkflowContextPolicy, WorkflowDiagnosticsManifest, WorkflowHarnessPlan, WorkflowMergePolicy,
    WorkflowModelRouteSnapshot, WorkflowModelRoutingPolicy, WorkflowPattern,
    WorkflowPermissionCeilingDecision, WorkflowPermissionPolicy, WorkflowPrd000ReleaseEvidence,
    WorkflowPrd000ReleaseEvidenceBucket, WorkflowPrd000ReleaseEvidenceChecklist,
    WorkflowProjection, WorkflowQuarantineDecision, WorkflowQuarantinePolicy, WorkflowRecipe,
    WorkflowRecipeReadiness, WorkflowResumeDecision, WorkflowResumePolicy, WorkflowRunRecord,
    WorkflowRunState, WorkflowSpec024ReleaseEvidence, WorkflowSpec024ReleaseEvidenceBucket,
    WorkflowSpec024ReleaseEvidenceChecklist, WorkflowStep, WorkflowStepPrivilege,
    WorkflowStopCondition, WorkflowSynthesisOutcome, WorkflowToolScopePolicy,
    WorkflowVerificationGate, WorkflowVerifierSpec, WorkflowVerifierVerdict,
    WorkflowVerifierVerdictKind, WorkflowWorktreeDecision, WorkflowWorktreePolicy,
    WorkflowWorktreeRequest,
};

pub type Dream<'a> = DreamProcessor<'a>;
pub type SkillsLoader = ContextBuilder;
pub type SubagentManager = SubagentRuntime;
