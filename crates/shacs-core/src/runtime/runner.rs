use crate::runtime::tool_search::{
    BridgeUnderlyingMappingEvidence, ToolDescribeEvidence, ToolSearchDiagnosticsSummary,
    ToolSearchQueryEvidence,
};
use crate::runtime::{
    dispatch_bridge_tool_calls, CancellationToken, ResolvedDeferredToolCall,
    RuntimeAssistantToolCallMessage, RuntimeContextTools, RuntimeInterrupt, RuntimeToolCall,
    RuntimeToolExecutionReport, RuntimeToolExecutor, RuntimeToolMessage, ToolExecutionContext,
};
use crate::tools::{
    assemble_tool_surface, bridge_tool_names, DeferredToolCatalog, ToolRegistry,
    ToolSurfaceAssemblyInput,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shacs_providers::{
    chat_stream_with_retry_using_waiter, chat_with_retry_using_waiter, GenerationSettings,
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ProviderRetryMode,
    ProviderRetryWaiter, ThreadRetryWaiter,
};
use shacs_utils::tool_results::maybe_persist_text_tool_result;
use std::collections::{BTreeMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;

const DEFAULT_ERROR_MESSAGE: &str = "Sorry, I encountered an error calling the AI model.";
const MODEL_ERROR_PLACEHOLDER: &str = "[Assistant reply unavailable due to model error.]";
const EMPTY_FINAL_RESPONSE_MESSAGE: &str = "I completed the tool steps but couldn't produce a final answer. Please try again or narrow the task.";
const FINALIZATION_RETRY_PROMPT: &str =
    "Please provide your response to the user based on the conversation above.";
const LENGTH_RECOVERY_PROMPT: &str = "Output limit reached. Continue exactly where you left off — no recap, no apology. Break remaining work into smaller steps if needed.";
const MAX_EMPTY_RETRIES: usize = 2;
const MAX_LENGTH_RECOVERIES: usize = 3;
const MAX_INJECTION_CYCLES: usize = 5;
const MICROCOMPACT_KEEP_RECENT: usize = 10;
const MICROCOMPACT_MIN_CHARS: usize = 500;
const SNIP_SAFETY_BUFFER: usize = 1024;
const MAX_REPEAT_EXTERNAL_LOOKUPS: usize = 2;
const MAX_TOOL_SEARCH_EVIDENCE_MATCHES: usize = 5;
const MAX_TOOL_SEARCH_QUERY_CHARS: usize = 120;
const BACKFILL_CONTENT: &str = "[Tool result unavailable — call was interrupted or lost]";
const FATAL_SKIP_CONTENT: &str = "[Tool call skipped because a fatal tool error stopped the turn]";
const COMPACTABLE_TOOLS: [&str; 7] = [
    "read_file",
    "exec",
    "grep",
    "glob",
    "web_search",
    "web_fetch",
    "list_dir",
];

pub type ToolEventCallback = Arc<dyn Fn(&ToolEvent) + Send + Sync>;
pub type ProviderEventCallback = Arc<dyn Fn(&ProviderEvent) + Send + Sync>;
pub type CheckpointCallback = Arc<dyn Fn(&Value) + Send + Sync>;
pub type MidTurnInjectionCallback = Arc<dyn Fn() -> Vec<Value> + Send + Sync>;
pub type RetryWaitCallback = Arc<dyn Fn(f64, &str) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct AgentHookContext {
    pub iteration: usize,
    pub messages: Vec<Value>,
}

pub trait AgentHook: Send + Sync {
    fn wants_streaming(&self) -> bool {
        false
    }

    fn before_iteration(&self, _context: &AgentHookContext) {}

    fn on_stream(&self, _context: &AgentHookContext, _text: &str) {}

    fn on_stream_end(&self, _context: &AgentHookContext, _resuming: bool) {}

    fn before_execute_tools(&self, _context: &AgentHookContext, _calls: &[RuntimeToolCall]) {}

    fn after_response(&self, _context: &AgentHookContext, _response: &LlmResponse) {}

    fn after_iteration(&self, _context: &AgentHookContext) {}

    fn finalize_content(&self, _context: &AgentHookContext, content: String) -> String {
        content
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAgentHook;

impl AgentHook for NoopAgentHook {}

#[derive(Default, Clone)]
pub struct CompositeHook {
    hooks: Vec<Arc<dyn AgentHook>>,
}

impl CompositeHook {
    pub fn new(hooks: Vec<Arc<dyn AgentHook>>) -> Self {
        Self { hooks }
    }

    pub fn push(&mut self, hook: Arc<dyn AgentHook>) {
        self.hooks.push(hook);
    }
}

impl AgentHook for CompositeHook {
    fn wants_streaming(&self) -> bool {
        self.hooks.iter().any(|hook| hook.wants_streaming())
    }

    fn before_iteration(&self, context: &AgentHookContext) {
        for hook in &self.hooks {
            invoke_hook_lifecycle(|| hook.before_iteration(context));
        }
    }

    fn on_stream(&self, context: &AgentHookContext, text: &str) {
        for hook in &self.hooks {
            invoke_hook_lifecycle(|| hook.on_stream(context, text));
        }
    }

    fn on_stream_end(&self, context: &AgentHookContext, resuming: bool) {
        for hook in &self.hooks {
            invoke_hook_lifecycle(|| hook.on_stream_end(context, resuming));
        }
    }

    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        for hook in &self.hooks {
            invoke_hook_lifecycle(|| hook.before_execute_tools(context, calls));
        }
    }

    fn after_response(&self, context: &AgentHookContext, response: &LlmResponse) {
        for hook in &self.hooks {
            invoke_hook_lifecycle(|| hook.after_response(context, response));
        }
    }

    fn after_iteration(&self, context: &AgentHookContext) {
        for hook in &self.hooks {
            invoke_hook_lifecycle(|| hook.after_iteration(context));
        }
    }

    fn finalize_content(&self, context: &AgentHookContext, mut content: String) -> String {
        for hook in &self.hooks {
            content = invoke_hook_finalize(hook.as_ref(), context, content);
        }
        content
    }
}

#[derive(Clone)]
pub struct AgentRunSpec<'a> {
    pub initial_messages: Vec<Value>,
    pub tools: &'a ToolRegistry,
    pub client: &'a dyn ProviderClient,
    pub model: String,
    pub settings: GenerationSettings,
    pub max_iterations: usize,
    pub max_iterations_message: Option<String>,
    pub retry_mode: ProviderRetryMode,
    pub tool_context: ToolExecutionContext,
    pub max_tool_result_chars: usize,
    pub error_message: Option<String>,
    pub concurrent_tools: bool,
    pub fail_on_tool_error: bool,
    pub workspace: Option<PathBuf>,
    pub session_key: Option<String>,
    pub tool_search: ToolSearchConfig,
    pub context_window_tokens: Option<usize>,
    pub context_block_limit: Option<usize>,
    pub tool_event_callback: Option<ToolEventCallback>,
    pub provider_event_callback: Option<ProviderEventCallback>,
    pub retry_wait_callback: Option<RetryWaitCallback>,
    pub checkpoint_callback: Option<CheckpointCallback>,
    pub mid_turn_injection_callback: Option<MidTurnInjectionCallback>,
    pub agent_hook: Option<Arc<dyn AgentHook>>,
    pub context_tools: RuntimeContextTools,
    pub cancellation_token: Option<CancellationToken>,
}

impl<'a> AgentRunSpec<'a> {
    pub fn new(
        initial_messages: Vec<Value>,
        tools: &'a ToolRegistry,
        client: &'a dyn ProviderClient,
        model: impl Into<String>,
    ) -> Self {
        Self {
            initial_messages,
            tools,
            client,
            model: model.into(),
            settings: GenerationSettings::default(),
            max_iterations: 200,
            max_iterations_message: None,
            retry_mode: ProviderRetryMode::Standard,
            tool_context: ToolExecutionContext::default(),
            max_tool_result_chars: 20_000,
            error_message: Some(DEFAULT_ERROR_MESSAGE.to_owned()),
            concurrent_tools: false,
            fail_on_tool_error: false,
            workspace: None,
            session_key: None,
            tool_search: ToolSearchConfig::default(),
            context_window_tokens: None,
            context_block_limit: None,
            tool_event_callback: None,
            provider_event_callback: None,
            retry_wait_callback: None,
            checkpoint_callback: None,
            mid_turn_injection_callback: None,
            agent_hook: None,
            context_tools: RuntimeContextTools::new(),
            cancellation_token: None,
        }
    }

    pub fn tool_search_runtime_input(&self) -> ToolSearchRuntimeInput {
        ToolSearchRuntimeInput {
            config: self.tool_search,
            context_window_tokens: self.context_window_tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSearchMode {
    Off,
    On,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSearchConfig {
    pub enabled: ToolSearchMode,
    pub threshold_pct: u8,
    pub search_default_limit: usize,
    pub max_search_limit: usize,
}

impl Default for ToolSearchConfig {
    fn default() -> Self {
        Self {
            enabled: ToolSearchMode::Auto,
            threshold_pct: 10,
            search_default_limit: 5,
            max_search_limit: 20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSearchRuntimeInput {
    pub config: ToolSearchConfig,
    pub context_window_tokens: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Ok,
    Error,
    Waiting,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolEvent {
    pub name: String,
    pub status: ToolStatus,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub final_content: Option<String>,
    pub messages: Vec<Value>,
    pub tools_used: Vec<String>,
    pub usage: BTreeMap<String, u64>,
    pub stop_reason: String,
    pub error: Option<String>,
    pub error_message: Option<String>,
    pub interrupt: Option<RuntimeInterrupt>,
    pub tool_events: Vec<ToolEvent>,
    pub had_injections: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AgentRunner;

impl AgentRunner {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&self, spec: AgentRunSpec<'_>) -> Result<AgentRunResult, ProviderError> {
        let mut messages = spec.initial_messages.clone();
        let mut tools_used = Vec::new();
        let mut usage = BTreeMap::new();
        let executor =
            RuntimeToolExecutor::with_context_tools(spec.tools, spec.context_tools.clone());
        let mut tool_events = Vec::new();
        let mut external_lookup_counts = BTreeMap::new();
        let mut empty_content_retries = 0;
        let mut length_recoveries = 0;
        let mut had_injections = false;
        let mut injection_cycles = 0usize;
        start_runtime_turn(&spec.context_tools);

        for iteration in 0..spec.max_iterations {
            if cancellation_requested(&spec) {
                return Ok(cancelled_run_result(
                    messages,
                    tools_used,
                    usage,
                    tool_events,
                    had_injections,
                ));
            }
            append_mid_turn_injections(
                &spec,
                &mut messages,
                &mut had_injections,
                &mut injection_cycles,
            );
            invoke_agent_hook_before_iteration(&spec, &hook_context(iteration, &messages));
            let messages_for_model = govern_messages_for_model(&spec, &messages);
            let tool_surface = assemble_tool_surface(ToolSurfaceAssemblyInput {
                definitions: spec.tools.definitions(),
                runtime: spec.tool_search_runtime_input(),
            });
            let activation_summary = ToolSearchDiagnosticsSummary::from_assembly(
                spec.tool_search_runtime_input(),
                &tool_surface,
            );
            let activation_event = tool_search_activation_event(activation_summary);
            emit_events(&spec, std::slice::from_ref(&activation_event));
            tool_events.push(activation_event);
            let current_catalog = tool_surface.catalog;
            let request = ProviderRequest {
                messages: messages_for_model,
                tools: tool_surface.provider_tools,
                model: spec.model.clone(),
                settings: spec.settings.clone(),
                tool_choice: None,
            };
            let model = request_model(&spec, request, hook_context(iteration, &messages))?;
            let response = model.response;
            accumulate_usage(&mut usage, &response.usage);
            invoke_agent_hook_after_response(&spec, &hook_context(iteration, &messages), &response);

            if cancellation_requested(&spec) {
                return Ok(cancelled_run_result(
                    messages,
                    tools_used,
                    usage,
                    tool_events,
                    had_injections,
                ));
            }

            if response.should_execute_tools() {
                if model.hook_streamed {
                    invoke_agent_hook_stream_end(&spec, &hook_context(iteration, &messages), true);
                }
                let mut runtime_calls = response
                    .tool_calls
                    .iter()
                    .map(|call| {
                        RuntimeToolCall::new(
                            call.id.clone(),
                            call.name.clone(),
                            Value::Object(call.arguments.clone()),
                        )
                    })
                    .collect::<Vec<_>>();
                if let Some(ask_index) = runtime_calls
                    .iter()
                    .position(|call| call.name == "ask_user")
                {
                    runtime_calls.truncate(ask_index + 1);
                }
                messages.push(
                    RuntimeAssistantToolCallMessage::new(
                        Some(response.content.clone().unwrap_or_default()),
                        runtime_calls.clone(),
                    )
                    .with_reasoning_content(response.reasoning_content.clone())
                    .with_thinking_blocks(response.thinking_blocks.clone())
                    .to_json(),
                );
                let mut iteration_tool_uses = iteration_tool_uses(&runtime_calls);
                emit_checkpoint(
                    &spec,
                    "awaiting_tools",
                    messages.last().cloned(),
                    Vec::new(),
                    runtime_calls.clone(),
                );

                let (executable_calls, throttled_messages, throttled_events) =
                    apply_external_lookup_throttle(runtime_calls, &mut external_lookup_counts);
                emit_events(&spec, &throttled_events);
                tool_events.extend(throttled_events);
                let mut completed_tool_messages = Vec::new();
                let mut completed_tool_results = Vec::new();
                for message in throttled_messages {
                    let message = normalize_tool_message(&spec, message);
                    completed_tool_results.push(message.to_json());
                    completed_tool_messages.push(message.clone());
                    messages.push(message.to_json());
                }

                let executable_calls_for_checkpoint = executable_calls.clone();
                let executable_calls_by_id = executable_calls_for_checkpoint
                    .iter()
                    .map(|call| (call.id.clone(), call.clone()))
                    .collect::<BTreeMap<_, _>>();
                if cancellation_requested(&spec) {
                    append_skipped_tool_results(
                        &executable_calls_for_checkpoint,
                        &mut completed_tool_messages,
                        &mut completed_tool_results,
                        &mut messages,
                    );
                    append_iteration_tool_uses(&mut tools_used, iteration_tool_uses);
                    emit_checkpoint(
                        &spec,
                        "tools_completed",
                        latest_assistant_tool_message(&messages),
                        completed_tool_results,
                        Vec::new(),
                    );
                    return Ok(cancelled_run_result(
                        messages,
                        tools_used,
                        usage,
                        tool_events,
                        had_injections,
                    ));
                }
                invoke_agent_hook_before_execute_tools(
                    &spec,
                    &hook_context(iteration, &messages),
                    &executable_calls,
                );
                let report = execute_tool_dispatch(
                    executable_calls,
                    current_catalog.as_ref(),
                    &spec,
                    &executor,
                );
                apply_resolved_bridge_tool_uses(
                    &mut iteration_tool_uses,
                    &report.resolved_bridge_calls,
                );
                append_iteration_tool_uses(&mut tools_used, iteration_tool_uses);
                let resolved_bridge_calls_by_id = report
                    .resolved_bridge_calls
                    .iter()
                    .map(|resolved_call| (resolved_call.original_call_id.clone(), resolved_call))
                    .collect::<BTreeMap<_, _>>();
                for raw_message in &report.messages {
                    let event = tool_event_for_message(
                        raw_message,
                        executable_calls_by_id.get(&raw_message.tool_call_id),
                        current_catalog.as_ref(),
                        resolved_bridge_calls_by_id
                            .get(&raw_message.tool_call_id)
                            .copied(),
                    );
                    let message = normalize_tool_message(&spec, raw_message.clone());
                    emit_events(&spec, std::slice::from_ref(&event));
                    tool_events.push(event);
                    completed_tool_results.push(message.to_json());
                    completed_tool_messages.push(message.clone());
                    messages.push(message.to_json());
                }
                if let Some(error) = fatal_tool_error(&spec, &completed_tool_messages) {
                    append_skipped_tool_results(
                        &executable_calls_for_checkpoint,
                        &mut completed_tool_messages,
                        &mut completed_tool_results,
                        &mut messages,
                    );
                    emit_checkpoint(
                        &spec,
                        "tools_completed",
                        latest_assistant_tool_message(&messages),
                        completed_tool_results,
                        Vec::new(),
                    );
                    messages.push(serde_json::json!({"role": "assistant", "content": error}));
                    invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                    return Ok(AgentRunResult {
                        final_content: Some(error.clone()),
                        messages,
                        tools_used,
                        usage,
                        stop_reason: "tool_error".to_owned(),
                        error: Some(error.clone()),
                        error_message: Some(error),
                        interrupt: None,
                        tool_events,
                        had_injections,
                    });
                }
                if let Some(interrupt) = report.interrupt {
                    emit_checkpoint(
                        &spec,
                        "awaiting_tools",
                        latest_assistant_tool_message(&messages),
                        completed_tool_results,
                        pending_interrupt_tool_call(&interrupt, &executable_calls_for_checkpoint),
                    );
                    let pending_call =
                        pending_interrupt_tool_call(&interrupt, &executable_calls_for_checkpoint)
                            .into_iter()
                            .next();
                    let event = ToolEvent {
                        name: interrupt_name(&interrupt),
                        status: ToolStatus::Waiting,
                        detail: interrupt_text(&interrupt).unwrap_or_default(),
                        call_id: pending_call.as_ref().map(|call| call.id.clone()),
                        arguments: pending_call.map(|call| call.arguments),
                        result: None,
                    };
                    emit_events(&spec, std::slice::from_ref(&event));
                    tool_events.push(event);
                    if model.hook_streamed {
                        invoke_agent_hook_stream_end(
                            &spec,
                            &hook_context(iteration, &messages),
                            false,
                        );
                    }
                    invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                    return Ok(AgentRunResult {
                        final_content: interrupt_text(&interrupt),
                        messages,
                        tools_used,
                        usage,
                        stop_reason: "ask_user".to_owned(),
                        error: None,
                        error_message: None,
                        interrupt: Some(interrupt),
                        tool_events,
                        had_injections,
                    });
                }
                emit_checkpoint(
                    &spec,
                    "tools_completed",
                    latest_assistant_tool_message(&messages),
                    completed_tool_results,
                    Vec::new(),
                );
                invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                continue;
            }

            if response.finish_reason == "error" {
                if model.hook_streamed {
                    invoke_agent_hook_stream_end(&spec, &hook_context(iteration, &messages), false);
                }
                let content = response
                    .content
                    .clone()
                    .filter(|content| !content.trim().is_empty())
                    .or_else(|| spec.error_message.clone())
                    .unwrap_or_else(|| DEFAULT_ERROR_MESSAGE.to_owned());
                messages.push(assistant_message(Some(MODEL_ERROR_PLACEHOLDER), &response));
                emit_checkpoint(
                    &spec,
                    "final_response",
                    messages.last().cloned(),
                    Vec::new(),
                    Vec::new(),
                );
                invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                return Ok(AgentRunResult {
                    final_content: Some(content.clone()),
                    messages,
                    tools_used,
                    usage,
                    stop_reason: "error".to_owned(),
                    error: Some(content),
                    error_message: Some(DEFAULT_ERROR_MESSAGE.to_owned()),
                    interrupt: None,
                    tool_events,
                    had_injections,
                });
            }

            let final_content = response
                .content
                .clone()
                .map(|content| content.trim().to_owned());
            let is_blank = final_content.as_deref().map_or(true, str::is_empty);
            if is_blank {
                empty_content_retries += 1;
                if model.hook_streamed {
                    invoke_agent_hook_stream_end(&spec, &hook_context(iteration, &messages), false);
                }
                if empty_content_retries < MAX_EMPTY_RETRIES {
                    invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                    continue;
                }
                let retry_request = ProviderRequest {
                    messages: govern_messages_for_model(
                        &spec,
                        &finalization_retry_messages(&messages),
                    ),
                    tools: Vec::new(),
                    model: spec.model.clone(),
                    settings: spec.settings.clone(),
                    tool_choice: None,
                };
                let retry_model =
                    request_model(&spec, retry_request, hook_context(iteration, &messages))?;
                let retry_response = retry_model.response;
                accumulate_usage(&mut usage, &retry_response.usage);
                invoke_agent_hook_after_response(
                    &spec,
                    &hook_context(iteration, &messages),
                    &retry_response,
                );
                if retry_model.hook_streamed {
                    invoke_agent_hook_stream_end(&spec, &hook_context(iteration, &messages), false);
                }
                if let Some(content) = retry_response
                    .content
                    .clone()
                    .filter(|content| !content.trim().is_empty())
                {
                    let content =
                        finalize_content(&spec, &hook_context(iteration, &messages), content);
                    messages.push(assistant_message(Some(&content), &retry_response));
                    emit_checkpoint(
                        &spec,
                        "final_response",
                        messages.last().cloned(),
                        Vec::new(),
                        Vec::new(),
                    );
                    invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                    return Ok(AgentRunResult {
                        final_content: Some(content),
                        messages,
                        tools_used,
                        usage,
                        stop_reason: "completed".to_owned(),
                        error: None,
                        error_message: None,
                        interrupt: None,
                        tool_events,
                        had_injections,
                    });
                }
                let content = finalize_content(
                    &spec,
                    &hook_context(iteration, &messages),
                    EMPTY_FINAL_RESPONSE_MESSAGE.to_owned(),
                );
                messages.push(serde_json::json!({"role": "assistant", "content": content}));
                emit_checkpoint(
                    &spec,
                    "final_response",
                    messages.last().cloned(),
                    Vec::new(),
                    Vec::new(),
                );
                invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                return Ok(AgentRunResult {
                    final_content: Some(content.clone()),
                    messages,
                    tools_used,
                    usage,
                    stop_reason: "empty_final_response".to_owned(),
                    error: Some(content.clone()),
                    error_message: Some(content),
                    interrupt: None,
                    tool_events,
                    had_injections,
                });
            }

            if response.finish_reason == "length" && length_recoveries < MAX_LENGTH_RECOVERIES {
                if model.hook_streamed {
                    invoke_agent_hook_stream_end(&spec, &hook_context(iteration, &messages), true);
                }
                length_recoveries += 1;
                let content = final_content.unwrap_or_default();
                messages.push(assistant_message(Some(&content), &response));
                messages
                    .push(serde_json::json!({"role": "user", "content": LENGTH_RECOVERY_PROMPT}));
                invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                continue;
            }

            if append_mid_turn_injections(
                &spec,
                &mut messages,
                &mut had_injections,
                &mut injection_cycles,
            ) {
                invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
                continue;
            }

            if model.hook_streamed {
                invoke_agent_hook_stream_end(&spec, &hook_context(iteration, &messages), false);
            }
            let final_content = final_content.map(|content| {
                finalize_content(&spec, &hook_context(iteration, &messages), content)
            });
            messages.push(assistant_message(final_content.as_deref(), &response));
            emit_checkpoint(
                &spec,
                "final_response",
                messages.last().cloned(),
                Vec::new(),
                Vec::new(),
            );
            invoke_agent_hook_after_iteration(&spec, &hook_context(iteration, &messages));
            return Ok(AgentRunResult {
                final_content,
                messages,
                tools_used,
                usage,
                stop_reason: "completed".to_owned(),
                error: None,
                error_message: None,
                interrupt: None,
                tool_events,
                had_injections,
            });
        }

        let final_content = spec
            .max_iterations_message
            .clone()
            .unwrap_or_else(|| {
                format!(
                    "I reached the maximum number of tool call iterations ({}) without completing the task. You can try breaking the task into smaller steps.",
                    spec.max_iterations
                )
            });
        let final_content = finalize_content(
            &spec,
            &hook_context(spec.max_iterations, &messages),
            final_content,
        );
        messages.push(serde_json::json!({"role": "assistant", "content": final_content}));
        emit_checkpoint(
            &spec,
            "final_response",
            messages.last().cloned(),
            Vec::new(),
            Vec::new(),
        );
        Ok(AgentRunResult {
            final_content: Some(final_content),
            messages,
            tools_used,
            usage,
            stop_reason: "max_iterations".to_owned(),
            error: None,
            error_message: None,
            interrupt: None,
            tool_events,
            had_injections,
        })
    }
}

fn cancellation_requested(spec: &AgentRunSpec<'_>) -> bool {
    spec.cancellation_token
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
}

fn cancelled_run_result(
    mut messages: Vec<Value>,
    tools_used: Vec<String>,
    usage: BTreeMap<String, u64>,
    tool_events: Vec<ToolEvent>,
    had_injections: bool,
) -> AgentRunResult {
    let content = "Turn cancelled before completion.".to_owned();
    messages.push(serde_json::json!({"role": "assistant", "content": content}));
    AgentRunResult {
        final_content: Some(content.clone()),
        messages,
        tools_used,
        usage,
        stop_reason: "cancelled".to_owned(),
        error: None,
        error_message: None,
        interrupt: None,
        tool_events,
        had_injections,
    }
}

struct ModelResponse {
    response: LlmResponse,
    hook_streamed: bool,
}

struct ToolDispatchReport {
    messages: Vec<RuntimeToolMessage>,
    interrupt: Option<RuntimeInterrupt>,
    resolved_bridge_calls: Vec<ResolvedDeferredToolCall>,
}

struct IterationToolUse {
    call_id: String,
    name: String,
}

fn execute_tool_dispatch(
    calls: Vec<RuntimeToolCall>,
    catalog: Option<&DeferredToolCatalog>,
    spec: &AgentRunSpec<'_>,
    executor: &RuntimeToolExecutor<'_>,
) -> ToolDispatchReport {
    let Some(catalog) = catalog else {
        return direct_tool_dispatch(calls, spec, executor);
    };
    if !calls.iter().any(is_bridge_tool_call) {
        return direct_tool_dispatch(calls, spec, executor);
    }

    let mut messages = Vec::new();
    let mut resolved_bridge_calls = Vec::new();
    let mut index = 0;
    while index < calls.len() {
        let segment_start = index;
        let bridge_segment = is_bridge_tool_call(&calls[index]);
        while index < calls.len() && is_bridge_tool_call(&calls[index]) == bridge_segment {
            index += 1;
        }
        let segment = calls[segment_start..index].to_vec();
        if bridge_segment {
            let report = dispatch_bridge_tool_calls(
                segment,
                Some(catalog),
                spec.tools,
                executor,
                &spec.tool_context,
                spec.concurrent_tools,
            );
            resolved_bridge_calls.extend(report.resolved_calls);
            messages.extend(report.results.into_iter().map(|result| result.message));
            if let Some(interrupt) = report.interrupt {
                return ToolDispatchReport {
                    messages,
                    interrupt: Some(interrupt),
                    resolved_bridge_calls,
                };
            }
        } else {
            let report = direct_runtime_report(segment, spec, executor);
            messages.extend(report.messages);
            if let Some(interrupt) = report.interrupt {
                return ToolDispatchReport {
                    messages,
                    interrupt: Some(interrupt),
                    resolved_bridge_calls,
                };
            }
        }
    }

    ToolDispatchReport {
        messages,
        interrupt: None,
        resolved_bridge_calls,
    }
}

fn direct_tool_dispatch(
    calls: Vec<RuntimeToolCall>,
    spec: &AgentRunSpec<'_>,
    executor: &RuntimeToolExecutor<'_>,
) -> ToolDispatchReport {
    let report = direct_runtime_report(calls, spec, executor);
    ToolDispatchReport {
        messages: report.messages,
        interrupt: report.interrupt,
        resolved_bridge_calls: Vec::new(),
    }
}

fn direct_runtime_report(
    calls: Vec<RuntimeToolCall>,
    spec: &AgentRunSpec<'_>,
    executor: &RuntimeToolExecutor<'_>,
) -> RuntimeToolExecutionReport {
    if spec.concurrent_tools {
        executor.execute_tool_calls_concurrent(calls, &spec.tool_context)
    } else {
        executor.execute_tool_calls(calls, &spec.tool_context)
    }
}

fn is_bridge_tool_call(call: &RuntimeToolCall) -> bool {
    bridge_tool_names().contains(&call.name.as_str())
}

fn iteration_tool_uses(calls: &[RuntimeToolCall]) -> Vec<IterationToolUse> {
    calls
        .iter()
        .map(|call| IterationToolUse {
            call_id: call.id.clone(),
            name: call.name.clone(),
        })
        .collect()
}

fn apply_resolved_bridge_tool_uses(
    tool_uses: &mut [IterationToolUse],
    resolved_calls: &[ResolvedDeferredToolCall],
) {
    for resolved_call in resolved_calls {
        if let Some(tool_use) = tool_uses
            .iter_mut()
            .find(|tool_use| tool_use.call_id == resolved_call.original_call_id)
        {
            tool_use.name = resolved_call.underlying_name.clone();
        }
    }
}

fn append_iteration_tool_uses(tools_used: &mut Vec<String>, tool_uses: Vec<IterationToolUse>) {
    tools_used.extend(tool_uses.into_iter().map(|tool_use| tool_use.name));
}

fn request_model(
    spec: &AgentRunSpec<'_>,
    request: ProviderRequest,
    context: AgentHookContext,
) -> Result<ModelResponse, ProviderError> {
    let hook_wants_streaming = spec
        .agent_hook
        .as_ref()
        .is_some_and(|hook| hook.wants_streaming());
    let mut waiter = RetryWaiter::new(spec.retry_wait_callback.as_ref());
    if spec.provider_event_callback.is_some() || hook_wants_streaming {
        let callback = spec.provider_event_callback.clone();
        let hook = spec.agent_hook.clone();
        let mut on_event = move |event: ProviderEvent| {
            if let Some(callback) = &callback {
                invoke_provider_event_callback(callback, &event);
            }
            if hook_wants_streaming {
                if let ProviderEvent::TextDelta { text } = &event {
                    if let Some(hook) = &hook {
                        invoke_hook_lifecycle(|| hook.on_stream(&context, text));
                    }
                }
            }
        };
        let response = match &mut waiter {
            RetryWaiter::Thread(waiter) => chat_stream_with_retry_using_waiter(
                spec.client,
                request,
                spec.retry_mode,
                &mut on_event,
                waiter,
            ),
            RetryWaiter::Callback(waiter) => chat_stream_with_retry_using_waiter(
                spec.client,
                request,
                spec.retry_mode,
                &mut on_event,
                waiter,
            ),
        }?;
        Ok(ModelResponse {
            response,
            hook_streamed: hook_wants_streaming,
        })
    } else {
        let response = match &mut waiter {
            RetryWaiter::Thread(waiter) => {
                chat_with_retry_using_waiter(spec.client, request, spec.retry_mode, waiter)
            }
            RetryWaiter::Callback(waiter) => {
                chat_with_retry_using_waiter(spec.client, request, spec.retry_mode, waiter)
            }
        }?;
        Ok(ModelResponse {
            response,
            hook_streamed: false,
        })
    }
}

enum RetryWaiter<'a> {
    Thread(ThreadRetryWaiter),
    Callback(CallbackRetryWaiter<'a>),
}

impl<'a> RetryWaiter<'a> {
    fn new(callback: Option<&'a RetryWaitCallback>) -> Self {
        if let Some(callback) = callback {
            Self::Callback(CallbackRetryWaiter {
                callback,
                thread: ThreadRetryWaiter,
            })
        } else {
            Self::Thread(ThreadRetryWaiter)
        }
    }
}

struct CallbackRetryWaiter<'a> {
    callback: &'a RetryWaitCallback,
    thread: ThreadRetryWaiter,
}

impl ProviderRetryWaiter for CallbackRetryWaiter<'_> {
    fn wait(&mut self, delay_s: f64, message: &str) {
        let callback = self.callback.clone();
        let _ = catch_unwind(AssertUnwindSafe(|| callback(delay_s, message)));
        self.thread.wait(delay_s, message);
    }
}

fn drain_mid_turn_injections(spec: &AgentRunSpec<'_>, injection_cycles: &mut usize) -> Vec<Value> {
    if *injection_cycles >= MAX_INJECTION_CYCLES {
        return Vec::new();
    }
    let injections = spec
        .mid_turn_injection_callback
        .as_ref()
        .map(|callback| {
            catch_unwind(AssertUnwindSafe(|| callback()))
                .ok()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if !injections.is_empty() {
        *injection_cycles += 1;
    }
    injections
}

fn append_mid_turn_injections(
    spec: &AgentRunSpec<'_>,
    messages: &mut Vec<Value>,
    had_injections: &mut bool,
    injection_cycles: &mut usize,
) -> bool {
    let injections = drain_mid_turn_injections(spec, injection_cycles);
    if injections.is_empty() {
        return false;
    }
    *had_injections = true;
    messages.extend(injections);
    true
}

fn emit_checkpoint(
    spec: &AgentRunSpec<'_>,
    phase: &str,
    assistant_message: Option<Value>,
    completed_tool_results: Vec<Value>,
    pending_tool_calls: Vec<RuntimeToolCall>,
) {
    let Some(callback) = &spec.checkpoint_callback else {
        return;
    };
    let pending_tool_calls = pending_tool_calls
        .into_iter()
        .map(|call| {
            serde_json::json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": call.arguments.to_string(),
                }
            })
        })
        .collect::<Vec<_>>();
    let checkpoint = serde_json::json!({
        "phase": phase,
        "assistant_message": assistant_message,
        "completed_tool_results": completed_tool_results,
        "pending_tool_calls": pending_tool_calls,
    });
    invoke_checkpoint_callback(callback, &checkpoint);
}

fn invoke_provider_event_callback(callback: &ProviderEventCallback, event: &ProviderEvent) {
    let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
}

fn invoke_checkpoint_callback(callback: &CheckpointCallback, checkpoint: &Value) {
    let _ = catch_unwind(AssertUnwindSafe(|| callback(checkpoint)));
}

fn invoke_tool_event_callback(callback: &ToolEventCallback, event: &ToolEvent) {
    let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
}

fn hook_context(iteration: usize, messages: &[Value]) -> AgentHookContext {
    AgentHookContext {
        iteration,
        messages: messages.to_vec(),
    }
}

fn invoke_hook_lifecycle(callback: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(callback));
}

fn invoke_hook_finalize(
    hook: &dyn AgentHook,
    context: &AgentHookContext,
    content: String,
) -> String {
    match catch_unwind(AssertUnwindSafe(|| {
        hook.finalize_content(context, content.clone())
    })) {
        Ok(content) => content,
        Err(_) => content,
    }
}

fn invoke_agent_hook_before_iteration(spec: &AgentRunSpec<'_>, context: &AgentHookContext) {
    if let Some(hook) = &spec.agent_hook {
        invoke_hook_lifecycle(|| hook.before_iteration(context));
    }
}

fn invoke_agent_hook_before_execute_tools(
    spec: &AgentRunSpec<'_>,
    context: &AgentHookContext,
    calls: &[RuntimeToolCall],
) {
    if let Some(hook) = &spec.agent_hook {
        invoke_hook_lifecycle(|| hook.before_execute_tools(context, calls));
    }
}

fn invoke_agent_hook_after_response(
    spec: &AgentRunSpec<'_>,
    context: &AgentHookContext,
    response: &LlmResponse,
) {
    if let Some(hook) = &spec.agent_hook {
        invoke_hook_lifecycle(|| hook.after_response(context, response));
    }
}

fn invoke_agent_hook_stream_end(
    spec: &AgentRunSpec<'_>,
    context: &AgentHookContext,
    resuming: bool,
) {
    if let Some(hook) = &spec.agent_hook {
        invoke_hook_lifecycle(|| hook.on_stream_end(context, resuming));
    }
}

fn invoke_agent_hook_after_iteration(spec: &AgentRunSpec<'_>, context: &AgentHookContext) {
    if let Some(hook) = &spec.agent_hook {
        invoke_hook_lifecycle(|| hook.after_iteration(context));
    }
}

fn finalize_content(
    spec: &AgentRunSpec<'_>,
    context: &AgentHookContext,
    content: String,
) -> String {
    spec.agent_hook
        .as_ref()
        .map(|hook| invoke_hook_finalize(hook.as_ref(), context, content.clone()))
        .unwrap_or(content)
}

fn start_runtime_turn(context_tools: &RuntimeContextTools) {
    if let Some(message) = &context_tools.message {
        message.start_turn();
    }
}

fn pending_interrupt_tool_call(
    interrupt: &RuntimeInterrupt,
    calls: &[RuntimeToolCall],
) -> Vec<RuntimeToolCall> {
    let tool_call_id = match interrupt {
        RuntimeInterrupt::AskUser { tool_call_id, .. } => tool_call_id,
    };
    calls
        .iter()
        .find(|call| call.id == *tool_call_id)
        .cloned()
        .into_iter()
        .collect()
}

fn append_skipped_tool_results(
    calls: &[RuntimeToolCall],
    completed_messages: &mut Vec<RuntimeToolMessage>,
    completed_results: &mut Vec<Value>,
    messages: &mut Vec<Value>,
) {
    let completed_ids = completed_messages
        .iter()
        .map(|message| message.tool_call_id.clone())
        .collect::<HashSet<_>>();
    for call in calls {
        if completed_ids.contains(&call.id) {
            continue;
        }
        let skipped = RuntimeToolMessage {
            tool_call_id: call.id.clone(),
            name: call.name.clone(),
            content: FATAL_SKIP_CONTENT.to_owned(),
        };
        let skipped_json = skipped.to_json();
        completed_messages.push(skipped);
        completed_results.push(skipped_json.clone());
        messages.push(skipped_json);
    }
}

fn latest_assistant_tool_message(messages: &[Value]) -> Option<Value> {
    messages.iter().rev().find_map(|message| {
        (message.get("role").and_then(Value::as_str) == Some("assistant")
            && message.get("tool_calls").is_some())
        .then(|| message.clone())
    })
}

fn govern_messages_for_model(spec: &AgentRunSpec<'_>, messages: &[Value]) -> Vec<Value> {
    let mut governed = drop_orphan_tool_results(messages);
    governed = backfill_missing_tool_results(&governed);
    governed = microcompact(&governed);
    governed = snip_history(spec, &governed);
    governed = drop_orphan_tool_results(&governed);
    backfill_missing_tool_results(&governed)
}

fn drop_orphan_tool_results(messages: &[Value]) -> Vec<Value> {
    let mut declared = HashSet::new();
    let mut updated = Vec::new();
    for message in messages {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                declared.extend(assistant_tool_ids(message));
                updated.push(message.clone());
            }
            Some("tool") => {
                let Some(tool_call_id) = message.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                if declared.contains(tool_call_id) {
                    updated.push(message.clone());
                }
            }
            _ => updated.push(message.clone()),
        }
    }
    updated
}

fn backfill_missing_tool_results(messages: &[Value]) -> Vec<Value> {
    let mut declared = Vec::new();
    let mut fulfilled = HashSet::new();
    for (index, message) in messages.iter().enumerate() {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                for call in message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        let name = call
                            .get("function")
                            .and_then(Value::as_object)
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        declared.push((index, id.to_owned(), name));
                    }
                }
            }
            Some("tool") => {
                if let Some(id) = message.get("tool_call_id").and_then(Value::as_str) {
                    fulfilled.insert(id.to_owned());
                }
            }
            _ => {}
        }
    }
    let missing = declared
        .into_iter()
        .filter(|(_, id, _)| !fulfilled.contains(id))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return messages.to_vec();
    }
    let mut updated = messages.to_vec();
    for (offset, (assistant_index, call_id, name)) in missing.into_iter().enumerate() {
        let mut insert_at = assistant_index + 1 + offset;
        while insert_at < updated.len()
            && updated[insert_at].get("role").and_then(Value::as_str) == Some("tool")
        {
            insert_at += 1;
        }
        updated.insert(
            insert_at,
            serde_json::json!({
                "role": "tool",
                "tool_call_id": call_id,
                "name": name,
                "content": BACKFILL_CONTENT,
            }),
        );
    }
    updated
}

fn microcompact(messages: &[Value]) -> Vec<Value> {
    let compactable_indices = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            let name = message.get("name").and_then(Value::as_str)?;
            (message.get("role").and_then(Value::as_str) == Some("tool")
                && COMPACTABLE_TOOLS.contains(&name))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    if compactable_indices.len() <= MICROCOMPACT_KEEP_RECENT {
        return messages.to_vec();
    }
    let mut updated = messages.to_vec();
    let stale_count = compactable_indices.len() - MICROCOMPACT_KEEP_RECENT;
    for index in &compactable_indices[..stale_count] {
        let Some(content) = updated[*index].get("content").and_then(Value::as_str) else {
            continue;
        };
        if content.len() >= MICROCOMPACT_MIN_CHARS {
            let name = updated[*index]
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            updated[*index]["content"] =
                Value::String(format!("[{name} result omitted from context]"));
        }
    }
    updated
}

fn snip_history(spec: &AgentRunSpec<'_>, messages: &[Value]) -> Vec<Value> {
    let Some(context_window_tokens) = spec.context_window_tokens else {
        return messages.to_vec();
    };
    let budget = spec.context_block_limit.unwrap_or_else(|| {
        context_window_tokens.saturating_sub(spec.settings.max_tokens as usize + SNIP_SAFETY_BUFFER)
    });
    if budget == 0 || estimate_messages_tokens(messages) <= budget {
        return messages.to_vec();
    }
    let system_messages = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .cloned()
        .collect::<Vec<_>>();
    let non_system = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) != Some("system"))
        .cloned()
        .collect::<Vec<_>>();
    let system_tokens = estimate_messages_tokens(&system_messages);
    let remaining_budget = budget.saturating_sub(system_tokens).max(128);
    let mut kept = Vec::new();
    let mut used = 0;
    for message in non_system.iter().rev() {
        let tokens = estimate_message_tokens(message);
        if !kept.is_empty() && used + tokens > remaining_budget {
            break;
        }
        kept.push(message.clone());
        used += tokens;
    }
    kept.reverse();
    if let Some(first_user) = kept
        .iter()
        .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        kept = kept[first_user..].to_vec();
    } else if let Some(latest_user) = non_system
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        kept = non_system[latest_user..].to_vec();
    }
    [system_messages, kept].concat()
}

fn assistant_tool_ids(message: &Value) -> HashSet<String> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| call.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn estimate_messages_tokens(messages: &[Value]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &Value) -> usize {
    (message.to_string().chars().count() / 4).max(1)
}

fn apply_external_lookup_throttle(
    calls: Vec<RuntimeToolCall>,
    counts: &mut BTreeMap<String, usize>,
) -> (
    Vec<RuntimeToolCall>,
    Vec<RuntimeToolMessage>,
    Vec<ToolEvent>,
) {
    let mut executable = Vec::new();
    let mut messages = Vec::new();
    let mut events = Vec::new();
    for call in calls {
        if let Some(signature) = external_lookup_signature(&call) {
            let count = counts.entry(signature).or_insert(0);
            *count += 1;
            if *count > MAX_REPEAT_EXTERNAL_LOOKUPS {
                let content = "Error: repeated external lookup blocked. Use the results you already have to answer, or try a meaningfully different source.\n\n[Analyze the error above and try a different approach.]".to_owned();
                let call_id = call.id.clone();
                let name = call.name.clone();
                let arguments = call.arguments.clone();
                messages.push(RuntimeToolMessage {
                    tool_call_id: call_id.clone(),
                    name: name.clone(),
                    content,
                });
                events.push(ToolEvent {
                    name,
                    status: ToolStatus::Error,
                    detail: "repeated external lookup blocked".to_owned(),
                    call_id: Some(call_id),
                    arguments: Some(arguments),
                    result: Some(Value::String("repeated external lookup blocked".to_owned())),
                });
                continue;
            }
        }
        executable.push(call);
    }
    (executable, messages, events)
}

fn external_lookup_signature(call: &RuntimeToolCall) -> Option<String> {
    let object = call.arguments.as_object()?;
    match call.name.as_str() {
        "web_fetch" => object
            .get("url")
            .and_then(Value::as_str)
            .map(|url| format!("web_fetch:{}", url.trim().to_ascii_lowercase())),
        "web_search" => object
            .get("query")
            .or_else(|| object.get("search_term"))
            .and_then(Value::as_str)
            .map(|query| format!("web_search:{}", query.trim().to_ascii_lowercase())),
        _ => None,
    }
}

fn normalize_tool_message(
    spec: &AgentRunSpec<'_>,
    mut message: RuntimeToolMessage,
) -> RuntimeToolMessage {
    if message.content.trim().is_empty() {
        message.content = format!("({} completed with no output)", message.name);
    }
    message.content = maybe_persist_text_tool_result(
        spec.workspace.as_deref(),
        spec.session_key.as_deref(),
        &message.tool_call_id,
        &message.content,
        spec.max_tool_result_chars,
    )
    .unwrap_or_else(|| truncate_text(&message.content, spec.max_tool_result_chars));
    message
}

fn truncate_text(content: &str, max_chars: usize) -> String {
    if max_chars == 0 || content.chars().count() <= max_chars {
        return content.to_owned();
    }
    let mut truncated = content.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n... (truncated)");
    truncated
}

fn tool_search_activation_event(summary: ToolSearchDiagnosticsSummary) -> ToolEvent {
    ToolEvent {
        name: "tool_search_activation".to_owned(),
        status: ToolStatus::Ok,
        detail: format!(
            "Tool Search mode={} activated={} visible={} deferred={}",
            summary.mode, summary.activated, summary.visible_count, summary.deferred_count
        ),
        call_id: None,
        arguments: None,
        result: Some(serde_json::json!({ "activation": summary })),
    }
}

fn tool_event_for_message(
    message: &RuntimeToolMessage,
    call: Option<&RuntimeToolCall>,
    catalog: Option<&DeferredToolCatalog>,
    resolved_bridge_call: Option<&ResolvedDeferredToolCall>,
) -> ToolEvent {
    if let Some(event) = bridge_tool_event_for_message(message, call, catalog, resolved_bridge_call)
    {
        return event;
    }

    let is_error = message.content.starts_with("Error") || is_workspace_violation(&message.content);
    ToolEvent {
        name: message.name.clone(),
        status: if is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Ok
        },
        detail: event_detail(&message.content),
        call_id: Some(message.tool_call_id.clone()),
        arguments: call.map(|call| call.arguments.clone()),
        result: Some(tool_event_result_value(&message.content)),
    }
}

fn bridge_tool_event_for_message(
    message: &RuntimeToolMessage,
    call: Option<&RuntimeToolCall>,
    catalog: Option<&DeferredToolCatalog>,
    resolved_bridge_call: Option<&ResolvedDeferredToolCall>,
) -> Option<ToolEvent> {
    let scope_digest = catalog.map(|catalog| catalog.scope_digest.clone())?;
    let status = if message.content.starts_with("Error") || is_workspace_violation(&message.content)
    {
        ToolStatus::Error
    } else {
        ToolStatus::Ok
    };
    match message.name.as_str() {
        "tool_search" => Some(tool_search_query_event(message, call, status, scope_digest)),
        "tool_describe" => Some(tool_describe_event(message, call, status, scope_digest)),
        "tool_call" => Some(tool_call_mapping_event(
            message,
            call,
            status,
            scope_digest,
            resolved_bridge_call,
        )),
        _ => None,
    }
}

fn tool_search_query_event(
    message: &RuntimeToolMessage,
    call: Option<&RuntimeToolCall>,
    status: ToolStatus,
    scope_digest: String,
) -> ToolEvent {
    let result = tool_event_result_value(&message.content);
    let matched_names = result
        .get("matches")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .take(MAX_TOOL_SEARCH_EVIDENCE_MATCHES)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let arguments = call.and_then(|call| call.arguments.as_object());
    let query = arguments
        .and_then(|arguments| arguments.get("query"))
        .and_then(Value::as_str)
        .or_else(|| result.get("query").and_then(Value::as_str))
        .unwrap_or_default();
    let limit = arguments
        .and_then(|arguments| arguments.get("limit"))
        .and_then(Value::as_u64)
        .and_then(|limit| usize::try_from(limit).ok());
    let evidence = ToolSearchQueryEvidence {
        redacted_query: redacted_tool_search_query(query),
        limit,
        matched_names,
        scope_digest,
    };

    ToolEvent {
        name: message.name.clone(),
        status,
        detail: format!(
            "tool_search matched {} deferred tools",
            evidence.matched_names.len()
        ),
        call_id: Some(message.tool_call_id.clone()),
        arguments: Some(serde_json::json!({
            "query": evidence.redacted_query,
            "limit": evidence.limit,
        })),
        result: Some(serde_json::json!({ "query_evidence": evidence })),
    }
}

fn tool_describe_event(
    message: &RuntimeToolMessage,
    call: Option<&RuntimeToolCall>,
    status: ToolStatus,
    scope_digest: String,
) -> ToolEvent {
    let result = tool_event_result_value(&message.content);
    let requested_name = call
        .and_then(|call| call.arguments.as_object())
        .and_then(|arguments| arguments.get("name"))
        .and_then(Value::as_str)
        .or_else(|| result.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    let evidence = ToolDescribeEvidence {
        requested_name,
        found: status == ToolStatus::Ok,
        scope_digest,
    };

    ToolEvent {
        name: message.name.clone(),
        status,
        detail: format!(
            "tool_describe requested {} found={}",
            evidence.requested_name, evidence.found
        ),
        call_id: Some(message.tool_call_id.clone()),
        arguments: Some(serde_json::json!({ "name": evidence.requested_name })),
        result: Some(serde_json::json!({ "describe_evidence": evidence })),
    }
}

fn tool_call_mapping_event(
    message: &RuntimeToolMessage,
    call: Option<&RuntimeToolCall>,
    status: ToolStatus,
    scope_digest: String,
    resolved_bridge_call: Option<&ResolvedDeferredToolCall>,
) -> ToolEvent {
    let requested_name = call
        .and_then(|call| call.arguments.as_object())
        .and_then(|arguments| arguments.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let evidence = resolved_bridge_call.map(|resolved_call| BridgeUnderlyingMappingEvidence {
        bridge_call_id: resolved_call.original_call_id.clone(),
        bridge_name: resolved_call.bridge_name.clone(),
        underlying_name: resolved_call.underlying_name.clone(),
        scope_digest: resolved_call.scope_digest.clone(),
    });

    ToolEvent {
        name: message.name.clone(),
        status,
        detail: evidence
            .as_ref()
            .map(|evidence| {
                format!(
                    "tool_call mapped bridge call {} to {}",
                    evidence.bridge_call_id, evidence.underlying_name
                )
            })
            .unwrap_or_else(|| format!("tool_call rejected {requested_name}")),
        call_id: Some(message.tool_call_id.clone()),
        arguments: None,
        result: Some(serde_json::json!({
            "mapping_evidence": evidence,
            "scope_digest": scope_digest,
        })),
    }
}

fn redacted_tool_search_query(query: &str) -> String {
    let trimmed = query.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if ["sk-", "token", "secret", "password", "api_key", "apikey"]
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return "[redacted]".to_owned();
    }
    if trimmed.chars().count() <= MAX_TOOL_SEARCH_QUERY_CHARS {
        return trimmed.to_owned();
    }
    let mut bounded = trimmed
        .chars()
        .take(MAX_TOOL_SEARCH_QUERY_CHARS)
        .collect::<String>();
    bounded.push_str("...");
    bounded
}

fn tool_event_result_value(content: &str) -> Value {
    serde_json::from_str(content).unwrap_or_else(|_| Value::String(content.to_owned()))
}

fn event_detail(content: &str) -> String {
    let mut detail = content.replace('\n', " ").trim().to_owned();
    if detail.is_empty() {
        detail = "(empty)".to_owned();
    }
    if detail.chars().count() > 120 {
        detail = detail.chars().take(120).collect::<String>();
        detail.push_str("...");
    }
    detail
}

fn emit_events(spec: &AgentRunSpec<'_>, events: &[ToolEvent]) {
    if let Some(callback) = &spec.tool_event_callback {
        for event in events {
            invoke_tool_event_callback(callback, event);
        }
    }
}

fn fatal_tool_error(spec: &AgentRunSpec<'_>, messages: &[RuntimeToolMessage]) -> Option<String> {
    messages.iter().find_map(|message| {
        if is_workspace_violation(&message.content)
            || (spec.fail_on_tool_error && message.content.starts_with("Error"))
        {
            Some(message.content.clone())
        } else {
            None
        }
    })
}

fn is_workspace_violation(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    [
        "blocked by safety guard",
        "outside the configured workspace",
        "outside allowed directory",
        "working_dir is outside",
        "working_dir could not be resolved",
        "path traversal detected",
        "path outside working dir",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

fn finalization_retry_messages(messages: &[Value]) -> Vec<Value> {
    let mut retry_messages = messages.to_vec();
    retry_messages.push(serde_json::json!({"role": "user", "content": FINALIZATION_RETRY_PROMPT}));
    retry_messages
}

fn assistant_message(content: Option<&str>, response: &LlmResponse) -> Value {
    let mut message = serde_json::Map::from_iter([
        ("role".to_owned(), Value::String("assistant".to_owned())),
        (
            "content".to_owned(),
            content.map_or(Value::Null, |content| Value::String(content.to_owned())),
        ),
    ]);
    if let Some(reasoning) = &response.reasoning_content {
        message.insert(
            "reasoning_content".to_owned(),
            Value::String(reasoning.clone()),
        );
    }
    if let Some(thinking_blocks) = &response.thinking_blocks {
        message.insert(
            "thinking_blocks".to_owned(),
            Value::Array(thinking_blocks.clone()),
        );
    }
    Value::Object(message)
}

fn accumulate_usage(target: &mut BTreeMap<String, u64>, usage: &BTreeMap<String, u64>) {
    for (key, value) in usage {
        *target.entry(key.clone()).or_insert(0) += value;
    }
}

fn interrupt_text(interrupt: &RuntimeInterrupt) -> Option<String> {
    match interrupt {
        RuntimeInterrupt::AskUser { question, .. } => Some(question.clone()),
    }
}

fn interrupt_name(interrupt: &RuntimeInterrupt) -> String {
    match interrupt {
        RuntimeInterrupt::AskUser { name, .. } => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RecordingHook {
        label: &'static str,
        events: Arc<Mutex<Vec<String>>>,
        append_suffix: bool,
    }

    impl AgentHook for RecordingHook {
        fn after_response(&self, _context: &AgentHookContext, response: &LlmResponse) {
            let content = response.content.clone().unwrap_or_default();
            self.events
                .lock()
                .expect("events lock")
                .push(format!("{}:response:{content}", self.label));
        }

        fn finalize_content(&self, _context: &AgentHookContext, content: String) -> String {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("{}:finalize:{content}", self.label));
            if self.append_suffix {
                format!("{content}{}", self.label)
            } else {
                content
            }
        }
    }

    struct QueueProviderClient {
        responses: Mutex<VecDeque<LlmResponse>>,
    }

    impl ProviderClient for QueueProviderClient {
        fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
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

    fn blank_response() -> LlmResponse {
        LlmResponse {
            content: Some(String::new()),
            ..LlmResponse::default()
        }
    }

    #[test]
    fn composite_hook_forwards_after_response_and_finalize_content() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let hook_a: Arc<dyn AgentHook> = Arc::new(RecordingHook {
            label: "a",
            events: events.clone(),
            append_suffix: true,
        });
        let hook_b: Arc<dyn AgentHook> = Arc::new(RecordingHook {
            label: "b",
            events: events.clone(),
            append_suffix: true,
        });
        let composite = CompositeHook::new(vec![hook_a, hook_b]);
        let context = AgentHookContext {
            iteration: 3,
            messages: vec![json!({"role": "user", "content": "hello"})],
        };
        let response = LlmResponse {
            content: Some("reply".to_owned()),
            ..LlmResponse::default()
        };

        composite.after_response(&context, &response);
        let finalized = composite.finalize_content(&context, "seed".to_owned());

        assert_eq!(finalized, "seedab");
        assert_eq!(
            events.lock().expect("events lock").clone(),
            vec![
                "a:response:reply",
                "b:response:reply",
                "a:finalize:seed",
                "b:finalize:seeda"
            ]
        );
    }

    #[test]
    fn agent_runner_invokes_after_response_for_retry_response(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client = QueueProviderClient {
            responses: Mutex::new(VecDeque::from(vec![
                blank_response(),
                blank_response(),
                LlmResponse {
                    content: Some("final answer".to_owned()),
                    ..LlmResponse::default()
                },
            ])),
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let hook: Arc<dyn AgentHook> = Arc::new(RecordingHook {
            label: "hook",
            events: events.clone(),
            append_suffix: false,
        });
        let tools = ToolRegistry::new();
        let mut spec = AgentRunSpec::new(
            vec![json!({"role": "user", "content": "hello"})],
            &tools,
            &client,
            "model",
        );
        spec.max_iterations = 2;
        spec.agent_hook = Some(hook);

        let result = AgentRunner::new().run(spec)?;

        assert_eq!(result.final_content.as_deref(), Some("final answer"));
        assert_eq!(
            events.lock().expect("events lock").clone(),
            vec![
                "hook:response:",
                "hook:response:",
                "hook:response:final answer",
                "hook:finalize:final answer",
            ]
        );
        Ok(())
    }
}
