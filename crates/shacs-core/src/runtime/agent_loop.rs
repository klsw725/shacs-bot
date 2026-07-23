use crate::runtime::permission_pattern::{
    same_session_approval_grant, session_approval_reuse_match,
};
use crate::runtime::tool_execution::session_approval_context_digest;
use crate::runtime::{
    apply_context_safety_gate, build_context_provider_handoff, correlate_approval,
    discover_context_files, dispatch_bridge_tool_calls, parse_context_references,
    resolve_context_reference,
};
use crate::runtime::{
    build_workflow_checkpoint, decide_workflow_admission,
    run_live_runtime_workflow_with_checkpoint_callback, runtime_workflow_diagnostics,
    workflow_projection, RuntimeWorkflowLiveInput, RuntimeWorkflowLiveOptions,
    RuntimeWorkflowLiveWorktreeConfig,
};
use crate::runtime::{
    clear_goal, create_persistent_goal, mark_goal_blocked, mark_goal_done, pause_goal,
    persistent_goal_from_session, remove_persistent_goal, resume_goal, store_persistent_goal,
};
use crate::runtime::{
    AgentHook, AgentRunSpec, AgentRunner, ApprovalActor, ApprovalCacheEntry, ApprovalDecision,
    ApprovalDecisionKind, ApprovalRequest, AutoCompact, AutoCompactArchiveOutcome,
    AutoEvaluatorVerdict, ContainmentSnapshotRef, ContextBudgetInput, ContextBuildRequest,
    ContextBuilder, ContextFileDiscoveryOptions, ContextFileProjection, ContextProviderHandoff,
    ContextReferenceResolverConfig, DreamProcessor, DreamRunOutcome, ExecutionDomain,
    ExecutionIdentity, ExecutionOutcome, ExecutionOutcomeFact, ExecutionScope, GoalMetadataError,
    InboundMessage, LateResultDecision, LoopTaskCancelResult, LoopTaskRegistry,
    MemoryConsolidationError, MemoryStore, MessageBus, OutboundMessage, PermissionCeilingSnapshot,
    PermissionMode, PermissionModeSnapshot, PermissionRuleInput, PermissionedAction,
    PersistentGoal, PersistentGoalStatus, PluginCommandDispatcher, ProviderArchiveConsolidator,
    ProviderEventCallback, RecentAutoModeDenial, RecentAutoModeDenialStore,
    RecentAutoModeRetryToken, RecentAutoModeRetryTokenConsumeError, RecentAutoModeRetryTokenStore,
    RuntimeContextTools, RuntimeExecutionLedger, RuntimeInterrupt, RuntimeToolCall,
    RuntimeToolExecutionReport, RuntimeToolExecutor, RuntimeToolMessage, Session,
    SessionApprovalCacheEntry, SessionHistoryOptions, SessionManager, SessionTurnAcquireError,
    SessionTurnLock, SubagentExecutionConfig, SubagentOutcomeKind, SubagentRuntime,
    TokenConsolidationConfig, ToolEventCallback, ToolExecutionContext, WorkflowAdmissionDecision,
    WorkflowAdmissionInput, WorkflowBudgetPolicy, WorkflowBudgetSlice, WorkflowBudgetUsage,
    WorkflowCheckpointInput, WorkflowCheckpointPolicy, WorkflowChildResult, WorkflowChildRunStatus,
    WorkflowChildSpec, WorkflowContextPolicy, WorkflowHarnessPlan, WorkflowMergePolicy,
    WorkflowModelRoutingPolicy, WorkflowPattern, WorkflowPermissionPolicy,
    WorkflowQuarantinePolicy, WorkflowRecipeReadiness, WorkflowResumeDecision,
    WorkflowResumePolicy, WorkflowRunState, WorkflowStep, WorkflowStopCondition,
    WorkflowToolScopePolicy, WorkflowVerifierSpec, WorkflowWorktreePolicy,
    DEFAULT_GOAL_TURN_BUDGET,
};
use crate::tools::{
    ask_user_options_from_messages, ask_user_outbound, assemble_tool_surface, bridge_tool_names,
    pending_ask_user_id, MessageSender, MessageTool, ToolRegistry, ToolSurfaceAssemblyInput,
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shacs_command::{
    build_help_text, is_builtin_command, parse_loop_command_route, CommandKind, GoalCommandArgs,
    HistoryCommandArgs, LoopCommand, PermissionCommandArgs,
};
use shacs_config::AutoApprovalConfig;
use shacs_providers::{GenerationSettings, ProviderClient, ProviderError, ProviderRetryMode};
use shacs_session::durable_child::DurableChildRecorder;
use shacs_session::durable_event::{
    DurableEventError, DurableEventInput, DurableEventPayload, DurableEventProvenance,
    DurableEventStore, DurableExecutionIdentityRef, SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED,
    SESSION_TURN_FAILED, WORKFLOW_COMPLETED, WORKFLOW_FAILED, WORKFLOW_PLANNED,
};
use shacs_skills::{
    discover_skill_registry, discover_workflow_recipes, SkillBackedWorkflowRecipe,
    SkillRegistryOptions,
};
use shacs_utils::gitstore::{GitCliStore, GitStore};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

const PENDING_USER_TURN_KEY: &str = "pending_user_turn";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowDurableFactState {
    Absent,
    Planned,
    Terminal,
}
const RUNTIME_CHECKPOINT_KEY: &str = "runtime_checkpoint";
const RUNTIME_EXECUTION_KEY: &str = "runtime_execution";
const PENDING_PERMISSION_APPROVAL_KEY: &str = "pending_permission_approval";
const PENDING_RECENT_RETRY_APPROVAL_KEY: &str = "pending_recent_retry_approval";
const PENDING_WORKFLOW_KEY: &str = "pending_workflow";
const SESSION_PERMISSION_APPROVALS_KEY: &str = "session_permission_approvals";
const SESSION_PERMISSION_APPROVAL_LIMIT: usize = 32;
const PENDING_PERMISSION_WIZARD_KEY: &str = "pending_permission_wizard";
const RECENT_AUTO_MODE_DENIALS_KEY: &str = "recent_auto_mode_denials";
const INTERRUPTED_PLACEHOLDER: &str =
    "[Assistant reply unavailable because the previous turn was interrupted.]";
const PENDING_TOOL_PLACEHOLDER: &str = "[Tool result unavailable — call was interrupted or lost]";
const MAX_INJECTIONS_PER_TURN: usize = 3;
const SYSTEM_BOOTSTRAP_CONTEXT_FILE_NAMES: [&str; 4] =
    ["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md"];

type MessageDeliveryTarget = Arc<Mutex<Option<(String, String)>>>;
pub type PermissionModeSetter =
    Arc<dyn Fn(PermissionMode) -> Result<PermissionModeSnapshot, String> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingPermissionWizard {
    session_key: String,
    channel: String,
    chat_id: String,
    sender_id: String,
    stage: PendingPermissionWizardStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PendingPermissionWizardStage {
    ChooseMode,
    ConfirmBypass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingPermissionApproval {
    approval_request_id: String,
    approval_request: ApprovalRequest,
    tool_call: RuntimeToolCall,
    tool_context: ToolExecutionContext,
    session_key: String,
    channel: String,
    chat_id: String,
    sender_id: String,
    #[serde(default)]
    status: PendingPermissionApprovalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRecentRetryApproval {
    denial_id: String,
    approval_request_id: String,
    action_digest: String,
    argument_digest: String,
    snapshot_digest: String,
    tool_name: String,
    expires_at_unix_ms: u64,
    requester_digest: String,
    #[serde(default)]
    status: PendingPermissionApprovalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingWorkflowTurn {
    session_key: String,
    admission: WorkflowAdmissionInput,
    plan: WorkflowHarnessPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe_evidence: Option<WorkflowRecipeSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WorkflowRecipeSelection {
    selected: SkillBackedWorkflowRecipe,
    ready_candidates: Vec<SkillBackedWorkflowRecipe>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PendingPermissionApprovalStatus {
    #[default]
    Pending,
    Executing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionApprovalReply {
    Approve,
    ApproveSession,
    Deny,
    Unknown,
}

#[derive(Clone)]
pub struct AgentLoopConfig {
    pub workspace: PathBuf,
    pub media_roots: Vec<PathBuf>,
    pub model: String,
    pub settings: GenerationSettings,
    pub retry_mode: ProviderRetryMode,
    pub max_iterations: usize,
    pub max_tool_result_chars: usize,
    pub history_options: SessionHistoryOptions,
    pub unified_session_key: Option<String>,
    pub max_iterations_message: Option<String>,
    pub tool_search: crate::runtime::ToolSearchConfig,
    pub context_window_tokens: Option<usize>,
    pub context_block_limit: Option<usize>,
    pub concurrent_tools: bool,
    pub fail_on_tool_error: bool,
    pub record_channel_delivery: bool,
    pub containment_snapshot: Option<ContainmentSnapshotRef>,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    pub permission_rule_input: PermissionRuleInput,
    pub permission_auto_approval: AutoApprovalConfig,
    pub permission_ceiling_snapshot: Option<PermissionCeilingSnapshot>,
    pub permission_evaluator: Option<AutoEvaluatorVerdict>,
    pub permission_interactive: bool,
    pub permission_mode_setter: Option<PermissionModeSetter>,
    pub durable_event_root: Option<PathBuf>,
}

impl AgentLoopConfig {
    pub fn new(workspace: impl Into<PathBuf>, model: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            media_roots: Vec::new(),
            model: model.into(),
            settings: GenerationSettings::default(),
            retry_mode: ProviderRetryMode::Standard,
            max_iterations: 200,
            max_tool_result_chars: 20_000,
            history_options: SessionHistoryOptions::default(),
            unified_session_key: None,
            max_iterations_message: None,
            tool_search: crate::runtime::ToolSearchConfig::default(),
            context_window_tokens: None,
            context_block_limit: None,
            concurrent_tools: false,
            fail_on_tool_error: false,
            record_channel_delivery: true,
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            permission_rule_input: PermissionRuleInput::default(),
            permission_auto_approval: AutoApprovalConfig::default(),
            permission_ceiling_snapshot: None,
            permission_evaluator: None,
            permission_interactive: false,
            permission_mode_setter: None,
            durable_event_root: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLoopCommandResult {
    RestartRequested,
    Status,
    NewSession,
    StopRequested,
    Goal,
    History,
    Dream,
    DreamLog,
    DreamRestore,
    Permission,
    Help,
    PluginCommand,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentLoopTurnResult {
    pub session_key: String,
    pub final_content: Option<String>,
    pub stop_reason: String,
    pub tools_used: Vec<String>,
    pub outbound_count: usize,
    pub had_injections: bool,
    pub command: Option<AgentLoopCommandResult>,
    pub ask_user_options: Vec<String>,
    pub message_tool_delivery_configured: bool,
}

pub type AgentLoopOutcome = AgentLoopTurnResult;

pub struct AgentLoop<'a> {
    bus: MessageBus,
    sessions: SessionManager,
    context_builder: ContextBuilder,
    runner: AgentRunner,
    tools: &'a ToolRegistry,
    client: &'a dyn ProviderClient,
    config: AgentLoopConfig,
    context_tools: RuntimeContextTools,
    message_delivery_target: Option<MessageDeliveryTarget>,
    turn_lock: SessionTurnLock,
    task_registry: LoopTaskRegistry,
    auto_compact: Option<AutoCompact>,
    tool_event_callback: Option<ToolEventCallback>,
    provider_event_callback: Option<ProviderEventCallback>,
    agent_hook: Option<Arc<dyn AgentHook>>,
    plugin_command_dispatcher: Option<PluginCommandDispatcher>,
    recent_retry_tokens: RecentAutoModeRetryTokenStore,
    durable_events: Option<DurableEventStore>,
    stopped: bool,
}

impl<'a> AgentLoop<'a> {
    pub fn new(
        bus: MessageBus,
        sessions: SessionManager,
        context_builder: ContextBuilder,
        tools: &'a ToolRegistry,
        client: &'a dyn ProviderClient,
        config: AgentLoopConfig,
    ) -> Self {
        Self {
            bus,
            sessions,
            context_builder,
            runner: AgentRunner::new(),
            tools,
            client,
            config,
            context_tools: RuntimeContextTools::new(),
            message_delivery_target: None,
            turn_lock: SessionTurnLock::new(),
            task_registry: LoopTaskRegistry::new(),
            auto_compact: None,
            tool_event_callback: None,
            provider_event_callback: None,
            agent_hook: None,
            plugin_command_dispatcher: None,
            recent_retry_tokens: RecentAutoModeRetryTokenStore::default(),
            durable_events: None,
            stopped: false,
        }
    }

    pub fn with_context_tools(mut self, context_tools: RuntimeContextTools) -> Self {
        self.context_tools = context_tools;
        self
    }

    pub fn with_tool_event_callback(mut self, callback: ToolEventCallback) -> Self {
        self.tool_event_callback = Some(callback);
        self
    }

    pub fn with_provider_event_callback(mut self, callback: ProviderEventCallback) -> Self {
        self.provider_event_callback = Some(callback);
        self
    }

    pub fn with_agent_hook(mut self, hook: Arc<dyn AgentHook>) -> Self {
        self.agent_hook = Some(hook);
        self
    }

    pub fn with_plugin_command_dispatcher(mut self, dispatcher: PluginCommandDispatcher) -> Self {
        self.plugin_command_dispatcher = Some(dispatcher);
        self
    }

    pub fn with_loop_task_registry(mut self, task_registry: LoopTaskRegistry) -> Self {
        self.task_registry = task_registry;
        self
    }

    pub fn with_session_turn_lock(mut self, turn_lock: SessionTurnLock) -> Self {
        self.turn_lock = turn_lock;
        self
    }

    pub fn with_auto_compact(mut self, auto_compact: AutoCompact) -> Self {
        self.auto_compact = Some(auto_compact);
        self
    }

    fn append_durable_fact(
        &mut self,
        session_id: &str,
        turn_id: Option<String>,
        kind: &str,
        payload: Value,
        provenance: Option<DurableEventProvenance>,
    ) -> Result<(), AgentLoopError> {
        let Some(root) = self.config.durable_event_root.clone() else {
            return Ok(());
        };
        if self.durable_events.is_none() {
            self.durable_events = Some(DurableEventStore::open(root)?);
        }
        let mut input = DurableEventInput::new(
            session_id,
            kind,
            DurableEventPayload::inline("orchestrator_fact", payload),
        );
        input.turn_id = turn_id;
        input.correlation_id = provenance
            .as_ref()
            .and_then(|value| value.execution_identity.as_ref())
            .map(|identity| identity.correlation_id.clone());
        input.causation_id = provenance
            .as_ref()
            .and_then(|value| value.execution_identity.as_ref())
            .map(|identity| identity.effect_id.clone());
        input.provenance = provenance;
        if let Some(store) = self.durable_events.as_mut() {
            store.append(input)?;
        }
        Ok(())
    }

    fn durable_workflow_fact_state(
        &mut self,
        workflow_id: &str,
    ) -> Result<WorkflowDurableFactState, AgentLoopError> {
        let Some(root) = self.config.durable_event_root.clone() else {
            return Ok(WorkflowDurableFactState::Absent);
        };
        if self.durable_events.is_none() {
            self.durable_events = Some(DurableEventStore::open(root)?);
        }
        let Some(store) = self.durable_events.as_ref() else {
            return Ok(WorkflowDurableFactState::Absent);
        };
        let scan = store.scan(usize::MAX)?;
        let mut state = WorkflowDurableFactState::Absent;
        for event in scan.records {
            if !matches!(
                event.kind.as_str(),
                WORKFLOW_PLANNED | WORKFLOW_COMPLETED | WORKFLOW_FAILED
            ) {
                continue;
            }
            let DurableEventPayload::Inline { data, .. } = event.payload else {
                continue;
            };
            if data.get("workflow_id").and_then(Value::as_str) != Some(workflow_id) {
                continue;
            }
            state = if event.kind == WORKFLOW_PLANNED {
                WorkflowDurableFactState::Planned
            } else {
                WorkflowDurableFactState::Terminal
            };
        }
        Ok(state)
    }

    fn ensure_durable_workflow_planned(
        &mut self,
        session_id: &str,
        turn_id: String,
        plan: &WorkflowHarnessPlan,
        provenance: DurableEventProvenance,
    ) -> Result<WorkflowDurableFactState, AgentLoopError> {
        let state = self.durable_workflow_fact_state(&plan.workflow_id)?;
        if state != WorkflowDurableFactState::Absent {
            return Ok(state);
        }
        let harness_plan_digest = shacs_workflow::workflow_harness_plan_digest(plan)
            .map_err(|error| AgentLoopError::Workflow(error.to_string()))?;
        self.append_durable_fact(
            session_id,
            Some(turn_id),
            WORKFLOW_PLANNED,
            json!({
                "workflow_id": plan.workflow_id.clone(),
                "harness_plan_digest": harness_plan_digest,
            }),
            Some(provenance),
        )?;
        Ok(WorkflowDurableFactState::Planned)
    }

    pub fn with_message_tool_delivery(mut self, message_tool: MessageTool) -> Self {
        let target = Arc::new(Mutex::new(None));
        let sender = Arc::new(BusMessageSender {
            bus: self.bus.clone(),
            workspace: self.config.workspace.clone(),
            media_roots: self.config.media_roots.clone(),
            current_target: target.clone(),
        });
        message_tool.set_sender(sender);
        self.context_tools = self.context_tools.with_message(message_tool);
        self.message_delivery_target = Some(target);
        self
    }

    pub fn bus(&self) -> &MessageBus {
        &self.bus
    }

    pub fn session_manager(&self) -> &SessionManager {
        &self.sessions
    }

    pub fn session_manager_mut(&mut self) -> &mut SessionManager {
        &mut self.sessions
    }

    pub fn active_session_keys(&self) -> Vec<String> {
        self.turn_lock.active_session_keys()
    }

    pub fn run_idle_auto_compact(
        &mut self,
    ) -> Result<Vec<AutoCompactArchiveOutcome>, AgentLoopError> {
        let Some(auto_compact) = self.auto_compact.as_mut() else {
            return Ok(Vec::new());
        };
        let store = MemoryStore::new(&self.config.workspace)?;
        let archive = ProviderArchiveConsolidator::new(self.client, self.config.model.clone())
            .with_settings(self.config.settings.clone())
            .with_retry_mode(self.config.retry_mode);
        let expired = auto_compact
            .mark_expired_sessions(&self.sessions, self.turn_lock.active_session_keys())?;
        let mut outcomes = Vec::new();
        for (index, key) in expired.iter().enumerate() {
            let outcome = (|| -> Result<AutoCompactArchiveOutcome, AgentLoopError> {
                self.sessions.invalidate(key);
                let session = self.sessions.get_or_create(key);
                let (archive_messages, _) = auto_compact.split_unconsolidated(&session);
                let summary = archive.archive(&store, &archive_messages)?.summary;
                auto_compact
                    .archive_session_with_summary(&mut self.sessions, key, summary.as_deref())
                    .map_err(AgentLoopError::from)
            })();
            match outcome {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    for pending_key in &expired[index..] {
                        auto_compact.release_archiving(pending_key);
                    }
                    return Err(error);
                }
            }
        }
        Ok(outcomes)
    }

    pub fn loop_task_registry(&self) -> LoopTaskRegistry {
        self.task_registry.clone()
    }

    pub fn process_next_inbound(&mut self) -> Result<Option<AgentLoopTurnResult>, AgentLoopError> {
        self.bus
            .try_consume_inbound()
            .map(|message| self.process_message(message))
            .transpose()
    }

    pub fn process_next_inbound_blocking(&mut self) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let message = self.bus.consume_inbound_blocking();
        self.process_message(message)
    }

    pub fn run_until_idle(
        &mut self,
        max_messages: usize,
    ) -> Result<AgentLoopRunSummary, AgentLoopError> {
        let mut summary = AgentLoopRunSummary::default();
        for _ in 0..max_messages {
            let Some(result) = self.process_next_inbound()? else {
                break;
            };
            summary.processed += 1;
            summary.outbound_count += result.outbound_count;
            summary.results.push(result);
            if self.stopped {
                summary.stopped = true;
                break;
            }
        }
        Ok(summary)
    }

    pub fn process_direct(
        &mut self,
        content: impl Into<String>,
        session_key: Option<&str>,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let mut message = InboundMessage::new("direct", "user", "direct", content);
        message.session_key_override = session_key.map(str::to_owned);
        self.process_message(message)
    }

    pub fn process_message(
        &mut self,
        message: InboundMessage,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let session_key = self.effective_session_key(&message);
        let routed_command = parse_loop_command_route(&message.content);

        if let Some(route) = routed_command
            .as_ref()
            .filter(|route| route.parsed.kind == CommandKind::Priority)
        {
            return match self.turn_lock.acquire_priority(session_key.clone()) {
                Ok(_turn_guard) => {
                    let mut session = self.sessions.get_or_create(&session_key);
                    materialize_recovery_markers(&mut session);
                    self.handle_loop_command(route.command.clone(), &message, session, true)
                }
                Err(SessionTurnAcquireError::AlreadyActive { .. }) => {
                    if matches!(&route.command, LoopCommand::Stop) {
                        self.turn_lock.cancel_active_or_reserved(&session_key);
                    }
                    let session = self.sessions.get_or_create(&session_key);
                    self.handle_loop_command(route.command.clone(), &message, session, false)
                }
            };
        }

        let _turn_guard = self.turn_lock.acquire(session_key.clone())?;
        let mut session = self.sessions.get_or_create(&session_key);
        materialize_recovery_markers(&mut session);

        if let Some(route) = routed_command {
            return self.handle_loop_command(route.command, &message, session, true);
        }

        if pending_permission_wizard(&session).is_some() {
            return self.handle_pending_permission_wizard(&message, session, &session_key);
        }

        if let Some(result) =
            self.try_resume_pending_workflow(&message, &mut session, &session_key)?
        {
            return Ok(result);
        }

        if self.stopped {
            let content = "Stopped. Use /new to start a fresh session or send /status for state.";
            return self.publish_command_response(
                &message,
                session,
                content,
                Some(AgentLoopCommandResult::StopRequested),
                "stopped",
                true,
            );
        }

        let mut session_summary = None;
        if let Some(auto_compact) = self.auto_compact.as_mut() {
            let prepared =
                auto_compact.prepare_session(&mut self.sessions, session, &session_key)?;
            session = prepared.0;
            session_summary = prepared.1;
        }

        let pending_recent_retry_approval = pending_recent_retry_approval(&session);
        let pending_permission_approval = pending_permission_approval(&session);
        let pending_ask_id = pending_ask_user_id(&session.messages);
        let execution_ledger = Arc::new(Mutex::new(execution_ledger_for_turn(&message, &session)));
        let turn_provenance =
            durable_event_provenance(&self.context_builder, &recover_lock(&execution_ledger));
        let (initial_messages, context_provider_handoff) = if let Some(approval) =
            pending_recent_retry_approval
        {
            if approval.status == PendingPermissionApprovalStatus::Executing {
                return self.publish_command_response(
                    &message,
                    session,
                    "Recent retry approval is already executing. Wait for the tool result or start a new session if recovery is required.",
                    None,
                    "permission_recent_retry_executing",
                    true,
                );
            }
            if !recent_retry_approval_reply_matches_request(&approval, &message, &session_key) {
                return self.publish_command_response(
                    &message,
                    session,
                    "Recent retry approval pending. Only the original requester in the original channel can approve or deny this tool call.",
                    None,
                    "permission_recent_retry_pending",
                    true,
                );
            }
            match parse_permission_approval_reply(&message.content) {
                PermissionApprovalReply::Approve => {
                    let token_result = self.recent_retry_tokens.consume(
                        &approval.denial_id,
                        &approval.action_digest,
                        &approval.argument_digest,
                        &approval.snapshot_digest,
                        now_unix_ms(),
                    );
                    let token = match token_result {
                        Ok(token) => token,
                        Err(error) => {
                            session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
                            return self.publish_command_response(
                                &message,
                                session,
                                &recent_retry_closed_message(error),
                                None,
                                "permission_recent_retry_closed",
                                true,
                            );
                        }
                    };
                    let approval_request = recent_retry_approval_request_from_pending(
                        &approval,
                        token
                            .tool_context()
                            .session_key
                            .as_deref()
                            .unwrap_or(&session.key),
                    );
                    let decision = recent_retry_approval_decision(
                        &approval_request,
                        ApprovalDecisionKind::Approved,
                    );
                    let approval_cache = ApprovalCacheEntry {
                        request: approval_request,
                        decision,
                    };
                    if !correlate_approval(
                        &approval_cache.request,
                        &approval_cache.decision,
                        now_unix_ms(),
                    )
                    .is_approved()
                    {
                        session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
                        return self.publish_command_response(
                            &message,
                            session,
                            "Recent retry approval failed closed; the action was not run. Request the action again if still needed.",
                            None,
                            "permission_recent_retry_closed",
                            true,
                        );
                    }
                    let mut executing_approval = approval.clone();
                    executing_approval.status = PendingPermissionApprovalStatus::Executing;
                    set_pending_recent_retry_approval(&mut session, &executing_approval);
                    self.sessions.save(&session)?;
                    let report = self.execute_approved_permission_payload(&token, approval_cache);
                    let fatal_error = self.append_approved_tool_messages(
                        &message,
                        &session_key,
                        &mut session,
                        &execution_ledger,
                        report.messages,
                    );
                    session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
                    if let Some(error) = fatal_error {
                        clear_runtime_markers(&mut session);
                        store_runtime_execution(&mut session, &recover_lock(&execution_ledger));
                        session.add_message("assistant", error.clone(), Map::new());
                        return self.publish_command_response(
                            &message,
                            session,
                            &error,
                            None,
                            "tool_error",
                            true,
                        );
                    }
                    let history = session.get_history_with_options(self.config.history_options);
                    let mut messages = vec![json!({
                        "role": "system",
                        "content": self.context_builder.build_system_prompt(Some(&message.channel)),
                    })];
                    messages.extend(history);
                    let context_provider_handoff = build_live_context_provider_handoff(
                        &self.config.workspace,
                        &message.content,
                        &messages,
                        current_working_directory(),
                        live_context_budget_bytes(self.config.context_block_limit),
                    );
                    (messages, Some(context_provider_handoff))
                }
                PermissionApprovalReply::ApproveSession => {
                    self.recent_retry_tokens.invalidate(&approval.denial_id);
                    session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
                    return self.publish_command_response(
                        &message,
                        session,
                        "Recent retry supports only one-shot `approve`; `approve_session` is rejected and the action was not run.",
                        None,
                        "permission_recent_retry_rejected",
                        true,
                    );
                }
                PermissionApprovalReply::Deny => {
                    self.recent_retry_tokens.invalidate(&approval.denial_id);
                    session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
                    return self.publish_command_response(
                        &message,
                        session,
                        "Recent retry cancelled. The denied action was not run.",
                        None,
                        "permission_recent_retry_denied",
                        true,
                    );
                }
                PermissionApprovalReply::Unknown => {
                    return self.publish_command_response(
                        &message,
                        session,
                        "Recent retry approval pending. Reply with `1` or `approve` to run the exact denied action once, or `2`/`deny` to cancel. `approve_session` is not available for recent retry.",
                        None,
                        "permission_recent_retry_pending",
                        true,
                    );
                }
            }
        } else if let Some(approval) = pending_permission_approval {
            if approval.status == PendingPermissionApprovalStatus::Executing {
                return self.publish_command_response(
                    &message,
                    session,
                    "Permission approval is already executing. Wait for the tool result or start a new session if recovery is required.",
                    None,
                    "permission_approval_executing",
                    true,
                );
            }
            if !permission_approval_reply_matches_request(&approval, &message, &session_key) {
                return self.publish_command_response(
                    &message,
                    session,
                    "Approval pending. Only the original requester in the original channel can approve or deny this tool call.",
                    None,
                    "permission_approval_pending",
                    true,
                );
            }
            match parse_permission_approval_reply(&message.content) {
                PermissionApprovalReply::Approve | PermissionApprovalReply::ApproveSession => {
                    let session_scoped = matches!(
                        parse_permission_approval_reply(&message.content),
                        PermissionApprovalReply::ApproveSession
                    );
                    let decision = approval_decision(
                        &approval,
                        if session_scoped {
                            ApprovalDecisionKind::ApprovedForSession
                        } else {
                            ApprovalDecisionKind::Approved
                        },
                    );
                    let approval_cache = ApprovalCacheEntry {
                        request: approval.approval_request.clone(),
                        decision,
                    };
                    let mut executing_approval = approval.clone();
                    executing_approval.status = PendingPermissionApprovalStatus::Executing;
                    set_pending_permission_approval(&mut session, &executing_approval);
                    self.sessions.save(&session)?;
                    let report =
                        self.execute_approved_permission_tool(&approval, approval_cache.clone());
                    let approved_action = report.permissioned_actions.first().cloned();
                    let fatal_error = self.append_approved_tool_messages(
                        &message,
                        &session_key,
                        &mut session,
                        &execution_ledger,
                        report.messages,
                    );
                    session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
                    if let (true, Some(action)) = (session_scoped, approved_action) {
                        store_session_permission_approval(
                            &mut session,
                            &session_key,
                            approval_cache,
                            &action,
                        );
                    }
                    if let Some(error) = fatal_error {
                        clear_runtime_markers(&mut session);
                        store_runtime_execution(&mut session, &recover_lock(&execution_ledger));
                        session.add_message("assistant", error.clone(), Map::new());
                        return self.publish_command_response(
                            &message,
                            session,
                            &error,
                            None,
                            "tool_error",
                            true,
                        );
                    }
                    let history = session.get_history_with_options(self.config.history_options);
                    let mut messages = vec![json!({
                        "role": "system",
                        "content": self.context_builder.build_system_prompt(Some(&message.channel)),
                    })];
                    messages.extend(history);
                    let context_provider_handoff = build_live_context_provider_handoff(
                        &self.config.workspace,
                        &message.content,
                        &messages,
                        current_working_directory(),
                        live_context_budget_bytes(self.config.context_block_limit),
                    );
                    (messages, Some(context_provider_handoff))
                }
                PermissionApprovalReply::Deny => {
                    append_session_message(
                        &mut session,
                        RuntimeToolMessage {
                            tool_call_id: approval.tool_call.id,
                            name: approval.tool_call.name,
                            content: "Permission denied by user.".to_owned(),
                        }
                        .to_json(),
                    );
                    session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
                    return self.publish_command_response(
                        &message,
                        session,
                        "Tool execution cancelled.",
                        None,
                        "permission_denied_by_user",
                        true,
                    );
                }
                PermissionApprovalReply::Unknown => {
                    return self.publish_command_response(
                            &message,
                            session,
                            "Approval pending. Reply with `1` or `approve` to run it once, `3` or `approve_session` to approve matching actions in this session, or `2` or `deny` to cancel.",
                            None,
                            "permission_approval_pending",
                            true,
                        );
                }
            }
        } else if let Some(tool_call_id) = pending_ask_id {
            append_ask_user_resume(&mut session, &tool_call_id, &message.content);
            let history = session.get_history_with_options(self.config.history_options);
            let mut messages = vec![json!({
                "role": "system",
                "content": self.context_builder.build_system_prompt(Some(&message.channel)),
            })];
            messages.extend(history);
            let context_provider_handoff = build_live_context_provider_handoff(
                &self.config.workspace,
                &message.content,
                &messages,
                current_working_directory(),
                live_context_budget_bytes(self.config.context_block_limit),
            );
            (messages, Some(context_provider_handoff))
        } else {
            if let Some(result) = self.try_plugin_command(&message, &session)? {
                return Ok(result);
            }
            self.maybe_consolidate_session_by_tokens(&mut session)?;
            if let Some(result) =
                self.try_live_workflow_turn(&message, &mut session, &session_key)?
            {
                return Ok(result);
            }
            let history = session.get_history_with_options(self.config.history_options);
            let initial_messages = self.context_builder.build_messages(ContextBuildRequest {
                history,
                current_message: &message.content,
                media: &message.media,
                channel: Some(&message.channel),
                chat_id: Some(&message.chat_id),
                current_role: "user",
                session_summary: session_summary.as_deref(),
            });
            let context_provider_handoff = build_live_context_provider_handoff(
                &self.config.workspace,
                &message.content,
                &initial_messages,
                current_working_directory(),
                live_context_budget_bytes(self.config.context_block_limit),
            );
            append_user_turn(&mut session, &message);
            let accepted_turn_id = turn_id_for_message(&message, &session);
            self.append_durable_fact(
                &session_key,
                Some(accepted_turn_id),
                SESSION_TURN_ACCEPTED,
                json!({
                    "channel": message.channel.clone(),
                    "content_hash": format!("sha256:{:x}", Sha256::digest(message.content.as_bytes())),
                    "media_count": message.media.len(),
                }),
                Some(turn_provenance.clone()),
            )?;
            session
                .metadata
                .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
            (initial_messages, Some(context_provider_handoff))
        };
        let turn_id = turn_id_for_message(&message, &session);
        self.sessions.save(&session)?;

        let _delivery_target_guard = self.message_delivery_target.as_ref().map(|target| {
            MessageDeliveryTargetGuard::new(
                target.clone(),
                message.channel.clone(),
                message.chat_id.clone(),
            )
        });
        let checkpoint_session = Arc::new(Mutex::new(session.clone()));
        let checkpoint_capture = checkpoint_session.clone();
        let checkpoint_manager = Arc::new(Mutex::new(self.sessions.clone()));
        let checkpoint_manager_capture = checkpoint_manager.clone();
        let checkpoint_execution_ledger = execution_ledger.clone();
        let tool_context = ToolExecutionContext {
            channel: message.channel.clone(),
            chat_id: message.chat_id.clone(),
            message_id: message
                .metadata
                .get("message_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            metadata: Value::Object(message.metadata.clone()),
            session_key: Some(session_key.clone()),
            containment_snapshot: self.config.containment_snapshot.clone(),
            permission_mode_snapshot: self.config.permission_mode_snapshot.clone(),
            permission_rule_input: self.config.permission_rule_input.clone(),
            permission_auto_approval: self.config.permission_auto_approval.clone(),
            permission_ceiling_snapshot: self.config.permission_ceiling_snapshot.clone(),
            permission_evaluator: self.config.permission_evaluator.clone(),
            permission_interactive: self.config.permission_interactive,
            permission_approval_cache: None,
            permission_session_approval_cache: session_permission_approvals(&session),
            in_cron_context: false,
            record_channel_delivery: self.config.record_channel_delivery,
        };
        let mut spec = AgentRunSpec::new(
            initial_messages.clone(),
            self.tools,
            self.client,
            self.config.model.clone(),
        );
        spec.permission_classifier_client = Some(self.client);
        spec.settings = self.config.settings.clone();
        spec.retry_mode = self.config.retry_mode;
        spec.max_iterations = self.config.max_iterations;
        spec.max_iterations_message = self.config.max_iterations_message.clone();
        spec.max_tool_result_chars = self.config.max_tool_result_chars;
        spec.workspace = Some(self.config.workspace.clone());
        spec.session_key = Some(session_key.clone());
        spec.tool_search = self.config.tool_search;
        spec.context_window_tokens = self.config.context_window_tokens;
        spec.context_block_limit = self.config.context_block_limit;
        spec.context_provider_handoff = context_provider_handoff;
        spec.concurrent_tools = self.config.concurrent_tools;
        spec.fail_on_tool_error = self.config.fail_on_tool_error;
        spec.tool_context = tool_context.clone();
        spec.context_tools = self.context_tools.clone();
        spec.cancellation_token = self
            .task_registry
            .cancellation_token(&session_key)
            .or_else(|| self.turn_lock.cancellation_token(&session_key));
        spec.execution_scope = Some(ExecutionScope::new(session_key.clone(), turn_id.clone()));
        spec.execution_ledger = Some(execution_ledger.clone());
        spec.tool_event_callback = self.tool_event_callback.clone();
        spec.provider_event_callback = self.provider_event_callback.clone();
        spec.agent_hook = self.agent_hook.clone();
        spec.mid_turn_injection_callback = Some(mid_turn_injection_callback(
            self.bus.clone(),
            session_key.clone(),
            self.config.unified_session_key.clone(),
            self.context_builder.clone(),
        ));
        spec.checkpoint_callback = Some(Arc::new(move |checkpoint| {
            let mut session = recover_lock(&checkpoint_capture);
            session
                .metadata
                .insert(RUNTIME_CHECKPOINT_KEY.to_owned(), checkpoint.clone());
            store_runtime_execution(&mut session, &recover_lock(&checkpoint_execution_ledger));
            let _ = recover_lock(&checkpoint_manager_capture).save(&session);
        }));

        let run_result = match self.runner.run(spec) {
            Ok(result) => result,
            Err(error) => {
                session.add_message("assistant", provider_error_text(&error), Map::new());
                clear_runtime_markers(&mut session);
                let execution_ledger = recover_lock(&execution_ledger);
                store_runtime_execution(&mut session, &execution_ledger);
                let provenance =
                    durable_event_provenance_from_snapshot(&turn_provenance, &execution_ledger);
                drop(execution_ledger);
                self.append_durable_fact(
                    &session_key,
                    Some(turn_id),
                    SESSION_TURN_FAILED,
                    json!({"stop_reason": "provider_error"}),
                    Some(provenance),
                )?;
                self.sessions.save(&session)?;
                let outbound_count = self.publish_error(&message, &session_key, &error);
                return Ok(AgentLoopTurnResult {
                    session_key,
                    final_content: Some(provider_error_text(&error)),
                    stop_reason: "error".to_owned(),
                    tools_used: Vec::new(),
                    outbound_count,
                    had_injections: false,
                    command: None,
                    ask_user_options: Vec::new(),
                    message_tool_delivery_configured: self.context_tools.message.is_some(),
                });
            }
        };
        append_new_runner_messages(&mut session, &initial_messages, &run_result.messages);
        self.recent_retry_tokens
            .extend(run_result.recent_auto_mode_retry_tokens.clone());
        store_recent_auto_mode_denials(&mut session, run_result.recent_auto_mode_denials.clone());
        clear_runtime_markers(&mut session);
        store_runtime_execution(&mut session, &recover_lock(&execution_ledger));
        store_pending_permission_approval(
            &mut session,
            &run_result.interrupt,
            &tool_context,
            &message,
            &session_key,
        );
        let execution_ledger = recover_lock(&execution_ledger);
        let provenance =
            durable_event_provenance_from_snapshot(&turn_provenance, &execution_ledger);
        let outcome_count = execution_ledger.outcomes.len();
        let pending_effect_count = execution_ledger.pending.len();
        drop(execution_ledger);
        self.append_durable_fact(
            &session_key,
            Some(turn_id),
            SESSION_TURN_COMPLETED,
            json!({
                "stop_reason": run_result.stop_reason.clone(),
                "tool_count": run_result.tools_used.len(),
                "outcome_count": outcome_count,
                "pending_effect_count": pending_effect_count,
            }),
            Some(provenance),
        )?;
        self.sessions.save(&session)?;

        let (outbound_count, ask_user_options) =
            self.publish_run_outbound(&message, &session_key, &run_result)?;
        Ok(AgentLoopTurnResult {
            session_key,
            final_content: run_result.final_content,
            stop_reason: run_result.stop_reason,
            tools_used: run_result.tools_used,
            outbound_count,
            had_injections: run_result.had_injections,
            command: None,
            ask_user_options,
            message_tool_delivery_configured: self.context_tools.message.is_some(),
        })
    }

    fn effective_session_key(&self, message: &InboundMessage) -> String {
        effective_message_session_key(message, self.config.unified_session_key.as_deref())
    }

    fn maybe_consolidate_session_by_tokens(
        &mut self,
        session: &mut Session,
    ) -> Result<(), AgentLoopError> {
        let Some(context_window_tokens) = self.config.context_window_tokens else {
            return Ok(());
        };
        let store = MemoryStore::new(&self.config.workspace)?;
        let archive = ProviderArchiveConsolidator::new(self.client, self.config.model.clone())
            .with_settings(self.config.settings.clone())
            .with_retry_mode(self.config.retry_mode);
        let config = TokenConsolidationConfig::new(
            context_window_tokens,
            self.config.settings.max_tokens as usize,
        );
        let session_summary = session.key.clone();
        archive.maybe_consolidate_session_by_tokens(
            &store,
            &mut self.sessions,
            session,
            &config,
            &self.tools.definitions(),
            Some(&session_summary),
        )?;
        Ok(())
    }

    fn try_live_workflow_turn(
        &mut self,
        message: &InboundMessage,
        session: &mut Session,
        session_key: &str,
    ) -> Result<Option<AgentLoopTurnResult>, AgentLoopError> {
        let Some((admission, plan, recipe_selection)) = workflow_request_from_message(
            message,
            session_key,
            &self.config.workspace,
            &self.config.permission_mode_snapshot,
        )?
        else {
            return Ok(None);
        };

        self.run_admitted_workflow_turn(
            message,
            session,
            session_key,
            admission,
            plan,
            recipe_selection,
        )
        .map(Some)
    }

    fn try_resume_pending_workflow(
        &mut self,
        message: &InboundMessage,
        session: &mut Session,
        session_key: &str,
    ) -> Result<Option<AgentLoopTurnResult>, AgentLoopError> {
        let Some(mut pending) = pending_workflow(session) else {
            return Ok(None);
        };
        if pending.session_key != session_key {
            return Ok(None);
        }
        if !workflow_plan_is_live_read_only(&pending.plan)
            || pending.admission.requires_write_isolation
        {
            return self.block_pending_workflow_recovery(
                message,
                session,
                session_key,
                &pending.plan,
                "ambiguous_write_phase",
                "Workflow blocked: restart recovery found an ambiguous write-capable workflow phase. Re-run the request after confirming the write scope.",
            )
            .map(Some);
        }
        if let Some(block_reason) = pending_workflow_resume_block_reason(
            session,
            &pending.plan,
            pending.recipe_evidence.as_ref(),
            &self.config.workspace,
        ) {
            return self
                .block_pending_workflow_recovery(
                    message,
                    session,
                    session_key,
                    &pending.plan,
                    "resume_validation_failed",
                    &format!("Workflow blocked: restart recovery could not safely resume the saved workflow checkpoint: {block_reason}"),
                )
                .map(Some);
        }
        pending.plan.origin_session_id = session_key.to_owned();
        if apply_completed_workflow_checkpoint(&mut pending.plan, session) {
            let workflow_turn_id = turn_id_for_message(message, session);
            let workflow_provenance =
                durable_event_provenance(&self.context_builder, &RuntimeExecutionLedger::default());
            let durable_state = self.ensure_durable_workflow_planned(
                session_key,
                workflow_turn_id.clone(),
                &pending.plan,
                workflow_provenance.clone(),
            )?;
            if durable_state != WorkflowDurableFactState::Terminal {
                self.append_durable_fact(
                    session_key,
                    Some(workflow_turn_id),
                    WORKFLOW_COMPLETED,
                    json!({
                        "workflow_id": pending.plan.workflow_id.clone(),
                        "state": "completed_from_checkpoint",
                        "child_result_count": 0,
                    }),
                    Some(workflow_provenance),
                )?;
            }
            let content = format!(
                "Workflow completed from saved checkpoint: {}",
                pending.plan.workflow_id
            );
            session.add_message("assistant", content.clone(), Map::new());
            clear_runtime_markers(session);
            self.sessions.save(session)?;
            self.bus.publish_outbound(outbound_for(
                message,
                session_key,
                content.clone(),
                Vec::new(),
                "workflow_completed",
            ));
            return Ok(Some(AgentLoopTurnResult {
                session_key: session_key.to_owned(),
                final_content: Some(content),
                stop_reason: "workflow_completed".to_owned(),
                tools_used: Vec::new(),
                outbound_count: 1,
                had_injections: false,
                command: None,
                ask_user_options: Vec::new(),
                message_tool_delivery_configured: self.context_tools.message.is_some(),
            }));
        }
        self.run_admitted_workflow_turn(
            message,
            session,
            session_key,
            pending.admission,
            pending.plan,
            pending.recipe_evidence,
        )
        .map(Some)
    }

    fn block_pending_workflow_recovery(
        &mut self,
        message: &InboundMessage,
        session: &mut Session,
        session_key: &str,
        plan: &WorkflowHarnessPlan,
        reason: &str,
        content: &str,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        session.add_message("assistant", content, Map::new());
        clear_runtime_markers(session);
        session.metadata.insert(
            RUNTIME_CHECKPOINT_KEY.to_owned(),
            json!({
                "phase": "workflow_blocked_recovery",
                "workflow_id": plan.workflow_id,
                "reason": reason,
            }),
        );
        let workflow_turn_id = turn_id_for_message(message, session);
        let workflow_provenance =
            durable_event_provenance(&self.context_builder, &RuntimeExecutionLedger::default());
        let durable_state = self.ensure_durable_workflow_planned(
            session_key,
            workflow_turn_id.clone(),
            plan,
            workflow_provenance.clone(),
        )?;
        if durable_state != WorkflowDurableFactState::Terminal {
            self.append_durable_fact(
                session_key,
                Some(workflow_turn_id),
                WORKFLOW_FAILED,
                json!({
                    "workflow_id": plan.workflow_id.clone(),
                    "state": "blocked_recovery",
                    "reason": reason,
                }),
                Some(workflow_provenance),
            )?;
        }
        self.sessions.save(session)?;
        self.bus.publish_outbound(outbound_for(
            message,
            session_key,
            content.to_owned(),
            Vec::new(),
            "workflow_blocked",
        ));
        Ok(AgentLoopTurnResult {
            session_key: session_key.to_owned(),
            final_content: Some(content.to_owned()),
            stop_reason: "workflow_blocked".to_owned(),
            tools_used: Vec::new(),
            outbound_count: 1,
            had_injections: false,
            command: None,
            ask_user_options: Vec::new(),
            message_tool_delivery_configured: self.context_tools.message.is_some(),
        })
    }

    fn run_admitted_workflow_turn(
        &mut self,
        message: &InboundMessage,
        session: &mut Session,
        session_key: &str,
        admission: WorkflowAdmissionInput,
        mut plan: WorkflowHarnessPlan,
        recipe_selection: Option<WorkflowRecipeSelection>,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        match decide_workflow_admission(&admission) {
            WorkflowAdmissionDecision::UseRegularLoop => Err(AgentLoopError::Workflow(
                "workflow metadata admission resolved to regular loop".to_owned(),
            )),
            WorkflowAdmissionDecision::AskUserForScope { question } => self
                .publish_command_response(
                    message,
                    session.clone(),
                    &question,
                    None,
                    "workflow_ask_user",
                    true,
                ),
            WorkflowAdmissionDecision::BlockedByPolicy { reasons } => {
                let content = format!("Workflow blocked: {}", reasons.join("; "));
                self.publish_command_response(
                    message,
                    session.clone(),
                    &content,
                    None,
                    "workflow_blocked",
                    true,
                )
            }
            WorkflowAdmissionDecision::UseQuickWorkflow { .. }
            | WorkflowAdmissionDecision::UseDynamicWorkflow { .. } => {
                if admission.requires_write_isolation && live_plan_missing_write_isolation(&plan) {
                    return self.publish_command_response(
                        message,
                        session.clone(),
                        "Workflow blocked: write isolation required but the live plan is read-only",
                        None,
                        "workflow_blocked",
                        true,
                    );
                }
                plan.origin_session_id = session_key.to_owned();
                if pending_workflow(session).is_none() {
                    append_user_turn(session, message);
                }
                session
                    .metadata
                    .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
                let recipe_evidence = recipe_selection;
                store_pending_workflow(
                    session,
                    session_key,
                    &admission,
                    &plan,
                    recipe_evidence.clone(),
                )?;
                store_planned_workflow_checkpoint(session, &plan, recipe_evidence.as_ref());
                let workflow_turn_id = turn_id_for_message(message, session);
                let workflow_provenance = durable_event_provenance(
                    &self.context_builder,
                    &RuntimeExecutionLedger::default(),
                );
                let durable_workflow_state = self.ensure_durable_workflow_planned(
                    session_key,
                    workflow_turn_id.clone(),
                    &plan,
                    workflow_provenance.clone(),
                )?;
                if durable_workflow_state == WorkflowDurableFactState::Terminal {
                    return Err(AgentLoopError::Workflow(format!(
                        "workflow {} is already terminal",
                        plan.workflow_id
                    )));
                }
                self.sessions.save(session)?;

                let mut subagent_runtime = SubagentRuntime::new();
                if let Some(root) = self.config.durable_event_root.as_deref() {
                    subagent_runtime = subagent_runtime
                        .attach_durable_recorder(DurableChildRecorder::open(root)?)
                        .map_err(AgentLoopError::Workflow)?;
                }
                let checkpoint_session = Arc::new(Mutex::new(session.clone()));
                let checkpoint_capture = checkpoint_session.clone();
                let checkpoint_manager = Arc::new(Mutex::new(self.sessions.clone()));
                let checkpoint_manager_capture = checkpoint_manager.clone();
                let recipe_evidence_for_checkpoint = recipe_evidence.clone();
                let mut checkpoint_callback =
                    move |payload: &shacs_workflow::WorkflowRuntimeCheckpointPayload| {
                        let checkpoint = &payload.checkpoint;
                        let mut session = recover_lock(&checkpoint_capture);
                        session.metadata.insert(
                            RUNTIME_CHECKPOINT_KEY.to_owned(),
                            json!({
                                "phase": checkpoint.last_safe_resume_point,
                                "workflow": checkpoint,
                                "workflow_checkpoint_payload": payload,
                                "recipe_evidence": recipe_evidence_for_checkpoint.clone(),
                            }),
                        );
                        let _ = recover_lock(&checkpoint_manager_capture).save(&session);
                    };
                let outcome = match run_live_runtime_workflow_with_checkpoint_callback(
                    RuntimeWorkflowLiveOptions {
                        input: RuntimeWorkflowLiveInput {
                            plan: plan.clone(),
                            subagent_runtime: &subagent_runtime,
                            provider_client: self.client,
                            execution_config: self.workflow_subagent_execution_config(&plan),
                            admitted_at_ms: now_unix_ms(),
                        },
                        worktree_config: self.workflow_worktree_config(&plan),
                        cancellation_token: self
                            .task_registry
                            .cancellation_token(session_key)
                            .or_else(|| self.turn_lock.cancellation_token(session_key)),
                    },
                    &mut checkpoint_callback,
                ) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        let final_content = format!("Workflow failed: {error}");
                        session.add_message("assistant", final_content.clone(), Map::new());
                        clear_runtime_markers(session);
                        self.append_durable_fact(
                            session_key,
                            Some(workflow_turn_id),
                            WORKFLOW_FAILED,
                            json!({
                                "workflow_id": plan.workflow_id.clone(),
                                "stop_reason": "workflow_error",
                            }),
                            Some(workflow_provenance),
                        )?;
                        self.sessions.save(session)?;
                        self.bus.publish_outbound(outbound_for(
                            message,
                            session_key,
                            final_content.clone(),
                            Vec::new(),
                            "workflow_failed",
                        ));
                        return Ok(AgentLoopTurnResult {
                            session_key: session_key.to_owned(),
                            final_content: Some(final_content),
                            stop_reason: "workflow_failed".to_owned(),
                            tools_used: Vec::new(),
                            outbound_count: 1,
                            had_injections: false,
                            command: None,
                            ask_user_options: Vec::new(),
                            message_tool_delivery_configured: self.context_tools.message.is_some(),
                        });
                    }
                };
                let recorded_at_ms = now_unix_ms();
                let mut diagnostics = runtime_workflow_diagnostics(&plan, &outcome)
                    .map_err(|error| AgentLoopError::Workflow(error.to_string()))?;
                if let Some(source_ref) = recipe_evidence
                    .as_ref()
                    .map(|evidence| evidence.selected.recipe.source_ref.clone())
                {
                    diagnostics
                        .manifest
                        .runtime_diagnostic_refs
                        .push(source_ref);
                    diagnostics.manifest.runtime_diagnostic_refs.sort();
                    diagnostics.manifest.runtime_diagnostic_refs.dedup();
                }
                let diagnostics_ref = format!(
                    "workflow-diagnostics:{}:{}",
                    diagnostics.manifest.workflow_id, diagnostics.manifest.harness_plan_digest
                );
                let worktree_refs = outcome
                    .worktree_evidence
                    .iter()
                    .map(|evidence| evidence.create.worktree_ref.clone())
                    .collect::<Vec<_>>();
                let mut evidence_refs = outcome
                    .synthesis_outcome
                    .evidence_refs
                    .iter()
                    .map(|evidence_ref| evidence_ref.id.clone())
                    .collect::<Vec<_>>();
                evidence_refs.extend(
                    outcome
                        .verifier_verdicts
                        .iter()
                        .flat_map(|verdict| verdict.evidence_refs.iter())
                        .map(|evidence_ref| evidence_ref.id.clone()),
                );
                if let Some(source_ref) = recipe_evidence
                    .as_ref()
                    .map(|evidence| evidence.selected.recipe.source_ref.clone())
                {
                    evidence_refs.push(source_ref);
                    evidence_refs.sort();
                    evidence_refs.dedup();
                }
                let checkpoint = build_workflow_checkpoint(
                    &plan,
                    &outcome.run,
                    WorkflowCheckpointInput {
                        state: outcome.run.state,
                        completed_steps: completed_workflow_step_ids(&plan, &outcome.child_results),
                        active_children: Vec::new(),
                        pending_barriers: Vec::new(),
                        budget_usage: outcome.budget_usage.clone(),
                        worktree_refs,
                        evidence_refs,
                        last_safe_resume_point: workflow_stop_reason(outcome.run.state).to_owned(),
                        recorded_at_ms,
                    },
                );
                let projection = workflow_projection(
                    &outcome.run,
                    &plan,
                    Some(&checkpoint),
                    &outcome.verification_gate,
                    &outcome.synthesis_outcome.evidence_refs,
                );
                let final_content = format_workflow_turn_content(&outcome);
                session.add_message("assistant", final_content.clone(), Map::new());
                clear_runtime_markers(session);
                session.metadata.insert(
                    RUNTIME_CHECKPOINT_KEY.to_owned(),
                    json!({
                        "phase": workflow_stop_reason(outcome.run.state),
                        "workflow": checkpoint,
                        "recipe_evidence": recipe_evidence,
                    }),
                );
                session.metadata.insert(
                    "runtime_diagnostics".to_owned(),
                    json!({
                        "refs": [diagnostics_ref],
                        "workflow_manifest": diagnostics.manifest,
                        "event_phases": diagnostics.event_phases,
                        "terminal_state": diagnostics.terminal_state,
                        "child_result_count": diagnostics.child_result_count,
                        "verifier_status": diagnostics.verifier_status,
                        "replay_live_actions_allowed": diagnostics.replay_live_actions_allowed,
                        "cleanup_evidence": [],
                    }),
                );
                session.metadata.insert(
                    "runtime_workflow".to_owned(),
                    json!({
                        "workflow_id": outcome.run.workflow_id.clone(),
                        "harness_plan_digest": outcome.run.harness_plan_digest.clone(),
                        "state": outcome.run.state,
                        "projection": projection,
                        "events": outcome.events.clone(),
                        "child_result_count": outcome.child_results.len(),
                        "budget_usage": outcome.budget_usage,
                        "verifier_status": format!("{:?}", outcome.verification_gate),
                        "recipe_evidence": recipe_evidence,
                    }),
                );
                let workflow_ledger =
                    workflow_execution_ledger(&plan, &outcome.child_results, recorded_at_ms);
                store_runtime_execution(session, &workflow_ledger);
                self.append_durable_fact(
                    session_key,
                    Some(workflow_turn_id),
                    WORKFLOW_COMPLETED,
                    json!({
                        "workflow_id": outcome.run.workflow_id.clone(),
                        "state": outcome.run.state,
                        "child_result_count": outcome.child_results.len(),
                    }),
                    Some(durable_event_provenance_from_snapshot(
                        &workflow_provenance,
                        &workflow_ledger,
                    )),
                )?;
                self.sessions.save(session)?;
                let cleanup_evidence = self
                    .workflow_worktree_config(&plan)
                    .map(|config| {
                        outcome
                            .worktree_evidence
                            .iter()
                            .map(|evidence| {
                                shacs_utils::worktree::cleanup_git_worktree(
                                    &config.repo_path,
                                    &evidence.create.worktree_path,
                                    true,
                                )
                                .map(|cleanup| json!(cleanup))
                                .unwrap_or_else(|reason| {
                                    json!({
                                        "worktree_path": evidence.create.worktree_path,
                                        "diagnostics_recorded": true,
                                        "removed": false,
                                        "message": format!("cleanup failed after diagnostics: {reason}"),
                                    })
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                session.metadata["runtime_diagnostics"]["cleanup_evidence"] =
                    json!(cleanup_evidence);
                self.sessions.save(session)?;
                let stop_reason = workflow_stop_reason(outcome.run.state);
                self.bus.publish_outbound(outbound_for(
                    message,
                    session_key,
                    final_content.clone(),
                    Vec::new(),
                    stop_reason,
                ));
                Ok(AgentLoopTurnResult {
                    session_key: session_key.to_owned(),
                    final_content: Some(final_content),
                    stop_reason: stop_reason.to_owned(),
                    tools_used: Vec::new(),
                    outbound_count: 1,
                    had_injections: false,
                    command: None,
                    ask_user_options: Vec::new(),
                    message_tool_delivery_configured: self.context_tools.message.is_some(),
                })
            }
        }
    }

    fn workflow_subagent_execution_config(
        &self,
        plan: &WorkflowHarnessPlan,
    ) -> SubagentExecutionConfig {
        let model = plan
            .model_routing_policy
            .child_model_hint
            .clone()
            .unwrap_or_else(|| self.config.model.clone());
        let mut config = SubagentExecutionConfig::new(self.config.workspace.clone(), model);
        config.settings = self.config.settings.clone();
        config.retry_mode = self.config.retry_mode;
        config.containment_snapshot = self.config.containment_snapshot.clone();
        config.permission_mode_snapshot = self.config.permission_mode_snapshot.clone();
        config.permission_rule_input = self.config.permission_rule_input.clone();
        config.permission_ceiling_snapshot = self.config.permission_ceiling_snapshot.clone();
        config.max_iterations = self.config.max_iterations;
        config.max_tool_result_chars = self.config.max_tool_result_chars;
        config.fail_on_tool_error = self.config.fail_on_tool_error;
        config.allow_side_effect_tools = false;
        config.enable_exec = false;
        config
    }

    fn workflow_worktree_config(
        &self,
        plan: &WorkflowHarnessPlan,
    ) -> Option<RuntimeWorkflowLiveWorktreeConfig> {
        if !workflow_plan_requests_isolated_worktree(plan) {
            return None;
        }
        Some(RuntimeWorkflowLiveWorktreeConfig {
            enabled: true,
            approval_granted: workflow_worktree_approval_granted(&self.config, plan),
            repo_path: self.config.workspace.clone(),
            worktree_root: self.config.workspace.join(".shacs/workflow-worktrees"),
            base_ref: "HEAD".to_owned(),
        })
    }

    fn handle_loop_command(
        &mut self,
        command: LoopCommand,
        message: &InboundMessage,
        mut session: Session,
        save_session: bool,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        match command {
            LoopCommand::Status => {
                let content = if self.stopped {
                    "Status: stopped".to_owned()
                } else if let Some(task) = self.task_registry.snapshot(&session.key) {
                    format!(
                        "Status: active async task {} ({:?})",
                        task.task_id, task.status
                    )
                } else {
                    "Status: no active task".to_owned()
                };
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::Status),
                    "status",
                    save_session,
                )
            }
            LoopCommand::New => {
                self.stopped = false;
                let _ = self.task_registry.cancel(&session.key);
                session.clear();
                clear_runtime_markers(&mut session);
                session.metadata.remove(SESSION_PERMISSION_APPROVALS_KEY);
                remove_persistent_goal(&mut session);
                self.publish_command_response(
                    message,
                    session,
                    "Started a new session.",
                    Some(AgentLoopCommandResult::NewSession),
                    "new_session",
                    save_session,
                )
            }
            LoopCommand::Goal(args) => {
                let content = handle_goal_command(&mut session, args)?;
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::Goal),
                    "goal",
                    save_session,
                )
            }
            LoopCommand::Stop => {
                let synchronous_turn_cancelled = self
                    .turn_lock
                    .cancellation_token(&session.key)
                    .is_some_and(|token| token.is_cancelled());
                let content = match self.task_registry.cancel(&session.key) {
                    LoopTaskCancelResult::NoAsyncTask => {
                        if synchronous_turn_cancelled {
                            "Stop requested. Cancellation requested for the active session turn. Provider/tool execution will stop only if it observes the cancellation token."
                                .to_owned()
                        } else {
                            "Stop requested. No async task is running in this synchronous loop."
                                .to_owned()
                        }
                    }
                    LoopTaskCancelResult::CancellationRequested(snapshot) => format!(
                        "Stop requested. Cancellation requested for async task {}. Provider/tool execution will stop only if the task observes the cancellation token.",
                        snapshot.task_id
                    ),
                };
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::StopRequested),
                    "stop_requested",
                    save_session,
                )
            }
            LoopCommand::Restart => self.publish_command_response(
                message,
                session,
                "Restart requested. Stop and start the local shacs-bot process to apply it.",
                Some(AgentLoopCommandResult::RestartRequested),
                "restart_requested",
                save_session,
            ),
            LoopCommand::History(HistoryCommandArgs::Count(count)) => {
                let content = format_history_command(&session, count);
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::History),
                    "history",
                    save_session,
                )
            }
            LoopCommand::History(HistoryCommandArgs::Invalid) => self.publish_command_response(
                message,
                session,
                "Usage: /history [n] where n is between 1 and 50.",
                Some(AgentLoopCommandResult::History),
                "history_usage",
                save_session,
            ),
            LoopCommand::Dream => {
                let store = MemoryStore::new(&self.config.workspace)?;
                let git = memory_git_store(&self.config.workspace);
                let outcome = DreamProcessor::new(store, self.client, self.config.model.clone())
                    .with_settings(self.config.settings.clone())
                    .with_retry_mode(self.config.retry_mode)
                    .with_git_boundary(&git)
                    .run()?;
                let content = format_dream_outcome(&outcome);
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::Dream),
                    "dream",
                    save_session,
                )
            }
            LoopCommand::DreamLog { sha } => {
                let git = memory_git_store(&self.config.workspace);
                let content = format_dream_log(&git, sha.as_deref());
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::DreamLog),
                    "dream_log",
                    save_session,
                )
            }
            LoopCommand::DreamRestore { sha } => {
                let git = memory_git_store(&self.config.workspace);
                let content = format_dream_restore(&git, sha.as_deref());
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::DreamRestore),
                    "dream_restore",
                    save_session,
                )
            }
            LoopCommand::Permission(PermissionCommandArgs::ModeWizard) => {
                let wizard = PendingPermissionWizard {
                    session_key: session.key.clone(),
                    channel: message.channel.clone(),
                    chat_id: message.chat_id.clone(),
                    sender_id: message.sender_id.clone(),
                    stage: PendingPermissionWizardStage::ChooseMode,
                };
                set_pending_permission_wizard(&mut session, &wizard);
                self.publish_command_response(
                    message,
                    session,
                    permission_wizard_choices_text(),
                    Some(AgentLoopCommandResult::Permission),
                    "permission",
                    save_session,
                )
            }
            LoopCommand::Permission(PermissionCommandArgs::Recent) => {
                let content = format_recent_auto_mode_denials(
                    &session,
                    &self.recent_retry_tokens,
                    now_unix_ms(),
                );
                self.publish_command_response(
                    message,
                    session,
                    &content,
                    Some(AgentLoopCommandResult::Permission),
                    "permission_recent",
                    save_session,
                )
            }
            LoopCommand::Permission(PermissionCommandArgs::RecentRetry(denial_id)) => self
                .handle_permission_recent_retry_command(message, session, &denial_id, save_session),
            LoopCommand::Permission(PermissionCommandArgs::Invalid) => self
                .publish_command_response(
                message,
                session,
                "Usage: /permission, /permission recent, or /permission recent retry <denial_id>.",
                Some(AgentLoopCommandResult::Permission),
                "permission_usage",
                save_session,
            ),
            LoopCommand::Help => self.publish_command_response(
                message,
                session,
                &build_help_text(),
                Some(AgentLoopCommandResult::Help),
                "help",
                save_session,
            ),
        }
    }

    fn try_plugin_command(
        &mut self,
        message: &InboundMessage,
        session: &Session,
    ) -> Result<Option<AgentLoopTurnResult>, AgentLoopError> {
        let Some(dispatcher) = &self.plugin_command_dispatcher else {
            return Ok(None);
        };
        let Ok(execution) = dispatcher.dispatch_text(&message.content) else {
            return Ok(None);
        };
        let stop_reason = format!(
            "plugin_command:{}:{}",
            execution.plugin_id, execution.command_name
        );
        Ok(Some(self.publish_command_response(
            message,
            session.clone(),
            &execution.output.into_text(),
            Some(AgentLoopCommandResult::PluginCommand),
            &stop_reason,
            true,
        )?))
    }

    fn handle_pending_permission_wizard(
        &mut self,
        message: &InboundMessage,
        mut session: Session,
        session_key: &str,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let Some(mut wizard) = pending_permission_wizard(&session) else {
            return self.publish_command_response(
                message,
                session,
                permission_wizard_choices_text(),
                Some(AgentLoopCommandResult::Permission),
                "permission",
                true,
            );
        };
        if !permission_wizard_reply_matches_request(&wizard, message, session_key) {
            return self.publish_command_response(
                message,
                session,
                "Permission mode change pending. Only the original requester in the original channel can complete it.",
                Some(AgentLoopCommandResult::Permission),
                "permission_pending",
                true,
            );
        }

        let normalized = message.content.trim().to_ascii_lowercase();
        match (wizard.stage, normalized.as_str()) {
            (_, "cancel") => {
                session.metadata.remove(PENDING_PERMISSION_WIZARD_KEY);
                self.publish_command_response(
                    message,
                    session,
                    "Permission mode change cancelled.",
                    Some(AgentLoopCommandResult::Permission),
                    "permission_cancelled",
                    true,
                )
            }
            (PendingPermissionWizardStage::ChooseMode, "default") => {
                self.save_permission_mode_and_respond(message, session, PermissionMode::Default)
            }
            (PendingPermissionWizardStage::ChooseMode, "auto") => {
                self.save_permission_mode_and_respond(message, session, PermissionMode::Auto)
            }
            (PendingPermissionWizardStage::ChooseMode, "bypass_permissions") => {
                wizard.stage = PendingPermissionWizardStage::ConfirmBypass;
                set_pending_permission_wizard(&mut session, &wizard);
                self.publish_command_response(
                    message,
                    session,
                    "bypass_permissions disables permission prompts for this local config. Reply with exact `confirm bypass_permissions` to save it, or `cancel` to stop.",
                    Some(AgentLoopCommandResult::Permission),
                    "permission_confirm_bypass",
                    true,
                )
            }
            (PendingPermissionWizardStage::ConfirmBypass, "confirm bypass_permissions") => self
                .save_permission_mode_and_respond(
                    message,
                    session,
                    PermissionMode::BypassPermissions,
                ),
            (PendingPermissionWizardStage::ConfirmBypass, "bypass_permissions") => self
                .publish_command_response(
                    message,
                    session,
                    "Reply with exact `confirm bypass_permissions` to save bypass_permissions, or `cancel` to stop.",
                    Some(AgentLoopCommandResult::Permission),
                    "permission_confirm_bypass",
                    true,
                ),
            _ => self.publish_command_response(
                message,
                session,
                permission_wizard_choices_text(),
                Some(AgentLoopCommandResult::Permission),
                "permission_pending",
                true,
            ),
        }
    }

    fn save_permission_mode_and_respond(
        &mut self,
        message: &InboundMessage,
        mut session: Session,
        mode: PermissionMode,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let Some(setter) = self.config.permission_mode_setter.as_ref() else {
            return self.publish_command_response(
                message,
                session,
                "Permission mode saving is not available in this runtime.",
                Some(AgentLoopCommandResult::Permission),
                "permission_unavailable",
                true,
            );
        };
        let applied_snapshot = setter(mode).map_err(AgentLoopError::PermissionModeSave)?;
        self.config.permission_mode_snapshot = applied_snapshot.clone();
        session.metadata.remove(PENDING_PERMISSION_WIZARD_KEY);
        let content = if applied_snapshot.mode == mode {
            format!(
                "Permission mode `{}` saved and applied for subsequent turns.",
                mode.as_str()
            )
        } else {
            format!(
                "Permission mode `{}` saved. Active permission mode is `{}` for subsequent turns.",
                mode.as_str(),
                applied_snapshot.mode.as_str()
            )
        };
        self.publish_command_response(
            message,
            session,
            &content,
            Some(AgentLoopCommandResult::Permission),
            "permission_saved",
            true,
        )
    }

    fn handle_permission_recent_retry_command(
        &mut self,
        message: &InboundMessage,
        mut session: Session,
        denial_id: &str,
        save_session: bool,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        if !self.config.permission_interactive {
            return self.publish_command_response(
                message,
                session,
                "Recent retry requires an interactive permission channel. The denied action was not run.",
                Some(AgentLoopCommandResult::Permission),
                "permission_recent_retry_non_interactive",
                save_session,
            );
        }
        if pending_recent_retry_approval(&session).is_some() {
            return self.publish_command_response(
                message,
                session,
                "Recent retry approval is already pending. Reply to the existing approval with `approve` or `deny` before starting another recent retry.",
                Some(AgentLoopCommandResult::Permission),
                "permission_recent_retry_pending",
                save_session,
            );
        }
        if pending_permission_approval(&session).is_some() {
            return self.publish_command_response(
                message,
                session,
                "A permission approval is already pending. Reply to the existing approval with `approve`, `approve_session`, or `deny` before starting a recent retry.",
                Some(AgentLoopCommandResult::Permission),
                "permission_approval_pending",
                save_session,
            );
        }
        let Some(denial) = recent_auto_mode_denials(&session)
            .into_iter()
            .find(|candidate| candidate.denial_id == denial_id)
        else {
            return self.publish_command_response(
                message,
                session,
                "No matching recent denial was found. The denied action was not run; request the action again if still needed.",
                Some(AgentLoopCommandResult::Permission),
                "permission_recent_retry_missing_denial",
                save_session,
            );
        };
        if !denial.retryable {
            return self.publish_command_response(
                message,
                session,
                "Recent retry is unavailable for this classifier denial because it was not a high-confidence requested-scope denial. The denied action was not run; request the action again if still needed.",
                Some(AgentLoopCommandResult::Permission),
                "permission_recent_retry_unavailable",
                save_session,
            );
        }
        let token = match self.recent_retry_tokens.peek(denial_id, now_unix_ms()) {
            Ok(token) => token,
            Err(error) => {
                if !matches!(error, RecentAutoModeRetryTokenConsumeError::Missing) {
                    self.recent_retry_tokens.invalidate(denial_id);
                }
                return self.publish_command_response(
                    message,
                    session,
                    &recent_retry_closed_message(error),
                    Some(AgentLoopCommandResult::Permission),
                    "permission_recent_retry_closed",
                    save_session,
                );
            }
        };
        if token.action_digest() != denial.action_digest
            || token.argument_digest() != denial.argument_digest
            || token.snapshot_digest() != denial.snapshot_digest
        {
            self.recent_retry_tokens.invalidate(denial_id);
            return self.publish_command_response(
                message,
                session,
                "Recent retry token no longer matches the denial metadata. The denied action was not run; request the action again if still needed.",
                Some(AgentLoopCommandResult::Permission),
                "permission_recent_retry_closed",
                save_session,
            );
        }
        let approval_request_id = recent_retry_approval_request_id(&denial.denial_id);
        let pending = PendingRecentRetryApproval {
            denial_id: denial.denial_id.clone(),
            approval_request_id: approval_request_id.clone(),
            action_digest: denial.action_digest.clone(),
            argument_digest: denial.argument_digest.clone(),
            snapshot_digest: denial.snapshot_digest.clone(),
            tool_name: denial.tool_name.clone(),
            expires_at_unix_ms: token.expires_at_unix_ms(),
            requester_digest: recent_retry_requester_digest(message, &session.key),
            status: PendingPermissionApprovalStatus::Pending,
        };
        set_pending_recent_retry_approval(&mut session, &pending);
        let content = format!(
            "Recent retry approval required for denial `{}`. Reply with `1` or `approve` to run the exact denied `{}` action once, or `2`/`deny` to cancel. `approve_session` is not available for recent retry. Approval id: `{}`",
            denial.denial_id, denial.tool_name, approval_request_id
        );
        self.publish_command_response(
            message,
            session,
            &content,
            Some(AgentLoopCommandResult::Permission),
            "permission_recent_retry_pending",
            save_session,
        )
    }

    fn publish_command_response(
        &mut self,
        message: &InboundMessage,
        session: Session,
        content: &str,
        command: Option<AgentLoopCommandResult>,
        stop_reason: &str,
        save_session: bool,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let session_key = session.key.clone();
        if save_session {
            self.append_durable_fact(
                &session_key,
                Some(turn_id_for_message(message, &session)),
                SESSION_TURN_COMPLETED,
                json!({
                    "command": command.clone(),
                    "response_hash": format!("sha256:{:x}", Sha256::digest(content.as_bytes())),
                    "stop_reason": stop_reason,
                }),
                Some(durable_event_provenance(
                    &self.context_builder,
                    &RuntimeExecutionLedger::default(),
                )),
            )?;
            self.sessions.save(&session)?;
        }
        self.bus.publish_outbound(outbound_for(
            message,
            &session_key,
            content.to_owned(),
            Vec::new(),
            stop_reason,
        ));
        Ok(AgentLoopTurnResult {
            session_key,
            final_content: Some(content.to_owned()),
            stop_reason: stop_reason.to_owned(),
            tools_used: Vec::new(),
            outbound_count: 1,
            had_injections: false,
            command,
            ask_user_options: Vec::new(),
            message_tool_delivery_configured: self.context_tools.message.is_some(),
        })
    }

    fn publish_run_outbound(
        &self,
        message: &InboundMessage,
        session_key: &str,
        result: &crate::runtime::AgentRunResult,
    ) -> Result<(usize, Vec<String>), AgentLoopError> {
        let mut count = 0;
        let mut ask_options = Vec::new();
        let message_sent_in_turn = self
            .context_tools
            .message
            .as_ref()
            .is_some_and(MessageTool::sent_in_turn);
        if let Some(RuntimeInterrupt::AskUser {
            question, options, ..
        }) = &result.interrupt
        {
            let options = if options.is_empty() {
                ask_user_options_from_messages(&result.messages)
            } else {
                options.clone()
            };
            let (content, buttons) = ask_user_outbound(Some(question), &options, &message.channel);
            self.bus.publish_outbound(outbound_for(
                message,
                session_key,
                content.unwrap_or_else(|| question.clone()),
                buttons,
                "ask_user",
            ));
            count += 1;
            ask_options = options;
        } else if let Some(RuntimeInterrupt::PermissionApproval {
            question, options, ..
        }) = &result.interrupt
        {
            let (content, buttons) = ask_user_outbound(Some(question), options, &message.channel);
            self.bus.publish_outbound(outbound_for(
                message,
                session_key,
                content.unwrap_or_else(|| question.clone()),
                buttons,
                "permission_approval",
            ));
            count += 1;
            ask_options = options.clone();
        } else if !message_sent_in_turn {
            if let Some(content) = result
                .final_content
                .as_ref()
                .filter(|content| !content.is_empty())
            {
                self.bus.publish_outbound(outbound_for(
                    message,
                    session_key,
                    content.clone(),
                    Vec::new(),
                    &result.stop_reason,
                ));
                count += 1;
            }
        }
        Ok((count, ask_options))
    }

    fn publish_error(
        &self,
        message: &InboundMessage,
        session_key: &str,
        error: &ProviderError,
    ) -> usize {
        self.bus.publish_outbound(outbound_for(
            message,
            session_key,
            provider_error_text(error),
            Vec::new(),
            "error",
        ));
        1
    }

    fn execute_approved_permission_tool(
        &self,
        approval: &PendingPermissionApproval,
        approval_cache: ApprovalCacheEntry,
    ) -> RuntimeToolExecutionReport {
        self.execute_approved_permission_call(
            approval.tool_call.clone(),
            approval.tool_context.clone(),
            approval_cache,
        )
    }

    fn execute_approved_permission_payload(
        &self,
        token: &RecentAutoModeRetryToken,
        approval_cache: ApprovalCacheEntry,
    ) -> RuntimeToolExecutionReport {
        self.execute_approved_permission_call(
            token.tool_call().clone(),
            token.tool_context().clone(),
            approval_cache,
        )
    }

    fn execute_approved_permission_call(
        &self,
        tool_call: RuntimeToolCall,
        tool_context: ToolExecutionContext,
        approval_cache: ApprovalCacheEntry,
    ) -> RuntimeToolExecutionReport {
        let executor =
            RuntimeToolExecutor::with_context_tools(self.tools, self.context_tools.clone());
        let mut context = tool_context;
        context.permission_approval_cache = Some(approval_cache);
        context.permission_session_approval_cache = Vec::new();
        if bridge_tool_names().contains(&tool_call.name.as_str()) {
            let tool_surface = assemble_tool_surface(ToolSurfaceAssemblyInput {
                definitions: self.tools.definitions(),
                runtime: crate::runtime::ToolSearchRuntimeInput {
                    config: self.config.tool_search,
                    context_window_tokens: self.config.context_window_tokens,
                },
            });
            return dispatch_bridge_tool_calls(
                vec![tool_call],
                tool_surface.catalog.as_ref(),
                self.tools,
                &executor,
                &context,
                self.config.concurrent_tools,
            )
            .into_runtime_report();
        }
        executor.execute_tool_calls(vec![tool_call], &context)
    }

    fn append_approved_tool_messages(
        &self,
        message: &InboundMessage,
        session_key: &str,
        session: &mut Session,
        execution_ledger: &Arc<Mutex<RuntimeExecutionLedger>>,
        messages: Vec<RuntimeToolMessage>,
    ) -> Option<String> {
        let mut spec = AgentRunSpec::new(
            Vec::new(),
            self.tools,
            self.client,
            self.config.model.clone(),
        );
        spec.fail_on_tool_error = self.config.fail_on_tool_error;
        spec.max_tool_result_chars = self.config.max_tool_result_chars;
        spec.workspace = Some(self.config.workspace.clone());
        spec.session_key = Some(session_key.to_owned());
        spec.execution_scope = Some(ExecutionScope::new(
            session_key.to_owned(),
            turn_id_for_message(message, session),
        ));
        spec.execution_ledger = Some(execution_ledger.clone());
        spec.tool_event_callback = self.tool_event_callback.clone();
        let calls = messages
            .iter()
            .map(|message| {
                RuntimeToolCall::new(
                    message.tool_call_id.clone(),
                    message.name.clone(),
                    Value::Object(Map::new()),
                )
            })
            .collect::<Vec<_>>();
        crate::runtime::runner::begin_tool_executions(&spec, &calls);
        let mut fatal_error = None;
        for raw_message in messages {
            let event =
                crate::runtime::runner::tool_event_for_message(&raw_message, None, None, None);
            let (tool_message, fatal) =
                crate::runtime::runner::finalize_tool_message(&spec, raw_message);
            crate::runtime::runner::emit_events(&spec, std::slice::from_ref(&event));
            if fatal_error.is_none() && fatal {
                fatal_error = Some(tool_message.content.clone());
            }
            append_session_message(session, tool_message.to_json());
        }
        fatal_error
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentLoopRunSummary {
    pub processed: usize,
    pub outbound_count: usize,
    pub stopped: bool,
    pub results: Vec<AgentLoopTurnResult>,
}

#[derive(Debug)]
pub enum AgentLoopError {
    Session(std::io::Error),
    DurableEvent(DurableEventError),
    Memory(MemoryConsolidationError),
    GoalMetadata(GoalMetadataError),
    PermissionModeSave(String),
    Workflow(String),
    DuplicateActiveTurn { session_key: String },
}

impl fmt::Display for AgentLoopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "session persistence failed: {error}"),
            Self::DurableEvent(error) => write!(formatter, "durable event append failed: {error}"),
            Self::Memory(error) => write!(formatter, "memory consolidation failed: {error}"),
            Self::GoalMetadata(error) => write!(formatter, "goal metadata failed: {error}"),
            Self::PermissionModeSave(error) => {
                write!(formatter, "permission mode save failed: {error}")
            }
            Self::Workflow(error) => write!(formatter, "workflow execution failed: {error}"),
            Self::DuplicateActiveTurn { session_key } => {
                write!(
                    formatter,
                    "session already has an active turn: {session_key}"
                )
            }
        }
    }
}

impl Error for AgentLoopError {}

impl From<std::io::Error> for AgentLoopError {
    fn from(error: std::io::Error) -> Self {
        Self::Session(error)
    }
}

impl From<DurableEventError> for AgentLoopError {
    fn from(error: DurableEventError) -> Self {
        Self::DurableEvent(error)
    }
}

impl From<MemoryConsolidationError> for AgentLoopError {
    fn from(error: MemoryConsolidationError) -> Self {
        Self::Memory(error)
    }
}

impl From<SessionTurnAcquireError> for AgentLoopError {
    fn from(error: SessionTurnAcquireError) -> Self {
        match error {
            SessionTurnAcquireError::AlreadyActive { session_key } => {
                Self::DuplicateActiveTurn { session_key }
            }
        }
    }
}

struct MessageDeliveryTargetGuard {
    target: MessageDeliveryTarget,
}

impl MessageDeliveryTargetGuard {
    fn new(target: MessageDeliveryTarget, channel: String, chat_id: String) -> Self {
        *recover_lock(&target) = Some((channel, chat_id));
        Self { target }
    }
}

impl Drop for MessageDeliveryTargetGuard {
    fn drop(&mut self) {
        *recover_lock(&self.target) = None;
    }
}

struct BusMessageSender {
    bus: MessageBus,
    workspace: PathBuf,
    media_roots: Vec<PathBuf>,
    current_target: MessageDeliveryTarget,
}

impl MessageSender for BusMessageSender {
    fn send(&self, message: crate::tools::OutboundMessage) -> Result<(), String> {
        let allowed_target = recover_lock(&self.current_target).clone();
        if let Some((channel, chat_id)) = allowed_target {
            if message.channel != channel || message.chat_id != chat_id {
                return Err("cross-target message delivery is not allowed in AgentLoop".to_owned());
            }
        }
        let media = validate_outbound_media(&self.workspace, &self.media_roots, &message.media)?;
        let metadata = message.metadata.as_object().cloned().unwrap_or_default();
        self.bus.publish_outbound(OutboundMessage {
            channel: message.channel,
            chat_id: message.chat_id,
            content: message.content,
            reply_to: message.reply_to,
            media,
            metadata,
            buttons: message.buttons,
        });
        Ok(())
    }
}

fn validate_outbound_media(
    workspace: &Path,
    media_roots: &[PathBuf],
    media: &[String],
) -> Result<Vec<String>, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| format!("workspace could not be resolved: {error}"))?;
    let mut allowed_roots = vec![workspace.clone()];
    allowed_roots.extend(
        media_roots
            .iter()
            .filter_map(|root| root.canonicalize().ok()),
    );
    media
        .iter()
        .map(|item| {
            if item.starts_with("http://") || item.starts_with("https://") {
                return Err("remote media delivery is not allowed in AgentLoop".to_owned());
            }
            let candidate = PathBuf::from(item);
            let metadata = std::fs::symlink_metadata(&candidate)
                .map_err(|error| format!("media metadata could not be read: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("media path is not an allowed regular file".to_owned());
            }
            reject_symlink_media_parents(&candidate)?;
            let canonical = candidate
                .canonicalize()
                .map_err(|error| format!("media path could not be resolved: {error}"))?;
            if !allowed_roots.iter().any(|root| canonical.starts_with(root)) {
                return Err("media path escapes allowed media roots".to_owned());
            }
            Ok(canonical.to_string_lossy().into_owned())
        })
        .collect()
}

fn reject_symlink_media_parents(candidate: &Path) -> Result<(), String> {
    for parent in candidate
        .ancestors()
        .skip(1)
        .filter(|path| !path.as_os_str().is_empty())
    {
        let metadata = std::fs::symlink_metadata(parent)
            .map_err(|error| format!("media parent metadata could not be read: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("media path is not an allowed regular file".to_owned());
        }
    }
    Ok(())
}

fn append_user_turn(session: &mut Session, message: &InboundMessage) {
    let mut extra = Map::new();
    extra.insert(
        "timestamp".to_owned(),
        Value::String(message.timestamp.clone()),
    );
    if !message.media.is_empty() {
        extra.insert("media".to_owned(), json!(message.media));
    }
    if !message.metadata.is_empty() {
        extra.insert(
            "metadata".to_owned(),
            Value::Object(message.metadata.clone()),
        );
    }
    session.add_message("user", message.content.clone(), extra);
}

fn live_plan_missing_write_isolation(plan: &WorkflowHarnessPlan) -> bool {
    !matches!(
        plan.worktree_policy,
        WorkflowWorktreePolicy::IsolatedWorktreeRequired
    ) || plan.child_graph.iter().any(|child| {
        !matches!(
            child.worktree_policy,
            WorkflowWorktreePolicy::IsolatedWorktreeRequired
        )
    })
}

fn workflow_plan_is_live_read_only(plan: &WorkflowHarnessPlan) -> bool {
    matches!(
        plan.worktree_policy,
        WorkflowWorktreePolicy::None | WorkflowWorktreePolicy::ReadOnlySnapshot
    ) && plan.child_graph.iter().all(|child| {
        matches!(
            child.worktree_policy,
            WorkflowWorktreePolicy::None | WorkflowWorktreePolicy::ReadOnlySnapshot
        )
    }) && matches!(
        plan.tool_scope_policy.quarantine,
        WorkflowQuarantinePolicy::None | WorkflowQuarantinePolicy::ReadOnlyUntrusted
    ) && plan
        .tool_scope_policy
        .allowed_tools
        .iter()
        .all(|tool| !matches!(tool.as_str(), "write_file" | "edit_file" | "exec"))
}

fn workflow_plan_requests_isolated_worktree(plan: &WorkflowHarnessPlan) -> bool {
    matches!(
        plan.worktree_policy,
        WorkflowWorktreePolicy::IsolatedWorktreeRequired
            | WorkflowWorktreePolicy::IsolatedWorktreeOptional
    ) || plan.child_graph.iter().any(|child| {
        matches!(
            child.worktree_policy,
            WorkflowWorktreePolicy::IsolatedWorktreeRequired
                | WorkflowWorktreePolicy::IsolatedWorktreeOptional
        )
    })
}

fn pending_workflow_resume_block_reason(
    session: &Session,
    plan: &WorkflowHarnessPlan,
    recipe_evidence: Option<&WorkflowRecipeSelection>,
    workspace: &Path,
) -> Option<String> {
    let checkpoint = pending_workflow_checkpoint(session)?;
    let checkpoint_payload = pending_workflow_checkpoint_payload(session);
    let digest = match shacs_workflow::workflow_harness_plan_digest(plan) {
        Ok(digest) => digest,
        Err(error) => return Some(format!("plan digest failed: {error}")),
    };
    let mut completed_steps = checkpoint.completed_steps.clone();
    if let Some(completed_step_id) = checkpoint_payload
        .as_ref()
        .and_then(|payload| payload.completed_step_id.as_ref())
        .filter(|step_id| !completed_steps.contains(step_id))
    {
        completed_steps.push(completed_step_id.clone());
    }
    if let Some(missing_step) = completed_steps
        .iter()
        .find(|step_id| !plan.steps.iter().any(|step| step.step_id == **step_id))
    {
        return Some(format!(
            "checkpoint completed step `{missing_step}` is not present in the saved plan"
        ));
    }
    if !completed_steps.is_empty() {
        let Some(payload) = checkpoint_payload.as_ref() else {
            return Some("completed workflow steps lack checkpoint payload evidence".to_owned());
        };
        if let Some(missing_child) = plan
            .child_graph
            .iter()
            .filter(|child| completed_steps.contains(&child.step_id))
            .find(|child| !payload.completed_child_ids.contains(&child.child_id))
        {
            return Some(format!(
                "completed workflow step `{}` lacks child evidence for `{}`",
                missing_child.step_id, missing_child.child_id
            ));
        }
    }
    if let Some(reason) = recipe_evidence_resume_block_reason(recipe_evidence, workspace) {
        return Some(reason);
    }
    match shacs_workflow::workflow_resume_validation_decision(
        &shacs_workflow::WorkflowResumeValidationInput {
            checkpoint,
            resume_policy: plan.resume_policy.clone(),
            current_harness_plan_digest: digest,
            required_completed_steps: completed_steps,
            required_worktree_refs: checkpoint_payload
                .as_ref()
                .map(|payload| payload.worktree_refs.clone())
                .unwrap_or_default(),
            required_evidence_refs: required_workflow_resume_evidence_refs(
                recipe_evidence,
                checkpoint_payload.as_ref(),
            ),
        },
    ) {
        WorkflowResumeDecision::ResumeAllowed { .. } => None,
        WorkflowResumeDecision::AlreadyTerminal { state } => Some(format!(
            "workflow checkpoint is already terminal: {state:?}"
        )),
        WorkflowResumeDecision::Blocked { reason } => Some(reason),
    }
}

fn pending_workflow_checkpoint(session: &Session) -> Option<shacs_workflow::WorkflowCheckpoint> {
    session
        .metadata
        .get(RUNTIME_CHECKPOINT_KEY)
        .and_then(|value| value.get("workflow"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn pending_workflow_checkpoint_payload(
    session: &Session,
) -> Option<shacs_workflow::WorkflowRuntimeCheckpointPayload> {
    session
        .metadata
        .get(RUNTIME_CHECKPOINT_KEY)
        .and_then(|value| value.get("workflow_checkpoint_payload"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn required_workflow_resume_evidence_refs(
    recipe_evidence: Option<&WorkflowRecipeSelection>,
    checkpoint_payload: Option<&shacs_workflow::WorkflowRuntimeCheckpointPayload>,
) -> Vec<String> {
    let mut refs = checkpoint_payload
        .map(|payload| payload.evidence_refs.clone())
        .unwrap_or_default();
    if let Some(source_ref) =
        recipe_evidence.map(|evidence| evidence.selected.recipe.source_ref.clone())
    {
        refs.push(source_ref);
    }
    refs.sort();
    refs.dedup();
    refs
}

fn workflow_worktree_approval_granted(
    config: &AgentLoopConfig,
    plan: &WorkflowHarnessPlan,
) -> bool {
    if !workflow_plan_is_workspace_edit_only(plan) {
        return false;
    }
    match config.permission_mode_snapshot.mode {
        PermissionMode::AcceptEdits => true,
        PermissionMode::Auto => {
            config.permission_auto_approval.enabled
                && config.permission_auto_approval.allow_workspace_edits
        }
        PermissionMode::Plan
        | PermissionMode::Default
        | PermissionMode::DontAsk
        | PermissionMode::BypassPermissions => false,
    }
}

fn workflow_plan_is_workspace_edit_only(plan: &WorkflowHarnessPlan) -> bool {
    plan.tool_scope_policy
        .allowed_tools
        .iter()
        .all(|tool_name| {
            matches!(
                tool_name.as_str(),
                "read_file"
                    | "list_dir"
                    | "glob"
                    | "grep"
                    | "write_file"
                    | "edit_file"
                    | "notebook_read"
                    | "notebook_edit"
            )
        })
}

fn workflow_request_from_message(
    message: &InboundMessage,
    session_key: &str,
    workspace: &Path,
    permission_snapshot: &PermissionModeSnapshot,
) -> Result<
    Option<(
        WorkflowAdmissionInput,
        WorkflowHarnessPlan,
        Option<WorkflowRecipeSelection>,
    )>,
    AgentLoopError,
> {
    if let Some(request) = workflow_request_from_metadata(&message.metadata)? {
        return Ok(Some((request.0, request.1, None)));
    }
    let Some((admission, plan, recipe_selection)) = compile_automatic_workflow_request(
        &message.content,
        session_key,
        workspace,
        message
            .metadata
            .get("message_id")
            .and_then(Value::as_str)
            .unwrap_or("auto-turn"),
        permission_snapshot,
    ) else {
        return Ok(None);
    };
    if matches!(
        decide_workflow_admission(&admission),
        WorkflowAdmissionDecision::UseRegularLoop
    ) {
        return Ok(None);
    }
    Ok(Some((admission, plan, recipe_selection)))
}

fn compile_automatic_workflow_request(
    content: &str,
    session_key: &str,
    workspace: &Path,
    turn_id: &str,
    permission_snapshot: &PermissionModeSnapshot,
) -> Option<(
    WorkflowAdmissionInput,
    WorkflowHarnessPlan,
    Option<WorkflowRecipeSelection>,
)> {
    let profile = automatic_workflow_profile(content)?;
    let admission = WorkflowAdmissionInput {
        objective_complexity: profile.objective_complexity,
        estimated_item_count: profile.estimated_item_count,
        requires_parallelism: profile.requires_parallelism,
        requires_independent_verification: true,
        requires_adversarial_review: false,
        requires_large_context_partitioning: profile.requires_large_context_partitioning,
        requires_write_isolation: false,
        requires_recurring_loop: false,
        risk_level: profile.risk_level,
        user_requested_workflow: profile.user_requested_workflow,
        available_budget_tokens: Some(10_000),
        blocking_reasons: Vec::new(),
        missing_scope_questions: Vec::new(),
    };
    let mut plan =
        compile_read_only_workflow_plan(content, session_key, turn_id, permission_snapshot);
    let recipe_selection = apply_ready_workflow_recipe(content, workspace, &mut plan);
    Some((admission, plan, recipe_selection))
}

fn apply_ready_workflow_recipe(
    content: &str,
    workspace: &Path,
    plan: &mut WorkflowHarnessPlan,
) -> Option<WorkflowRecipeSelection> {
    let selection = discover_ready_workflow_recipe_selection(content, workspace)?;
    let selected = &selection.selected;
    plan.pattern = selected.recipe.pattern;
    for step in &mut plan.steps {
        step.pattern = selected.recipe.pattern;
    }
    if let Some(max_tokens) = selected.recipe.suggested_budget_tokens {
        plan.budget_policy.max_total_tokens = Some(max_tokens);
    }
    if let Some(scope_ref) = selected.recipe.suggested_tool_scope_ref.as_ref() {
        plan.tool_scope_policy.scope_digest = format!(
            "{}:recipe:{}",
            plan.tool_scope_policy.scope_digest, scope_ref
        );
    }
    if !selected.recipe.safety_notes.is_empty() {
        plan.constraints.extend(
            selected
                .recipe
                .safety_notes
                .iter()
                .map(|note| format!("Recipe safety note: {note}")),
        );
    }
    plan.constraints.push(format!(
        "Workflow recipe source evidence: {}",
        selected.recipe.source_ref
    ));
    Some(selection)
}

fn discover_ready_workflow_recipe_selection(
    content: &str,
    workspace: &Path,
) -> Option<WorkflowRecipeSelection> {
    let mut candidates = discover_workspace_workflow_recipes(workspace)
        .into_iter()
        .filter(|recipe| matches!(recipe.readiness, WorkflowRecipeReadiness::Ready))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        recipe_selection_rank(content, left)
            .cmp(&recipe_selection_rank(content, right))
            .then_with(|| left.recipe.recipe_id.cmp(&right.recipe.recipe_id))
            .then_with(|| left.skill_name.cmp(&right.skill_name))
            .then_with(|| left.recipe.source_ref.cmp(&right.recipe.source_ref))
    });
    let selected = candidates.first().cloned()?;
    Some(WorkflowRecipeSelection {
        selected,
        ready_candidates: candidates,
    })
}

fn discover_selected_workflow_recipe(
    workspace: &Path,
    selected: &SkillBackedWorkflowRecipe,
) -> Option<shacs_skills::SkillBackedWorkflowRecipe> {
    discover_workspace_workflow_recipes(workspace)
        .into_iter()
        .find(|recipe| {
            matches!(recipe.readiness, WorkflowRecipeReadiness::Ready)
                && recipe.recipe.recipe_id == selected.recipe.recipe_id
                && recipe.skill_name == selected.skill_name
                && recipe.recipe.source_ref == selected.recipe.source_ref
                && recipe.body_hash == selected.body_hash
        })
}

fn discover_workspace_workflow_recipes(
    workspace: &Path,
) -> Vec<shacs_skills::SkillBackedWorkflowRecipe> {
    discover_skill_registry(SkillRegistryOptions::new(workspace))
        .map(|registry| discover_workflow_recipes(&registry))
        .unwrap_or_default()
}

fn recipe_selection_rank(content: &str, evidence: &SkillBackedWorkflowRecipe) -> u8 {
    let normalized = content.to_ascii_lowercase();
    let recipe_id = evidence.recipe.recipe_id.to_ascii_lowercase();
    let skill_name = evidence.skill_name.to_ascii_lowercase();
    if normalized.contains(&recipe_id) || normalized.contains(&skill_name) {
        0
    } else {
        1
    }
}

fn recipe_evidence_resume_block_reason(
    evidence: Option<&WorkflowRecipeSelection>,
    workspace: &Path,
) -> Option<String> {
    let evidence = evidence?;
    let selected = &evidence.selected;
    if discover_selected_workflow_recipe(workspace, selected).is_some() {
        None
    } else {
        Some(format!(
            "workflow recipe evidence is no longer valid for `{}` from `{}`",
            selected.recipe.recipe_id, selected.recipe.source_ref
        ))
    }
}

#[derive(Debug, Clone, Copy)]
struct AutomaticWorkflowProfile {
    objective_complexity: u8,
    estimated_item_count: usize,
    requires_parallelism: bool,
    requires_large_context_partitioning: bool,
    risk_level: u8,
    user_requested_workflow: bool,
}

fn automatic_workflow_profile(content: &str) -> Option<AutomaticWorkflowProfile> {
    let normalized = content.trim();
    if normalized.is_empty() || normalized.len() < 80 || looks_like_simple_turn(normalized) {
        return None;
    }
    let lower = normalized.to_ascii_lowercase();
    let user_requested_workflow = ["workflow", "plan", "parallel", "verify", "review", "audit"]
        .iter()
        .any(|needle| lower.contains(needle));
    let estimated_item_count = estimated_workflow_item_count(normalized);
    let has_clear_complex_action = [
        "implement",
        "refactor",
        "fix",
        "migrate",
        "analyze",
        "audit",
        "review",
        "verify",
        "investigate",
        "compare",
        "summarize",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let requires_parallelism = estimated_item_count >= 4 || lower.contains("parallel");
    let requires_large_context_partitioning = normalized.len() > 1_200 || estimated_item_count >= 8;
    let objective_complexity = if requires_large_context_partitioning || estimated_item_count >= 8 {
        8
    } else if has_clear_complex_action && estimated_item_count >= 3 {
        6
    } else {
        4
    };
    if !(user_requested_workflow || requires_parallelism || objective_complexity >= 5) {
        return None;
    }
    Some(AutomaticWorkflowProfile {
        objective_complexity,
        estimated_item_count,
        requires_parallelism,
        requires_large_context_partitioning,
        risk_level: 3,
        user_requested_workflow,
    })
}

fn looks_like_simple_turn(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    content.split_whitespace().count() <= 16
        && !content.contains('\n')
        && ["hello", "hi", "thanks", "status", "help"]
            .iter()
            .any(|needle| lower.contains(needle))
}

fn estimated_workflow_item_count(content: &str) -> usize {
    let listed = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit())
                    && trimmed.contains('.')
        })
        .count();
    listed.max(content.matches(" and ").count().saturating_add(1))
}

fn compile_read_only_workflow_plan(
    content: &str,
    session_key: &str,
    turn_id: &str,
    permission_snapshot: &PermissionModeSnapshot,
) -> WorkflowHarnessPlan {
    let digest = digest_json(&json!({
        "session_key": session_key,
        "turn_id": turn_id,
        "content": content,
    }));
    let short_digest = digest.chars().take(12).collect::<String>();
    let workflow_id = format!("auto_workflow_{short_digest}");
    WorkflowHarnessPlan {
        workflow_id: workflow_id.clone(),
        origin_session_id: session_key.to_owned(),
        origin_turn_id: turn_id.to_owned(),
        objective: content.trim().to_owned(),
        constraints: vec!["Use read-only tools only; do not modify files or merge changes.".to_owned()],
        pattern: WorkflowPattern::FanOutAndSynthesize,
        steps: vec![WorkflowStep {
            step_id: "read-only-analysis".to_owned(),
            label: "Read-only analysis".to_owned(),
            pattern: WorkflowPattern::ClassifyAndAct,
            depends_on: Vec::new(),
            required: true,
            expected_output_schema: None,
        }],
        child_graph: vec![WorkflowChildSpec {
            child_id: "child-1".to_owned(),
            step_id: "read-only-analysis".to_owned(),
            goal: content.trim().to_owned(),
            tool_scope_ref: Some("auto-read-only".to_owned()),
            worktree_policy: WorkflowWorktreePolicy::ReadOnlySnapshot,
            budget: WorkflowBudgetSlice {
                max_tokens: Some(2_000),
                max_wall_clock_ms: Some(60_000),
            },
            verifier_required: true,
        }],
        verifier_graph: vec![WorkflowVerifierSpec {
            verifier_id: "verifier-1".to_owned(),
            target_child_id: "child-1".to_owned(),
            rubric: "The result addresses the user request, cites uncertainty, and does not rely on write-side effects.".to_owned(),
            independent_evidence_required: false,
        }],
        context_policy: WorkflowContextPolicy {
            root_objective_snapshot: content.trim().to_owned(),
            include_constraints_in_children: true,
            untrusted_input_labels: vec!["user_turn".to_owned()],
        },
        tool_scope_policy: WorkflowToolScopePolicy {
            scope_digest: format!("auto-read-only:{short_digest}"),
            allowed_tools: vec!["read_file".to_owned(), "list_dir".to_owned(), "glob".to_owned(), "grep".to_owned()],
            deferred_tool_search_allowed: true,
            quarantine: WorkflowQuarantinePolicy::ReadOnlyUntrusted,
        },
        permission_policy: WorkflowPermissionPolicy {
            permission_snapshot_ref: workflow_permission_snapshot_ref(permission_snapshot),
            denied_capabilities: vec![
                "fs_write".to_owned(),
                "proc_exec".to_owned(),
                "runtime_config_write".to_owned(),
                "external_delivery".to_owned(),
            ],
            approval_required_for_privileged_steps: true,
        },
        worktree_policy: WorkflowWorktreePolicy::ReadOnlySnapshot,
        model_routing_policy: WorkflowModelRoutingPolicy {
            classifier_model_hint: None,
            child_model_hint: None,
            verifier_model_hint: None,
            synthesis_model_hint: None,
            fallback_model_policy: "use runtime provider default".to_owned(),
        },
        budget_policy: WorkflowBudgetPolicy {
            max_total_tokens: Some(10_000),
            max_child_tokens: Some(2_000),
            max_verifier_tokens: Some(2_000),
            max_iterations: 4,
            max_parallel_children: 2,
            max_wall_clock_ms: Some(120_000),
            max_heavy_commands: Some(0),
        },
        checkpoint_policy: WorkflowCheckpointPolicy {
            checkpoint_required: true,
            checkpoint_before_privileged_steps: true,
        },
        merge_policy: WorkflowMergePolicy {
            require_verifier_pass: true,
            allow_partial_completion: false,
            surface_disagreements: true,
        },
        stop_condition: WorkflowStopCondition {
            description: "read-only workflow result synthesized".to_owned(),
            no_new_findings_threshold: None,
        },
        resume_policy: WorkflowResumePolicy {
            require_plan_digest_match: true,
            allow_completed_resume: false,
        },
    }
}

fn workflow_permission_snapshot_ref(snapshot: &PermissionModeSnapshot) -> String {
    digest_json(&json!({
        "mode": snapshot.mode.as_str(),
        "source": snapshot.source,
        "scope_ref": snapshot.scope_ref,
    }))
}

fn workflow_request_from_metadata(
    metadata: &Map<String, Value>,
) -> Result<Option<(WorkflowAdmissionInput, WorkflowHarnessPlan)>, AgentLoopError> {
    let admission_value = metadata
        .get("workflow_admission")
        .or_else(|| metadata.get("workflow_admission_input"));
    let plan_value = metadata
        .get("workflow_plan")
        .or_else(|| metadata.get("workflow_harness_plan"));
    let (Some(admission_value), Some(plan_value)) = (admission_value, plan_value) else {
        if admission_value.is_some() || plan_value.is_some() {
            return Err(AgentLoopError::Workflow(
                "workflow metadata must include both admission and plan".to_owned(),
            ));
        }
        return Ok(None);
    };
    let admission = serde_json::from_value(admission_value.clone()).map_err(|error| {
        AgentLoopError::Workflow(format!("invalid workflow admission: {error}"))
    })?;
    let plan = serde_json::from_value(plan_value.clone())
        .map_err(|error| AgentLoopError::Workflow(format!("invalid workflow plan: {error}")))?;
    Ok(Some((admission, plan)))
}

fn format_workflow_turn_content(outcome: &crate::runtime::RuntimeWorkflowOutcome) -> String {
    let status = match outcome.run.state {
        WorkflowRunState::Completed => "completed",
        WorkflowRunState::Cancelled => "cancelled",
        WorkflowRunState::Blocked => "blocked",
        _ => "failed",
    };
    let mut lines = vec![format!("Workflow {status}: {}", outcome.run.workflow_id)];
    lines.push(format!(
        "Children: {} accepted, {} rejected, {} unresolved.",
        outcome.synthesis_outcome.accepted_child_ids.len(),
        outcome.synthesis_outcome.rejected_child_ids.len(),
        outcome.synthesis_outcome.unresolved_child_ids.len()
    ));
    for result in &outcome.child_results {
        lines.push(format!("- {}: {}", result.child_id, result.summary));
    }
    lines.push(format!("Verification: {:?}", outcome.verification_gate));
    lines.join("\n")
}

fn workflow_stop_reason(state: WorkflowRunState) -> &'static str {
    match state {
        WorkflowRunState::Completed => "workflow_completed",
        WorkflowRunState::Cancelled => "workflow_cancelled",
        WorkflowRunState::Blocked => "workflow_blocked",
        _ => "workflow_failed",
    }
}

fn build_live_context_provider_handoff(
    workspace: &Path,
    current_message: &str,
    initial_messages: &[Value],
    current_dir: Option<PathBuf>,
    max_context_bytes: Option<usize>,
) -> ContextProviderHandoff {
    let parsed = parse_context_references(current_message);
    let resolver_config = ContextReferenceResolverConfig::new(workspace);
    let resolved = parsed
        .references
        .iter()
        .map(|reference| resolve_context_reference(reference, &resolver_config))
        .collect::<Vec<_>>();
    let safety = apply_context_safety_gate(&resolved);
    let context_files = discover_context_files(
        workspace,
        ContextFileDiscoveryOptions {
            current_dir,
            ..ContextFileDiscoveryOptions::default()
        },
    );
    let context_file_entries = live_provider_context_files(workspace, context_files.entries);
    build_context_provider_handoff(
        &safety.artifacts,
        &context_file_entries,
        ContextBudgetInput {
            reserved_user_message_bytes: current_message.len(),
            reserved_runtime_instruction_bytes: runtime_instruction_bytes(initial_messages),
            max_context_bytes,
        },
    )
}

fn live_provider_context_files(
    _workspace: &Path,
    entries: Vec<ContextFileProjection>,
) -> Vec<ContextFileProjection> {
    entries
        .into_iter()
        .filter(|entry| !is_workspace_root_bootstrap_context_file(entry))
        .collect()
}

fn is_workspace_root_bootstrap_context_file(entry: &ContextFileProjection) -> bool {
    entry.source == crate::runtime::ContextFileSource::DefaultCandidate
        && entry.source_directory_depth == 0
        && SYSTEM_BOOTSTRAP_CONTEXT_FILE_NAMES.contains(&entry.filename.as_str())
}

fn current_working_directory() -> Option<PathBuf> {
    std::env::current_dir().ok()
}

fn live_context_budget_bytes(context_block_limit_tokens: Option<usize>) -> Option<usize> {
    context_block_limit_tokens.map(|tokens| tokens.saturating_mul(4))
}

fn runtime_instruction_bytes(messages: &[Value]) -> usize {
    messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .map(str::len)
        .sum()
}

fn mid_turn_injection_callback(
    bus: MessageBus,
    session_key: String,
    unified_session_key: Option<String>,
    context_builder: ContextBuilder,
) -> Arc<dyn Fn() -> Vec<Value> + Send + Sync> {
    Arc::new(move || {
        bus.drain_inbound_matching(MAX_INJECTIONS_PER_TURN, |message| {
            effective_message_session_key(message, unified_session_key.as_deref()) == session_key
                && !is_builtin_command(&message.content)
        })
        .into_iter()
        .map(|message| inbound_to_injected_user_message(&context_builder, &message))
        .collect()
    })
}

fn effective_message_session_key(
    message: &InboundMessage,
    unified_session_key: Option<&str>,
) -> String {
    if message.session_key_override.is_some() {
        message.session_key()
    } else {
        unified_session_key
            .map(str::to_owned)
            .unwrap_or_else(|| message.session_key())
    }
}

fn inbound_to_injected_user_message(builder: &ContextBuilder, message: &InboundMessage) -> Value {
    builder
        .build_messages(ContextBuildRequest {
            history: Vec::new(),
            current_message: &message.content,
            media: &message.media,
            channel: Some(&message.channel),
            chat_id: Some(&message.chat_id),
            current_role: "user",
            session_summary: None,
        })
        .into_iter()
        .find(|candidate| candidate.get("role").and_then(Value::as_str) == Some("user"))
        .unwrap_or_else(|| json!({"role": "user", "content": message.content}))
}

fn append_ask_user_resume(session: &mut Session, tool_call_id: &str, content: &str) {
    append_session_message(
        session,
        json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "name": "ask_user",
            "content": content,
            "timestamp": now_iso(),
        }),
    );
}

fn append_new_runner_messages(
    session: &mut Session,
    initial_messages: &[Value],
    returned: &[Value],
) {
    for message in returned.iter().skip(initial_messages.len()) {
        append_session_message(session, message.clone());
    }
}

fn turn_id_for_message(message: &InboundMessage, session: &Session) -> String {
    message
        .metadata
        .get("message_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message_id| !message_id.is_empty())
        .map(|message_id| format!("turn:{message_id}"))
        .unwrap_or_else(|| format!("turn:{}:{}", session.key, session.messages.len()))
}

fn durable_event_provenance(
    context_builder: &ContextBuilder,
    ledger: &RuntimeExecutionLedger,
) -> DurableEventProvenance {
    let skill_body_hashes = context_builder.skill_body_hashes_for_context();
    let skill_registry_hash = serde_json::to_vec(&skill_body_hashes)
        .ok()
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)));
    DurableEventProvenance {
        skill_registry_hash,
        skill_body_hashes,
        execution_identity: durable_execution_identity(ledger),
    }
}

fn durable_event_provenance_from_snapshot(
    snapshot: &DurableEventProvenance,
    ledger: &RuntimeExecutionLedger,
) -> DurableEventProvenance {
    let mut provenance = snapshot.clone();
    provenance.execution_identity = durable_execution_identity(ledger);
    provenance
}

fn durable_execution_identity(
    ledger: &RuntimeExecutionLedger,
) -> Option<DurableExecutionIdentityRef> {
    ledger
        .outcomes
        .iter()
        .rev()
        .find(|outcome| matches!(outcome.decision, LateResultDecision::Accepted))
        .map(|outcome| DurableExecutionIdentityRef {
            session_id: outcome.fact.identity.scope.session_id.clone(),
            turn_id: outcome.fact.identity.scope.turn_id.clone(),
            effect_id: outcome.fact.identity.effect_id.clone(),
            attempt_id: outcome.fact.identity.attempt_id.clone(),
            correlation_id: outcome.fact.identity.correlation_id.clone(),
        })
}

fn store_runtime_execution(session: &mut Session, ledger: &RuntimeExecutionLedger) {
    if let Ok(value) = serde_json::to_value(ledger) {
        session
            .metadata
            .insert(RUNTIME_EXECUTION_KEY.to_owned(), value);
    }
}

fn execution_ledger_for_turn(
    message: &InboundMessage,
    session: &Session,
) -> RuntimeExecutionLedger {
    let mut ledger: RuntimeExecutionLedger = session
        .metadata
        .get(RUNTIME_EXECUTION_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    ledger
        .pending
        .retain(|pending| pending.domain == ExecutionDomain::Subagent);
    ledger.outcomes.clear();
    if message.channel != "system"
        || message.sender_id != "subagent"
        || message
            .metadata
            .get("injected_event")
            .and_then(Value::as_str)
            != Some("subagent_result")
    {
        return ledger;
    }
    if let Some(fact) = message
        .metadata
        .get("execution_fact")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .filter(|fact: &ExecutionOutcomeFact| {
            fact.identity.scope.session_id == session.key
                && matches!(fact.outcome, ExecutionOutcome::Subagent(_))
        })
    {
        ledger
            .pending
            .retain(|pending| pending.identity.effect_id != fact.identity.effect_id);
        ledger.record(fact);
    }
    ledger
}

fn workflow_execution_ledger(
    plan: &WorkflowHarnessPlan,
    child_results: &[WorkflowChildResult],
    finished_at_ms: u64,
) -> RuntimeExecutionLedger {
    let mut ledger = RuntimeExecutionLedger::default();
    for child in child_results {
        let identity = ExecutionIdentity::new(
            ExecutionScope::new(plan.origin_session_id.clone(), plan.origin_turn_id.clone()),
            format!("spawn:{}", child.child_id),
            child.child_id.clone(),
        )
        .with_idempotency_key(format!(
            "workflow-child:{}:{}",
            plan.workflow_id, child.child_id
        ));
        if !child.status.is_terminal() {
            ledger.begin(crate::runtime::PendingExecution {
                identity,
                domain: ExecutionDomain::Subagent,
                started_at_ms: finished_at_ms.into(),
            });
            continue;
        }
        let outcome = match child.status {
            WorkflowChildRunStatus::Completed => SubagentOutcomeKind::Completed,
            WorkflowChildRunStatus::Failed => SubagentOutcomeKind::Failed,
            WorkflowChildRunStatus::Cancelled => SubagentOutcomeKind::Cancelled,
            WorkflowChildRunStatus::TimedOut => SubagentOutcomeKind::TimedOut,
            WorkflowChildRunStatus::Stale => SubagentOutcomeKind::Stale,
            WorkflowChildRunStatus::Pending | WorkflowChildRunStatus::Running => continue,
        };
        let fact = ExecutionOutcomeFact::new(
            identity,
            ExecutionOutcome::Subagent(outcome),
            finished_at_ms.into(),
        );
        let decision = if child.status == WorkflowChildRunStatus::Stale {
            LateResultDecision::DiscardedStale {
                reason: "workflow child result was stale".to_owned(),
            }
        } else {
            LateResultDecision::Accepted
        };
        ledger.record_with_decision(fact, decision);
    }
    ledger
}

fn clear_runtime_markers(session: &mut Session) {
    session.metadata.remove(PENDING_USER_TURN_KEY);
    session.metadata.remove(RUNTIME_CHECKPOINT_KEY);
    session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
    session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
    session.metadata.remove(PENDING_PERMISSION_WIZARD_KEY);
    session.metadata.remove(PENDING_WORKFLOW_KEY);
}

fn permission_wizard_choices_text() -> &'static str {
    "Choose permissions.mode: `default`, `auto`, `bypass_permissions`, or `cancel`. Use `/permission recent` to inspect recent auto-mode classifier denials."
}

fn store_recent_auto_mode_denials(session: &mut Session, denials: Vec<RecentAutoModeDenial>) {
    if denials.is_empty() {
        return;
    }
    let mut store = RecentAutoModeDenialStore::from_denials(recent_auto_mode_denials(session));
    let mut denials = denials.into_iter().enumerate().collect::<Vec<_>>();
    denials.sort_by(|(left_index, left), (right_index, right)| {
        right
            .created_at_unix_ms
            .cmp(&left.created_at_unix_ms)
            .then_with(|| right_index.cmp(left_index))
    });
    let denials = denials
        .into_iter()
        .map(|(_, denial)| denial)
        .collect::<Vec<_>>();
    store.extend_newest_first(denials);
    if let Ok(value) = serde_json::to_value(store.into_vec()) {
        session
            .metadata
            .insert(RECENT_AUTO_MODE_DENIALS_KEY.to_owned(), value);
    }
}

fn recent_auto_mode_denials(session: &Session) -> Vec<RecentAutoModeDenial> {
    session
        .metadata
        .get(RECENT_AUTO_MODE_DENIALS_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<RecentAutoModeDenial>>(value).ok())
        .unwrap_or_default()
}

fn format_recent_auto_mode_denials(
    session: &Session,
    retry_tokens: &RecentAutoModeRetryTokenStore,
    now_unix_ms: u64,
) -> String {
    let denials = recent_auto_mode_denials(session);
    if denials.is_empty() {
        return "No recent auto-mode classifier denials.".to_owned();
    }
    let mut lines = vec!["Recent auto-mode classifier denials:".to_owned()];
    for denial in denials.iter().take(20) {
        let action_digest = denial.action_digest.chars().take(12).collect::<String>();
        let snapshot_digest = denial.snapshot_digest.chars().take(12).collect::<String>();
        let capabilities = denial
            .capabilities
            .iter()
            .map(|capability| format!("{capability:?}"))
            .collect::<Vec<_>>()
            .join(",");
        lines.push(format!(
            "- id={} tool={} capabilities=[{}] verdict={:?} confidence={:?} scope={:?} action={} snapshot={} retry_state={}",
            denial.denial_id,
            denial.tool_name,
            capabilities,
            denial.classifier_verdict,
            denial.classifier_confidence,
            denial.classifier_scope_match,
            action_digest,
            snapshot_digest,
            if retry_tokens.is_available(&denial.denial_id, now_unix_ms) {
                "available"
            } else {
                "unavailable"
            },
        ));
    }
    lines.join("\n")
}

fn set_pending_permission_wizard(session: &mut Session, wizard: &PendingPermissionWizard) {
    if let Ok(value) = serde_json::to_value(wizard) {
        session
            .metadata
            .insert(PENDING_PERMISSION_WIZARD_KEY.to_owned(), value);
    }
}

fn pending_permission_wizard(session: &Session) -> Option<PendingPermissionWizard> {
    session
        .metadata
        .get(PENDING_PERMISSION_WIZARD_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn permission_wizard_reply_matches_request(
    wizard: &PendingPermissionWizard,
    message: &InboundMessage,
    session_key: &str,
) -> bool {
    wizard.session_key == session_key
        && wizard.channel == message.channel
        && wizard.chat_id == message.chat_id
        && wizard.sender_id == message.sender_id
}

fn store_pending_permission_approval(
    session: &mut Session,
    interrupt: &Option<RuntimeInterrupt>,
    tool_context: &ToolExecutionContext,
    message: &InboundMessage,
    session_key: &str,
) {
    let Some(RuntimeInterrupt::PermissionApproval {
        approval_request_id,
        approval_request,
        tool_call,
        ..
    }) = interrupt
    else {
        return;
    };
    if let Ok(value) = serde_json::to_value(PendingPermissionApproval {
        approval_request_id: approval_request_id.clone(),
        approval_request: approval_request.as_ref().clone(),
        tool_call: tool_call.clone(),
        tool_context: tool_context.clone(),
        session_key: session_key.to_owned(),
        channel: message.channel.clone(),
        chat_id: message.chat_id.clone(),
        sender_id: message.sender_id.clone(),
        status: PendingPermissionApprovalStatus::Pending,
    }) {
        set_pending_permission_approval_value(session, value);
    }
}

fn set_pending_permission_approval(session: &mut Session, approval: &PendingPermissionApproval) {
    if let Ok(value) = serde_json::to_value(approval) {
        set_pending_permission_approval_value(session, value);
    }
}

fn set_pending_permission_approval_value(session: &mut Session, value: Value) {
    session
        .metadata
        .insert(PENDING_PERMISSION_APPROVAL_KEY.to_owned(), value);
}

fn pending_permission_approval(session: &Session) -> Option<PendingPermissionApproval> {
    session
        .metadata
        .get(PENDING_PERMISSION_APPROVAL_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn set_pending_recent_retry_approval(session: &mut Session, approval: &PendingRecentRetryApproval) {
    if let Ok(value) = serde_json::to_value(approval) {
        session
            .metadata
            .insert(PENDING_RECENT_RETRY_APPROVAL_KEY.to_owned(), value);
    }
}

fn pending_recent_retry_approval(session: &Session) -> Option<PendingRecentRetryApproval> {
    session
        .metadata
        .get(PENDING_RECENT_RETRY_APPROVAL_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn store_pending_workflow(
    session: &mut Session,
    session_key: &str,
    admission: &WorkflowAdmissionInput,
    plan: &WorkflowHarnessPlan,
    recipe_evidence: Option<WorkflowRecipeSelection>,
) -> Result<(), AgentLoopError> {
    let pending = PendingWorkflowTurn {
        session_key: session_key.to_owned(),
        admission: admission.clone(),
        plan: plan.clone(),
        recipe_evidence,
    };
    let value = serde_json::to_value(pending).map_err(|error| {
        AgentLoopError::Workflow(format!("workflow pending state failed: {error}"))
    })?;
    session
        .metadata
        .insert(PENDING_WORKFLOW_KEY.to_owned(), value);
    Ok(())
}

fn pending_workflow(session: &Session) -> Option<PendingWorkflowTurn> {
    session
        .metadata
        .get(PENDING_WORKFLOW_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn store_planned_workflow_checkpoint(
    session: &mut Session,
    plan: &WorkflowHarnessPlan,
    recipe_evidence: Option<&WorkflowRecipeSelection>,
) {
    let recorded_at_ms = now_unix_ms();
    let evidence_refs = recipe_evidence
        .map(|evidence| vec![evidence.selected.recipe.source_ref.clone()])
        .unwrap_or_default();
    let checkpoint = build_workflow_checkpoint(
        plan,
        &shacs_workflow::admit_workflow_plan(plan, recorded_at_ms).unwrap_or_else(|_| {
            shacs_workflow::WorkflowRunRecord {
                workflow_id: plan.workflow_id.clone(),
                origin_session_id: plan.origin_session_id.clone(),
                origin_turn_id: plan.origin_turn_id.clone(),
                state: WorkflowRunState::Planned,
                harness_plan_digest: digest_json(&serde_json::to_value(plan).unwrap_or_default()),
                admitted_at_ms: recorded_at_ms,
                updated_at_ms: recorded_at_ms,
                checkpoint: None,
            }
        }),
        WorkflowCheckpointInput {
            state: WorkflowRunState::Planned,
            completed_steps: Vec::new(),
            active_children: Vec::new(),
            pending_barriers: plan.steps.iter().map(|step| step.step_id.clone()).collect(),
            budget_usage: WorkflowBudgetUsage {
                known_tokens: 0,
                estimated_tokens: 0,
                child_runs: 0,
                verifier_runs: 0,
                heavy_commands: 0,
            },
            worktree_refs: Vec::new(),
            evidence_refs,
            last_safe_resume_point: "workflow_planned".to_owned(),
            recorded_at_ms,
        },
    );
    session.metadata.insert(
        RUNTIME_CHECKPOINT_KEY.to_owned(),
        json!({
            "phase": "workflow_planned",
            "workflow": checkpoint,
            "plan": plan,
            "recipe_evidence": recipe_evidence,
        }),
    );
}

fn apply_completed_workflow_checkpoint(plan: &mut WorkflowHarnessPlan, session: &Session) -> bool {
    let Some(checkpoint) = pending_workflow_checkpoint(session) else {
        return false;
    };
    if checkpoint.completed_steps.is_empty() {
        return false;
    }
    plan.child_graph
        .retain(|child| !checkpoint.completed_steps.contains(&child.step_id));
    plan.steps
        .retain(|step| !checkpoint.completed_steps.contains(&step.step_id));
    for step in &mut plan.steps {
        step.depends_on
            .retain(|step_id| !checkpoint.completed_steps.contains(step_id));
    }
    plan.verifier_graph.retain(|verifier| {
        plan.child_graph
            .iter()
            .any(|child| child.child_id == verifier.target_child_id)
    });
    plan.child_graph.is_empty() || plan.steps.is_empty()
}

fn completed_workflow_step_ids(
    plan: &WorkflowHarnessPlan,
    child_results: &[shacs_workflow::WorkflowChildResult],
) -> Vec<String> {
    plan.steps
        .iter()
        .filter(|step| {
            let step_children = plan
                .child_graph
                .iter()
                .filter(|child| child.step_id == step.step_id)
                .collect::<Vec<_>>();
            !step_children.is_empty()
                && step_children.iter().all(|child| {
                    child_results.iter().any(|result| {
                        result.child_id == child.child_id
                            && result.status == WorkflowChildRunStatus::Completed
                    })
                })
        })
        .map(|step| step.step_id.clone())
        .collect()
}

fn session_permission_approvals(session: &Session) -> Vec<SessionApprovalCacheEntry> {
    session
        .metadata
        .get(SESSION_PERMISSION_APPROVALS_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn store_session_permission_approval(
    session: &mut Session,
    session_key: &str,
    approval: ApprovalCacheEntry,
    action: &PermissionedAction,
) {
    let now = now_unix_ms();
    let approval_context_digest = session_approval_context_digest(action);
    let reuse_match = session_approval_reuse_match(action);
    let mut approvals = session_permission_approvals(session)
        .into_iter()
        .filter(|entry| entry.approval.request.expires_at_unix_ms >= now)
        .filter(|entry| {
            !(entry.session_key == session_key
                && same_session_approval_grant(
                    &entry.reuse_match,
                    &entry.approval.request.action_digest,
                    &reuse_match,
                    &action.action_digest,
                )
                && entry.approval.request.requested_scope == action.session_id
                && entry.approval_context_digest == approval_context_digest)
        })
        .collect::<Vec<_>>();
    approvals.push(SessionApprovalCacheEntry {
        session_key: session_key.to_owned(),
        approval_context_digest,
        reuse_match,
        approval,
    });
    if approvals.len() > SESSION_PERMISSION_APPROVAL_LIMIT {
        approvals.drain(0..approvals.len() - SESSION_PERMISSION_APPROVAL_LIMIT);
    }
    if let Ok(value) = serde_json::to_value(approvals) {
        session
            .metadata
            .insert(SESSION_PERMISSION_APPROVALS_KEY.to_owned(), value);
    }
}

fn permission_approval_reply_matches_request(
    approval: &PendingPermissionApproval,
    message: &InboundMessage,
    session_key: &str,
) -> bool {
    approval.session_key == session_key
        && approval.channel == message.channel
        && approval.chat_id == message.chat_id
        && approval.sender_id == message.sender_id
}

fn recent_retry_approval_reply_matches_request(
    approval: &PendingRecentRetryApproval,
    message: &InboundMessage,
    session_key: &str,
) -> bool {
    approval.requester_digest == recent_retry_requester_digest(message, session_key)
}

fn approval_decision(
    approval: &PendingPermissionApproval,
    decision: ApprovalDecisionKind,
) -> ApprovalDecision {
    ApprovalDecision {
        approval_request_id: approval.approval_request.approval_request_id.clone(),
        action_digest: approval.approval_request.action_digest.clone(),
        snapshot_digest: approval.approval_request.snapshot_digest.clone(),
        decision,
        approved_scope: approval.approval_request.requested_scope.clone(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: now_unix_ms(),
        consumed: false,
    }
}

fn recent_retry_approval_decision(
    approval_request: &ApprovalRequest,
    decision: ApprovalDecisionKind,
) -> ApprovalDecision {
    ApprovalDecision {
        approval_request_id: approval_request.approval_request_id.clone(),
        action_digest: approval_request.action_digest.clone(),
        snapshot_digest: approval_request.snapshot_digest.clone(),
        decision,
        approved_scope: approval_request.requested_scope.clone(),
        actor: ApprovalActor::LocalUser,
        decided_at_unix_ms: now_unix_ms(),
        consumed: false,
    }
}

fn recent_retry_approval_request_id(denial_id: &str) -> String {
    format!("recent_retry_{denial_id}")
}

fn recent_retry_approval_request_from_pending(
    approval: &PendingRecentRetryApproval,
    requested_scope: &str,
) -> ApprovalRequest {
    ApprovalRequest {
        approval_request_id: approval.approval_request_id.clone(),
        action_digest: approval.action_digest.clone(),
        snapshot_digest: approval.snapshot_digest.clone(),
        requested_scope: requested_scope.to_owned(),
        risk_summary: format!("Run exact recent denied tool `{}` once", approval.tool_name),
        allowed_decisions: vec![ApprovalDecisionKind::Approved, ApprovalDecisionKind::Denied],
        expires_at_unix_ms: approval.expires_at_unix_ms,
    }
}

fn recent_retry_requester_digest(message: &InboundMessage, session_key: &str) -> String {
    digest_json(&json!({
        "session_key": session_key,
        "channel": &message.channel,
        "chat_id": &message.chat_id,
        "sender_id": &message.sender_id,
    }))
}

fn digest_json(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn recent_retry_closed_message(error: RecentAutoModeRetryTokenConsumeError) -> String {
    let reason = match error {
        RecentAutoModeRetryTokenConsumeError::Missing => "process-local retry token is missing",
        RecentAutoModeRetryTokenConsumeError::Expired => "process-local retry token expired",
        RecentAutoModeRetryTokenConsumeError::Consumed => {
            "process-local retry token was already consumed"
        }
        RecentAutoModeRetryTokenConsumeError::Mismatched => {
            "process-local retry token did not match the denial metadata"
        }
    };
    format!("Recent retry failed closed because the {reason}. The denied action was not run; request the action again if still needed.")
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn parse_permission_approval_reply(content: &str) -> PermissionApprovalReply {
    let normalized = content.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "y" | "yes" | "approve" | "approved" | "allow" | "run" | "go" | "진행"
        | "진행해줘" | "승인" | "허용" => PermissionApprovalReply::Approve,
        "3" | "approve_session" | "approve-session" => PermissionApprovalReply::ApproveSession,
        "2" | "n" | "no" | "deny" | "denied" | "cancel" | "stop" | "취소" | "거절" => {
            PermissionApprovalReply::Deny
        }
        _ => PermissionApprovalReply::Unknown,
    }
}

fn handle_goal_command(
    session: &mut Session,
    args: GoalCommandArgs,
) -> Result<String, AgentLoopError> {
    match args {
        GoalCommandArgs::Status => Ok(format_goal_status(
            persistent_goal_from_session(session).as_ref(),
        )),
        GoalCommandArgs::Invalid => {
            Ok("Usage: /goal [status|pause|resume|clear|done|blocked <reason>|<text>].".to_owned())
        }
        GoalCommandArgs::Set(text) => set_goal_command(session, text),
        GoalCommandArgs::Pause => update_existing_goal(session, pause_goal, "Goal paused."),
        GoalCommandArgs::Resume => update_existing_goal(session, resume_goal, "Goal resumed."),
        GoalCommandArgs::Clear => update_existing_goal(session, clear_goal, "Goal cleared."),
        GoalCommandArgs::Done => update_existing_goal(session, mark_goal_done, "Goal marked done."),
        GoalCommandArgs::Blocked(reason) => {
            let Some(goal) = persistent_goal_from_session(session) else {
                return Ok("No persistent goal is set.".to_owned());
            };
            let next = mark_goal_blocked(&goal, reason, now_iso());
            store_persistent_goal(session, &next).map_err(AgentLoopError::GoalMetadata)?;
            Ok("Goal marked blocked.".to_owned())
        }
    }
}

fn set_goal_command(session: &mut Session, text: String) -> Result<String, AgentLoopError> {
    if let Some(existing) = persistent_goal_from_session(session) {
        if !existing.is_terminal() {
            return Ok(
                "A persistent goal is already active. Use /goal clear before setting a new goal."
                    .to_owned(),
            );
        }
    }
    let goal = create_persistent_goal(
        session.key.clone(),
        text,
        now_iso(),
        DEFAULT_GOAL_TURN_BUDGET,
    );
    store_persistent_goal(session, &goal).map_err(AgentLoopError::GoalMetadata)?;
    Ok(format!("Goal set: {}", goal.text))
}

fn update_existing_goal(
    session: &mut Session,
    update: impl FnOnce(&PersistentGoal, String) -> PersistentGoal,
    success: &str,
) -> Result<String, AgentLoopError> {
    let Some(goal) = persistent_goal_from_session(session) else {
        return Ok("No persistent goal is set.".to_owned());
    };
    let next = update(&goal, now_iso());
    store_persistent_goal(session, &next).map_err(AgentLoopError::GoalMetadata)?;
    Ok(success.to_owned())
}

fn format_goal_status(goal: Option<&PersistentGoal>) -> String {
    let Some(goal) = goal else {
        return "No persistent goal is set.".to_owned();
    };
    let status = match goal.status {
        PersistentGoalStatus::Active => "active",
        PersistentGoalStatus::Paused => "paused",
        PersistentGoalStatus::Blocked => "blocked",
        PersistentGoalStatus::Done => "done",
        PersistentGoalStatus::Cleared => "cleared",
    };
    let verdict = goal
        .last_verdict
        .map(|verdict| format!("{verdict:?}").to_ascii_lowercase())
        .unwrap_or_else(|| "none".to_owned());
    let blocked = goal
        .blocked_reason
        .as_ref()
        .map(|reason| format!("\nBlocked reason: {reason}"))
        .unwrap_or_default();
    format!(
        "Goal: {}\nStatus: {}\nBudget: {}/{} turns used\nLast verdict: {}{}",
        goal.text, status, goal.turns_used, goal.turn_budget, verdict, blocked
    )
}

fn format_history_command(session: &Session, count: usize) -> String {
    let count = count.clamp(1, 50);
    let history = session.get_history_with_options(SessionHistoryOptions {
        max_messages: 0,
        max_tokens: 0,
        include_timestamps: false,
    });
    let visible = history
        .iter()
        .filter_map(|message| {
            let role = message.get("role").and_then(Value::as_str)?;
            if !matches!(role, "user" | "assistant") {
                return None;
            }
            let content = history_content_text(message);
            if content.is_empty() {
                None
            } else {
                Some(format!("{role}: {}", truncate_history_line(&content, 200)))
            }
        })
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return "No conversation history yet.".to_owned();
    }
    visible
        .into_iter()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn memory_git_store(workspace: &Path) -> GitCliStore {
    GitCliStore::new(
        workspace,
        [
            "memory/MEMORY.md".to_owned(),
            "SOUL.md".to_owned(),
            "USER.md".to_owned(),
        ],
    )
}

fn format_dream_outcome(outcome: &DreamRunOutcome) -> String {
    if !outcome.worked {
        if outcome.processed_entries == 0 {
            return "Dream idle: no new memory history to consolidate.".to_owned();
        }
        return "Dream did not complete. Memory cursor was left unchanged.".to_owned();
    }

    let mut lines = vec![format!(
        "Dream complete: processed {} entr{} through cursor {}.",
        outcome.processed_entries,
        if outcome.processed_entries == 1 {
            "y"
        } else {
            "ies"
        },
        outcome.processed_cursor
    )];
    if outcome.changelog.is_empty() {
        lines.push("No memory file changes were recorded.".to_owned());
    } else {
        lines.push(format!(
            "Recorded {} memory change{}.",
            outcome.changelog.len(),
            if outcome.changelog.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
        lines.extend(outcome.changelog.iter().map(|entry| format!("- {entry}")));
    }
    if let Some(commit) = &outcome.commit {
        lines.push(format!("Commit: {commit}"));
    }
    lines.join("\n")
}

fn format_dream_log(git: &GitCliStore, sha: Option<&str>) -> String {
    if let Some(sha) = sha {
        return match git.show_commit_with_diff(sha, 50) {
            Ok(Some((commit, diff))) => format_dream_log_content(&commit, &diff, Some(sha)),
            Ok(None) => format!("No Dream diff found for commit {sha}."),
            Err(error) => format!("Dream log failed: {error}"),
        };
    }

    match git.log(1).and_then(|commits| match commits.first() {
        Some(commit) => git
            .show_commit_with_diff(&commit.sha, 50)
            .map(|result| result.or_else(|| Some((commit.clone(), String::new())))),
        None => Ok(None),
    }) {
        Ok(Some((commit, diff))) => format_dream_log_content(&commit, &diff, None),
        Ok(None) => "Dream memory has no saved versions yet.".to_owned(),
        Err(error) => format!("Dream log failed: {error}"),
    }
}

fn format_dream_restore(git: &GitCliStore, sha: Option<&str>) -> String {
    let Some(sha) = sha else {
        return match git.log(10) {
            Ok(commits) if commits.is_empty() => {
                "Dream memory has no saved versions to restore yet.".to_owned()
            }
            Ok(commits) => format_dream_restore_list(&commits),
            Err(error) => format!("Dream restore failed: {error}"),
        };
    };
    match git.revert(sha) {
        Ok(Some(commit)) => format!("Dream restore complete. Revert commit: {commit}"),
        Ok(None) => format!("No restorable Dream commit found for {sha}."),
        Err(error) => format!("Dream restore failed: {error}"),
    }
}

fn format_dream_log_content(
    commit: &shacs_utils::gitstore::CommitInfo,
    diff: &str,
    requested_sha: Option<&str>,
) -> String {
    let files = if commit.files_changed.is_empty() {
        "No tracked memory files changed.".to_owned()
    } else {
        commit
            .files_changed
            .iter()
            .map(|path| format!("`{path}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = vec![
        "## Dream Update".to_owned(),
        String::new(),
        if requested_sha.is_some() {
            "Here is the selected Dream memory change.".to_owned()
        } else {
            "Here is the latest Dream memory change.".to_owned()
        },
        String::new(),
        format!("- Commit: `{}`", commit.sha),
        format!("- Time: {}", commit.timestamp),
        format!("- Changed files: {files}"),
    ];
    if diff.trim().is_empty() {
        lines.extend([
            String::new(),
            "Dream recorded this version, but there is no file diff to display.".to_owned(),
        ]);
    } else {
        lines.extend([
            String::new(),
            format!("Use `/dream-restore {}` to undo this change.", commit.sha),
            String::new(),
            "```diff".to_owned(),
            diff.trim_end().to_owned(),
            "```".to_owned(),
        ]);
    }
    lines.join("\n")
}

fn format_dream_restore_list(commits: &[shacs_utils::gitstore::CommitInfo]) -> String {
    let mut lines = vec![
        "## Dream Restore".to_owned(),
        String::new(),
        "Choose a Dream memory version to restore. Latest first:".to_owned(),
        String::new(),
    ];
    lines.extend(commits.iter().map(|commit| {
        format!(
            "- `{}` {} - {}",
            commit.sha, commit.timestamp, commit.summary
        )
    }));
    lines.extend([
        String::new(),
        "Preview a version with `/dream-log <sha>` before restoring it.".to_owned(),
        "Restore a version with `/dream-restore <sha>`.".to_owned(),
    ]);
    lines.join("\n")
}

fn history_content_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn truncate_history_line(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn materialize_recovery_markers(session: &mut Session) {
    let had_checkpoint = session.metadata.contains_key(RUNTIME_CHECKPOINT_KEY);
    if let Some(checkpoint) = session.metadata.remove(RUNTIME_CHECKPOINT_KEY) {
        if workflow_checkpoint_is_resumable(&checkpoint) {
            session
                .metadata
                .insert(RUNTIME_CHECKPOINT_KEY.to_owned(), checkpoint);
        } else {
            materialize_checkpoint(session, checkpoint);
        }
    }

    if session
        .metadata
        .remove(PENDING_USER_TURN_KEY)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        && !had_checkpoint
    {
        append_session_message(
            session,
            json!({
                "role": "assistant",
                "content": INTERRUPTED_PLACEHOLDER,
                "timestamp": now_iso(),
                "_interrupted": true,
            }),
        );
    }
}

fn workflow_checkpoint_is_resumable(checkpoint: &Value) -> bool {
    let phase_is_resumable = checkpoint
        .get("phase")
        .and_then(Value::as_str)
        .is_some_and(|phase| phase == "workflow_planned" || phase.starts_with("after-"));
    phase_is_resumable && checkpoint.get("workflow").is_some()
}

fn materialize_checkpoint(session: &mut Session, checkpoint: Value) {
    let Some(object) = checkpoint.as_object() else {
        return;
    };
    let phase = object
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(
        phase,
        "awaiting_tools" | "tools_completed" | "final_response"
    ) {
        if let Some(message) = object.get("assistant_message") {
            append_session_message(session, normalize_assistant_checkpoint(message));
        }
        for result in object
            .get("completed_tool_results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            append_session_message(session, normalize_tool_checkpoint(result, None));
        }
        if phase == "awaiting_tools" {
            for call in object
                .get("pending_tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                append_session_message(session, placeholder_tool_result(call));
            }
        }
    }
}

fn normalize_assistant_checkpoint(value: &Value) -> Value {
    if value.get("role").and_then(Value::as_str) == Some("assistant") {
        return value.clone();
    }
    json!({
        "role": "assistant",
        "content": value.get("content").cloned().unwrap_or_else(|| Value::String(String::new())),
        "tool_calls": value.get("tool_calls").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "timestamp": now_iso(),
    })
}

fn normalize_tool_checkpoint(value: &Value, fallback: Option<(&str, &str)>) -> Value {
    if value.get("role").and_then(Value::as_str) == Some("tool") {
        return value.clone();
    }
    let (fallback_id, fallback_name) = fallback.unwrap_or(("", ""));
    json!({
        "role": "tool",
        "tool_call_id": value
            .get("tool_call_id")
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or(fallback_id),
        "name": value.get("name").and_then(Value::as_str).unwrap_or(fallback_name),
        "content": value.get("content").cloned().unwrap_or_else(|| Value::String(PENDING_TOOL_PLACEHOLDER.to_owned())),
        "timestamp": now_iso(),
    })
}

fn placeholder_tool_result(call: &Value) -> Value {
    let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
    let name = tool_call_name(call);
    normalize_tool_checkpoint(
        &json!({
            "tool_call_id": id,
            "name": name,
            "content": PENDING_TOOL_PLACEHOLDER,
        }),
        Some((id, &name)),
    )
}

fn tool_call_name(call: &Value) -> String {
    call.get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| call.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn append_session_message(session: &mut Session, mut message: Value) {
    if let Some(object) = message.as_object_mut() {
        object
            .entry("timestamp".to_owned())
            .or_insert_with(|| Value::String(now_iso()));
    }
    if session.messages.last() == Some(&message) {
        return;
    }
    session.messages.push(message);
    session.updated_at = now_iso();
}

fn outbound_for(
    inbound: &InboundMessage,
    session_key: &str,
    content: String,
    buttons: Vec<Vec<String>>,
    stop_reason: &str,
) -> OutboundMessage {
    let mut metadata = Map::new();
    metadata.insert(
        "session_key".to_owned(),
        Value::String(session_key.to_owned()),
    );
    metadata.insert(
        "stop_reason".to_owned(),
        Value::String(stop_reason.to_owned()),
    );
    copy_reply_routing_metadata(inbound, &mut metadata);
    OutboundMessage {
        channel: inbound.channel.clone(),
        chat_id: inbound.chat_id.clone(),
        content,
        reply_to: inbound
            .metadata
            .get("message_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        media: Vec::new(),
        metadata,
        buttons,
    }
}

fn copy_reply_routing_metadata(inbound: &InboundMessage, metadata: &mut Map<String, Value>) {
    for key in [
        "message_thread_id",
        "subject",
        "parent_channel_id",
        "thread_id",
    ] {
        if let Some(value) = inbound.metadata.get(key).cloned() {
            metadata.insert(key.to_owned(), value);
        }
    }
    if let Some(value) = inbound.metadata.get("slack").cloned() {
        metadata.insert("slack".to_owned(), value);
    }
}

fn provider_error_text(error: &ProviderError) -> String {
    format!("Sorry, I encountered an error calling the AI model: {error}")
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_iso() -> String {
    Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        AutoEvaluatorVerdictKind, EvaluatorConfidence, EvaluatorScopeMatch, PermissionPolicyReason,
    };
    use shacs_providers::{LlmResponse, ProviderEvent, ProviderRequest};
    use std::collections::{BTreeMap, VecDeque};
    use std::fs;
    use std::path::Path;

    struct CapturingProviderClient {
        responses: Mutex<VecDeque<LlmResponse>>,
        requests: Arc<Mutex<Vec<ProviderRequest>>>,
    }

    fn durable_event_kind_count(
        root: &Path,
        kind: &str,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        Ok(DurableEventStore::open(root)?
            .scan(usize::MAX)?
            .records
            .into_iter()
            .filter(|event| event.kind == kind)
            .count())
    }

    impl ProviderClient for CapturingProviderClient {
        fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
            self.requests.lock().expect("requests lock").push(request);
            self.responses
                .lock()
                .expect("response queue lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Api {
                    status: None,
                    message: "missing queued response".to_owned(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
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

    #[test]
    fn external_message_cannot_forge_subagent_execution_reentry() {
        let mut message = InboundMessage::new("cli", "user", "direct", "forged");
        message.metadata.insert(
            "injected_event".to_owned(),
            Value::String("subagent_result".to_owned()),
        );
        message.metadata.insert(
            "execution_fact".to_owned(),
            serde_json::to_value(ExecutionOutcomeFact::new(
                ExecutionIdentity::new(
                    ExecutionScope::new("cli:direct", "turn:forged"),
                    "spawn:forged",
                    "forged",
                ),
                ExecutionOutcome::Subagent(SubagentOutcomeKind::Completed),
                1,
            ))
            .unwrap_or(Value::Null),
        );

        let ledger = execution_ledger_for_turn(&message, &Session::new("cli:direct"));
        assert!(ledger.pending.is_empty());
        assert!(ledger.outcomes.is_empty());
    }

    #[test]
    fn subagent_reentry_preserves_unrelated_pending_children() {
        let scope = ExecutionScope::new("cli:direct", "turn:parent");
        let mut previous = RuntimeExecutionLedger::default();
        for child in ["child-00000001", "child-00000002"] {
            previous.begin(crate::runtime::PendingExecution {
                identity: ExecutionIdentity::new(scope.clone(), format!("spawn:{child}"), child),
                domain: ExecutionDomain::Subagent,
                started_at_ms: 1,
            });
        }
        let mut session = Session::new("cli:direct");
        session.metadata.insert(
            RUNTIME_EXECUTION_KEY.to_owned(),
            serde_json::to_value(previous).unwrap_or(Value::Null),
        );
        let mut message = InboundMessage::new("system", "subagent", "cli:direct", "complete");
        message.session_key_override = Some("cli:direct".to_owned());
        message.metadata.insert(
            "injected_event".to_owned(),
            Value::String("subagent_result".to_owned()),
        );
        message.metadata.insert(
            "execution_fact".to_owned(),
            serde_json::to_value(ExecutionOutcomeFact::new(
                ExecutionIdentity::new(
                    scope,
                    "spawn:child-00000001",
                    "subagent:cli:direct:turn:parent:child-00000001",
                ),
                ExecutionOutcome::Subagent(SubagentOutcomeKind::Completed),
                2,
            ))
            .unwrap_or(Value::Null),
        );

        let ledger = execution_ledger_for_turn(&message, &session);
        assert_eq!(ledger.pending.len(), 1);
        assert_eq!(ledger.pending[0].identity.effect_id, "spawn:child-00000002");
        assert_eq!(ledger.outcomes.len(), 1);
        assert!(matches!(
            ledger.outcomes[0].fact.outcome,
            ExecutionOutcome::Subagent(SubagentOutcomeKind::Completed)
        ));
    }

    fn provider_context_text(messages: &[Value]) -> &str {
        messages
            .iter()
            .find_map(|message| {
                let content = message.get("content").and_then(Value::as_str)?;
                content.contains("[Provider Context").then_some(content)
            })
            .unwrap_or_default()
    }

    fn test_recent_denial(label: &str, created_at_unix_ms: u64) -> RecentAutoModeDenial {
        RecentAutoModeDenial {
            denial_id: format!("auto_denial_{label}"),
            created_at_unix_ms,
            session_digest: "session-digest".to_owned(),
            turn_digest: format!("turn-digest-{label}"),
            tool_name: "exec".to_owned(),
            capabilities: vec![shacs_config::SafetyCapability::ProcExec],
            target_summary: vec!["target:test".to_owned()],
            action_digest: format!("action-{label}"),
            argument_digest: format!("argument-{label}"),
            snapshot_digest: format!("snapshot-{label}"),
            decision_reason: PermissionPolicyReason::EvaluatorUncertain,
            classifier_verdict: AutoEvaluatorVerdictKind::DenyCandidate,
            classifier_confidence: EvaluatorConfidence::High,
            classifier_scope_match: EvaluatorScopeMatch::Requested,
            retryable: true,
        }
    }

    #[test]
    fn store_recent_auto_mode_denials_orders_same_timestamp_by_collection_recency() {
        let mut session = Session::new("cli:recent-order");

        store_recent_auto_mode_denials(
            &mut session,
            vec![
                test_recent_denial("older", 10),
                test_recent_denial("newer", 10),
            ],
        );

        let ids = recent_auto_mode_denials(&session)
            .into_iter()
            .map(|denial| denial.denial_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["auto_denial_newer", "auto_denial_older"]);
    }

    #[test]
    fn live_context_handoff_uses_current_directory_for_context_files(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let nested = workspace.path().join("nested");
        fs::create_dir_all(&nested)?;
        fs::write(workspace.path().join("AGENTS.md"), "bootstrap-system-body")?;
        fs::write(workspace.path().join(".shacs.md"), "root-context-body")?;
        fs::write(nested.join("AGENTS.md"), "nested-context-body")?;

        let handoff = build_live_context_provider_handoff(
            workspace.path(),
            "plain request",
            &[json!({"role": "system", "content": "runtime instructions"})],
            Some(nested),
            None,
        );
        let provider_text = handoff
            .blocks
            .iter()
            .map(|block| block.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(provider_text.contains("root-context-body"));
        assert!(provider_text.contains("nested-context-body"));
        assert!(!provider_text.contains("bootstrap-system-body"));
        Ok(())
    }

    #[test]
    fn live_context_handoff_excludes_workspace_bootstrap_files_from_provider_context(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join("AGENTS.md"), "bootstrap-agents-body")?;
        fs::write(workspace.path().join(".shacs.md"), "provider-context-body")?;

        let handoff = build_live_context_provider_handoff(
            workspace.path(),
            "plain request",
            &[json!({"role": "system", "content": "bootstrap-agents-body"})],
            Some(workspace.path().to_path_buf()),
            None,
        );
        let provider_text = handoff
            .blocks
            .iter()
            .map(|block| block.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(provider_text.contains("provider-context-body"));
        assert!(!provider_text.contains("bootstrap-agents-body"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn live_context_handoff_excludes_workspace_bootstrap_symlink_alias(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let docs = workspace.path().join("docs");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("rules.md"), "aliased-bootstrap-body")?;
        fs::write(workspace.path().join(".shacs.md"), "provider-context-body")?;
        std::os::unix::fs::symlink(docs.join("rules.md"), workspace.path().join("AGENTS.md"))?;

        let handoff = build_live_context_provider_handoff(
            workspace.path(),
            "plain request",
            &[json!({"role": "system", "content": "aliased-bootstrap-body"})],
            Some(workspace.path().to_path_buf()),
            None,
        );
        let provider_text = handoff
            .blocks
            .iter()
            .map(|block| block.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(provider_text.contains("provider-context-body"));
        assert!(!provider_text.contains("aliased-bootstrap-body"));
        Ok(())
    }

    #[test]
    fn live_context_handoff_uses_configured_budget() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join("note.txt"), "inline-note-body")?;

        let handoff = build_live_context_provider_handoff(
            workspace.path(),
            "read @note.txt",
            &[json!({"role": "system", "content": "runtime instructions"})],
            Some(workspace.path().to_path_buf()),
            live_context_budget_bytes(Some(1)),
        );

        assert_eq!(live_context_budget_bytes(Some(1)), Some(4));
        assert_eq!(handoff.budget_bytes, 4);
        assert!(handoff.blocks.is_empty());
        assert!(handoff.used_context_bytes <= 4);
        Ok(())
    }

    #[test]
    fn process_message_builds_live_context_handoff_without_persisting_context_blocks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join("note.txt"), "inline-note-body")?;
        fs::write(workspace.path().join("AGENTS.md"), "bootstrap-system-body")?;
        fs::write(workspace.path().join(".shacs.md"), "workspace-context-body")?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::from(vec![LlmResponse {
                content: Some("answer".to_owned()),
                ..LlmResponse::default()
            }])),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let sessions = SessionManager::new(workspace.path())?;
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            sessions,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            AgentLoopConfig::new(workspace.path(), "model"),
        );
        let message = InboundMessage::new("direct", "user", "direct", "please read @note.txt");

        let outcome = loop_runtime.process_message(message)?;

        assert_eq!(outcome.final_content.as_deref(), Some("answer"));
        let requests = captured_requests.lock().expect("requests lock");
        let provider_messages = &requests[0].messages;
        let provider_text = provider_context_text(provider_messages);
        assert!(provider_text.contains("inline:note.txt"));
        assert!(provider_text.contains("inline-note-body"));
        assert!(provider_text.contains("context-file:"));
        assert!(provider_text.contains("workspace-context-body"));
        assert!(!provider_text.contains("bootstrap-system-body"));

        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:direct");
        let session_text = session
            .messages
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(session_text.contains("please read @note.txt"));
        assert!(!session_text.contains("inline-note-body"));
        assert!(!session_text.contains("workspace-context-body"));
        assert!(!session_text.contains("[Provider Context"));
        Ok(())
    }

    #[test]
    fn process_message_builds_live_context_handoff_for_ask_user_resume_without_persisting_context_blocks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join("note.txt"), "resume-inline-body")?;
        fs::write(workspace.path().join("AGENTS.md"), "resume-bootstrap-body")?;
        fs::write(workspace.path().join(".shacs.md"), "resume-context-body")?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::from(vec![LlmResponse {
                content: Some("resumed answer".to_owned()),
                ..LlmResponse::default()
            }])),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut sessions = SessionManager::new(workspace.path())?;
        let mut session = Session::new("direct:direct");
        session.messages.push(json!({
            "role": "assistant",
            "content": "need input",
            "tool_calls": [{
                "id": "ask-1",
                "type": "function",
                "function": {
                    "name": "ask_user",
                    "arguments": "{\"question\":\"Which note?\"}"
                }
            }]
        }));
        sessions.save(&session)?;
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            sessions,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            AgentLoopConfig::new(workspace.path(), "model"),
        );
        let message = InboundMessage::new("direct", "user", "direct", "resume with @note.txt");

        let outcome = loop_runtime.process_message(message)?;

        assert_eq!(outcome.final_content.as_deref(), Some("resumed answer"));
        let requests = captured_requests.lock().expect("requests lock");
        let provider_text = provider_context_text(&requests[0].messages);
        assert!(provider_text.contains("inline:note.txt"));
        assert!(provider_text.contains("resume-inline-body"));
        assert!(provider_text.contains("resume-context-body"));
        assert!(!provider_text.contains("resume-bootstrap-body"));

        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:direct");
        let session_text = session
            .messages
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(session_text.contains("resume with @note.txt"));
        assert!(!session_text.contains("resume-inline-body"));
        assert!(!session_text.contains("resume-context-body"));
        assert!(!session_text.contains("[Provider Context"));
        Ok(())
    }

    #[test]
    fn process_message_auto_admits_clear_complex_turn_without_metadata_or_planning_call(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::from(vec![
                LlmResponse {
                    content: Some("read-only analysis complete".to_owned()),
                    ..LlmResponse::default()
                },
                LlmResponse {
                    content: Some("pass".to_owned()),
                    ..LlmResponse::default()
                },
            ])),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            AgentLoopConfig::new(workspace.path(), "model"),
        );

        let outcome = loop_runtime.process_direct(
            "Please review this migration plan and verify each part:\n- inspect the data flow\n- compare the API assumptions\n- identify risks\n- produce a concise recommendation",
            Some("direct:auto-workflow"),
        )?;

        assert_eq!(outcome.stop_reason, "workflow_completed", "{outcome:?}");
        assert_eq!(captured_requests.lock().expect("requests lock").len(), 2);
        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:auto-workflow");
        assert!(session.metadata.get("runtime_workflow").is_some());
        assert!(session.metadata.get("runtime_checkpoint").is_some());
        assert!(session.metadata.get("pending_workflow").is_none());
        assert_eq!(session.messages.len(), 2);
        Ok(())
    }

    #[test]
    fn process_message_simple_turn_falls_back_to_regular_loop(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::from(vec![LlmResponse {
                content: Some("hello back".to_owned()),
                ..LlmResponse::default()
            }])),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            AgentLoopConfig::new(workspace.path(), "model"),
        );

        let outcome = loop_runtime.process_direct("hello", Some("direct:simple"))?;

        assert_eq!(outcome.final_content.as_deref(), Some("hello back"));
        assert_eq!(outcome.stop_reason, "completed");
        assert_eq!(captured_requests.lock().expect("requests lock").len(), 1);
        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:simple");
        assert!(session.metadata.get("runtime_workflow").is_none());
        Ok(())
    }

    #[test]
    fn process_message_persists_selected_skill_recipe_evidence(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        write_workflow_skill(
            workspace.path(),
            "reviewer",
            "review-fanout",
            "fan_out_and_synthesize",
        )?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::from(vec![
                LlmResponse {
                    content: Some("recipe-backed analysis complete".to_owned()),
                    ..LlmResponse::default()
                },
                LlmResponse {
                    content: Some("pass".to_owned()),
                    ..LlmResponse::default()
                },
            ])),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            AgentLoopConfig::new(workspace.path(), "model"),
        );

        let outcome = loop_runtime.process_direct(
            "Please use the reviewer workflow to review this migration plan and verify each part:\n- inspect the data flow\n- compare the API assumptions\n- identify risks\n- produce a concise recommendation",
            Some("direct:recipe-workflow"),
        )?;

        assert_eq!(outcome.stop_reason, "workflow_completed", "{outcome:?}");
        assert_eq!(captured_requests.lock().expect("requests lock").len(), 2);
        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:recipe-workflow");
        let selected = &session.metadata["runtime_checkpoint"]["recipe_evidence"]["selected"];
        assert_eq!(selected["recipe"]["recipe_id"], "review-fanout");
        assert_eq!(selected["skill_name"], "reviewer");
        assert!(
            session.metadata["runtime_checkpoint"]["workflow"]["evidence_refs"]
                .as_array()
                .is_some_and(|refs| refs.iter().any(|value| value
                    .as_str()
                    .is_some_and(|value| value.starts_with("skill://workspace-local/reviewer#"))))
        );
        Ok(())
    }

    #[test]
    fn process_message_resumes_read_only_pending_workflow_after_restart(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let durable_events = tempfile::tempdir()?;
        let content = "Please review this complex request in workflow form:\n- map inputs\n- inspect dependencies\n- verify assumptions\n- summarize risks";
        let (admission, plan, recipe_evidence) = compile_automatic_workflow_request(
            content,
            "direct:resume-read-only",
            workspace.path(),
            "turn-1",
            &PermissionModeSnapshot::default(),
        )
        .ok_or("missing auto workflow")?;
        let mut sessions = SessionManager::new(workspace.path())?;
        let mut session = Session::new("direct:resume-read-only");
        session.add_message("user", content, Map::new());
        session
            .metadata
            .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
        store_pending_workflow(
            &mut session,
            "direct:resume-read-only",
            &admission,
            &plan,
            recipe_evidence.clone(),
        )?;
        store_planned_workflow_checkpoint(&mut session, &plan, recipe_evidence.as_ref());
        sessions.save(&session)?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::from(vec![
                LlmResponse {
                    content: Some("resumed analysis complete".to_owned()),
                    ..LlmResponse::default()
                },
                LlmResponse {
                    content: Some("pass".to_owned()),
                    ..LlmResponse::default()
                },
            ])),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut config = AgentLoopConfig::new(workspace.path(), "model");
        config.durable_event_root = Some(durable_events.path().to_path_buf());
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            sessions,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            config,
        );

        let outcome = loop_runtime.process_direct("resume", Some("direct:resume-read-only"))?;

        assert_eq!(outcome.stop_reason, "workflow_completed", "{outcome:?}");
        assert_eq!(captured_requests.lock().expect("requests lock").len(), 2);
        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:resume-read-only");
        assert!(session.metadata.get("pending_workflow").is_none());
        assert!(session.metadata.get("runtime_workflow").is_some());
        assert_eq!(session.messages.len(), 2);
        assert_eq!(
            durable_event_kind_count(durable_events.path(), WORKFLOW_PLANNED)?,
            1
        );
        assert_eq!(
            durable_event_kind_count(durable_events.path(), WORKFLOW_COMPLETED)?,
            1
        );
        Ok(())
    }

    #[test]
    fn process_message_blocks_resume_when_recipe_evidence_is_invalid(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let durable_events = tempfile::tempdir()?;
        let skill_file = write_workflow_skill(
            workspace.path(),
            "reviewer",
            "review-fanout",
            "fan_out_and_synthesize",
        )?;
        let content = "Please use the reviewer workflow to review this complex request:\n- map inputs\n- inspect dependencies\n- verify assumptions\n- summarize risks";
        let (admission, plan, recipe_evidence) = compile_automatic_workflow_request(
            content,
            "direct:resume-recipe-invalid",
            workspace.path(),
            "turn-1",
            &PermissionModeSnapshot::default(),
        )
        .ok_or("missing auto workflow")?;
        let mut sessions = SessionManager::new(workspace.path())?;
        let mut session = Session::new("direct:resume-recipe-invalid");
        session.add_message("user", content, Map::new());
        session
            .metadata
            .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
        store_pending_workflow(
            &mut session,
            "direct:resume-recipe-invalid",
            &admission,
            &plan,
            recipe_evidence.clone(),
        )?;
        store_planned_workflow_checkpoint(&mut session, &plan, recipe_evidence.as_ref());
        sessions.save(&session)?;
        fs::write(skill_file, "---\nname: reviewer\n---\nchanged")?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::new()),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut config = AgentLoopConfig::new(workspace.path(), "model");
        config.durable_event_root = Some(durable_events.path().to_path_buf());
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            sessions,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            config,
        );

        let outcome =
            loop_runtime.process_direct("resume", Some("direct:resume-recipe-invalid"))?;

        assert_eq!(outcome.stop_reason, "workflow_blocked");
        assert!(outcome
            .final_content
            .as_deref()
            .unwrap_or_default()
            .contains("recipe evidence is no longer valid"));
        assert!(captured_requests.lock().expect("requests lock").is_empty());
        assert_eq!(
            durable_event_kind_count(durable_events.path(), WORKFLOW_PLANNED)?,
            1
        );
        assert_eq!(
            durable_event_kind_count(durable_events.path(), WORKFLOW_FAILED)?,
            1
        );
        Ok(())
    }

    #[test]
    fn process_message_completes_from_checkpoint_without_rerunning_completed_step(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let durable_events = tempfile::tempdir()?;
        let content = "Please review this complex request in workflow form:\n- map inputs\n- inspect dependencies\n- verify assumptions\n- summarize risks";
        let (admission, plan, recipe_evidence) = compile_automatic_workflow_request(
            content,
            "direct:resume-completed-step",
            workspace.path(),
            "turn-1",
            &PermissionModeSnapshot::default(),
        )
        .ok_or("missing auto workflow")?;
        let mut sessions = SessionManager::new(workspace.path())?;
        let mut session = Session::new("direct:resume-completed-step");
        session.add_message("user", content, Map::new());
        session
            .metadata
            .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
        store_pending_workflow(
            &mut session,
            "direct:resume-completed-step",
            &admission,
            &plan,
            recipe_evidence.clone(),
        )?;
        store_planned_workflow_checkpoint(&mut session, &plan, recipe_evidence.as_ref());
        session.metadata["runtime_checkpoint"]["workflow"]["completed_steps"] =
            json!(["read-only-analysis"]);
        session.metadata["runtime_checkpoint"]["workflow_checkpoint_payload"] = json!({
            "checkpoint": session.metadata["runtime_checkpoint"]["workflow"].clone(),
            "completed_step_id": "read-only-analysis",
            "completed_child_ids": ["child-1"],
            "ready_step_ids": [],
            "pending_step_ids": [],
            "worktree_refs": [],
            "evidence_refs": [],
            "resume_step_id": "read-only-analysis"
        });
        sessions.save(&session)?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::new()),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut config = AgentLoopConfig::new(workspace.path(), "model");
        config.durable_event_root = Some(durable_events.path().to_path_buf());
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            sessions,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            config,
        );

        let outcome =
            loop_runtime.process_direct("resume", Some("direct:resume-completed-step"))?;

        assert_eq!(outcome.stop_reason, "workflow_completed");
        assert!(outcome
            .final_content
            .as_deref()
            .unwrap_or_default()
            .contains("saved checkpoint"));
        assert!(captured_requests.lock().expect("requests lock").is_empty());
        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:resume-completed-step");
        assert!(session.metadata.get("pending_workflow").is_none());
        assert_eq!(
            durable_event_kind_count(durable_events.path(), WORKFLOW_PLANNED)?,
            1
        );
        assert_eq!(
            durable_event_kind_count(durable_events.path(), WORKFLOW_COMPLETED)?,
            1
        );
        Ok(())
    }

    #[test]
    fn completed_workflow_step_ids_excludes_partially_failed_step() {
        let mut plan = compile_read_only_workflow_plan(
            "inspect and verify",
            "direct:partial-step",
            "turn-1",
            &PermissionModeSnapshot::default(),
        );
        let mut sibling = plan.child_graph[0].clone();
        sibling.child_id = "child-2".to_owned();
        plan.child_graph.push(sibling);
        let mut results = vec![
            shacs_workflow::WorkflowChildResult {
                child_id: "child-1".to_owned(),
                step_id: "read-only-analysis".to_owned(),
                status: WorkflowChildRunStatus::Completed,
                summary: "completed".to_owned(),
                evidence_refs: Vec::new(),
            },
            shacs_workflow::WorkflowChildResult {
                child_id: "child-2".to_owned(),
                step_id: "read-only-analysis".to_owned(),
                status: WorkflowChildRunStatus::Failed,
                summary: "failed".to_owned(),
                evidence_refs: Vec::new(),
            },
        ];

        assert!(completed_workflow_step_ids(&plan, &results).is_empty());
        results[1].status = WorkflowChildRunStatus::Completed;
        assert_eq!(
            completed_workflow_step_ids(&plan, &results),
            vec!["read-only-analysis".to_owned()]
        );
    }

    #[test]
    fn workflow_recovery_preserves_live_checkpoint_and_repairs_remaining_dependencies(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut plan = compile_read_only_workflow_plan(
            "inspect and verify",
            "direct:resume-dag",
            "turn-1",
            &PermissionModeSnapshot::default(),
        );
        plan.steps.push(WorkflowStep {
            step_id: "verify-analysis".to_owned(),
            label: "Verify analysis".to_owned(),
            pattern: WorkflowPattern::AdversarialVerification,
            depends_on: vec!["read-only-analysis".to_owned()],
            required: true,
            expected_output_schema: None,
        });
        plan.child_graph.push(WorkflowChildSpec {
            child_id: "child-2".to_owned(),
            step_id: "verify-analysis".to_owned(),
            goal: "verify sanitized analysis".to_owned(),
            tool_scope_ref: Some("auto-read-only".to_owned()),
            worktree_policy: WorkflowWorktreePolicy::ReadOnlySnapshot,
            budget: WorkflowBudgetSlice {
                max_tokens: Some(1_000),
                max_wall_clock_ms: Some(30_000),
            },
            verifier_required: false,
        });
        let mut session = Session::new("direct:resume-dag");
        store_planned_workflow_checkpoint(&mut session, &plan, None);
        session.metadata["runtime_checkpoint"]["phase"] = json!("after-read-only-analysis");
        session.metadata["runtime_checkpoint"]["workflow"]["completed_steps"] =
            json!(["read-only-analysis"]);
        session.metadata["runtime_checkpoint"]["workflow_checkpoint_payload"] = json!({
            "checkpoint": session.metadata["runtime_checkpoint"]["workflow"].clone(),
            "completed_step_id": "read-only-analysis",
            "completed_child_ids": ["child-1"],
            "ready_step_ids": ["verify-analysis"],
            "pending_step_ids": [],
            "worktree_refs": [],
            "evidence_refs": [],
            "resume_step_id": "read-only-analysis"
        });

        assert!(workflow_checkpoint_is_resumable(
            &session.metadata["runtime_checkpoint"]
        ));
        assert!(!apply_completed_workflow_checkpoint(&mut plan, &session));
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].step_id, "verify-analysis");
        assert!(plan.steps[0].depends_on.is_empty());
        assert_eq!(plan.child_graph.len(), 1);
        assert_eq!(plan.child_graph[0].child_id, "child-2");
        Ok(())
    }

    #[test]
    fn process_message_blocks_write_capable_pending_workflow_after_restart(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let content = "Please implement this workflow with several coordinated file edits and verification steps:\n- update the main module\n- update tests\n- verify behavior\n- summarize the write risks";
        let (mut admission, mut plan, recipe_evidence) = compile_automatic_workflow_request(
            content,
            "direct:resume-write",
            workspace.path(),
            "turn-1",
            &PermissionModeSnapshot::default(),
        )
        .ok_or("missing auto workflow")?;
        admission.requires_write_isolation = true;
        plan.worktree_policy = WorkflowWorktreePolicy::IsolatedWorktreeRequired;
        plan.child_graph[0].worktree_policy = WorkflowWorktreePolicy::IsolatedWorktreeRequired;
        let mut sessions = SessionManager::new(workspace.path())?;
        let mut session = Session::new("direct:resume-write");
        session.add_message("user", content, Map::new());
        session
            .metadata
            .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
        store_pending_workflow(
            &mut session,
            "direct:resume-write",
            &admission,
            &plan,
            recipe_evidence.clone(),
        )?;
        store_planned_workflow_checkpoint(&mut session, &plan, recipe_evidence.as_ref());
        sessions.save(&session)?;
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let client = CapturingProviderClient {
            responses: Mutex::new(VecDeque::new()),
            requests: captured_requests.clone(),
        };
        let tools = ToolRegistry::new();
        let mut loop_runtime = AgentLoop::new(
            MessageBus::new(),
            sessions,
            ContextBuilder::new(workspace.path()),
            &tools,
            &client,
            AgentLoopConfig::new(workspace.path(), "model"),
        );

        let outcome = loop_runtime.process_direct("resume", Some("direct:resume-write"))?;

        assert_eq!(outcome.stop_reason, "workflow_blocked");
        assert!(outcome
            .final_content
            .as_deref()
            .unwrap_or_default()
            .contains("ambiguous write-capable workflow phase"));
        assert!(captured_requests.lock().expect("requests lock").is_empty());
        let session = loop_runtime
            .session_manager_mut()
            .get_or_create("direct:resume-write");
        assert_eq!(
            session.metadata["runtime_checkpoint"]["phase"],
            "workflow_blocked_recovery"
        );
        assert!(session.metadata.get("pending_workflow").is_none());
        Ok(())
    }

    #[test]
    fn workflow_worktree_approval_never_treats_bypass_as_workflow_approval(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let workspace = tempfile::tempdir()?;
        let mut plan = compile_read_only_workflow_plan(
            "Please implement coordinated workspace edits across several files",
            "direct:permission-map",
            "turn-1",
            &PermissionModeSnapshot::default(),
        );
        plan.worktree_policy = WorkflowWorktreePolicy::IsolatedWorktreeRequired;
        plan.child_graph[0].worktree_policy = WorkflowWorktreePolicy::IsolatedWorktreeRequired;
        plan.tool_scope_policy.allowed_tools =
            vec!["read_file".to_owned(), "write_file".to_owned()];

        let mut config = AgentLoopConfig::new(workspace.path(), "model");
        config.permission_mode_snapshot.mode = PermissionMode::BypassPermissions;
        assert!(!workflow_worktree_approval_granted(&config, &plan));

        config.permission_mode_snapshot.mode = PermissionMode::AcceptEdits;
        assert!(workflow_worktree_approval_granted(&config, &plan));

        config.permission_mode_snapshot.mode = PermissionMode::Auto;
        config.permission_auto_approval.enabled = true;
        config.permission_auto_approval.allow_workspace_edits = true;
        assert!(workflow_worktree_approval_granted(&config, &plan));

        plan.tool_scope_policy.allowed_tools.push("exec".to_owned());
        assert!(!workflow_worktree_approval_granted(&config, &plan));
        Ok(())
    }

    fn write_workflow_skill(
        workspace: &Path,
        name: &str,
        recipe_id: &str,
        pattern: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let skill_dir = workspace.join("skills").join(name);
        fs::create_dir_all(&skill_dir)?;
        let skill_file = skill_dir.join("SKILL.md");
        fs::write(
            &skill_file,
            format!(
                "---\nname: {name}\ndescription: Workflow recipe\nworkflow.recipe.id: {recipe_id}\nworkflow.recipe.pattern: {pattern}\nworkflow.recipe.prompt_template_ref: prompts/{recipe_id}.md\nworkflow.recipe.suggested_budget_tokens: 12000\nworkflow.recipe.safety_notes: read-only, no permission grants\n---\nUse this recipe as read-only guidance."
            ),
        )?;
        Ok(skill_file)
    }
}
