use super::{
    AgentRunSpec, AnalyzerInvocation, CancellationToken, ContextBuildRequest, ContextBuilder,
    InboundMessage, MessageBus, ToolExecutionContext,
};
use serde_json::{json, Value};
use shacs_config::RawCredential;
use shacs_providers::ProviderInvocation;
use std::sync::Arc;
use std::time::Instant;

pub(crate) struct MediaTurnControl {
    cancellation: Option<CancellationToken>,
    deadline: Option<Instant>,
    analyzer: AnalyzerInvocation,
    runtime_override: Option<RawCredential>,
    bus: MessageBus,
    session_key: String,
    unified_session_key: Option<String>,
    context_builder: ContextBuilder,
    max_injections: usize,
    is_command: Option<fn(&str) -> bool>,
}

pub(crate) struct MediaTurnInput<'a> {
    pub(crate) cancellation: Option<CancellationToken>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) context_builder: &'a ContextBuilder,
    pub(crate) runtime_override: Option<RawCredential>,
    pub(crate) bus: MessageBus,
    pub(crate) session_key: String,
    pub(crate) unified_session_key: Option<String>,
    pub(crate) max_injections: usize,
}

impl MediaTurnControl {
    pub(crate) fn new(input: MediaTurnInput<'_>) -> Self {
        let analyzer = input.context_builder.analyzer_invocation(
            input.cancellation.clone().unwrap_or_default(),
            input.deadline,
        );
        Self {
            cancellation: input.cancellation,
            deadline: input.deadline,
            analyzer,
            runtime_override: input.runtime_override,
            bus: input.bus,
            session_key: input.session_key,
            unified_session_key: input.unified_session_key,
            context_builder: input.context_builder.clone(),
            max_injections: input.max_injections,
            is_command: None,
        }
    }

    pub(crate) fn analyzer(&self) -> AnalyzerInvocation {
        self.analyzer.clone()
    }

    pub(crate) fn invoke(&mut self, is_command: fn(&str) -> bool) -> ProviderInvocation {
        self.is_command = Some(is_command);
        let invocation = self.cancellation.as_ref().map_or_else(
            || ProviderInvocation::uncancelled(self.runtime_override.clone()),
            |token| token.provider_invocation(self.runtime_override.clone()),
        );
        match self.deadline {
            Some(deadline) => invocation.with_deadline(deadline),
            None => invocation,
        }
    }

    pub(crate) fn apply(self, spec: &mut AgentRunSpec<'_>, mut tool_context: ToolExecutionContext) {
        tool_context.cancellation_token = self.cancellation.clone();
        spec.tool_context = tool_context;
        spec.cancellation_token = self.cancellation;
        spec.deadline = self.deadline;
        spec.mid_turn_injection_callback = self.is_command.map(|is_command| {
            mid_turn_injection_callback(
                self.bus,
                self.session_key,
                self.unified_session_key,
                self.context_builder,
                self.max_injections,
                is_command,
            )
        });
    }
}

macro_rules! turn {
    ($agent:expr, $session_key:expr, $max_injections:expr) => {{
        let cancellation = $agent
            .task_registry
            .cancellation_token($session_key)
            .or_else(|| {
                $agent
                    .config
                    .execution_control
                    .as_ref()
                    .map(crate::runtime::AutomationExecutionControl::cancellation_token)
            })
            .or_else(|| $agent.turn_lock.cancellation_token($session_key));
        let deadline = $agent
            .config
            .execution_control
            .as_ref()
            .map(crate::runtime::AutomationExecutionControl::deadline);
        crate::runtime::MediaTurnControl::new(crate::runtime::MediaTurnInput {
            cancellation,
            deadline,
            context_builder: &$agent.context_builder,
            runtime_override: $agent.config.provider_runtime_override.clone(),
            bus: $agent.bus.clone(),
            session_key: $session_key.to_owned(),
            unified_session_key: $agent.config.unified_session_key.clone(),
            max_injections: $max_injections,
        })
    }};
}

pub(crate) use turn;

fn mid_turn_injection_callback(
    bus: MessageBus,
    session_key: String,
    unified_session_key: Option<String>,
    context_builder: ContextBuilder,
    max_injections: usize,
    is_command: fn(&str) -> bool,
) -> Arc<dyn Fn() -> Vec<Value> + Send + Sync> {
    Arc::new(move || {
        bus.drain_inbound_matching(max_injections, |message| {
            effective_session_key(message, unified_session_key.as_deref()) == session_key
                && !is_command(&message.content)
        })
        .into_iter()
        .map(|message| injected_user_message(&context_builder, &message))
        .collect()
    })
}

fn effective_session_key(message: &InboundMessage, unified_session_key: Option<&str>) -> String {
    if message.session_key_override.is_some() {
        message.session_key()
    } else {
        unified_session_key
            .map(str::to_owned)
            .unwrap_or_else(|| message.session_key())
    }
}

fn injected_user_message(builder: &ContextBuilder, message: &InboundMessage) -> Value {
    builder
        .build_messages(ContextBuildRequest {
            history: Vec::new(),
            current_message: &message.content,
            media: &message.media,
            channel: Some(&message.channel),
            chat_id: Some(&message.chat_id),
            current_role: "user",
            session_summary: None,
            analyzer_invocation: None,
        })
        .into_iter()
        .find(|candidate| candidate.get("role").and_then(Value::as_str) == Some("user"))
        .unwrap_or_else(|| json!({"role": "user", "content": message.content}))
}
