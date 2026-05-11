use crate::runtime::{
    AgentRunResult, AgentRunSpec, AgentRunner, CancellationToken, ContextBuilder, InboundMessage,
    MessageBus, RuntimeCapabilityReport, RuntimeCapabilityStatus, ToolEvent, ToolStatus,
};
use crate::tools::{
    EditFileTool, ExecConfig, ExecTool, FileState, GlobTool, GrepTool, ListDirTool, PathContext,
    ReadFileTool, SpawnRequest, SubagentSpawner, ToolRegistry, WebFetchTool, WebSearchTool,
    WriteFileTool,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shacs_providers::{GenerationSettings, ProviderClient, ProviderRetryMode};
use shacs_templates::{render_agent_template, template_variables, AgentTemplate};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_CHILD_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentState {
    SpawnRequested,
    Spawned,
    Running,
    AwaitingMerge,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildResultStatus {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnEnvelope {
    pub session_id: String,
    pub parent_turn_id: String,
    pub child_task_id: String,
    pub spawn_effect_id: String,
    pub subagent_kind: String,
    pub task_goal: String,
    pub task_scope: String,
    pub inherited_context_snapshot: Value,
    pub inherited_policy_snapshot: Value,
    pub inherited_safety_snapshot: Value,
    pub input_budget_snapshot: Value,
    pub output_budget_snapshot: Value,
    pub timeout_ms: Option<u64>,
    pub parallelism_group: String,
    pub issued_at_ms: u128,
    pub origin_channel: String,
    pub origin_chat_id: String,
    pub label: String,
}

impl SpawnEnvelope {
    pub fn new(
        parent_session_key: impl Into<String>,
        child_id: impl Into<String>,
        task: impl Into<String>,
    ) -> Self {
        let session_id = parent_session_key.into();
        let child_task_id = child_id.into();
        let task_goal = task.into();
        Self {
            parent_turn_id: "turn:unknown".to_owned(),
            spawn_effect_id: format!("spawn:{child_task_id}"),
            subagent_kind: "default".to_owned(),
            task_scope: task_goal.clone(),
            inherited_context_snapshot: Value::Null,
            inherited_policy_snapshot: Value::Null,
            inherited_safety_snapshot: json!({"inherits_parent_safety": true}),
            input_budget_snapshot: Value::Null,
            output_budget_snapshot: Value::Null,
            timeout_ms: None,
            parallelism_group: session_id.clone(),
            issued_at_ms: now_millis(),
            origin_channel: "system".to_owned(),
            origin_chat_id: session_id.clone(),
            label: default_label(&task_goal),
            session_id,
            child_task_id,
            task_goal,
        }
    }

    pub fn from_spawn_request(request: SpawnRequest, child_task_id: String) -> Self {
        let label = request
            .label
            .clone()
            .unwrap_or_else(|| default_label(&request.task));
        let session_id = request.session_key;
        Self {
            parent_turn_id: format!("turn:{session_id}"),
            spawn_effect_id: format!("spawn:{child_task_id}"),
            subagent_kind: "default".to_owned(),
            task_scope: request.task.clone(),
            inherited_context_snapshot: json!({
                "origin_channel": request.origin_channel,
                "origin_chat_id": request.origin_chat_id,
            }),
            inherited_policy_snapshot: json!({"capability_ceiling": "parent"}),
            inherited_safety_snapshot: json!({"inherits_parent_safety": true}),
            input_budget_snapshot: Value::Null,
            output_budget_snapshot: Value::Null,
            timeout_ms: None,
            parallelism_group: session_id.clone(),
            issued_at_ms: now_millis(),
            origin_channel: request.origin_channel,
            origin_chat_id: request.origin_chat_id,
            label,
            session_id,
            child_task_id,
            task_goal: request.task,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChildResultEnvelope {
    pub session_id: String,
    pub parent_turn_id: String,
    pub child_task_id: String,
    pub spawn_effect_id: String,
    pub subagent_kind: String,
    pub status: ChildResultStatus,
    pub started_at_ms: u128,
    pub finished_at_ms: u128,
    pub duration_ms: u128,
    pub summary: String,
    pub structured_result: Option<Value>,
    pub error: Option<String>,
    pub observations: Option<Value>,
    pub budget_usage: Option<Value>,
}

impl ChildResultEnvelope {
    pub fn new(
        parent_session_key: impl Into<String>,
        child_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let session_id = parent_session_key.into();
        let child_task_id = child_id.into();
        let summary = content.into();
        let finished_at_ms = now_millis();
        Self {
            parent_turn_id: "turn:unknown".to_owned(),
            spawn_effect_id: format!("spawn:{child_task_id}"),
            subagent_kind: "default".to_owned(),
            status: ChildResultStatus::Completed,
            started_at_ms: finished_at_ms,
            finished_at_ms,
            duration_ms: 0,
            summary,
            structured_result: None,
            error: None,
            observations: None,
            budget_usage: None,
            session_id,
            child_task_id,
        }
    }

    pub fn from_spawn(
        spawn: &SpawnEnvelope,
        status: ChildResultStatus,
        summary: impl Into<String>,
    ) -> Self {
        let finished_at_ms = now_millis();
        Self {
            session_id: spawn.session_id.clone(),
            parent_turn_id: spawn.parent_turn_id.clone(),
            child_task_id: spawn.child_task_id.clone(),
            spawn_effect_id: spawn.spawn_effect_id.clone(),
            subagent_kind: spawn.subagent_kind.clone(),
            status,
            started_at_ms: spawn.issued_at_ms,
            finished_at_ms,
            duration_ms: finished_at_ms.saturating_sub(spawn.issued_at_ms),
            summary: summary.into(),
            structured_result: None,
            error: None,
            observations: None,
            budget_usage: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeDecision {
    AcceptFull,
    AcceptSummaryOnly,
    AcceptFailureFact,
    RetryChild,
    DiscardAsStale { reason: String },
    AbortParentTurn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentStatus {
    pub task_id: String,
    pub label: String,
    pub task_description: String,
    pub started_at_ms: u128,
    pub state: SubagentState,
    pub iteration: u32,
    pub tool_events: Vec<Value>,
    pub usage: Value,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentProgressUpdate {
    pub phase: String,
    pub iteration: u32,
    pub tool_events: Vec<Value>,
    pub usage: Value,
    pub error: Option<String>,
}

impl SubagentStatus {
    fn from_spawn(spawn: &SpawnEnvelope) -> Self {
        Self {
            task_id: spawn.child_task_id.clone(),
            label: spawn.label.clone(),
            task_description: spawn.task_goal.clone(),
            started_at_ms: spawn.issued_at_ms,
            state: SubagentState::Spawned,
            iteration: 0,
            tool_events: Vec::new(),
            usage: Value::Null,
            stop_reason: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRuntimeConfig {
    pub max_parallelism: usize,
}

impl Default for SubagentRuntimeConfig {
    fn default() -> Self {
        Self { max_parallelism: 4 }
    }
}

#[derive(Debug, Clone)]
pub struct SubagentExecutionConfig {
    pub workspace: PathBuf,
    pub model: String,
    pub settings: GenerationSettings,
    pub retry_mode: ProviderRetryMode,
    pub max_iterations: usize,
    pub max_tool_result_chars: usize,
    pub fail_on_tool_error: bool,
    pub allow_side_effect_tools: bool,
    pub enable_exec: bool,
    pub enable_web: bool,
    pub restrict_to_workspace: bool,
    pub exec_timeout_seconds: u64,
    pub exec_sandbox: Option<String>,
    pub exec_path_append: Option<String>,
    pub exec_allowed_env_keys: Vec<String>,
    pub exec_env: BTreeMap<String, String>,
}

impl SubagentExecutionConfig {
    pub fn new(workspace: impl Into<PathBuf>, model: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            model: model.into(),
            settings: GenerationSettings::default(),
            retry_mode: ProviderRetryMode::Standard,
            max_iterations: 200,
            max_tool_result_chars: 20_000,
            fail_on_tool_error: true,
            allow_side_effect_tools: false,
            enable_exec: false,
            enable_web: false,
            restrict_to_workspace: true,
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagentSpawnOutcome {
    pub envelope: SpawnEnvelope,
    pub user_message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticSubagentCommand {
    SubagentCompleted,
    SubagentFailed,
    SubagentTimedOut,
    SubagentCancelled,
    SubagentProgressObserved,
}

#[derive(Debug, Clone)]
struct SubagentTaskRecord {
    envelope: SpawnEnvelope,
    status: SubagentStatus,
    cancelled: bool,
    cancellation_token: CancellationToken,
}

#[derive(Debug, Default)]
struct SubagentRuntimeState {
    tasks: BTreeMap<String, SubagentTaskRecord>,
    session_tasks: BTreeMap<String, BTreeSet<String>>,
    adopted_scope: BTreeMap<(String, String, String), String>,
}

#[derive(Clone)]
pub struct SubagentRuntime {
    state: Arc<Mutex<SubagentRuntimeState>>,
    bus: Option<MessageBus>,
    config: SubagentRuntimeConfig,
}

impl Default for SubagentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SubagentRuntime {
    pub fn new() -> Self {
        Self::with_config(SubagentRuntimeConfig::default())
    }

    pub fn with_bus(bus: MessageBus) -> Self {
        Self::with_config(SubagentRuntimeConfig::default()).attach_bus(bus)
    }

    pub fn with_config(config: SubagentRuntimeConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(SubagentRuntimeState::default())),
            bus: None,
            config,
        }
    }

    pub fn attach_bus(mut self, bus: MessageBus) -> Self {
        self.bus = Some(bus);
        self
    }

    pub fn status(&self) -> RuntimeCapabilityReport {
        RuntimeCapabilityReport {
            component: "subagent_runtime".to_owned(),
            status: RuntimeCapabilityStatus::Available,
            reason: "local subagent lifecycle, correlation, cancellation, and synthetic reentry contracts are available".to_owned(),
        }
    }

    pub fn spawn_from_request(
        &self,
        request: SpawnRequest,
    ) -> Result<SubagentSpawnOutcome, String> {
        let child_task_id = next_child_id();
        let envelope = SpawnEnvelope::from_spawn_request(request, child_task_id);
        self.register_spawn(envelope)
    }

    pub fn register_spawn(&self, envelope: SpawnEnvelope) -> Result<SubagentSpawnOutcome, String> {
        let mut state = recover_lock(&self.state);
        let active_count = state
            .session_tasks
            .get(&envelope.session_id)
            .map(BTreeSet::len)
            .unwrap_or(0);
        if active_count >= self.config.max_parallelism {
            return Err(format!(
                "subagent parallelism limit reached for {}: {}",
                envelope.session_id, self.config.max_parallelism
            ));
        }
        if state.tasks.contains_key(&envelope.child_task_id) {
            return Err(format!(
                "subagent task already exists: {}",
                envelope.child_task_id
            ));
        }
        let status = SubagentStatus::from_spawn(&envelope);
        state
            .session_tasks
            .entry(envelope.session_id.clone())
            .or_default()
            .insert(envelope.child_task_id.clone());
        state.tasks.insert(
            envelope.child_task_id.clone(),
            SubagentTaskRecord {
                envelope: envelope.clone(),
                status,
                cancelled: false,
                cancellation_token: CancellationToken::new(),
            },
        );
        let user_message = format!(
            "Subagent [{}] started (id: {}). I'll notify you when it completes.",
            envelope.label, envelope.child_task_id
        );
        Ok(SubagentSpawnOutcome {
            envelope,
            user_message,
        })
    }

    pub fn mark_running(&self, child_task_id: &str) -> Option<SubagentStatus> {
        self.update_state(child_task_id, SubagentState::Running)
    }

    pub fn cancel_by_session(&self, session_id: &str) -> usize {
        let mut state = recover_lock(&self.state);
        let task_ids = state
            .session_tasks
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let mut cancelled = 0;
        for task_id in task_ids {
            if let Some(record) = state.tasks.get_mut(&task_id) {
                if !record.cancelled {
                    record.cancelled = true;
                    record.cancellation_token.cancel();
                    record.status.state = SubagentState::Cancelled;
                    cancelled += 1;
                }
            }
        }
        cancelled
    }

    pub fn running_count(&self) -> usize {
        recover_lock(&self.state).tasks.len()
    }

    pub fn running_count_by_session(&self, session_id: &str) -> usize {
        recover_lock(&self.state)
            .session_tasks
            .get(session_id)
            .map(BTreeSet::len)
            .unwrap_or(0)
    }

    pub fn snapshot(&self, child_task_id: &str) -> Option<SubagentStatus> {
        recover_lock(&self.state)
            .tasks
            .get(child_task_id)
            .map(|record| record.status.clone())
    }

    pub fn update_progress(
        &self,
        child_task_id: &str,
        update: SubagentProgressUpdate,
    ) -> Option<SubagentStatus> {
        let mut state = recover_lock(&self.state);
        let record = state.tasks.get_mut(child_task_id)?;
        record.status.state = subagent_state_from_phase(&update.phase);
        record.status.iteration = update.iteration;
        record.status.tool_events = update.tool_events;
        record.status.usage = update.usage;
        record.status.error = update.error;
        Some(record.status.clone())
    }

    pub fn classify_result(
        &self,
        expected: &SpawnEnvelope,
        result: &ChildResultEnvelope,
    ) -> MergeDecision {
        correlation_decision(expected, result).unwrap_or_else(|| merge_decision_for_status(result))
    }

    pub fn classify_active_result(&self, result: &ChildResultEnvelope) -> MergeDecision {
        let state = recover_lock(&self.state);
        let Some(record) = state.tasks.get(&result.child_task_id) else {
            return MergeDecision::DiscardAsStale {
                reason: format!("child task is not active: {}", result.child_task_id),
            };
        };
        if let Some(decision) = correlation_decision(&record.envelope, result) {
            return decision;
        }
        let scope_key = scope_key(&record.envelope);
        if let Some(adopted_child) = state.adopted_scope.get(&scope_key) {
            if adopted_child != &result.child_task_id {
                return MergeDecision::DiscardAsStale {
                    reason: format!("scope already adopted by newer child: {adopted_child}"),
                };
            }
        }
        merge_decision_for_status(result)
    }

    pub fn finish_child(
        &self,
        result: ChildResultEnvelope,
    ) -> (MergeDecision, Option<InboundMessage>) {
        let decision = self.classify_active_result(&result);
        let message = should_publish_decision(&decision)
            .then(|| self.synthetic_inbound_for_result(&result, &decision));
        self.apply_finish(&result, &decision);
        (decision, message)
    }

    pub fn publish_child_result(&self, result: ChildResultEnvelope) -> MergeDecision {
        let (decision, message) = self.finish_child(result);
        if let (Some(bus), Some(message)) = (&self.bus, message) {
            bus.publish_inbound(message);
        }
        decision
    }

    pub fn run_spawn(
        &self,
        envelope: SpawnEnvelope,
        client: &dyn ProviderClient,
        config: SubagentExecutionConfig,
    ) -> ChildResultEnvelope {
        self.mark_running(&envelope.child_task_id);
        let cancellation_token = self
            .cancellation_token(&envelope.child_task_id)
            .unwrap_or_default();
        let result = if cancellation_token.is_cancelled() {
            ChildResultEnvelope::from_spawn(
                &envelope,
                ChildResultStatus::Cancelled,
                "Subagent cancelled before it started.",
            )
        } else {
            self.execute_spawn(&envelope, client, &config, cancellation_token)
        };
        self.publish_child_result(result.clone());
        result
    }

    pub fn spawn_and_run_background(
        &self,
        request: SpawnRequest,
        client: Arc<dyn ProviderClient>,
        config: SubagentExecutionConfig,
    ) -> Result<SubagentSpawnOutcome, String> {
        let outcome = self.spawn_from_request(request)?;
        let runtime = self.clone();
        let envelope = outcome.envelope.clone();
        thread::spawn(move || {
            runtime.run_spawn(envelope, client.as_ref(), config);
        });
        Ok(outcome)
    }

    fn execute_spawn(
        &self,
        envelope: &SpawnEnvelope,
        client: &dyn ProviderClient,
        config: &SubagentExecutionConfig,
        cancellation_token: CancellationToken,
    ) -> ChildResultEnvelope {
        let registry = build_subagent_tool_registry(config);
        let system_prompt = ContextBuilder::new(&config.workspace)
            .with_configured_env(config.exec_env.clone())
            .build_subagent_prompt();
        let messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": envelope.task_goal}),
        ];
        let mut spec = AgentRunSpec::new(messages, &registry, client, config.model.clone());
        spec.settings = config.settings.clone();
        spec.retry_mode = config.retry_mode;
        spec.max_iterations = config.max_iterations;
        spec.max_tool_result_chars = config.max_tool_result_chars;
        spec.max_iterations_message =
            Some("Task completed but no final response was generated.".to_owned());
        spec.fail_on_tool_error = config.fail_on_tool_error;
        spec.cancellation_token = Some(cancellation_token.clone());
        let progress_runtime = self.clone();
        let progress_child = envelope.child_task_id.clone();
        spec.tool_event_callback = Some(Arc::new(move |event| {
            let events = vec![serde_json::to_value(event).unwrap_or(Value::Null)];
            let _ = progress_runtime.update_progress(
                &progress_child,
                SubagentProgressUpdate {
                    phase: "awaiting_tools".to_owned(),
                    iteration: 0,
                    tool_events: events,
                    usage: Value::Null,
                    error: None,
                },
            );
        }));
        match AgentRunner::new().run(spec) {
            Ok(result) => self.child_result_from_agent_run(envelope, result),
            Err(error) => {
                let mut result = ChildResultEnvelope::from_spawn(
                    envelope,
                    ChildResultStatus::Failed,
                    format!("Error: {error}"),
                );
                result.error = Some(error.to_string());
                result
            }
        }
    }

    fn child_result_from_agent_run(
        &self,
        envelope: &SpawnEnvelope,
        result: AgentRunResult,
    ) -> ChildResultEnvelope {
        let status = match result.stop_reason.as_str() {
            "cancelled" => ChildResultStatus::Cancelled,
            "tool_error" | "error" => ChildResultStatus::Failed,
            _ => ChildResultStatus::Completed,
        };
        let summary = if result.stop_reason == "tool_error" {
            format_partial_progress(&result)
        } else {
            result
                .final_content
                .clone()
                .unwrap_or_else(|| "Task completed but no final response was generated.".to_owned())
        };
        let mut envelope_result = ChildResultEnvelope::from_spawn(envelope, status, summary);
        envelope_result.error = result.error.clone();
        envelope_result.budget_usage = Some(json!(result.usage));
        let tool_events = result
            .tool_events
            .iter()
            .map(|event| serde_json::to_value(event).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        let _ = self.update_progress(
            &envelope.child_task_id,
            SubagentProgressUpdate {
                phase: "done".to_owned(),
                iteration: 0,
                tool_events,
                usage: envelope_result.budget_usage.clone().unwrap_or(Value::Null),
                error: result.error,
            },
        );
        envelope_result
    }

    fn cancellation_token(&self, child_task_id: &str) -> Option<CancellationToken> {
        recover_lock(&self.state)
            .tasks
            .get(child_task_id)
            .map(|record| record.cancellation_token.clone())
    }

    pub fn synthetic_inbound_for_result(
        &self,
        result: &ChildResultEnvelope,
        decision: &MergeDecision,
    ) -> InboundMessage {
        let command = synthetic_command_for(result, decision);
        let (label, task) = self.announcement_context(result);
        let mut message = InboundMessage::new(
            "system",
            "subagent",
            result.session_id.clone(),
            format_subagent_announcement(result, decision, &label, &task),
        );
        message.session_key_override = Some(result.session_id.clone());
        message.metadata.insert(
            "injected_event".to_owned(),
            Value::String("subagent_result".to_owned()),
        );
        message.metadata.insert(
            "subagent_task_id".to_owned(),
            Value::String(result.child_task_id.clone()),
        );
        message.metadata.insert(
            "parent_turn_id".to_owned(),
            Value::String(result.parent_turn_id.clone()),
        );
        message.metadata.insert(
            "spawn_effect_id".to_owned(),
            Value::String(result.spawn_effect_id.clone()),
        );
        message.metadata.insert(
            "subagent_command".to_owned(),
            serde_json::to_value(command).unwrap_or(Value::String("subagent_failed".to_owned())),
        );
        message
    }

    fn announcement_context(&self, result: &ChildResultEnvelope) -> (String, String) {
        recover_lock(&self.state)
            .tasks
            .get(&result.child_task_id)
            .map(|record| {
                (
                    record.envelope.label.clone(),
                    record.envelope.task_goal.clone(),
                )
            })
            .unwrap_or_else(|| (result.child_task_id.clone(), result.child_task_id.clone()))
    }

    fn update_state(&self, child_task_id: &str, next: SubagentState) -> Option<SubagentStatus> {
        let mut state = recover_lock(&self.state);
        let record = state.tasks.get_mut(child_task_id)?;
        record.status.state = next;
        Some(record.status.clone())
    }

    fn apply_finish(&self, result: &ChildResultEnvelope, decision: &MergeDecision) {
        let mut state = recover_lock(&self.state);
        let Some(record) = state.tasks.get(&result.child_task_id) else {
            return;
        };
        if correlation_decision(&record.envelope, result).is_some() {
            return;
        };
        let Some(mut record) = state.tasks.remove(&result.child_task_id) else {
            return;
        };
        if let Some(ids) = state.session_tasks.get_mut(&record.envelope.session_id) {
            ids.remove(&result.child_task_id);
            if ids.is_empty() {
                state.session_tasks.remove(&record.envelope.session_id);
            }
        }
        record.status.state = match decision {
            MergeDecision::DiscardAsStale { .. } => SubagentState::Stale,
            MergeDecision::AcceptFailureFact | MergeDecision::AbortParentTurn => {
                SubagentState::Failed
            }
            MergeDecision::RetryChild => SubagentState::TimedOut,
            MergeDecision::AcceptFull | MergeDecision::AcceptSummaryOnly => {
                SubagentState::Completed
            }
        };
        record.status.stop_reason = Some(format!("{:?}", result.status));
        record.status.error = result.error.clone();
        if matches!(
            decision,
            MergeDecision::AcceptFull | MergeDecision::AcceptSummaryOnly
        ) {
            state
                .adopted_scope
                .insert(scope_key(&record.envelope), result.child_task_id.clone());
        }
    }
}

pub fn build_subagent_tool_registry(config: &SubagentExecutionConfig) -> ToolRegistry {
    let allowed_dir = config
        .restrict_to_workspace
        .then(|| config.workspace.clone());
    let path_context = PathContext {
        workspace: Some(config.workspace.clone()),
        allowed_dir,
        media_dir: None,
        extra_allowed_dirs: Vec::new(),
    };
    let file_state = Arc::new(Mutex::new(FileState::new()));
    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool::with_file_state(
        path_context.clone(),
        file_state.clone(),
    ));
    if config.allow_side_effect_tools {
        registry.register(WriteFileTool::with_file_state(
            path_context.clone(),
            file_state.clone(),
        ));
        registry.register(EditFileTool::with_file_state(
            path_context.clone(),
            file_state,
        ));
    }
    registry.register(ListDirTool::new(path_context.clone()));
    registry.register(GlobTool::new(path_context.clone()));
    registry.register(GrepTool::new(path_context.clone()));
    if config.allow_side_effect_tools && config.enable_exec {
        let mut exec_config = ExecConfig::new(path_context.clone());
        exec_config.timeout_seconds = config.exec_timeout_seconds;
        exec_config.restrict_to_workspace = config.restrict_to_workspace;
        exec_config.sandbox = config.exec_sandbox.clone();
        exec_config.path_append = config.exec_path_append.clone();
        exec_config.allowed_env_keys = config.exec_allowed_env_keys.clone();
        exec_config.env = config.exec_env.clone();
        registry.register(ExecTool::new(exec_config));
    }
    if config.enable_web {
        registry.register(WebFetchTool::default());
        registry.register(WebSearchTool::default());
    }
    registry
}

impl SubagentSpawner for SubagentRuntime {
    fn spawn(&self, request: SpawnRequest) -> Result<String, String> {
        self.spawn_from_request(request)
            .map(|outcome| outcome.user_message)
    }
}

fn correlation_decision(
    expected: &SpawnEnvelope,
    result: &ChildResultEnvelope,
) -> Option<MergeDecision> {
    if expected.child_task_id != result.child_task_id {
        return Some(MergeDecision::DiscardAsStale {
            reason: format!(
                "child id mismatch: expected {}, got {}",
                expected.child_task_id, result.child_task_id
            ),
        });
    }
    if expected.session_id != result.session_id {
        return Some(MergeDecision::DiscardAsStale {
            reason: format!(
                "parent session mismatch: expected {}, got {}",
                expected.session_id, result.session_id
            ),
        });
    }
    if expected.parent_turn_id != result.parent_turn_id {
        return Some(MergeDecision::DiscardAsStale {
            reason: format!(
                "parent turn mismatch: expected {}, got {}",
                expected.parent_turn_id, result.parent_turn_id
            ),
        });
    }
    if expected.spawn_effect_id != result.spawn_effect_id {
        return Some(MergeDecision::DiscardAsStale {
            reason: format!(
                "spawn effect mismatch: expected {}, got {}",
                expected.spawn_effect_id, result.spawn_effect_id
            ),
        });
    }
    None
}

fn synthetic_command_for(
    result: &ChildResultEnvelope,
    decision: &MergeDecision,
) -> SyntheticSubagentCommand {
    if matches!(decision, MergeDecision::DiscardAsStale { .. }) {
        return SyntheticSubagentCommand::SubagentProgressObserved;
    }
    match result.status {
        ChildResultStatus::Completed => SyntheticSubagentCommand::SubagentCompleted,
        ChildResultStatus::Failed => SyntheticSubagentCommand::SubagentFailed,
        ChildResultStatus::TimedOut => SyntheticSubagentCommand::SubagentTimedOut,
        ChildResultStatus::Cancelled => SyntheticSubagentCommand::SubagentCancelled,
    }
}

fn should_publish_decision(decision: &MergeDecision) -> bool {
    !matches!(decision, MergeDecision::DiscardAsStale { .. })
}

fn merge_decision_for_status(result: &ChildResultEnvelope) -> MergeDecision {
    match result.status {
        ChildResultStatus::Completed => MergeDecision::AcceptSummaryOnly,
        ChildResultStatus::Failed => MergeDecision::AcceptFailureFact,
        ChildResultStatus::Cancelled => MergeDecision::DiscardAsStale {
            reason: "cancelled child result is observational only".to_owned(),
        },
        ChildResultStatus::TimedOut => MergeDecision::RetryChild,
    }
}

pub fn format_partial_progress(result: &AgentRunResult) -> String {
    let completed = result
        .tool_events
        .iter()
        .filter(|event| event.status == ToolStatus::Ok)
        .collect::<Vec<_>>();
    let failure = result
        .tool_events
        .iter()
        .rev()
        .find(|event| event.status == ToolStatus::Error);
    format_partial_progress_from_events(&completed, failure, result.error.as_deref())
}

pub fn format_partial_progress_from_tool_events(
    tool_events: &[ToolEvent],
    error: Option<&str>,
) -> String {
    let completed = tool_events
        .iter()
        .filter(|event| event.status == ToolStatus::Ok)
        .collect::<Vec<_>>();
    let failure = tool_events
        .iter()
        .rev()
        .find(|event| event.status == ToolStatus::Error);
    format_partial_progress_from_events(&completed, failure, error)
}

fn format_partial_progress_from_events(
    completed: &[&ToolEvent],
    failure: Option<&ToolEvent>,
    error: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    if !completed.is_empty() {
        lines.push("Completed steps:".to_owned());
        for event in completed.iter().rev().take(3).rev() {
            lines.push(format!("- {}: {}", event.name, event.detail));
        }
    }
    if let Some(failure) = failure {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Failure:".to_owned());
        lines.push(format!("- {}: {}", failure.name, failure.detail));
    } else if let Some(error) = error.filter(|value| !value.is_empty()) {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("Failure:".to_owned());
        lines.push(format!("- {error}"));
    }
    lines
        .join("\n")
        .if_empty_then("Error: subagent execution failed.")
}

fn format_subagent_announcement(
    result: &ChildResultEnvelope,
    _decision: &MergeDecision,
    label: &str,
    task: &str,
) -> String {
    let status_text = match result.status {
        ChildResultStatus::Completed => "completed successfully",
        ChildResultStatus::Failed => "failed",
        ChildResultStatus::Cancelled => "was cancelled",
        ChildResultStatus::TimedOut => "timed out",
    };
    render_agent_template(
        AgentTemplate::SubagentAnnounce,
        &template_variables(&[
            ("label", label),
            ("status_text", status_text),
            ("task", task),
            ("result", &result.summary),
        ]),
    )
    .unwrap_or_else(|_| {
        format!(
            "Subagent [{label}] {status_text}.\n\nTask: {task}\n\nResult:\n{}",
            result.summary
        )
    })
}

fn subagent_state_from_phase(phase: &str) -> SubagentState {
    match phase {
        "awaiting_tools" | "tools_completed" | "initializing" => SubagentState::Running,
        "final_response" | "done" => SubagentState::Completed,
        "error" => SubagentState::Failed,
        _ => SubagentState::Running,
    }
}

trait EmptyStringFallback {
    fn if_empty_then(self, fallback: &str) -> String;
}

impl EmptyStringFallback for String {
    fn if_empty_then(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_owned()
        } else {
            self
        }
    }
}

fn scope_key(envelope: &SpawnEnvelope) -> (String, String, String) {
    (
        envelope.session_id.clone(),
        envelope.parent_turn_id.clone(),
        envelope.task_scope.clone(),
    )
}

fn default_label(task: &str) -> String {
    let mut label = task.chars().take(30).collect::<String>();
    if task.chars().count() > 30 {
        label.push_str("...");
    }
    label
}

fn next_child_id() -> String {
    let id = NEXT_CHILD_ID.fetch_add(1, Ordering::SeqCst);
    format!("child-{id:08}")
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn recover_lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
