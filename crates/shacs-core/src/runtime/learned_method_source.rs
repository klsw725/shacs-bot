use super::learned_method::{EvidenceKind, MethodEvidence, RunOutcome};
use super::learned_method_evidence::digest;
use super::runner::is_tool_unsuccessful_message;
use super::task_method_report_source::failure;
use super::RuntimeToolMessage;
use serde_json::{Map, Value};

/// Metadata of user-role messages that the runtime injected for someone else.
const NOT_FROM_OWNER: [&str; 3] = ["injected_event", "goal_continuation", "automation_route"];
/// Set by the runtime on a stored user-role message the user typed. A turn
/// from a system, cron or automation channel stores no channel, so without
/// this mark it is never taken for the user's words.
pub(crate) const OWNER_TURN_KEY: &str = "owner_turn";

pub(super) fn injected(metadata: &Map<String, Value>) -> bool {
    NOT_FROM_OWNER.iter().any(|key| metadata.contains_key(*key))
}

pub(super) fn marked_owner_turn(message: &Value) -> bool {
    message.get(OWNER_TURN_KEY) == Some(&Value::Bool(true))
}

pub(super) fn role(message: &Value) -> &str {
    message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub(super) fn content(message: &Value) -> &str {
    message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

pub(super) fn timestamp(message: &Value) -> Option<&str> {
    message.get("timestamp").and_then(Value::as_str)
}

pub(super) fn calls(message: &Value) -> Option<&Vec<Value>> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .filter(|calls| !calls.is_empty())
}

pub(super) fn call_id(call: &Value) -> &str {
    call.get("id").and_then(Value::as_str).unwrap_or_default()
}

pub(super) fn arguments(call: &Value) -> String {
    match call
        .get("function")
        .and_then(|function| function.get("arguments"))
    {
        Some(Value::String(arguments)) => arguments.clone(),
        Some(arguments) => arguments.to_string(),
        None => String::new(),
    }
}

/// The result of the call made in `messages[index]`: the last tool message
/// with its ID before any later call reuses that ID. A call whose ID is
/// missing or repeated in the same batch has no result it can be tied to.
pub(super) fn paired_result<'a>(
    messages: &'a [Value],
    index: usize,
    call: &Value,
) -> Option<&'a Value> {
    let id = call_id(call);
    let batch = calls(&messages[index])?;
    if id.is_empty() || batch.iter().filter(|other| call_id(other) == id).count() > 1 {
        return None;
    }
    messages[index + 1..]
        .iter()
        .take_while(|message| {
            calls(message).is_none_or(|calls| calls.iter().all(|other| call_id(other) != id))
        })
        .filter(|message| {
            role(message) == "tool"
                && message.get("tool_call_id").and_then(Value::as_str) == Some(id)
        })
        .last()
}

/// Only an output with no sign of failure counts as a run that went through.
/// Withheld output (Spec037 quiet records) cannot show that, so it counts as
/// unfinished unless it is marked as an error. A process run reports its exit
/// code on the last line; `exec` output without one is unfinished. A read
/// answered from the read cache read nothing in this task, so it is
/// unfinished too.
pub(super) fn outcome(tool: &str, result: Option<&str>) -> RunOutcome {
    let Some(head) = result.map(str::trim_start) else {
        return RunOutcome::Incomplete;
    };
    let exit_code = head
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| line.trim().strip_prefix("Exit code:"))
        .and_then(|code| code.trim().parse::<i64>().ok());
    if head.is_empty()
        || head.starts_with("[Tool ")
        || head.starts_with("Permission approval required")
        || head.starts_with("<withheld:")
        || head.starts_with("[File unchanged since last read:")
    {
        RunOutcome::Incomplete
    } else if failure(head).is_some()
        || exit_code.is_some_and(|code| code != 0)
        || refused_by_cron(tool, head)
        || is_tool_unsuccessful_message(&RuntimeToolMessage {
            tool_call_id: String::new(),
            name: tool.to_owned(),
            content: head.to_owned(),
        })
    {
        RunOutcome::Error
    } else if tool == "exec" && exit_code.is_none() {
        RunOutcome::Incomplete
    } else {
        RunOutcome::Ok
    }
}

/// The cron tool reports a refused or missing job without an error prefix.
fn refused_by_cron(tool: &str, head: &str) -> bool {
    tool == "cron"
        && (head.starts_with("Cannot remove job")
            || (head.starts_with("Job ") && head.trim_end().ends_with(" not found")))
}

pub(super) fn call_ref(call: &Value) -> String {
    format!("#call-{}", &digest(call_id(call))[7..19])
}

/// The message index in a source ref as `collect` writes it for the session
/// whose key digest is `origin`: `session://<12 hex>/<index>`, followed by
/// `#call-<12 hex>` for a tool call and by nothing otherwise.
pub(super) fn ref_index(source_ref: &str, origin: &str, tool_call: bool) -> Option<usize> {
    let hex = |text: &str| {
        text.len() == 12
            && text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    };
    let rest = source_ref
        .strip_prefix("session://")?
        .strip_prefix(origin.get(7..19)?)?
        .strip_prefix('/')?;
    let (index, call) = match rest.split_once('#') {
        Some((index, call)) => (index, Some(call)),
        None => (rest, None),
    };
    let well_formed = match call {
        Some(call) => tool_call && call.strip_prefix("call-").is_some_and(hex),
        None => !tool_call,
    };
    let digits = !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit());
    (well_formed && digits)
        .then(|| index.parse().ok())
        .flatten()
}

/// Whether `message` is the one an evidence item came from, as written
/// (same kind, and same text or arguments; the time is matched by the
/// caller), and the call in it for a tool call.
pub(super) fn source_in<'a>(
    message: &'a Value,
    record: &MethodEvidence,
) -> Option<Option<&'a Value>> {
    match record.kind {
        EvidenceKind::UserMessage => {
            (role(message) == "user" && digest(content(message)) == record.digest).then_some(None)
        }
        EvidenceKind::Answer => (role(message) == "assistant"
            && calls(message).is_none()
            && digest(content(message)) == record.digest)
            .then_some(None),
        EvidenceKind::ToolCall => calls(message)?
            .iter()
            .find(|call| {
                record.source_ref.ends_with(&call_ref(call))
                    && digest(&arguments(call)) == record.digest
            })
            .map(Some),
    }
}
