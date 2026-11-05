//! Auto mode across a session (Spec 048 PRD 020): the block counts the
//! session keeps, the system guidance, and the approval `/permission recent`
//! adds.

use super::AgentLoopConfig;
use crate::runtime::auto_mode::retry_approval;
use crate::runtime::{AutoModeBlockCounts, AutoModeBlocks, PermissionMode, PermissionedAction};
use serde_json::{Map, Value};
use shacs_session::Session;

pub(super) const BLOCK_COUNTS_KEY: &str = "auto_mode_block_counts";
/// The longest arguments `/permission recent` approves: the approval must
/// reach the classifier whole (Spec 048 PRD 020 scope 6).
pub(super) const MAX_RETRY_ARGUMENT_CHARS: usize = 3_000;
const GUIDANCE: &str = "\n\n---\n\n# Auto Mode\n\nAuto mode is on: a classifier checks your tool calls instead of the user approving each one. Go ahead with routine steps without asking the user to confirm them. Ask only when the user's request or a skill you are following says to confirm first.";

/// A turn that decides by the auto mode table: auto mode with a permission
/// environment.
pub(super) fn active(config: &AgentLoopConfig) -> bool {
    config.permission_mode_snapshot.mode == PermissionMode::Auto
        && config.permission_environment.is_some()
}

/// The session's counts. A missing or unreadable entry counts from zero:
/// blocked calls still do not run, only the question comes later.
pub(super) fn blocks(session: &Session) -> AutoModeBlocks {
    AutoModeBlocks::new(stored_counts(session).unwrap_or_default())
}

pub(super) fn store_blocks(
    config: &AgentLoopConfig,
    session: &mut Session,
    blocks: &AutoModeBlocks,
) {
    if active(config) {
        store_counts(session, blocks.counts());
    }
}

/// The user approved a call, so the row of stopped calls starts over.
pub(super) fn clear_consecutive(session: &mut Session) {
    if let Some(mut counts) = stored_counts(session) {
        counts.consecutive = 0;
        store_counts(session, counts);
    }
}

/// Adds the auto mode guidance to the end of the run's system prompt.
pub(super) fn guide(config: &AgentLoopConfig, messages: &mut [Value]) {
    if !active(config) {
        return;
    }
    if let Some(system) = messages
        .first_mut()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
    {
        let content = system
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        system["content"] = Value::String(format!("{content}{GUIDANCE}"));
    }
}

/// The tool name and arguments `/permission recent` shows and approves: the
/// arguments the user may see, with `.env.json` values masked. `None` when
/// they are too long to reach the classifier whole.
pub(super) fn shown_call(action: &PermissionedAction) -> Option<(String, String)> {
    let arguments = action.redacted_arguments.to_string();
    (arguments.chars().count() <= MAX_RETRY_ARGUMENT_CHARS)
        .then(|| (action.tool_name.clone(), arguments))
}

/// Adds the approval as the user's message, which the assistant reads and
/// the classifier sees as an approval entry.
pub(super) fn add_approval(session: &mut Session, tool: &str, arguments: &str) {
    session.add_message("user", retry_approval::message(tool, arguments), Map::new());
}

fn stored_counts(session: &Session) -> Option<AutoModeBlockCounts> {
    session
        .metadata
        .get(BLOCK_COUNTS_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn store_counts(session: &mut Session, counts: AutoModeBlockCounts) {
    if let Ok(value) = serde_json::to_value(counts) {
        session.metadata.insert(BLOCK_COUNTS_KEY.to_owned(), value);
    }
}
