//! What an image or composed run lets out as it goes, and what its turns
//! save: none of the model's words, nor tool calls' arguments or outputs
//! (see `quiet_tools`; the outputs a turn saves are held in `held`). The
//! method observation, attached after these, keeps the full view.

use super::*;
use crate::runtime::quiet_tools::{
    quiet_event, quiet_finish_reason, quiet_id, quiet_media, quiet_tool_message, quiet_usage,
    rename_unoffered, UNOFFERED_TOOL,
};
use crate::runtime::ToolEventCallback;

/// None of the model's words in an image or composed request leave the run
/// as it goes: text, reasoning and tool-call input deltas are held back (only
/// call starts, the finish and media events pass; the finish keeps only a
/// known finish reason and token counts), call starts and progress events of
/// calls to tools it was not offered are dropped, the others carry a digest
/// of the call id and neither arguments nor output, and runtime hooks get a
/// quiet view (see [`quiet`]). No thinking is asked for, and none from
/// earlier turns is sent: its blocks are the model's words, and a tool loop
/// continued after an approval could not give them back as the provider
/// requires. The execution record keeps none of the model's words either
/// (`AgentRunSpec::quiet_ledger`). A run of a request that is no longer an
/// image or composed one while it holds what it saved as one still does, and
/// gives its hooks the messages of those calls the same quiet way
/// ([`quiet::Unheld`]). Call before the method observation is attached,
/// which keeps the full view.
pub(in crate::runtime::agent_loop) fn hold_model_text(
    session: &Session,
    spec: &mut AgentRunSpec<'_>,
) -> std::io::Result<()> {
    use shacs_providers::ProviderEvent;
    if reported(session)?.is_none() {
        let Some(held) = super::held::held_view(session)? else {
            return Ok(());
        };
        spec.quiet_ledger = true;
        if !held.outputs.is_empty() || !held.calls.is_empty() {
            if let Some(rest) = spec.agent_hook.take() {
                spec.agent_hook = Some(Arc::new(super::quiet::Unheld { rest, held }));
            }
        }
        return Ok(());
    }
    spec.settings.reasoning_effort = None;
    spec.quiet_ledger = true;
    for message in &mut spec.initial_messages {
        if let Some(message) = message.as_object_mut() {
            message.remove("thinking_blocks");
        }
    }
    let offered = spec.tools.tool_names();
    if let Some(callback) = spec.provider_event_callback.take() {
        let known = offered.clone();
        spec.provider_event_callback = Some(Arc::new(move |event: &ProviderEvent| match event {
            ProviderEvent::ToolCallStart { id, name } if known.contains(name) => {
                callback(&ProviderEvent::ToolCallStart {
                    id: quiet_id(id),
                    name: name.clone(),
                })
            }
            ProviderEvent::Finish { usage, reason } => callback(&ProviderEvent::Finish {
                usage: quiet_usage(usage),
                reason: quiet_finish_reason(reason),
            }),
            ProviderEvent::MediaLifecycle(seen) => {
                if let Some(seen) = quiet_media(seen) {
                    callback(&ProviderEvent::MediaLifecycle(seen));
                }
            }
            _ => {}
        }));
    }
    if let Some(rest) = spec.agent_hook.take() {
        spec.agent_hook = Some(Arc::new(super::quiet::Quiet {
            rest,
            offered: offered.clone(),
        }));
    }
    if let Some(callback) = spec.tool_event_callback.take() {
        spec.tool_event_callback = Some(Arc::new(move |event: &crate::runtime::ToolEvent| {
            if event.call_id.is_none() {
                callback(event);
            } else if offered.contains(&event.name) {
                callback(&quiet_event(event));
            }
        }));
    }
    Ok(())
}

/// The progress callback for calls that run after an approval, outside a run:
/// in an image or composed request their events carry neither arguments nor
/// output.
pub(in crate::runtime::agent_loop) fn quiet_events(
    session: &Session,
    callback: Option<ToolEventCallback>,
) -> std::io::Result<Option<ToolEventCallback>> {
    if reported(session)?.is_none() {
        return Ok(callback);
    }
    Ok(callback.map(|callback| -> ToolEventCallback {
        Arc::new(move |event: &crate::runtime::ToolEvent| callback(&quiet_event(event)))
    }))
}

/// Checkpoints of an image or composed turn, which stay on disk if the
/// process stops mid-run and are replayed into the history on recovery, hold
/// none of the model's words: its text, reasoning and thinking are dropped,
/// tool outputs keep only their digests, and calls to tools it was not
/// offered keep no name of its own (the runner already redacts call
/// arguments). After a recovery the model sees those digests, not the outputs.
pub(in crate::runtime::agent_loop) fn quiet_checkpoints(
    session: &Session,
    spec: &mut AgentRunSpec<'_>,
) -> std::io::Result<()> {
    if reported(session)?.is_none() {
        return Ok(());
    }
    let offered = spec.tools.tool_names();
    if let Some(callback) = spec.checkpoint_callback.take() {
        spec.checkpoint_callback = Some(Arc::new(move |checkpoint: &Value| {
            let known = |name: &str| offered.iter().any(|offered| offered == name);
            let mut quiet = checkpoint.clone();
            if let Some(message) = quiet.get_mut("assistant_message") {
                rename_unoffered(message, &known);
                if let Some(message) = message.as_object_mut() {
                    message.insert("content".to_owned(), Value::String(String::new()));
                    message.remove("reasoning_content");
                    message.remove("thinking_blocks");
                }
            }
            for key in ["completed_tool_results", "pending_tool_calls"] {
                let entries = quiet.get_mut(key).and_then(Value::as_array_mut);
                for entry in entries.into_iter().flatten() {
                    quiet_tool_message(entry);
                    rename_unoffered(entry, &known);
                }
            }
            callback(&quiet);
        }));
    }
    Ok(())
}

/// Clears the model's text from the messages a run added to an image or
/// composed request: text sent with a call was written before any part was
/// checked, and a call to a tool it was not `offered` (which failed) keeps
/// neither its arguments nor its name (it shows as `UNOFFERED_TOOL`, in its
/// output message and in the tools the run reports as `used` too). Reasoning
/// text goes too, and thinking blocks when the messages are saved (see
/// `hold_results`; none is asked for, see [`hold_model_text`]). The final
/// reply is replaced by [`answer`]
/// with the runtime's answer (for a composed request, the report with the
/// runtime's checks on its lines).
pub(in crate::runtime::agent_loop) fn withhold_run_text(
    session: &Session,
    added: &mut [Value],
    used: &mut [String],
    offered: &ToolRegistry,
) -> std::io::Result<()> {
    if reported(session)?.is_none() {
        return Ok(());
    }
    for name in used.iter_mut().filter(|name| !offered.has(name)) {
        *name = UNOFFERED_TOOL.to_owned();
    }
    for message in added {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            message["content"] = Value::String(String::new());
            if let Some(message) = message.as_object_mut() {
                message.remove("reasoning_content");
            }
            let calls = message.get_mut("tool_calls").and_then(Value::as_array_mut);
            for function in calls
                .into_iter()
                .flatten()
                .filter_map(|call| call.get_mut("function"))
            {
                let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                if !offered.has(name) {
                    function["arguments"] = Value::String("{}".to_owned());
                }
            }
        }
        rename_unoffered(message, &|name| offered.has(name));
    }
    Ok(())
}
