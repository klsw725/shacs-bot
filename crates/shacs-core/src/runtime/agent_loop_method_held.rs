//! The tool outputs an image or composed request saves: the history keeps
//! their digests (see `quiet_tools`), while the outputs as written stay in
//! session metadata and are given back to the model in the request's own
//! later runs. `MethodStore::save` drops them once the request is not active.

use super::*;
use crate::runtime::quiet_tools::{
    output_text, quiet_execution_record, quiet_text, quiet_tool_message, rename_unoffered,
};
use crate::runtime::task_method_store::HELD_RESULTS_KEY;

#[cfg(test)]
#[path = "agent_loop_method_held_tests.rs"]
mod tests;

/// Withholds the outputs of the tool messages saved from `from` on in an image
/// or composed request: the history keeps their digests, while the outputs as
/// written stay in session metadata for the request's own later runs (see
/// [`restore_results`]).
pub(in crate::runtime::agent_loop) fn hold_results(
    session: &mut Session,
    from: usize,
) -> std::io::Result<()> {
    if let Some(request) = reported(session)? {
        hold_from(session, &request.request_id, from);
    }
    Ok(())
}

/// Where a request's own messages begin in `messages`: at its goal message
/// (the first user message whose text has the goal's digest, which may sit
/// in an earlier request and then only widens the range), or, if that is
/// gone, at the first message saved since the request began. `None` when
/// neither can be found.
pub(in crate::runtime::agent_loop) fn request_start(
    messages: &[Value],
    goal_digest: &str,
    started_at: Option<&str>,
) -> Option<usize> {
    let goal = messages.iter().position(|message| {
        message.get("role").and_then(Value::as_str) == Some("user")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|text| format!("sha256:{:x}", Sha256::digest(text)) == goal_digest)
    });
    let parse = |at: &str| chrono::DateTime::parse_from_rfc3339(at).ok();
    goal.or_else(|| {
        let started = parse(started_at?)?;
        messages.iter().position(|message| {
            let at = message.get("timestamp").and_then(Value::as_str);
            at.and_then(parse).is_some_and(|at| at >= started)
        })
    })
}

/// The answer when a method change would make a request an image or
/// composed one but its earlier messages cannot be found to clear.
pub(in crate::runtime::agent_loop) const NOT_CLEARED: &str = "Method not changed: this request's earlier messages could not be found, so they could not be cleared for an image or composed request. Start a new request instead; the current selection is unchanged.";

/// Where `request`'s saved messages to clear begin when the `next` selection
/// makes it an image or composed one (`Some(None)` when `next` does not);
/// `None` when that start cannot be found, so the change is refused.
pub(in crate::runtime::agent_loop) fn reported_start(
    session: &Session,
    request: &MethodRequest,
    next: &Selection,
) -> Option<Option<usize>> {
    if !matches!(
        next.method,
        MethodId::ImageGenerationReview | MethodId::Composed
    ) {
        return Some(None);
    }
    let start = request_start(
        &session.messages,
        &request.goal_digest,
        request.started_at.as_deref(),
    );
    start.map(Some)
}

/// When a method change makes `request` an image or composed one, what it
/// saved from `from` on (see [`request_start`]) goes the same way as a run's:
/// the model's text and thinking in its assistant messages, its tool outputs
/// (held, see [`hold_results`]), and the names of calls to tools no one
/// registered in `tools` (which only the model could have written).
pub(in crate::runtime::agent_loop) fn quiet_request_history(
    session: &mut Session,
    request: &MethodRequest,
    from: usize,
    tools: &ToolRegistry,
) {
    for message in session.messages.iter_mut().skip(from) {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            message["content"] = Value::String(String::new());
            if let Some(message) = message.as_object_mut() {
                message.remove("reasoning_content");
            }
        }
        rename_unoffered(message, &|name| tools.has(name));
    }
    hold_from(session, &request.request_id, from);
    if let Some(record) = session.metadata.get_mut(RUNTIME_EXECUTION_KEY) {
        quiet_execution_record(record);
    }
}

/// Holds the tool outputs saved from `from` on for request `id`, as a list
/// per tool call (a call id may repeat); an output already held shows as its
/// digest and is left as it is. The ids of the calls saved are noted too
/// (see [`Held`]). Thinking blocks are dropped, not held: these requests ask
/// for none (see `hold_model_text`), so none is given back.
fn hold_from(session: &mut Session, id: &str, from: usize) {
    let mut results = held(session, id, "results").unwrap_or_default();
    let mut calls = held(session, id, "calls").unwrap_or_default();
    for message in session.messages.iter_mut().skip(from) {
        if let Some(message) = message.as_object_mut() {
            message.remove("thinking_blocks");
        }
        if message.get("role").and_then(Value::as_str) == Some("tool") {
            hold_output(&mut results, message);
        }
        let saved = message.get("tool_calls").and_then(Value::as_array);
        for call in saved.into_iter().flatten() {
            if let Some(call) = call.get("id").and_then(Value::as_str) {
                calls.insert(call.to_owned(), Value::Bool(true));
            }
        }
    }
    let held = json!({"request_id": id, "results": results, "calls": calls});
    session.metadata.insert(HELD_RESULTS_KEY.to_owned(), held);
}

fn hold_output(results: &mut Map<String, Value>, message: &mut Value) {
    let Some(output) = message.get("content").cloned() else {
        return;
    };
    let call = message.get("tool_call_id").and_then(Value::as_str);
    let outputs = results
        .entry(call.unwrap_or_default().to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !outputs.is_array() {
        *outputs = Value::Array(vec![outputs.take()]);
    }
    let Some(outputs) = outputs.as_array_mut() else {
        return;
    };
    let shown = output_text(&output);
    if outputs
        .iter()
        .any(|held| quiet_text(&output_text(held)) == shown)
    {
        return;
    }
    outputs.push(output);
    quiet_tool_message(message);
}

/// Gives the active request's later runs the tool outputs its saved history
/// withholds (see [`hold_results`]), also after a method change; outputs held
/// for another request go.
pub(in crate::runtime::agent_loop) fn restore_results(
    session: &mut Session,
    messages: &mut [Value],
) -> std::io::Result<()> {
    match held_view(session)? {
        Some(held) => restore_from(&held.outputs, messages),
        None => {
            session.metadata.remove(HELD_RESULTS_KEY);
        }
    }
    Ok(())
}

/// What the active request holds from when it was an image or composed one:
/// the tool outputs its later runs give back to the model (also after a
/// method change), and the ids of the calls saved then. A later run of
/// another method gives its hooks those calls' messages as an image or
/// composed run does (see `quiet::Unheld`).
pub(in crate::runtime::agent_loop) struct Held {
    pub(in crate::runtime::agent_loop) outputs: Map<String, Value>,
    pub(in crate::runtime::agent_loop) calls: Map<String, Value>,
}

/// What the active request holds (see [`Held`]).
pub(in crate::runtime::agent_loop) fn held_view(
    session: &Session,
) -> std::io::Result<Option<Held>> {
    let active = MethodStore::load(session)?
        .active
        .map(|request| request.request_id);
    Ok(active.and_then(|id| {
        Some(Held {
            outputs: held(session, &id, "results")?,
            calls: held(session, &id, "calls").unwrap_or_default(),
        })
    }))
}

/// Puts back, into each tool message, the held output whose digest it shows.
fn restore_from(results: &Map<String, Value>, messages: &mut [Value]) {
    for message in messages {
        let call = message.get("tool_call_id").and_then(Value::as_str);
        let Some(outputs) = call.and_then(|call| results.get(call)) else {
            continue;
        };
        let shown = message.get("content").map(output_text);
        let outputs = outputs
            .as_array()
            .map_or(std::slice::from_ref(outputs), Vec::as_slice);
        let output = outputs
            .iter()
            .find(|output| shown.as_deref() == Some(quiet_text(&output_text(output)).as_str()));
        if let Some(output) = output {
            message["content"] = output.clone();
        }
    }
}

/// What request `id` holds in `section` (`results` or `calls`).
fn held(session: &Session, id: &str, section: &str) -> Option<Map<String, Value>> {
    let held = session.metadata.get(HELD_RESULTS_KEY)?;
    (held.get("request_id").and_then(Value::as_str) == Some(id))
        .then(|| held.get(section).and_then(Value::as_object).cloned())
        .flatten()
}
