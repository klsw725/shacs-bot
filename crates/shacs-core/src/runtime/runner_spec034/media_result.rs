use super::public_result::AgentRunResult as PublicAgentRunResult;
use crate::generated_media::GeneratedArtifactRef;
use crate::runtime::runner::AgentRunResult as AgentRunCoreResult;
use crate::runtime::{AgentRunSpec, AgentRunner, RuntimeToolMessage};
use serde_json::Value;
use shacs_providers::ProviderError;
use std::cell::RefCell;

thread_local! {
    static ACTIVE_ARTIFACT_SINKS: RefCell<Vec<Vec<GeneratedArtifactRef>>> =
        const { RefCell::new(Vec::new()) };
}

impl AgentRunner {
    pub fn run(&self, spec: AgentRunSpec<'_>) -> Result<PublicAgentRunResult, ProviderError> {
        let collection = MediaArtifactCollection::start();
        let result = self.run_core(spec)?;
        Ok(materialize_run_result(result, collection.finish()))
    }
}

pub(crate) fn prepare_run(mut spec: AgentRunSpec<'_>) -> (AgentRunSpec<'_>, Vec<Value>) {
    if let Some(cancellation_token) = &spec.cancellation_token {
        spec.tool_context.cancellation_token = Some(cancellation_token.clone());
    }
    if let Some(deadline) = spec.deadline {
        spec.tool_context.deadline = Some(deadline);
    }
    let messages = spec.initial_messages.clone();
    (spec, messages)
}

pub(crate) fn has_generated_artifacts() -> bool {
    ACTIVE_ARTIFACT_SINKS.with(|sinks| {
        sinks
            .borrow()
            .last()
            .is_some_and(|sink| !sink.is_empty())
    })
}

pub(super) fn observe_tool_message(message: &RuntimeToolMessage) {
    if message.name != "image_generate" {
        return;
    }
    let Some(items) = serde_json::from_str::<Value>(&message.content)
        .ok()
        .and_then(|value| value.get("generatedArtifacts").cloned())
        .and_then(|value| value.as_array().cloned())
    else {
        return;
    };
    ACTIVE_ARTIFACT_SINKS.with(|sinks| {
        let mut sinks = sinks.borrow_mut();
        let Some(artifacts) = sinks.last_mut() else {
            return;
        };
        for item in items {
            let Ok(artifact) = serde_json::from_value::<GeneratedArtifactRef>(item) else {
                continue;
            };
            if artifacts
                .iter()
                .any(|existing| existing.artifact_id == artifact.artifact_id)
            {
                continue;
            }
            artifacts.push(artifact);
        }
    });
}

struct MediaArtifactCollection {
    active: bool,
}

impl MediaArtifactCollection {
    fn start() -> Self {
        ACTIVE_ARTIFACT_SINKS.with(|sinks| sinks.borrow_mut().push(Vec::new()));
        Self { active: true }
    }

    fn finish(mut self) -> Vec<GeneratedArtifactRef> {
        let artifacts = ACTIVE_ARTIFACT_SINKS
            .with(|sinks| sinks.borrow_mut().pop())
            .unwrap_or_default();
        self.active = false;
        artifacts
    }
}

impl Drop for MediaArtifactCollection {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_ARTIFACT_SINKS.with(|sinks| {
                sinks.borrow_mut().pop();
            });
        }
    }
}

fn materialize_run_result(
    result: AgentRunCoreResult,
    generated_artifacts: Vec<GeneratedArtifactRef>,
) -> PublicAgentRunResult {
    PublicAgentRunResult {
        final_content: result.final_content,
        messages: result.messages,
        tools_used: result.tools_used,
        usage: result.usage,
        stop_reason: result.stop_reason,
        error: result.error,
        error_message: result.error_message,
        interrupt: result.interrupt,
        tool_events: result.tool_events,
        had_injections: result.had_injections,
        generated_artifacts,
        recent_auto_mode_denials: result.recent_auto_mode_denials,
        recent_auto_mode_retry_tokens: result.recent_auto_mode_retry_tokens,
    }
}
