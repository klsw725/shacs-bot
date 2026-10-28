//! What an image or composed request lets out of its tool calls: progress
//! events, diagnostics (verbose logs, observability and plugin hooks), the
//! saved history and recovery checkpoints. The model writes a call's
//! arguments (an image prompt, file content, a command, a search query, even
//! a path or a call id), and a tool's output can carry its words too (a
//! command prints them, a file it wrote holds them); nothing checks those
//! words. So these keep only lengths and digests. Running the call, deciding
//! on its permission and recording it for the method keep everything as
//! written, and the saved history keeps call ids (the provider pairs calls
//! and outputs by them).

use super::{RuntimeToolCall, ToolEvent, ToolStatus};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

#[cfg(test)]
#[path = "quiet_tools_tests.rs"]
mod tests;

/// The name a copy of a call gets when the model was not offered its tool:
/// the name it wrote is its own word, and such a call never runs.
pub(crate) const UNOFFERED_TOOL: &str = "unoffered_tool";

/// Gives every call in `value` to a tool `offered` does not list the name
/// [`UNOFFERED_TOOL`]: in a tool message, in an assistant message's tool
/// calls, or in a single call entry. Call ids stay, so calls and their
/// outputs still pair.
pub(crate) fn rename_unoffered(value: &mut Value, offered: &dyn Fn(&str) -> bool) {
    fn rename(name: Option<&mut Value>, offered: &dyn Fn(&str) -> bool) {
        if let Some(name) = name.filter(|name| !name.as_str().is_some_and(offered)) {
            *name = Value::String(UNOFFERED_TOOL.to_owned());
        }
    }
    if value.get("role").and_then(Value::as_str) == Some("tool") {
        rename(value.get_mut("name"), offered);
    } else if let Some(calls) = value.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for call in calls {
            rename(
                call.get_mut("function").and_then(|f| f.get_mut("name")),
                offered,
            );
        }
    } else {
        rename(
            value.get_mut("function").and_then(|f| f.get_mut("name")),
            offered,
        );
    }
}

pub(crate) fn quiet_arguments(arguments: &Value) -> Value {
    let text = quiet_text(&arguments.to_string());
    Value::Object(Map::from_iter([(
        "arguments".to_owned(),
        Value::String(text),
    )]))
}

/// A provider's finish reason as diagnostics see it: one the runtime knows,
/// or `other` for any string the provider made up.
pub(crate) fn quiet_finish_reason(reason: &str) -> String {
    match reason {
        "stop" | "tool_calls" | "length" | "content_filter" | "error" => reason.to_owned(),
        _ => "other".to_owned(),
    }
}

/// A response's token counts as diagnostics see them: only those under the
/// names the runtime's providers report, so no key carries a provider's words.
pub(crate) fn quiet_usage(usage: &Value) -> Value {
    const COUNTS: [&str; 8] = [
        "prompt_tokens",
        "completion_tokens",
        "total_tokens",
        "cached_tokens",
        "input_tokens",
        "output_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ];
    let counts = usage.as_object().into_iter().flatten();
    Value::Object(
        counts
            .filter(|(key, count)| COUNTS.contains(&key.as_str()) && count.is_u64())
            .map(|(key, count)| (key.clone(), count.clone()))
            .collect(),
    )
}

/// An image's lifecycle as events show it: its status and sequence, under a
/// digest of the candidate id the provider chose.
pub(crate) fn quiet_media(
    seen: &shacs_providers::ProviderMediaLifecycleObservation,
) -> Option<shacs_providers::ProviderMediaLifecycleObservation> {
    use shacs_providers::{
        ProviderMediaLifecycleObservation as Seen, ProviderMediaLifecycleStatus as Status,
    };
    let digest = Sha256::digest(seen.candidate_id().as_str().as_bytes());
    let id = shacs_providers::ProviderMediaCandidateId::new(format!("sha256-{digest:x}")).ok()?;
    let sequence = seen.sequence();
    Some(match seen.status() {
        Status::Started => Seen::started(id),
        Status::Partial => Seen::partial(id, sequence.unwrap_or_default()),
        Status::Final => Seen::final_candidate(id, sequence.unwrap_or_default()),
        Status::Failed => Seen::failed(id, sequence),
        Status::Cancelled => Seen::cancelled(id, sequence),
    })
}

/// How the execution record of an image or composed request names a tool
/// call: by the full digest of its id, which the model may have written.
pub(crate) fn opaque_call_id(id: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(id.as_bytes()))
}

/// Quiets what a saved execution `record` still keeps of the model's words
/// (from before its request was an image or composed one) the way such a
/// request's runs keep it: the ids of tool calls, in each tool outcome's
/// identity and in the cause of each subagent spawned by a call, go by
/// [`opaque_call_id`], and a question the model asked by its digest.
pub(crate) fn quiet_execution_record(record: &mut Value) {
    let outcomes = record.get_mut("outcomes").and_then(Value::as_array_mut);
    for fact in outcomes
        .into_iter()
        .flatten()
        .filter_map(|outcome| outcome.get_mut("fact"))
    {
        if let Some(identity) = fact.get_mut("identity") {
            quiet_identity(identity);
        }
        let question = fact["detail"]
            .as_str()
            .filter(|detail| *detail != "Permission approval required.")
            .filter(|detail| !is_quiet_text(detail))
            .map(quiet_text);
        if let Some(question) = question.filter(|_| fact["outcome"]["domain"] == "tool") {
            fact["detail"] = Value::String(question);
        }
    }
    let pending = record.get_mut("pending").and_then(Value::as_array_mut);
    for pending in pending.into_iter().flatten() {
        if let Some(identity) = pending.get_mut("identity") {
            quiet_identity(identity);
        }
    }
}

fn quiet_identity(identity: &mut Value) {
    let Some(turn) = identity["scope"]["turn_id"].as_str() else {
        return;
    };
    let prefix = format!("tool:{turn}:");
    let raw = |id: &Value, prefix: &str| {
        id.as_str()
            .and_then(|id| id.strip_prefix(prefix))
            .filter(|call| !is_opaque_call_id(call))
            .map(str::to_owned)
    };
    if let Some(call) = raw(&identity["effect_id"], &prefix) {
        let opaque = format!("{prefix}{}", opaque_call_id(&call));
        identity["effect_id"] = Value::String(opaque.clone());
        identity["idempotency_key"] = Value::String(opaque);
        identity["correlation_id"] = Value::String(opaque_call_id(&call));
    }
    if let Some(call) = raw(&identity["causation_id"], &prefix) {
        identity["causation_id"] = Value::String(format!("{prefix}{}", opaque_call_id(&call)));
    }
}

/// Whether `id` is a digest [`opaque_call_id`] made, not an id the model
/// wrote to look like one.
fn is_opaque_call_id(id: &str) -> bool {
    id.strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && is_lower_hex(hex))
}

/// Whether `text` is exactly what [`quiet_text`] makes, not a text the model
/// wrote to look like it.
fn is_quiet_text(text: &str) -> bool {
    let text = text.strip_prefix("Error: ").unwrap_or(text);
    let shown = text
        .strip_prefix("<withheld: ")
        .and_then(|text| text.strip_suffix('>'))
        .and_then(|text| text.split_once(" chars, sha256:"));
    shown.is_some_and(|(count, hex)| {
        !count.is_empty()
            && count.bytes().all(|byte| byte.is_ascii_digit())
            && hex.len() == 12
            && is_lower_hex(hex)
    })
}

fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// A call id as diagnostics see it: the same digest wherever it shows, so a
/// call's start and finish still match.
pub(crate) fn quiet_id(id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(id.as_bytes()));
    format!("call-{}", &digest[..12])
}

pub(crate) fn quiet_tool_calls(calls: &[RuntimeToolCall]) -> Vec<RuntimeToolCall> {
    calls
        .iter()
        .map(|call| RuntimeToolCall {
            id: quiet_id(&call.id),
            name: call.name.clone(),
            arguments: quiet_arguments(&call.arguments),
        })
        .collect()
}

/// The length and digest of `text`, still marked as an error if it was one.
pub(crate) fn quiet_text(text: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    let error = if text.trim_start().starts_with("Error") {
        "Error: "
    } else {
        ""
    };
    format!(
        "{error}<withheld: {} chars, sha256:{}>",
        text.chars().count(),
        &digest[..12]
    )
}

/// A tool's output, as text whatever its form.
pub(crate) fn output_text(output: &Value) -> String {
    output
        .as_str()
        .map_or_else(|| output.to_string(), str::to_owned)
}

/// A saved tool message (role `tool`) with its output withheld; any other
/// message is left as it is.
pub(crate) fn quiet_tool_message(message: &mut Value) {
    if message.get("role").and_then(Value::as_str) != Some("tool") {
        return;
    }
    if let Some(output) = message.get("content") {
        let quiet = quiet_text(&output_text(output));
        message["content"] = Value::String(quiet);
    }
}

/// A progress event without the call's arguments or the tool's output. A
/// wait for approval keeps its runtime detail ("Permission approval
/// required.").
pub(crate) fn quiet_event(event: &ToolEvent) -> ToolEvent {
    let mut quiet = event.clone();
    quiet.call_id = event.call_id.as_deref().map(quiet_id);
    quiet.arguments = event.arguments.as_ref().map(quiet_arguments);
    if event.status != ToolStatus::Waiting {
        quiet.detail = quiet_text(&event.detail);
    }
    quiet.result = event
        .result
        .as_ref()
        .map(|result| Value::String(quiet_text(&output_text(result))));
    quiet
}
