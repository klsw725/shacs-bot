mod activation_execution;
mod activation_record;
mod activation_store;
mod activation_wire;
mod agent_loop;
mod app_extension_projection;
mod app_extension_provenance;
mod app_supervisor;
mod autocompact;
mod automation;
mod automation_adapter;
mod automation_dispatch;
mod automation_gates;
mod automation_lifecycle;
mod automation_payload;
mod automation_production;
mod classifier_evidence;
mod containment_permission;
mod context;
mod context_diagnostics;
mod context_files;
mod context_handoff;
mod context_refs;
mod context_resolvers;
mod context_safety;
mod diagnostics;
mod durable_dispatch;
mod execution_contract;
mod execution_snapshot;
mod execution_snapshot_source;
mod file_context;
mod goal;
mod goal_accounting;
mod goal_evaluator;
mod goal_surface;
mod lifecycle;
mod loop_control;
mod memory;
mod memory_skill_curator;
mod permission_action;
mod permission_approval;
mod permission_audit;
mod permission_ceiling;
mod permission_pattern;
mod permission_policy;
mod permission_recent_denials;
mod permission_remembered;
mod permission_replay;
mod permission_rules;
mod plugin_discovery;
mod plugin_extension_projection;
mod plugin_hooks;
mod plugin_runtime;
mod plugin_surface;
mod plugin_tool_before;
mod policy_safety_snapshot;
mod process_envelope;
mod process_gate;
mod provider_credentials;
mod replay;
mod runner;
pub mod sandbox_adapter;
mod self_improvement;
mod self_improvement_live;
mod skill_trust_permission;
mod snapshot_replay;
mod spec031_context;
mod spec033_projection;
mod spec033_release;
mod subagent;
mod surface_action;
mod tool_before;
mod tool_execution;
mod tool_execution_provider;
mod tool_search;
mod trajectory_store;
mod trusted_javascript_tool_before;
pub mod trusted_resources;
pub mod trusted_runtime;
mod trusted_tool_before_registry;
mod workflow;

pub use activation_execution::{
    admit_activation_for_execution, digest_reason, ActivationAdmissionError,
    ActivationCurrentIdentity, ActivationLiveFacts, ActivationReplay, ActivationSnapshotCandidate,
    ReplayDispatchCounters,
};
pub use activation_record::{
    ActivationDiagnostic, ActivationDigestObservation, ActivationReason, ActivationRecord,
    ActivationRecordInput, ActivationSource, ActivationStatus, WorkspaceTrustRef,
    ACTIVATION_SCHEMA_VERSION,
};
pub use activation_store::{
    ActivationMutation, ActivationMutationReceipt, ActivationMutationRequest, ActivationStore,
    ActivationStoreError,
};
pub use agent_loop::{
    AgentLoop, AgentLoopCommandResult, AgentLoopConfig, AgentLoopError, AgentLoopOutcome,
    AgentLoopRunSummary, AgentLoopTurnResult, PermissionModeSetter, ProjectPermissionStoreConfig,
};
pub use app_extension_projection::resolve_app_extension_provenance;
pub use app_extension_provenance::{
    AppExtensionBlocker, AppExtensionProvenanceProjection, AppExtensionReplayDispatchCounters,
    AppExtensionReplayInput, AppExtensionSourceFacts, AppExtensionStatus,
};
pub use app_supervisor::{
    AppProcessDriver, AppProcessRunOutcome, AppStartFacts, AppSupervisor, AppSupervisorError,
    AppSupervisorRun, AppSupervisorTerminal,
};
pub use autocompact::{AutoCompact, AutoCompactArchiveOutcome, RECENT_SUFFIX_MESSAGES};
pub use automation::{
    coordinate_automation_run, AutomationCoordinationOutcome, AutomationPrd008LinkageMetadata,
    AutomationSourceEvent, AutomationSourceEventKind, AutomationSuppressionReason,
    AutomationTaskOutcomeEligibility, SubagentMergeState,
};
pub use automation_adapter::{
    AutomationDispatchRequest, AutomationExecutionControl, AutomationExecutionReceipt,
    AutomationExecutionTerminalFact, AutomationExecutor, AutomationGateResolution,
    AutomationGateResolver, AutomationHookEvaluation, AutomationProcessCleanupFact,
    AUTOMATION_RUNTIME_DEFAULT_TIMEOUT,
};
pub use automation_dispatch::{
    AutomationDispatchSummary, AutomationWorkEnqueueInput, AutomationWorkEnvelope,
    AUTOMATION_WORK_KIND,
};
pub use automation_lifecycle::{
    own_automation_lifecycle, AutomationConfirmationFact, AutomationDeliveryResult,
    AutomationExecutionRequirements, AutomationGateRecord, AutomationJobResult,
    AutomationLifecycleInput, AutomationLifecycleOutcome, AutomationLifecycleRecord,
    AutomationNoDispatchReason, AutomationScheduleKind,
};
pub use automation_production::{
    enqueue_production_automation, route_task_outcome, AutomationOutcomePolicy,
    AutomationOwnerEffect, AutomationProductionJob, AutomationRouteEvidence, AutomationRouteOwners,
    AutomationTaskOutcomeDecision, AutomationTaskOutcomeEvaluator, AutomationTaskOutcomeInput,
    AutomationTaskOutcomeRecord, ConservativeAutomationTaskOutcomeEvaluator,
};
pub use classifier_evidence::{
    classifier_decision_evidence, skipped_classifier_evidence, AccountingState,
    AccountingUnavailableReason, AccountingValue, ClassifierActionCorrelation,
    ClassifierAttemptStatus, ClassifierCostAccounting, ClassifierDecisionEvidence,
    ClassifierDisposition, ClassifierEvidenceId, ClassifierEvidenceInput,
    ClassifierEvidenceSchemaId, ClassifierFallbackCause, ClassifierFallbackEvidence,
    ClassifierLatencyAccounting, ClassifierModelEvidence, ClassifierRequestCorrelation,
    ClassifierRouteEvidence, ClassifierRouteKind, ClassifierTokenAccounting,
    ClassifierVerdictEvidence, RedactedDiagnosticRef, StaticPolicyPrecedence,
    CLASSIFIER_EVIDENCE_SCHEMA_V1,
};
pub use containment_permission::{
    containment_permission_proof_for_process_gate, evaluate_containment_permission,
    BlockedExternalSurface, BlockedExternalSurfaceReason, ContainmentBoundaryRef,
    ContainmentComparisonOutcome, ContainmentEvidenceState, ContainmentPermissionError,
    ContainmentPermissionInput, ContainmentPermissionProof,
    ContainmentPermissionProofProjectionInput, ContainmentProofViolation,
    PermissionCeilingComparisonOutcome, PermissionCeilingProofInput, ProcessEnvelopeAdmission,
    RuntimeBoundaryKind, WorkspaceComparisonOutcome, WorkspaceScopeProof,
};
pub use context::{add_assistant_message, add_tool_result, ContextBuildRequest, ContextBuilder};
pub use context_diagnostics::{
    build_context_diagnostics_summary, ContextArtifactDiagnosticEntry,
    ContextArtifactDiagnosticsSummary, ContextBudgetDiagnosticEntry,
    ContextBudgetDiagnosticsSummary, ContextDiagnosticsCount, ContextDiagnosticsInput,
    ContextDiagnosticsSummary, ContextFileDiagnosticEntry, ContextFileDiagnosticsSummary,
    ContextProviderBlockDiagnosticEntry, ContextReferenceDiagnosticEntry,
    ContextReferenceDiagnosticsSummary, ContextReferenceParseDiagnosticEntry,
    ContextReplayDiagnosticEntry, ContextSafetyDiagnosticEntry, ContextSafetyDiagnosticsSummary,
};
pub use context_files::{
    discover_context_files, ContextFileDigest, ContextFileDiscovery, ContextFileDiscoveryOptions,
    ContextFileProjection, ContextFileReadStatus, ContextFileSource,
    DEFAULT_CONTEXT_FILE_MAX_BYTES, DEFAULT_CONTEXT_FILE_NAMES,
};
pub use context_handoff::{
    build_context_provider_handoff, select_token_estimator, ContextArtifactPriority,
    ContextBudgetDecision, ContextBudgetEvidence, ContextBudgetInput, ContextProviderHandoff,
    ProviderContextBlock, RequiredBudgetEvidence, RequiredContextKind, TokenEstimatorSelection,
    DEFAULT_CONTEXT_HANDOFF_BUDGET_TOKENS,
};
pub use context_refs::{
    parse_context_references, ContextPermissionEvidence, ContextPermissionStatus,
    ContextRedactionStatus, ContextReferenceKind, ContextReferenceParse, ContextReferenceSpan,
    ContextResolutionState, ContextTruncationStatus, ReferenceParseDiagnostic,
    ReferenceParseDiagnosticKind, ResolvedContextArtifact,
};
pub use context_resolvers::{resolve_context_reference, ContextReferenceResolverConfig};
pub use context_safety::{
    apply_context_safety_gate, context_trust_label_name, protected_context_path_reason,
    replay_context_artifact_from_evidence, trust_label_for_kind, ContextPermissionDecision,
    ContextReplayEvidence, ContextSafetyDiagnostic, ContextSafetyReport, ContextTrustLabel,
};
pub use diagnostics::{
    build_core_diagnostics_aggregate, ClassifierDiagnosticsDto, ClassifierEvidenceDiagnostic,
    ContainmentDiagnosticsDto, ContainmentProofDiagnostic, CoreDiagnosticsAggregate,
    CoreDiagnosticsAggregateInput, CoreDiagnosticsError, PolicySafetyDiagnosticsDto,
    PolicySafetyRefDiagnostic, ProcessDiagnosticsDto, ProcessReceiptDiagnostic,
    SecretDiagnosticsDto, SecretRefDiagnostic, TrustDecisionDiagnostic, TrustDiagnosticsDto,
};
pub use durable_dispatch::{
    inline_control_payload, runtime_control_payload, DurableDispatchError, DurableDispatchSummary,
    DurableStaleRecoverySummary, DurableWorkDispatcher, DurableWorkEnqueueInput,
};
pub use execution_contract::{
    ExecutionDomain, ExecutionIdentity, ExecutionOutcome, ExecutionOutcomeFact, ExecutionScope,
    LateResultDecision, PendingExecution, ProviderOutcomeKind, RecordedExecutionOutcome,
    RuntimeExecutionLedger, SubagentOutcomeKind, ToolFailureClass, ToolInterruptKind,
    ToolOutcomeKind,
};
pub use execution_snapshot::{
    trusted_runtime_fact_refs, AdapterSandboxRef, ConfigMigrationState, ConfigSnapshotRef,
    ContextInclusion, ContextSourceSnapshot, CredentialSnapshotRef, DataDisclosureWarning,
    ExecutionSnapshot, ExecutionSnapshotError, ExecutionSnapshotInput, ProfileSelectionSnapshot,
    ProviderExecutionHandoff, ProviderInputSnapshot, ReplayContract, ResourceIdentitySnapshot,
    SandboxMode, SelectedIdentitySnapshot, Spec030ExecutionRefs, TokenBudgetSnapshot,
    TrustedRuntimeFactRef, EXECUTION_SNAPSHOT_SCHEMA_V1,
};
pub use execution_snapshot_source::LiveExecutionSnapshotSource;
pub use file_context::{
    AudioAnalysisPolicy, AudioContextAnalysis, AudioContextAnalyzer, AudioContextError,
    AudioContextRequest, TranscriptionAudioAnalyzer, VideoAnalysisPolicy, VideoComponentFailure,
    VideoContextAnalysis, VideoContextAnalyzer, VideoContextError, VideoContextRequest,
    VideoMetadata,
};
pub use goal::{
    apply_completion_verdict, build_goal_completion_evaluation_request, clear_goal,
    consume_evaluator_decision, continuation_decision, create_persistent_goal,
    evaluator_consumption_idempotency_key, mark_goal_blocked, mark_goal_done, pause_goal,
    persistent_goal_from_session, record_goal_stop, remove_persistent_goal, resume_goal,
    store_persistent_goal, EvaluatorDecisionInput, GoalCompletionVerdict, GoalContinuationDecision,
    GoalContinuationStopReason, GoalEvaluationRequest, GoalMetadataError, LedgerConsumptionRecord,
    LedgerConsumptionStatus, PersistentGoal, PersistentGoalStatus, RuntimeContinuationDecision,
    RuntimeDecisionKind, RuntimeDecisionRecord, RuntimePolicyGateResults, RuntimeSelectedAction,
    StaleVerdictRecord, DEFAULT_GOAL_TURN_BUDGET, GOAL_TRANSITION_HISTORY_METADATA_KEY,
    PERSISTENT_GOAL_METADATA_KEY,
};
pub use goal_accounting::{
    GoalBudgetAccounting, GoalEvidenceAvailability, GoalObservedState, GoalStopReason,
    GoalTransitionError, GoalTransitionFact, GoalTransitionKind,
};
pub use goal_evaluator::{
    ConservativeGoalCompletionEvaluator, GoalCompletionEvaluator, GoalEvaluatorOutcome,
    GOAL_EVALUATOR_BOUNDARY_METADATA_KEY,
};
pub use goal_surface::{
    apply_goal_surface_action, build_spec033_snapshot, build_spec033_snapshot_from,
    GoalSurfaceAction, GoalSurfaceError,
};
pub use lifecycle::{
    DreamLifecycle, McpLifecycle, ProviderHotSwapResult, ProviderSelectionSnapshot,
    RuntimeCapabilityReport, RuntimeCapabilityStatus, StaticProviderSelector,
};
pub use loop_control::{
    ActiveLoopTask, ActiveLoopTaskSnapshot, CancellationToken, LoopTaskCancelResult,
    LoopTaskRegisterResult, LoopTaskRegistry, LoopTaskStatus, SessionTurnAcquireError,
    SessionTurnCancelOutcome, SessionTurnGuard, SessionTurnLock, SessionTurnReservation,
    StreamDeltaBatch, StreamDeltaCoalescer,
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
    PermissionMode, PermissionModeSnapshot, PermissionSecretRefEvidence, PermissionSecretRefStatus,
    PermissionedAction, PermissionedActionInput, PermissionedActionOrigin, SafetyCapability,
    TargetRef,
};
pub use permission_approval::{
    approval_decision_options, correlate_approval, correlate_policy_safety_snapshot_ref,
    ApprovalActor, ApprovalCacheEntry, ApprovalCorrelation, ApprovalCorrelationError,
    ApprovalDecision, ApprovalDecisionEffect, ApprovalDecisionKind, ApprovalDecisionOption,
    ApprovalDecisionScope, ApprovalRequest, SessionApprovalCacheEntry, SessionApprovalReuseMatch,
    SessionRememberedPermissionDiagnostic, SessionRememberedPermissionRule,
    SessionRememberedPermissionRules,
};
pub use permission_audit::{
    build_permission_audit_record, build_permission_diagnostics_summary,
    permission_prd005_006_contract_cases, permission_release_evidence_complete,
    required_permission_release_evidence_buckets, PermissionAuditRecord, PermissionContractCase,
    PermissionDiagnosticsSummary, PermissionPolicySafetySnapshotAuditStatus,
    PermissionPolicySafetySnapshotAuditSummary, PermissionPolicySafetySnapshotDiagnosticsSummary,
    PermissionReleaseEvidence, PermissionReleaseEvidenceBucket, PermissionSecretRefAuditSummary,
    PermissionSecretRefDiagnosticsSummary,
};
pub use permission_ceiling::{
    app_declaration_grants_permission, boundary_origin_from_action, ceiling_for_origin,
    evaluate_inherited_ceiling, late_result_permission_disposition, AppDeclarationPermissionInput,
    BoundaryPermissionViolation, CeilingEvaluation, InheritedPermissionContext,
    LateResultPermissionDisposition, LateResultPermissionInput, PermissionCeilingSnapshot,
    RuntimeBoundaryOrigin,
};
pub use permission_policy::{
    decide_permission, AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, EvaluatorConfidence,
    EvaluatorScopeMatch, PermissionPolicyDecision, PermissionPolicyDecisionKind,
    PermissionPolicyInput, PermissionPolicyReason, PromptInjectionSignal,
    RememberedPermissionPolicyMatch,
};
pub use permission_recent_denials::{
    recent_auto_mode_denial_from_classifier_decision, RecentAutoModeDenial,
    RecentAutoModeDenialStore, RecentAutoModeRetryToken, RecentAutoModeRetryTokenConsumeError,
    RecentAutoModeRetryTokenMatch, RecentAutoModeRetryTokenStore, RECENT_AUTO_MODE_DENIAL_LIMIT,
};
pub use permission_remembered::{
    remembered_permission_matcher_matches, safe_remembered_permission_matcher,
    RememberedPermissionMatcherError, SafeRememberedPermissionMatcher,
};
pub use permission_replay::{
    evaluate_permission_replay, evaluate_permission_replay_value, PermissionReplayInput,
    PermissionReplayInvariant, PermissionReplayOutcome, PermissionReplayPolicySafetySnapshotStatus,
    PermissionReplayViolation,
};
pub use permission_rules::{
    classify_permission_action, evaluate_static_rules, CapabilityClassification,
    ContainerNetworkMode, ContainerRuntimeKind, DockerContainmentSnapshot, PermissionRuleInput,
    ProcExecSummary, ProtectedTargetClass, RuleDiagnostics, StaticRuleDecision,
    StaticRuleDecisionKind, StaticRuleReason, TargetClassification, PERMISSION_STATIC_RULE_VERSION,
};
pub use plugin_discovery::{
    discover_plugins, DiscoveredPlugin, PluginBlockReason, PluginDiscovery, PluginDiscoveryError,
    PluginManifest, PluginManifestSource, PluginState,
};
pub use plugin_extension_projection::build_spec031_extension_projection;
pub use plugin_hooks::{
    plugin_hook_catalog, plugin_hook_error_diagnostic, plugin_hook_output_policy,
    plugin_hook_timeout_diagnostic, summarize_plugin_hook_dispatch, validate_plugin_hook_output,
    PluginHookCallbackResult, PluginHookCatalog, PluginHookCatalogEntry, PluginHookDispatchAttempt,
    PluginHookDispatchEffect, PluginHookDispatchRecord, PluginHookDispatchStatus,
    PluginHookDispatchSummary, PluginHookErrorDiagnostic, PluginHookEvent,
    PluginHookOutputEvidence, PluginHookOutputPolicy, PluginHookOutputValidation,
    PluginHookTimeoutDiagnostic,
};
pub use plugin_runtime::{
    build_plugin_runtime_snapshot, plugin_runtime_commands, plugin_runtime_tools,
    register_plugin_runtime_tools, PluginCommandDispatchError, PluginCommandDispatcher,
    PluginCommandExecution, PluginCommandInvocation, PluginCommandToolInvocation,
    PluginExecutableCommand, PluginHookCommandExecutor, PluginHookCommandInvocation,
    PluginHookDispatchMode, PluginHookDispatchSink, PluginProcessPermissionContext,
    PluginRuntimeCommand, PluginRuntimeDiagnostic, PluginRuntimeHook, PluginRuntimePlugin,
    PluginRuntimeSnapshot, PluginRuntimeTool, ProcessPluginHookCommandExecutor,
};
pub use plugin_surface::{
    build_plugin_surface_projection, evaluate_plugin_permission_ceiling,
    plugin_spec025_evidence_ref, plugin_spec025_release_evidence_checklist,
    plugin_surface_diagnostic, reject_plugin_replay_live_dispatch,
    required_spec025_release_evidence_buckets, PluginCommandDescriptor, PluginDescriptor,
    PluginHookDescriptor, PluginMcpDescriptor, PluginPermissionCeilingDecision,
    PluginPermissionCeilingRequest, PluginReplayRejection, PluginSecretRef, PluginSecretRefKind,
    PluginSkillDescriptor, PluginSpec025ReleaseEvidence, PluginSpec025ReleaseEvidenceBucket,
    PluginSpec025ReleaseEvidenceChecklist, PluginSurfaceDiagnostic, PluginSurfaceProjection,
    PluginToolDescriptor,
};
pub use plugin_tool_before::PluginRuntimeHookAgentHook;
pub use policy_safety_snapshot::{
    CapabilityCeilingRef, PolicySafetyDigest, PolicySafetyProvenanceKind,
    PolicySafetyProvenanceRef, PolicySafetySnapshot, PolicySafetySnapshotCreationReason,
    PolicySafetySnapshotError, PolicySafetySnapshotId, PolicySafetySnapshotInput,
    PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId, PolicySafetySourceKind,
    PolicySafetySourceRef, RedactedPolicySafetySummary, POLICY_SAFETY_SNAPSHOT_SCHEMA_V1,
};
pub use process_envelope::{
    ProcessAdapterKind, ProcessEnvelopeError, ProcessExecutionEnvelope,
    ProcessExecutionEnvelopeInput, ProcessIdentity, ProcessRedactedCommand,
};
pub use process_gate::{
    ProcessContainmentProofCandidate, ProcessExecutionReceipt, ProcessGate, ProcessGateError,
    ProcessGateInput, ProcessGateTerminalPrecondition, ProcessRedactedSpawnSummary,
    ProcessRedactedStatus, ProcessRedactedStreamKind, ProcessRedactedStreamSummary,
    ProcessSpawnAuthorization, ProcessSpawnReport, ProcessTerminalOutcome,
};
pub(crate) use provider_credentials::ProviderInvocationClient;
pub use provider_credentials::{
    CredentialResolvingImageGenerationClient, CredentialResolvingProviderClient,
    OAuthCredentialRefresher, ProviderClientResolutionRequest, ProviderCredentialClientConfig,
    ProviderCredentialInvocation, ProviderCredentialRuntime,
};
pub use replay::{run_local_replay, RuntimeReplayInput, RuntimeReplayOutcome};
pub use runner::{
    AgentHook, AgentHookContext, AgentRunSpec, AgentRunner, CompositeHook,
    ExecutionSnapshotCallback, ExecutionSnapshotResolver, MidTurnInjectionCallback, NoopAgentHook,
    ProviderEventCallback, RetryWaitCallback, ToolEvent, ToolEventCallback, ToolSearchConfig,
    ToolSearchMode, ToolSearchRuntimeInput, ToolStatus,
};
pub use self_improvement::{
    runtime_improvement_apply_readiness, runtime_improvement_apply_record,
    runtime_improvement_approved_scope_matches, runtime_improvement_proposal_behavior_inert,
    runtime_improvement_rollback_projection, runtime_improvement_status_after_apply_record,
    runtime_improvement_verification_record, runtime_mcp_exposure_projection,
    SelfImprovementApplyReadiness, SelfImprovementRollbackProjection,
};
pub use self_improvement_live::{
    ApplyBlock, ApplyGateDecision, ApplyGateReceipt, ApplyReceipt, CheckpointReceipt,
    CurrentGateEvidence, CurrentImprovementGates, CurrentSpec030Receipts, ExecutionSnapshotRef,
    ImprovementOwner, ImprovementVerifier, InMemoryImprovementStore, LocalApplyReceipt,
    LocalArtifactOwner, LocalDigestVerifier, LocalGateSource, LocalImprovementBlock,
    LocalImprovementProposal, LocalImprovementRuntime, LocalImprovementService,
    LocalImprovementStatus, LocalImprovementStore, LocalImprovementVerifier,
    LocalRollbackCandidate, LocalRollbackReceipt, OwnerApplyEvidence, OwnerRollbackEvidence,
    ProductionLocalGateSource, RollbackCandidate, RollbackReceipt, SelfImprovementCoordinator,
    SelfImprovementProposal, VerificationEvidence,
};
pub use shacs_bus::{
    InboundMessage, MessageBus, MessageBusError, OutboundMessage, OwnerAcceptedAutomationResult,
};
pub use shacs_heartbeat::{
    build_decision_request, current_time_str, heartbeat_tool_schema, is_deliverable,
    parse_decision_response, read_heartbeat_file, HeartbeatAction, HeartbeatDecision,
    HeartbeatError, HeartbeatNotifier, HeartbeatResponseEvaluator, HeartbeatService,
    HeartbeatStartResult, HeartbeatTaskExecutor, HeartbeatTickOutcome, HeartbeatWorker,
    ProviderNotificationEvaluator, HEARTBEAT_FILE_NAME, HEARTBEAT_TOOL_NAME,
};
pub use shacs_projection::{
    build_spec018_diagnostics_manifest, build_spec018_ledger_inspect_result,
    build_spec018_projection, evaluate_spec018_release_gate, runtime_spec018_channel_projection,
    runtime_spec018_local_api_projection, tool_search_prd005_release_evidence_checklist,
    tool_search_prd006_release_evidence_checklist, RuntimeSpec018DiagnosticsManifestInput,
    RuntimeSpec018LedgerInspectInput, RuntimeSpec018ProjectionInput,
    RuntimeSpec018ReleaseGateInput, ToolSearchReleaseEvidence, ToolSearchReleaseEvidenceBucket,
    ToolSearchReleaseEvidenceChecklist,
};
pub use shacs_providers::{GenerationSettings, ProviderClient, ProviderRetryMode};
pub use shacs_session::{
    find_legal_message_start, Session, SessionHistoryOptions, SessionManager, SessionSummary,
    FILE_MAX_MESSAGES,
};
pub use skill_trust_permission::{
    blocked_skill_trust_external_surface, validate_skill_trust_permission, SkillTrustActionKind,
    SkillTrustDigestPair, SkillTrustGuardInput, SkillTrustPermissionDecision,
    SkillTrustPermissionDecisionKind, SkillTrustPermissionInput, SkillTrustPermissionSchemaId,
    SkillTrustRejectionReason, TrustLifecycleStatus,
};
pub use snapshot_replay::{
    replay_recorded_trajectory, RecordedTrajectoryReplayError, RecordedTrajectoryReplayReceipt,
};
pub use spec031_context::{
    project_spec031_context_evidence, Spec031ContextEvidenceInput,
    Spec031ContextEvidenceProjection, Spec031ContextEvidenceReason, Spec031ContextEvidenceRow,
    Spec031ContextEvidenceRowKind, Spec031ContextOwnerRef,
};
pub use spec033_release::{
    collect_spec033_replay_evidence, redact_spec033_artifact, run_spec033_release_runner,
    validate_spec033_release_artifacts, validate_spec033_release_artifacts_against,
    validate_spec033_release_coverage, Spec033RedactionReceipt, Spec033ReleaseArtifactError,
    Spec033ReleaseCheck, Spec033ReleaseCommandEvidence, Spec033ReleaseConfig,
    Spec033ReleaseEvidenceError, Spec033ReleaseManifest, Spec033ReleaseMode, Spec033SourceManifest,
    Spec033TrajectoryProvenance,
};
pub use subagent::{
    build_subagent_tool_registry, format_partial_progress,
    format_partial_progress_from_tool_events, ChildResultEnvelope, ChildResultStatus,
    MergeDecision, SpawnEnvelope, SubagentExecutionConfig, SubagentProgressUpdate, SubagentRuntime,
    SubagentRuntimeConfig, SubagentSpawnOutcome, SubagentState, SubagentStatus,
    SyntheticSubagentCommand,
};
pub use surface_action::{
    recover_runtime_surface, request_runtime_control, request_surface_approval,
    runtime_stop_request_marker_path, surface_approval_availability, SurfaceAction,
    SurfaceActionError, SurfaceActionOutcome, SurfaceActionOutcomeKind, SurfaceActionRequestKind,
    SurfaceApprovalAvailability, SurfaceApprovalDecision, SurfaceApprovalRequest,
    SURFACE_APPROVAL_PAYLOAD_TYPE, SURFACE_APPROVAL_WORK_KIND,
};
pub use tool_before::{
    HeadlessToolBeforeInteraction, ToolBeforeConfirmRequest, ToolBeforeConfirmation,
    ToolBeforeContext, ToolBeforeDecision, ToolBeforeHandler, ToolBeforeInteraction,
    ToolBeforeNotifyRequest, ToolBeforeOrderKey, ToolBeforeSelectRequest,
};
pub use tool_execution::{
    session_approval_context_digest, session_approval_context_digest_for_input,
    session_remembered_context_digest, session_remembered_context_digest_for_input,
    RuntimeAssistantToolCallMessage, RuntimeContextTools, RuntimeInterrupt, RuntimeToolCall,
    RuntimeToolExecutionReport, RuntimeToolExecutor, RuntimeToolMessage, ToolExecutionContext,
};
pub(crate) use tool_search::dispatch_bridge_tool_calls_with_context_resolver;
pub use tool_search::{
    bridge_underlying_mapping_evidence_ref, dispatch_bridge_tool_call, dispatch_bridge_tool_calls,
    BridgeToolCall, BridgeToolExecutionReport, BridgeToolResult, BridgeUnderlyingMappingEvidence,
    ResolvedDeferredToolCall, ToolCallScopeError, ToolDescribeEvidence, ToolSearchActivationReason,
    ToolSearchDiagnosticsSummary, ToolSearchQueryEvidence,
};
pub use trajectory_store::{
    RecordedArtifactRef, RecordedBoundaryRequirement, RecordedSourceArtifact,
    RecordedSourceArtifactInput, RecordedTrajectoryInput, RecordedTrajectoryOrigin,
    RecordedTrajectoryRecord, RecordedTrajectoryStore, RecordedTrajectoryStoreError,
};
pub use trusted_javascript_tool_before::register_trusted_javascript_tool_before_handlers;
pub use trusted_tool_before_registry::TrustedToolBeforeRegistry;
pub use workflow::{
    cancel_runtime_workflow, read_only_child_tool_names, run_live_runtime_workflow,
    run_live_runtime_workflow_with_checkpoint_callback, run_live_runtime_workflow_with_options,
    run_read_only_runtime_workflow, run_runtime_workflow_admission_branch,
    runtime_workflow_diagnostics, runtime_workflow_execution_handle,
    runtime_workflow_resume_worktree_decision, RuntimeWorkflowAdmissionBranchInput,
    RuntimeWorkflowAdmissionBranchOutcome, RuntimeWorkflowDiagnostics, RuntimeWorkflowEvent,
    RuntimeWorkflowExecutionHandle, RuntimeWorkflowInput, RuntimeWorkflowInterruptOutcome,
    RuntimeWorkflowLiveError, RuntimeWorkflowLiveInput, RuntimeWorkflowLiveOptions,
    RuntimeWorkflowLiveWorktreeConfig, RuntimeWorkflowOutcome, RuntimeWorkflowWorktreeEvidence,
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
// allow: SIZE_OK — preexisting runtime API index; Spec034 declarations and re-exports expand from one focused module macro
mod spec034_modules;
spec034_modules::declare_spec034_runtime_modules!();
