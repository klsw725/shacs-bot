use super::context::RUNTIME_CONTEXT_TAG;
use super::learned_method::{EvidenceKind, MethodEvidence, SourceTrust};
use super::learned_method_source::{
    arguments, call_ref, calls, content, injected, marked_owner_turn, outcome, paired_result, role,
    timestamp,
};
use super::quiet_tools::output_text;
use super::runner::LENGTH_RECOVERY_PROMPT;
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_session::Session;

const MAX_USER_TURNS: usize = 6;
const MAX_ITEMS: usize = 40;
const MESSAGE_CHARS: usize = 600;
const ARGUMENT_CHARS: usize = 240;
const MATERIAL_CHARS: usize = 4_000;
const REDACTION_MARGIN: usize = 256;

pub(super) struct Evidence {
    pub record: MethodEvidence,
    /// Bounded, redacted text; never stored, and shown to the generator only
    /// when it is trusted (see `learned_method_generate`).
    pub text: String,
    /// What a tool call brought back, never shown or stored; kept only to
    /// catch draft text copied from it.
    pub material: Option<String>,
}

pub(super) struct EvidenceSet {
    /// Digest of the session key.
    pub session: String,
    pub items: Vec<Evidence>,
    pub omitted: usize,
}

/// The recent task window (the last few user turns and everything after
/// them) out of the whole session. A tool call names its tool only when
/// `registered` knows that tool: the name is the model's text otherwise.
pub(super) fn collect(session: &Session, registered: &dyn Fn(&str) -> bool) -> EvidenceSet {
    let messages = &session.messages;
    let users: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| role(message) == "user")
        .map(|(index, _)| index)
        .collect();
    let start = users
        .len()
        .checked_sub(MAX_USER_TURNS)
        .map(|skip| users[skip])
        .or_else(|| users.first().copied())
        .unwrap_or(messages.len());
    let session_digest = digest(&session.key);
    let trusts = trusts(messages);
    let mut items = Vec::new();
    let mut omitted = 0;
    for (index, message) in messages.iter().enumerate() {
        let trust = trusts[index];
        let answer = role(message) == "assistant"
            && calls(message).is_none()
            && !content(message).trim().is_empty()
            && message.get("_interrupted").is_none();
        if index < start {
            omitted +=
                usize::from(role(message) == "user" || answer) + calls(message).map_or(0, Vec::len);
            continue;
        }
        let source = format!("session://{}/{index}", &session_digest[7..19]);
        match (role(message), calls(message)) {
            ("user", _) => {
                items.push(text(EvidenceKind::UserMessage, source, trust, message));
            }
            ("assistant", Some(calls)) => items.extend(
                calls
                    .iter()
                    .map(|call| tool_call(messages, index, call, (&source, trust), registered)),
            ),
            ("assistant", None) if answer => {
                items.push(text(EvidenceKind::Answer, source, trust, message));
            }
            _ => {}
        }
    }
    let overflow = items.len().saturating_sub(MAX_ITEMS);
    items.drain(..overflow);
    for (position, item) in items.iter_mut().enumerate() {
        item.record.id = format!("e{}", position + 1);
    }
    EvidenceSet {
        session: session_digest,
        items,
        omitted: omitted + overflow,
    }
}

/// The trust of each message: only the user's own turns are the owner's.
/// A model that has read untrusted material stays quarantined for the rest
/// of the session (Spec024): what it writes afterwards is untrusted.
pub(super) fn trusts(messages: &[Value]) -> Vec<SourceTrust> {
    let mut exposed = false;
    messages
        .iter()
        .map(|message| {
            let trust = match role(message) {
                "user" if from_owner(message) => SourceTrust::Owner,
                "user" => SourceTrust::Untrusted,
                _ if exposed => SourceTrust::Untrusted,
                _ => SourceTrust::Model,
            };
            exposed |= exposes(message);
            trust
        })
        .collect()
}

/// The evidence `collect` would make now from `messages[index]` (from `call`
/// in it, for a tool call), with the tool's own name.
pub(super) fn recorded(
    messages: &[Value],
    index: usize,
    call: Option<&Value>,
    kind: EvidenceKind,
    trust: SourceTrust,
) -> Evidence {
    match call {
        Some(call) => tool_call(messages, index, call, ("", trust), &|_| true),
        None => text(kind, String::new(), trust, &messages[index]),
    }
}

/// A message's text as the generator was shown it.
pub(super) fn shown_form(content: &str) -> String {
    bounded(content, MESSAGE_CHARS)
}

fn text(kind: EvidenceKind, source: String, trust: SourceTrust, message: &Value) -> Evidence {
    let mut record = record(kind, source, trust, content(message), attachments(message));
    record.at = timestamp(message).map(|at| bounded(at, 64));
    Evidence {
        record,
        text: shown_form(content(message)),
        material: None,
    }
}

fn tool_call(
    messages: &[Value],
    index: usize,
    call: &Value,
    (source, trust): (&str, SourceTrust),
    registered: &dyn Fn(&str) -> bool,
) -> Evidence {
    let name = call
        .get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| call.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let arguments = arguments(call);
    let result = paired_result(messages, index, call)
        .map(|message| output_text(message.get("content").unwrap_or(&Value::Null)));
    let mut record = record(
        EvidenceKind::ToolCall,
        format!("{source}{}", call_ref(call)),
        trust,
        &arguments,
        result.as_deref().map(digest),
    );
    record.at = timestamp(&messages[index]).map(|at| bounded(at, 64));
    record.tool = Some(if registered(name) {
        bounded(name, 64)
    } else {
        "unregistered tool".to_owned()
    });
    record.outcome = Some(outcome(name, result.as_deref()));
    Evidence {
        record,
        text: bounded(&arguments, ARGUMENT_CHARS),
        material: result.map(|result| result.chars().take(MATERIAL_CHARS).collect()),
    }
}

fn record(
    kind: EvidenceKind,
    source_ref: String,
    trust: SourceTrust,
    text: &str,
    material_digest: Option<String>,
) -> MethodEvidence {
    MethodEvidence {
        id: String::new(),
        kind,
        source_ref,
        at: None,
        trust,
        digest: digest(text),
        classified_as: None,
        tool: None,
        outcome: None,
        material_digest,
    }
}

fn attachments(message: &Value) -> Option<String> {
    message
        .get("media")
        .filter(|media| media.as_array().is_some_and(|media| !media.is_empty()))
        .map(|media| digest(&media.to_string()))
}

/// Only turns the runtime marked as typed by the user are the owner's words.
/// The other checks catch user-role messages it wrote itself (length
/// recovery, mid-turn injections that carry its runtime context block) or
/// injected on someone else's behalf, should one ever carry the mark.
fn from_owner(message: &Value) -> bool {
    let for_someone_else = message
        .get("metadata")
        .and_then(Value::as_object)
        .is_some_and(injected);
    match message.get("content") {
        Some(Value::String(content)) => {
            marked_owner_turn(message)
                && !for_someone_else
                && !content.starts_with(RUNTIME_CONTEXT_TAG)
                && content != LENGTH_RECOVERY_PROMPT
        }
        _ => false,
    }
}

fn exposes(message: &Value) -> bool {
    let role = role(message);
    role == "tool" || attachments(message).is_some() || (role == "user" && !from_owner(message))
}

pub(super) fn digest(text: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
}

/// Redacts a window wider than the limit so a secret cut at the limit is
/// still recognised, then the cut text once more: a cut through a
/// redaction marker would otherwise change when redacted again, and saved
/// text must not (see `learned_method_validate`).
pub(super) fn bounded(text: &str, limit: usize) -> String {
    let window: String = text.chars().take(limit + REDACTION_MARGIN).collect();
    let mut bounded: String = shacs_redaction::redact_string(&window)
        .chars()
        .take(limit)
        .collect();
    if text.chars().count() > limit {
        bounded.push_str(" [truncated]");
    }
    shacs_redaction::redact_string(&bounded)
}
