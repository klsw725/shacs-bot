use crate::runtime::tool_execution::session_approval_context_digest;
use crate::runtime::{
    apply_context_safety_gate, build_context_provider_handoff, discover_context_files,
    dispatch_bridge_tool_calls, parse_context_references, resolve_context_reference,
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
    ContextReferenceResolverConfig, DreamProcessor, DreamRunOutcome, GoalMetadataError,
    InboundMessage, LoopTaskCancelResult, LoopTaskRegistry, MemoryConsolidationError, MemoryStore,
    MessageBus, OutboundMessage, PermissionCeilingSnapshot, PermissionMode, PermissionModeSnapshot,
    PermissionRuleInput, PermissionedAction, PersistentGoal, PersistentGoalStatus,
    ProviderArchiveConsolidator, ProviderEventCallback, RuntimeContextTools, RuntimeInterrupt,
    RuntimeToolCall, RuntimeToolExecutionReport, RuntimeToolExecutor, RuntimeToolMessage, Session,
    SessionApprovalCacheEntry, SessionHistoryOptions, SessionManager, SessionTurnAcquireError,
    SessionTurnLock, TokenConsolidationConfig, ToolEventCallback, ToolExecutionContext,
    DEFAULT_GOAL_TURN_BUDGET,
};
use crate::tools::{
    ask_user_options_from_messages, ask_user_outbound, assemble_tool_surface, bridge_tool_names,
    pending_ask_user_id, MessageSender, MessageTool, ToolRegistry, ToolSurfaceAssemblyInput,
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use shacs_command::{
    build_help_text, is_builtin_command, parse_loop_command_route, CommandKind, GoalCommandArgs,
    HistoryCommandArgs, LoopCommand,
};
use shacs_providers::{GenerationSettings, ProviderClient, ProviderError, ProviderRetryMode};
use shacs_utils::gitstore::{GitCliStore, GitStore};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

const PENDING_USER_TURN_KEY: &str = "pending_user_turn";
const RUNTIME_CHECKPOINT_KEY: &str = "runtime_checkpoint";
const PENDING_PERMISSION_APPROVAL_KEY: &str = "pending_permission_approval";
const SESSION_PERMISSION_APPROVALS_KEY: &str = "session_permission_approvals";
const SESSION_PERMISSION_APPROVAL_LIMIT: usize = 32;
const PENDING_PERMISSION_WIZARD_KEY: &str = "pending_permission_wizard";
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
    pub permission_ceiling_snapshot: Option<PermissionCeilingSnapshot>,
    pub permission_evaluator: Option<AutoEvaluatorVerdict>,
    pub permission_interactive: bool,
    pub permission_mode_setter: Option<PermissionModeSetter>,
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
            permission_ceiling_snapshot: None,
            permission_evaluator: None,
            permission_interactive: false,
            permission_mode_setter: None,
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
            return match self.turn_lock.acquire(session_key.clone()) {
                Ok(_turn_guard) => {
                    let mut session = self.sessions.get_or_create(&session_key);
                    materialize_recovery_markers(&mut session);
                    self.handle_loop_command(route.command.clone(), &message, session, true)
                }
                Err(SessionTurnAcquireError::AlreadyActive { .. }) => {
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

        let pending_permission_approval = pending_permission_approval(&session);
        let pending_ask_id = pending_ask_user_id(&session.messages);
        let (initial_messages, context_provider_handoff) = if let Some(approval) =
            pending_permission_approval
        {
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
                    for tool_message in report.messages {
                        append_session_message(&mut session, tool_message.to_json());
                    }
                    session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
                    if let (true, Some(action)) = (session_scoped, approved_action) {
                        store_session_permission_approval(
                            &mut session,
                            &session_key,
                            approval_cache,
                            &action,
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
            self.maybe_consolidate_session_by_tokens(&mut session)?;
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
            session
                .metadata
                .insert(PENDING_USER_TURN_KEY.to_owned(), Value::Bool(true));
            (initial_messages, Some(context_provider_handoff))
        };
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
        spec.cancellation_token = self.task_registry.cancellation_token(&session_key);
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
            let _ = recover_lock(&checkpoint_manager_capture).save(&session);
        }));

        let run_result = match self.runner.run(spec) {
            Ok(result) => result,
            Err(error) => {
                session.add_message("assistant", provider_error_text(&error), Map::new());
                clear_runtime_markers(&mut session);
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
        clear_runtime_markers(&mut session);
        store_pending_permission_approval(
            &mut session,
            &run_result.interrupt,
            &tool_context,
            &message,
            &session_key,
        );
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
                let content = match self.task_registry.cancel(&session.key) {
                    LoopTaskCancelResult::NoAsyncTask => {
                        "Stop requested. No async task is running in this synchronous loop."
                            .to_owned()
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
            LoopCommand::Permission => {
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
        let executor =
            RuntimeToolExecutor::with_context_tools(self.tools, self.context_tools.clone());
        let mut context = approval.tool_context.clone();
        context.permission_approval_cache = Some(approval_cache);
        context.permission_session_approval_cache = Vec::new();
        if bridge_tool_names().contains(&approval.tool_call.name.as_str()) {
            let tool_surface = assemble_tool_surface(ToolSurfaceAssemblyInput {
                definitions: self.tools.definitions(),
                runtime: crate::runtime::ToolSearchRuntimeInput {
                    config: self.config.tool_search,
                    context_window_tokens: self.config.context_window_tokens,
                },
            });
            return dispatch_bridge_tool_calls(
                vec![approval.tool_call.clone()],
                tool_surface.catalog.as_ref(),
                self.tools,
                &executor,
                &context,
                self.config.concurrent_tools,
            )
            .into_runtime_report();
        }
        executor.execute_tool_calls(vec![approval.tool_call.clone()], &context)
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
    Memory(MemoryConsolidationError),
    GoalMetadata(GoalMetadataError),
    PermissionModeSave(String),
    DuplicateActiveTurn { session_key: String },
}

impl fmt::Display for AgentLoopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "session persistence failed: {error}"),
            Self::Memory(error) => write!(formatter, "memory consolidation failed: {error}"),
            Self::GoalMetadata(error) => write!(formatter, "goal metadata failed: {error}"),
            Self::PermissionModeSave(error) => {
                write!(formatter, "permission mode save failed: {error}")
            }
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

fn clear_runtime_markers(session: &mut Session) {
    session.metadata.remove(PENDING_USER_TURN_KEY);
    session.metadata.remove(RUNTIME_CHECKPOINT_KEY);
    session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
    session.metadata.remove(PENDING_PERMISSION_WIZARD_KEY);
}

fn permission_wizard_choices_text() -> &'static str {
    "Choose permissions.mode: `default`, `auto`, `bypass_permissions`, or `cancel`."
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
    let mut approvals = session_permission_approvals(session)
        .into_iter()
        .filter(|entry| entry.approval.request.expires_at_unix_ms >= now)
        .filter(|entry| {
            !(entry.session_key == session_key
                && entry.approval.request.action_digest == action.action_digest
                && entry.approval.request.requested_scope == action.session_id
                && entry.approval_context_digest == approval_context_digest)
        })
        .collect::<Vec<_>>();
    approvals.push(SessionApprovalCacheEntry {
        session_key: session_key.to_owned(),
        approval_context_digest,
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
        materialize_checkpoint(session, checkpoint);
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
    use shacs_providers::{LlmResponse, ProviderEvent, ProviderRequest};
    use std::collections::{BTreeMap, VecDeque};
    use std::fs;

    struct CapturingProviderClient {
        responses: Mutex<VecDeque<LlmResponse>>,
        requests: Arc<Mutex<Vec<ProviderRequest>>>,
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

    fn provider_context_text(messages: &[Value]) -> &str {
        messages
            .iter()
            .find_map(|message| {
                let content = message.get("content").and_then(Value::as_str)?;
                content.contains("[Provider Context").then_some(content)
            })
            .unwrap_or_default()
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
}
