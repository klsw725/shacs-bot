use super::*;
use crate::runtime::learned_method::{LearnedMethod, LearnedMethodState};
use crate::runtime::learned_method_store::LearnedMethodStore;
use crate::runtime::learned_method_use::{
    chosen, unavailable, MethodTurn, METHOD_TURN_KEY, METHOD_USE_KEY,
};
use crate::runtime::AgentRunResult;

impl AgentLoop<'_> {
    /// Before a new request of the user runs, on the regular path or as a
    /// new workflow: the confirmed method it uses, if one fits. Saved
    /// methods are read and rechecked against their sessions first; nothing
    /// is used when that fails or when no method is confirmed (and then no
    /// model call is made).
    pub(super) fn choose_learned_method(&self, message: &InboundMessage, session: &mut Session) {
        session.metadata.remove(METHOD_TURN_KEY);
        if !super::learn::owner_turn(message) || message.content.trim().is_empty() {
            return;
        }
        let turn = match LearnedMethodStore::new(&self.config.workspace).list() {
            Err(error) => unavailable(&error.to_string()),
            Ok(methods)
                if !methods
                    .iter()
                    .any(|method| method.state == LearnedMethodState::Confirmed) =>
            {
                return;
            }
            Ok(methods) => match self.check(session, methods) {
                Err(error) => unavailable(&error.to_string()),
                Ok(checked) => {
                    let unchecked: Vec<String> = checked
                        .iter()
                        .filter(|checked| !checked.rechecked())
                        .map(|checked| checked.method.method_id.clone())
                        .collect();
                    let methods: Vec<LearnedMethod> =
                        checked.into_iter().map(|checked| checked.method).collect();
                    match self.generator().match_request(&message.content, &methods) {
                        Ok(matched) => chosen(&matched, &unchecked),
                        Err(reason) => unavailable(&format!("matching did not finish ({reason})")),
                    }
                }
            },
        };
        if let Ok(turn) = serde_json::to_value(turn) {
            session.metadata.insert(METHOD_TURN_KEY.to_owned(), turn);
        }
    }
}

fn turn(session: &Session) -> Option<MethodTurn> {
    session
        .metadata
        .get(METHOD_TURN_KEY)
        .and_then(|turn| serde_json::from_value(turn.clone()).ok())
}

/// A new request starts with no method chosen.
pub(super) fn reset(session: &mut Session) {
    session.metadata.remove(METHOD_TURN_KEY);
}

/// The guidance of the method the request uses, for every run of it: the
/// regular loop's and each workflow child's.
pub(super) fn guidance(session: &Session) -> Option<String> {
    turn(session).and_then(|turn| turn.guidance)
}

/// Adds the method the request uses to the system prompt of this run.
pub(super) fn guide(session: &Session, messages: &mut [Value]) {
    let Some(guidance) = guidance(session) else {
        return;
    };
    if let Some(system) = messages
        .first_mut()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
    {
        let content = system
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        system["content"] = Value::String(format!("{content}{guidance}"));
    }
}

/// Whether the request in progress waits for the user (an approval, an
/// answer, an action decision) or for a workflow.
fn waiting(session: &Session) -> bool {
    pending_permission_approval(session).is_some()
        || pending_recent_retry_approval(session).is_some()
        || crate::tools::pending_ask_user_id(&session.messages).is_some()
        || pending_workflow(session).is_some()
        || super::action::waiting(session)
}

/// After a regular run: once the request has its answer, concludes it on the
/// reply the user gets; while it waits, notes that it does.
pub(super) fn finish(session: &mut Session, result: &mut AgentRunResult, history_start: usize) {
    if result.interrupt.is_some() || waiting(session) {
        if let Some(mut turn) = turn(session) {
            turn.waited = true;
            if let Ok(turn) = serde_json::to_value(turn) {
                session.metadata.insert(METHOD_TURN_KEY.to_owned(), turn);
            }
        }
        return;
    }
    let mut reply = result.final_content.clone().unwrap_or_default();
    let saved = session
        .messages
        .iter()
        .enumerate()
        .skip(history_start)
        .rev()
        .find(|(_, message)| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message.get("content").and_then(Value::as_str) == Some(reply.as_str())
        })
        .map(|(index, _)| index);
    if conclude(session, &mut reply, saved, &result.stop_reason) {
        result.final_content = Some(reply);
    }
}

/// A command reply ends the request when the request was waiting and, after
/// the command, waits for nothing (an approval denied or not saved, an
/// action decision declined or cancelled, …); the reply is saved as its
/// answer.
pub(super) fn conclude_command(session: &mut Session, reply: &mut String, reason: &str) {
    if turn(session).is_some_and(|turn| turn.waited) && !waiting(session) {
        conclude(session, reply, None, reason);
    }
}

/// Ends the learned-method part of a request on `reply`, the reply the user
/// gets: it gets the footer, and the session keeps it with the footer and,
/// for a method used, the record of its version and how the request ended
/// (`ended`). `saved` is the session message already holding `reply`;
/// without one the reply is saved as a new answer. The request's choice is
/// cleared. Returns whether there was a choice to conclude.
pub(super) fn conclude(
    session: &mut Session,
    reply: &mut String,
    saved: Option<usize>,
    ended: &str,
) -> bool {
    let Some(turn) = turn(session) else {
        return false;
    };
    session.metadata.remove(METHOD_TURN_KEY);
    reply.push_str(&turn.footer);
    let record = turn
        .used
        .and_then(|used| serde_json::to_value(used).ok())
        .map(|mut record| {
            record["ended"] = Value::String(ended.to_owned());
            record
        });
    match saved.and_then(|index| session.messages.get_mut(index)) {
        Some(message) => {
            message["content"] = Value::String(reply.clone());
            if let Some(record) = record {
                message[METHOD_USE_KEY] = record;
            }
        }
        None => {
            let mut extra = Map::new();
            if let Some(record) = record {
                extra.insert(METHOD_USE_KEY.to_owned(), record);
            }
            session.add_message("assistant", reply.clone(), extra);
        }
    }
    true
}
