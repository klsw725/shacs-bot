use super::plugin_hooks::{
    summarize_plugin_hook_dispatch, PluginHookCallbackResult, PluginHookDispatchAttempt,
    PluginHookDispatchSummary, PluginHookEvent,
};
use super::plugin_runtime::{
    hook_invocation, llm_after_context_payload, tool_before_context_payload,
    PluginHookCommandExecutor, PluginHookDispatchMode, PluginHookDispatchSink, PluginRuntimeHook,
    PluginRuntimePlugin, PluginRuntimeSnapshot, ProcessPluginHookCommandExecutor,
};
use super::runner::{AgentHook, AgentHookContext};
use super::tool_before::{
    HeadlessToolBeforeInteraction, ToolBeforeConfirmation, ToolBeforeContext, ToolBeforeDecision,
    ToolBeforeHandler, ToolBeforeInteraction, ToolBeforeOrderKey, ToolBeforeRuntimeFacts,
};
use super::trusted_runtime::Spec030FactStore;
use super::{RuntimeToolCall, RuntimeToolMessage};
use serde_json::Value;
use shacs_projection::{HookDenialReason, HookDiagnosticKind, HookRuntimeProjection};
use shacs_providers::LlmResponse;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

mod entries;
mod evaluation;
mod event_dispatch;

use entries::{command_handlers, HandlerEntry};

const TOOL_BLOCK_ERROR_HINT: &str = "\n\n[Analyze the error above and try a different approach.]";

#[derive(Clone)]
pub struct PluginRuntimeHookAgentHook {
    snapshot: PluginRuntimeSnapshot,
    mode: PluginHookDispatchMode,
    executor: Arc<dyn PluginHookCommandExecutor>,
    sink: Option<PluginHookDispatchSink>,
    trusted_handlers: Vec<Arc<dyn ToolBeforeHandler>>,
    interaction: Arc<dyn ToolBeforeInteraction>,
    facts: ToolBeforeRuntimeFacts,
    spec030_facts: Option<Spec030FactStore>,
}

impl PluginRuntimeHookAgentHook {
    pub fn new(snapshot: PluginRuntimeSnapshot) -> Self {
        Self::with_executor(
            snapshot,
            PluginHookDispatchMode::LiveDiagnostics,
            Arc::new(ProcessPluginHookCommandExecutor::default()),
        )
    }

    pub fn with_executor(
        snapshot: PluginRuntimeSnapshot,
        mode: PluginHookDispatchMode,
        executor: Arc<dyn PluginHookCommandExecutor>,
    ) -> Self {
        let registered_handlers = command_handlers(&snapshot).count();
        Self {
            snapshot,
            mode,
            executor,
            sink: None,
            trusted_handlers: Vec::new(),
            interaction: Arc::new(HeadlessToolBeforeInteraction),
            facts: ToolBeforeRuntimeFacts::new(registered_handlers),
            spec030_facts: None,
        }
    }

    pub fn with_sink(mut self, sink: PluginHookDispatchSink) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn with_trusted_handlers(mut self, handlers: Vec<Arc<dyn ToolBeforeHandler>>) -> Self {
        self.trusted_handlers = handlers;
        self.refresh_facts();
        self
    }

    pub fn with_interaction(mut self, interaction: Arc<dyn ToolBeforeInteraction>) -> Self {
        self.interaction = interaction;
        self
    }

    pub fn with_spec030_fact_store(mut self, facts: Spec030FactStore) -> Self {
        if let Some(projection) = facts.hook_projection() {
            self.facts.restore_history(&projection);
        }
        facts.publish_hooks(self.facts.projection());
        self.spec030_facts = Some(facts);
        self
    }

    pub fn hook_runtime_projection(&self) -> HookRuntimeProjection {
        self.facts.projection()
    }

    pub fn dispatch_llm_after(
        &self,
        context: &AgentHookContext,
        response: &LlmResponse,
    ) -> Option<PluginHookDispatchSummary> {
        event_dispatch::dispatch_event(
            self,
            PluginHookEvent::LlmAfter,
            llm_after_context_payload(context, response),
        )
    }

    pub fn dispatch_tool_before(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Option<PluginHookDispatchSummary> {
        self.run_tool_before(context, calls).1
    }

    pub fn blocked_tool_messages(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.run_tool_before(context, calls).0
    }

    fn run_tool_before(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> (Vec<RuntimeToolMessage>, Option<PluginHookDispatchSummary>) {
        let mut attempts = Vec::new();
        let mut messages = Vec::new();
        let entries = self.ordered_entries();
        for call in calls {
            for entry in &entries {
                let outcome = match entry {
                    HandlerEntry::Command { plugin, hook, .. } => {
                        evaluation::run_command(self, context, call, plugin, hook, &mut attempts)
                    }
                    HandlerEntry::Trusted { handler, .. } => {
                        evaluation::run_trusted(self, call, handler)
                    }
                };
                if let Some((hook_ref, reason, denial)) = outcome {
                    self.facts.denial(&hook_ref, &call.id, denial);
                    messages.push(blocked_message(call, &hook_ref, &reason));
                    break;
                }
            }
        }
        let summary = (!attempts.is_empty())
            .then(|| summarize_plugin_hook_dispatch(PluginHookEvent::ToolBefore, attempts));
        if let (Some(sink), Some(summary)) = (&self.sink, &summary) {
            sink(summary.clone());
        }
        if let Some(facts) = &self.spec030_facts {
            facts.publish_hooks(self.facts.projection());
        }
        (messages, summary)
    }

    fn ordered_entries(&self) -> Vec<HandlerEntry<'_>> {
        let mut entries = command_handlers(&self.snapshot)
            .map(|(plugin, hook)| HandlerEntry::Command {
                key: ToolBeforeOrderKey::new(hook.plugin_id.clone()),
                plugin,
                hook,
            })
            .chain(
                self.trusted_handlers
                    .iter()
                    .map(|handler| HandlerEntry::Trusted {
                        key: handler.order_key(),
                        handler,
                    }),
            )
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.key()
                .cmp(right.key())
                .then_with(|| left.hook_ref().cmp(right.hook_ref()))
        });
        entries
    }

    fn refresh_facts(&mut self) {
        self.facts = ToolBeforeRuntimeFacts::new(
            command_handlers(&self.snapshot).count() + self.trusted_handlers.len(),
        );
        if let Some(facts) = &self.spec030_facts {
            facts.publish_hooks(self.facts.projection());
        }
    }
}

impl AgentHook for PluginRuntimeHookAgentHook {
    fn receives_tool_arguments(&self) -> bool {
        true
    }

    fn block_tool_calls(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.blocked_tool_messages(context, calls)
    }

    fn after_response(&self, context: &AgentHookContext, response: &LlmResponse) {
        let _ = self.dispatch_llm_after(context, response);
    }
}

fn blocked_message(call: &RuntimeToolCall, hook_ref: &str, reason: &str) -> RuntimeToolMessage {
    RuntimeToolMessage {
        tool_call_id: call.id.clone(),
        name: call.name.clone(),
        content: format!(
            "Error: Tool `{}` blocked by plugin hook `{hook_ref}` for event `tool:before`: {reason}{TOOL_BLOCK_ERROR_HINT}",
            call.name
        ),
    }
}
