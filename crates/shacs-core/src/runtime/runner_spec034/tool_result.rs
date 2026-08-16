use super::media_result::observe_tool_message;
use crate::runtime::{AgentRunSpec, RuntimeToolMessage};
use shacs_utils::tool_results::{
    maybe_persist_text_tool_result_with_artifact, ToolResultArtifactRef,
};

pub(crate) struct NormalizedToolMessage {
    pub(crate) message: RuntimeToolMessage,
    pub(crate) artifact_ref: Option<ToolResultArtifactRef>,
}

pub(crate) fn normalize_tool_message(
    spec: &AgentRunSpec<'_>,
    mut message: RuntimeToolMessage,
) -> NormalizedToolMessage {
    observe_tool_message(&message);
    if message.content.trim().is_empty() {
        message.content = format!("({} completed with no output)", message.name);
    }
    let artifact_outcome = maybe_persist_text_tool_result_with_artifact(
        spec.workspace.as_deref(),
        spec.session_key.as_deref(),
        &message.tool_call_id,
        &message.content,
        spec.max_tool_result_chars,
    );
    let artifact_ref = artifact_outcome
        .as_ref()
        .and_then(|outcome| outcome.artifact.clone());
    message.content = artifact_outcome
        .map(|outcome| outcome.content)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| truncate_text(&message.content, spec.max_tool_result_chars));
    NormalizedToolMessage {
        message,
        artifact_ref,
    }
}

fn truncate_text(content: &str, max_chars: usize) -> String {
    if max_chars == 0 || content.chars().count() <= max_chars {
        return content.to_owned();
    }
    let mut truncated = content.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n... (truncated)");
    truncated
}
