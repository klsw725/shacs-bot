use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use shacs_session::Session;

use super::{action, learn, AgentLoop, AgentLoopError, AgentLoopTurnResult, InboundMessage};
use crate::runtime::learned_method_source::marked_owner_turn;
use crate::runtime::web_watch_conversation::{
    classify_reply, clear_pending_summary, demote_pending_summary, last_assistant_index,
    pending_summary, store_pending_summary, PendingWebWatchSummary, WebWatchReply,
};
use crate::runtime::web_watch_conversation_setup::{set_up, SetupOutcome};
use crate::runtime::web_watch_conversation_status::render_status;
use crate::runtime::web_watch_conversation_summary::{
    build_summary, proposal_after_other_content, proposal_from_another_sender,
    proposal_not_from_user, render_problem, SummaryContext,
};
use crate::runtime::web_watch_read::web_watch_snapshot;
use crate::runtime::web_watch_redact::safe_text;
use crate::runtime::web_watch_store::web_watches;
use crate::runtime::EntrustedWorkReadRequest;
use crate::tools::{
    ask_user_outbound, pending_ask_user_id, web_watch_proposal_from_tool_content,
    web_watch_status_request_from_tool_content, PROPOSE_WEB_WATCH_TOOL_NAME,
    WEB_WATCH_STATUS_TOOL_NAME,
};

static SUMMARY_NONCE: AtomicU64 = AtomicU64::new(0);

const TURN_OWNER_KEY: &str = "web_watch_turn_owner";

/// What a turn answers when it shows, asks again or settles a web watch summary.
pub(super) struct WebWatchAnswer {
    pub text: String,
    pub stop_reason: String,
    /// The summary this answer shows; it waits for its sender once the answer is stored.
    pub summary: Option<PendingWebWatchSummary>,
}

fn answer(text: impl Into<String>, stop_reason: &str) -> WebWatchAnswer {
    WebWatchAnswer {
        text: text.into(),
        stop_reason: stop_reason.to_owned(),
        summary: None,
    }
}

/// `text` followed by the summary that still waits, shown again so it stays the latest
/// message for its sender.
fn still_waiting(text: &str, stop_reason: &str, summary: PendingWebWatchSummary) -> WebWatchAnswer {
    WebWatchAnswer {
        text: format!("{text}\n\nStill waiting for an answer:\n{}", summary.text)
            .trim_start()
            .to_owned(),
        stop_reason: stop_reason.to_owned(),
        summary: Some(summary),
    }
}

impl AgentLoop<'_> {
    /// Answers a reply to the pending web watch summary before any provider call: only its
    /// sender's listed reply (summary still the latest message) or `/watch confirm <id> <rev>`
    /// for its current revision sets it up. A listed reply to a summary that is no longer the
    /// latest message, confirming or cancelling, answers the newer message instead. `None`
    /// lets the turn go on as usual.
    pub(super) fn web_watch_reply(
        &self,
        message: &InboundMessage,
        session: &mut Session,
    ) -> Result<Option<WebWatchAnswer>, AgentLoopError> {
        let reply = classify_reply(&message.content);
        let Some(pending) = pending_summary(session) else {
            return Ok(match reply {
                WebWatchReply::Confirm(Some(_))
                | WebWatchReply::Cancel(Some(_))
                | WebWatchReply::Invalid => Some(answer(
                    "No web watch summary is waiting in this conversation; nothing was set up.",
                    "web_watch_no_summary",
                )),
                _ => None,
            });
        };
        let now_ms = self.config.now_ms();
        let closed = pending.closed_reason(action::process_epoch(), now_ms);
        if reply == WebWatchReply::Other {
            if closed.is_some() {
                clear_pending_summary(session);
            }
            return Ok(None);
        }
        if !pending.matches_sender(message, session) {
            let text = "Only the person who asked for this web watch summary can answer it; \
                        nothing was set up.";
            return Ok(Some(match closed {
                Some(_) => answer(text, "web_watch_other_sender"),
                None => still_waiting(text, "web_watch_other_sender", pending),
            }));
        }
        if let Some(reason) = closed {
            clear_pending_summary(session);
            return Ok(Some(answer(
                format!(
                    "Web watch summary `{}` was closed because {reason}; nothing was set up. Ask \
                     again for a new summary.",
                    pending.summary_id
                ),
                "web_watch_summary_closed",
            )));
        }
        Ok(Some(match reply {
            WebWatchReply::Cancel(Some(id)) if id != pending.summary_id => still_waiting(
                "That is not the waiting summary; nothing changed.",
                "web_watch_stale_reply",
                pending,
            ),
            WebWatchReply::Confirm(None) | WebWatchReply::Cancel(None)
                if !pending.is_latest_response(session, &self.config.workspace) =>
            {
                return Ok(None)
            }
            WebWatchReply::Cancel(_) => {
                clear_pending_summary(session);
                answer(
                    format!(
                        "Dropped web watch summary `{}`; nothing was set up.",
                        pending.summary_id
                    ),
                    "web_watch_cancelled",
                )
            }
            WebWatchReply::Confirm(Some((id, revision)))
                if id != pending.summary_id || revision != pending.revision =>
            {
                still_waiting(
                    &format!(
                        "That confirmation is for summary `{}` revision {revision}, which is not \
                         the one waiting; nothing was set up.",
                        safe_text(&id, 64)
                    ),
                    "web_watch_stale_reply",
                    pending,
                )
            }
            WebWatchReply::Confirm(_) => {
                match set_up(session, &pending, self.config.web_watch.as_ref(), now_ms) {
                    SetupOutcome::Done(text) => {
                        clear_pending_summary(session);
                        answer(text, "web_watch_set_up")
                    }
                    SetupOutcome::Revised(revised) => WebWatchAnswer {
                        text: revised.text.clone(),
                        stop_reason: "web_watch_summary".to_owned(),
                        summary: Some(*revised),
                    },
                    SetupOutcome::Failed(reason) => {
                        clear_pending_summary(session);
                        answer(
                            format!(
                                "Web watch summary `{}` could not be set up, so nothing was set: \
                                 {reason}. Ask again for a new summary.",
                                pending.summary_id
                            ),
                            "web_watch_setup_failed",
                        )
                    }
                }
            }
            WebWatchReply::Invalid => still_waiting(
                "Use `/watch confirm <summary-id> <revision>` or `/watch cancel <summary-id>`; \
                 nothing was set up.",
                "web_watch_invalid",
                pending,
            ),
            WebWatchReply::Other => return Ok(None),
        }))
    }

    /// Publishes a deterministic web watch reply and keeps both messages in the history. While
    /// another question waits for its answer (an `ask_user` or action question, a permission
    /// approval), they stay out of the history so that question's open tool call is answered
    /// next, and a summary shown then is not the latest message: that question is newer.
    pub(super) fn publish_web_watch_reply(
        &mut self,
        message: &InboundMessage,
        mut session: Session,
        answer: WebWatchAnswer,
    ) -> Result<AgentLoopTurnResult, AgentLoopError> {
        let question_waits = pending_ask_user_id(&session.messages).is_some()
            || action::waiting(&session)
            || super::pending_permission_approval(&session).is_some()
            || super::pending_recent_retry_approval(&session).is_some();
        if !question_waits {
            session.add_message("user", message.content.clone(), Map::new());
            session.add_message("assistant", answer.text.clone(), Map::new());
        }
        let buttons = match answer.summary {
            Some(mut summary) => {
                summary.shown_at(
                    session.messages.len().saturating_sub(1),
                    &self.config.workspace,
                );
                store_pending_summary(&mut session, &summary)?;
                if question_waits {
                    demote_pending_summary(&mut session);
                }
                ask_user_outbound(None, &summary.buttons(), &message.channel).1
            }
            None => Vec::new(),
        };
        self.publish_command_response_with_buttons(
            message,
            session,
            &answer.text,
            buttons,
            &answer.stop_reason,
        )
    }

    /// After a model turn: a `propose_web_watch` call becomes the deterministic summary (or the
    /// reason it cannot be set up), a `web_watch_status` call becomes the stored state (before
    /// the summary when there are both), and a summary still waiting for this sender is asked
    /// again at the end. The answer replaces the turn's last assistant message, so a summary is
    /// the latest message when it is shown.
    pub(super) fn finish_web_watch_turn(
        &self,
        message: &InboundMessage,
        session: &mut Session,
        new_messages: &[Value],
        final_content: Option<&str>,
        call_start: usize,
    ) -> Option<WebWatchAnswer> {
        if let Some(shown) =
            self.web_watch_proposal_answer(message, session, new_messages, call_start)
        {
            return Some(shown);
        }
        let now_ms = self.config.now_ms();
        let epoch = action::process_epoch();
        let status = status_request(session, new_messages, call_start);
        let pending = pending_summary(session);
        if pending
            .as_ref()
            .is_some_and(|pending| pending.closed_reason(epoch, now_ms).is_some())
        {
            clear_pending_summary(session);
        }
        let waiting = pending.filter(|pending| {
            pending.closed_reason(epoch, now_ms).is_none()
                && pending.matches_sender(message, &*session)
        });
        let body = match status {
            Some(work_id) => Some(self.web_watch_status_text(session, work_id)),
            None => waiting
                .as_ref()
                .map(|_| final_content.unwrap_or_default().to_owned()),
        }?;
        Some(match waiting {
            Some(summary) => still_waiting(&body, "web_watch_summary", summary),
            None => answer(body, "web_watch_status"),
        })
    }

    /// Puts `answer` in place of the turn's final assistant answer (or adds it after the turn's
    /// tool messages, whose calls stay as they are) and, for a summary, stores it as waiting
    /// from that message. Returns the buttons to send with it.
    pub(super) fn record_web_watch_answer(
        &self,
        message: &InboundMessage,
        session: &mut Session,
        history_start: usize,
        answer: &WebWatchAnswer,
    ) -> Result<Vec<Vec<String>>, AgentLoopError> {
        let final_answer = last_assistant_index(session)
            .filter(|index| *index >= history_start && *index + 1 == session.messages.len())
            .filter(|index| session.messages[*index].get("tool_calls").is_none());
        let index = match final_answer {
            Some(index) => {
                if let Some(stored) = session.messages[index].as_object_mut() {
                    stored.insert("content".to_owned(), Value::String(answer.text.clone()));
                }
                index
            }
            None => {
                session.add_message("assistant", answer.text.clone(), Map::new());
                session.messages.len().saturating_sub(1)
            }
        };
        let Some(summary) = &answer.summary else {
            return Ok(Vec::new());
        };
        let mut summary = summary.clone();
        summary.shown_at(index, &self.config.workspace);
        store_pending_summary(session, &summary)?;
        Ok(ask_user_outbound(None, &summary.buttons(), &message.channel).1)
    }

    /// The last `propose_web_watch` call this message added, as its summary or the reason it is
    /// not used, after the stored facts when the turn also asked for them. Showing either closes
    /// the summary that was waiting.
    pub(super) fn web_watch_proposal_answer(
        &self,
        message: &InboundMessage,
        session: &mut Session,
        new_messages: &[Value],
        call_start: usize,
    ) -> Option<WebWatchAnswer> {
        let owned = owns_turn(session, message, call_start);
        let (start, _) = turn_start(session, call_start);
        let (tool_call_id, draft) = added_messages(session, new_messages, start)
            .rev()
            .find_map(|tool_message| {
                let content = tool_content(tool_message, PROPOSE_WEB_WATCH_TOOL_NAME)?;
                let tool_call_id = tool_message.get("tool_call_id").and_then(Value::as_str)?;
                Some((
                    tool_call_id.to_owned(),
                    web_watch_proposal_from_tool_content(content)?,
                ))
            })?;
        let status = status_request(session, new_messages, call_start)
            .map(|work_id| self.web_watch_status_text(session, work_id));
        let context = SummaryContext {
            session: &*session,
            message,
            config: self.config.web_watch.as_ref(),
            summary_id: fresh_summary_id(),
            evidence_ref: format!(
                "tool_call:{}",
                crate::runtime::runner_spec034::observable_call_id(&tool_call_id)
            ),
            epoch: action::process_epoch(),
            now_ms: self.config.now_ms(),
        };
        let turn_start = session
            .messages
            .iter()
            .rposition(marked_owner_turn)
            .unwrap_or(0);
        let other_content = new_messages
            .iter()
            .chain(&session.messages[turn_start..])
            .any(other_tool_output);
        let built = if !learn::owner_turn(message) {
            Err(proposal_not_from_user())
        } else if !owned {
            Err(proposal_from_another_sender())
        } else if other_content {
            Err(proposal_after_other_content())
        } else {
            build_summary(&context, &draft)
        };
        clear_pending_summary(session);
        let mut shown = match built {
            Ok(summary) => WebWatchAnswer {
                text: summary.text.clone(),
                stop_reason: "web_watch_summary".to_owned(),
                summary: Some(summary),
            },
            Err(problem) => answer(render_problem(&problem), "web_watch_summary_refused"),
        };
        if let Some(status) = status {
            shown.text = format!("{status}\n\n{}", shown.text);
        }
        Some(shown)
    }

    /// The stored facts for a `web_watch_status` call of a turn that did not end with its own
    /// final answer (it sent with the `message` tool or asked the user something).
    pub(super) fn web_watch_status_answer(
        &self,
        session: &Session,
        new_messages: &[Value],
        call_start: usize,
    ) -> Option<WebWatchAnswer> {
        let work_id = status_request(session, new_messages, call_start)?;
        Some(answer(
            self.web_watch_status_text(session, work_id),
            "web_watch_status",
        ))
    }

    fn web_watch_status_text(&self, session: &Session, work_id: Option<String>) -> String {
        let Some(config) = self.config.web_watch.as_ref() else {
            return "Web page watches are not available in this conversation.".to_owned();
        };
        let work_id = work_id.or_else(|| {
            web_watches(session)
                .ok()?
                .last()
                .map(|entry| entry.work_id.clone())
        });
        let Some(work_id) = work_id else {
            return "There is no web page watch in this conversation yet.".to_owned();
        };
        let request = EntrustedWorkReadRequest {
            owner_session_id: session.key.clone(),
            requester_target: session.key.clone(),
            work_id: work_id.clone(),
        };
        match web_watch_snapshot(session, &config.data_dir, &request) {
            Ok(snapshot) => render_status(&snapshot),
            Err(error) => format!(
                "The state of web watch `{}` could not be read: {}",
                safe_text(&work_id, 64),
                safe_text(&error.to_string(), 240)
            ),
        }
    }
}

/// Keeps the tool outputs a run's checkpoints report, so a run whose model request fails
/// afterwards can still answer its web watch calls: the web watch tools' outputs as they are,
/// any other tool's by name only (enough to tell that the turn read other content).
pub(super) fn keep_tool_outputs(kept: &Mutex<Vec<Value>>, checkpoint: &Value) {
    let Ok(mut kept) = kept.lock() else {
        return;
    };
    let outputs = checkpoint
        .get("completed_tool_results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for output in outputs {
        let call_id = output.get("tool_call_id");
        if kept.iter().any(|seen| seen.get("tool_call_id") == call_id) {
            continue;
        }
        let name = output.get("name").and_then(Value::as_str);
        kept.push(
            if matches!(
                name,
                Some(PROPOSE_WEB_WATCH_TOOL_NAME | WEB_WATCH_STATUS_TOOL_NAME)
            ) {
                output.clone()
            } else {
                serde_json::json!({"role": "tool", "tool_call_id": call_id, "name": name})
            },
        );
    }
}

/// What this message added: the run's new messages and, in the session, everything from where
/// this message began, which after a permission approval holds the approved tool's result.
fn added_messages<'a>(
    session: &'a Session,
    new_messages: &'a [Value],
    call_start: usize,
) -> impl DoubleEndedIterator<Item = &'a Value> {
    new_messages
        .iter()
        .chain(session.messages.get(call_start..).unwrap_or_default())
}

/// Where a turn's proposals are looked for, and whether this message went on with a turn that
/// stopped for the user (an answer, an approval): then from just after the last answer that
/// ended a turn, so a proposal made before the question is still answered.
fn turn_start(session: &Session, call_start: usize) -> (usize, bool) {
    let resumed = session
        .messages
        .get(call_start)
        .is_some_and(|message| message.get("role").and_then(Value::as_str) != Some("user"));
    if !resumed {
        return (call_start, false);
    }
    let start = session.messages[..call_start]
        .iter()
        .rposition(|message| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message.get("tool_calls").is_none()
        })
        .map_or(0, |answer| answer + 1);
    (start, true)
}

/// The turn starting at `start` as `message`'s sender would go on with it: the turn by a digest
/// of its first message, and the conversation and sender.
fn turn_owner(session: &Session, start: usize, message: &InboundMessage) -> Option<Value> {
    let first = session.messages.get(start)?;
    let shown = serde_json::json!([
        first.get("role"),
        first.get("content"),
        first.get("timestamp")
    ]);
    Some(serde_json::json!({
        "turn": format!("{:x}", Sha256::digest(shown.to_string().as_bytes())),
        "session_key": session.key,
        "channel": message.channel,
        "chat_id": message.chat_id,
        "sender_id": message.sender_id,
    }))
}

/// Notes whom a turn that stops for the user belongs to: the sender of the message the user sent
/// to start it. When anyone else (or no user) goes on with the turn, the note goes, so the
/// turn's proposals are not used however it ends.
pub(super) fn note_turn_owner(session: &mut Session, message: &InboundMessage, call_start: usize) {
    let (start, resumed) = turn_start(session, call_start);
    let owner = turn_owner(session, start, message).filter(|_| learn::owner_turn(message));
    match owner {
        Some(owner) if !resumed => {
            session.metadata.insert(TURN_OWNER_KEY.to_owned(), owner);
        }
        Some(owner) if session.metadata.get(TURN_OWNER_KEY) == Some(&owner) => {}
        _ => {
            session.metadata.remove(TURN_OWNER_KEY);
        }
    }
}

/// Whether the turn this message ends is all its sender's: one it started, or one it started
/// and went on with alone. The note is taken away, since the turn ends here.
fn owns_turn(session: &mut Session, message: &InboundMessage, call_start: usize) -> bool {
    let (start, resumed) = turn_start(session, call_start);
    let owner = turn_owner(session, start, message);
    let noted = session.metadata.remove(TURN_OWNER_KEY);
    !resumed || (owner.is_some() && noted == owner)
}

/// The last `web_watch_status` request this message added.
fn status_request(
    session: &Session,
    new_messages: &[Value],
    call_start: usize,
) -> Option<Option<String>> {
    added_messages(session, new_messages, call_start)
        .rev()
        .find_map(|message| {
            web_watch_status_request_from_tool_content(tool_content(
                message,
                WEB_WATCH_STATUS_TOOL_NAME,
            )?)
        })
}

/// Output of a tool other than the web watch tools and the user's answer to a question.
fn other_tool_output(message: &Value) -> bool {
    message.get("role").and_then(Value::as_str) == Some("tool")
        && !matches!(
            message.get("name").and_then(Value::as_str),
            Some(PROPOSE_WEB_WATCH_TOOL_NAME | WEB_WATCH_STATUS_TOOL_NAME | "ask_user")
        )
}

/// Whether `content` is a `/watch` command (a summary's button, or a mistyped one): it is
/// answered before an unrelated question that waits, while a plain reply answers that question.
pub(super) fn is_web_watch_command(content: &str) -> bool {
    matches!(
        classify_reply(content),
        WebWatchReply::Confirm(Some(_)) | WebWatchReply::Cancel(Some(_)) | WebWatchReply::Invalid
    )
}

fn tool_content<'a>(message: &'a Value, name: &str) -> Option<&'a str> {
    (message.get("role").and_then(Value::as_str) == Some("tool")
        && message.get("name").and_then(Value::as_str) == Some(name))
    .then(|| message.get("content").and_then(Value::as_str))
    .flatten()
}

fn fresh_summary_id() -> String {
    let identity = format!(
        "{}:{:?}:{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH),
        SUMMARY_NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
    format!("w-{}", &digest[..12])
}
