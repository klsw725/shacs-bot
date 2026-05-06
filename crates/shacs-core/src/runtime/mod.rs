mod agent_loop;
mod autocompact;
mod context;
mod lifecycle;
mod loop_control;
mod memory;
mod runner;
mod subagent;
mod tool_execution;

pub use agent_loop::{
    AgentLoop, AgentLoopCommandResult, AgentLoopConfig, AgentLoopError, AgentLoopOutcome,
    AgentLoopRunSummary, AgentLoopTurnResult,
};
pub use autocompact::{AutoCompact, AutoCompactArchiveOutcome, RECENT_SUFFIX_MESSAGES};
pub use context::{add_assistant_message, add_tool_result, ContextBuildRequest, ContextBuilder};
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
pub use runner::{
    AgentHook, AgentHookContext, AgentRunResult, AgentRunSpec, AgentRunner, CompositeHook,
    MidTurnInjectionCallback, NoopAgentHook, ProviderEventCallback, RetryWaitCallback, ToolEvent,
    ToolEventCallback, ToolStatus,
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

pub type Dream<'a> = DreamProcessor<'a>;
pub type SkillsLoader = ContextBuilder;
pub type SubagentManager = SubagentRuntime;
