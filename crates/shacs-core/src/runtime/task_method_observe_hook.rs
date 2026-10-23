use super::{Capture, MethodObservation};
use crate::runtime::{
    AgentHook, AgentHookContext, AgentRunSpec, CompositeHook, RuntimeToolCall, RuntimeToolMessage,
};
use shacs_providers::LlmResponse;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

struct Observed {
    capture: Arc<Capture>,
    rest: CompositeHook,
}

impl AgentHook for Observed {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn wants_streaming(&self) -> bool {
        self.rest.wants_streaming()
    }
    fn before_iteration(&self, context: &AgentHookContext) {
        self.rest.before_iteration(context);
    }
    fn on_stream(&self, context: &AgentHookContext, text: &str) {
        self.rest.on_stream(context, text);
    }
    fn on_stream_end(&self, context: &AgentHookContext, resuming: bool) {
        self.rest.on_stream_end(context, resuming);
    }
    fn block_tool_calls(
        &self,
        context: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.capture.record_calls(calls);
        let blocked = self.rest.block_tool_calls(context, calls);
        self.capture.record_blocked(&blocked);
        blocked
    }
    fn after_response(&self, context: &AgentHookContext, response: &LlmResponse) {
        self.rest.after_response(context, response);
    }
    fn after_iteration(&self, context: &AgentHookContext) {
        self.rest.after_iteration(context);
    }
    fn finalize_content(&self, context: &AgentHookContext, content: String) -> String {
        self.rest.finalize_content(context, content)
    }
}

pub(crate) fn attach(spec: &mut AgentRunSpec<'_>, observed: Arc<Mutex<MethodObservation>>) {
    let capture = Arc::new(Capture {
        tools: spec.tools.clone(),
        calls: Mutex::new(BTreeMap::new()),
        observed,
        turn_id: spec
            .execution_scope
            .as_ref()
            .map(|scope| scope.turn_id.clone())
            .unwrap_or_default(),
        workspace: spec.workspace.clone(),
    });
    spec.agent_hook = Some(Arc::new(Observed {
        capture: capture.clone(),
        rest: CompositeHook::new(spec.agent_hook.take().into_iter().collect()),
    }));
    let callback = spec.tool_event_callback.take();
    spec.tool_event_callback = Some(Arc::new(move |event| {
        capture.event(event);
        if let Some(callback) = &callback {
            callback(event);
        }
    }));
}
