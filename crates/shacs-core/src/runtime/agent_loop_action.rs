use super::{
    append_ask_user_resume, append_session_message, pending_ask_user_id,
    pending_permission_approval, record_permission_approval_terminal, AgentLoop, AgentLoopError,
    InboundMessage, RuntimeInterrupt, RuntimeToolMessage, Session, PENDING_PERMISSION_APPROVAL_KEY,
    PENDING_RECENT_RETRY_APPROVAL_KEY, RUNTIME_CHECKPOINT_KEY,
};
use crate::tools::ask_action::{is_command, pending_call, ExternalActionDraft};
use serde_json::Value;

#[path = "agent_loop_action_state.rs"]
mod state;
pub(super) use state::process_epoch;
use state::{Decision, Stage, KEY};

pub(super) enum Reply {
    Continue,
    Resume(String),
    Respond {
        content: String,
        reason: &'static str,
    },
}

fn response(reason: &'static str, content: impl Into<String>) -> Reply {
    Reply::Respond {
        content: content.into(),
        reason,
    }
}

pub(super) fn check_reply(
    session: &mut Session,
    message: &InboundMessage,
    epoch: &str,
) -> Result<Reply, AgentLoopError> {
    let Some(raw) = session.metadata.get(KEY) else {
        if pending_call(&session.messages).is_some() {
            cancel(session);
            return Ok(response("entrusted_action_closed", "Action decision unavailable after interruption. No action was run; request a new proposal."));
        }
        return Ok(if is_command(&message.content) {
            response(
                "entrusted_action_rejected",
                "No matching pending action proposal. No action was run.",
            )
        } else {
            Reply::Continue
        });
    };
    let Ok(mut decision) = serde_json::from_value::<Decision>(raw.clone()) else {
        cancel(session);
        return Ok(response(
            "entrusted_action_closed",
            "Action decision state is invalid. No action was run; request a new proposal.",
        ));
    };
    if !decision.matches_request(message, session) {
        return Ok(response("entrusted_action_rejected", "Only the original requester in the original session, channel and chat can decide this action."));
    }
    if !decision.is_current(session, epoch) {
        cancel(session);
        return Ok(response("entrusted_action_closed", "Action proposal expired after restart or work/goal change. No action was run; request a new proposal."));
    }
    let words = message.content.split_whitespace().collect::<Vec<_>>();
    if words == ["/action", "decline", decision.proposal_id.as_str()] {
        cancel(session);
        return Ok(response(
            "entrusted_action_declined",
            "Action proposal declined. No action was run.",
        ));
    }
    match decision.stage {
        Stage::Pending => {
            if pending_ask_user_id(&session.messages).as_deref() != Some(&decision.tool_call_id) {
                cancel(session);
                return Ok(response(
                    "entrusted_action_closed",
                    "Action interruption no longer matches. Request a new proposal.",
                ));
            }
            if words != ["/action", "accept", decision.proposal_id.as_str()] {
                return Ok(response("entrusted_action_pending", decision.prompt()));
            }
            decision.stage = Stage::Continuing;
            decision.save(session)?;
            Ok(Reply::Resume(decision.accepted_result()))
        }
        Stage::Continuing => Ok(if is_command(&message.content) {
            response("entrusted_action_rejected", "This action decision was already consumed. Any pending tool permission must be decided separately.")
        } else {
            Reply::Continue
        }),
    }
}

/// Whether an action decision of the user is still awaited.
pub(super) fn waiting(session: &Session) -> bool {
    session.metadata.contains_key(KEY) || pending_call(&session.messages).is_some()
}

pub(super) fn cancel(session: &mut Session) {
    if !session.metadata.contains_key(KEY) && pending_call(&session.messages).is_none() {
        return;
    }
    if let Some(tool_call_id) = pending_ask_user_id(&session.messages) {
        append_ask_user_resume(session, &tool_call_id, "{\"kind\":\"entrusted_action_decision\",\"decision\":\"cancelled\",\"permission_granted\":false}");
    }
    if let Some(approval) = pending_permission_approval(session) {
        record_permission_approval_terminal(
            session,
            &approval,
            shacs_session::PermissionApprovalTerminalState::Rejected,
        );
        append_session_message(
            session,
            RuntimeToolMessage {
                tool_call_id: approval.tool_call.id,
                name: approval.tool_call.name,
                content: "Action decision cancelled; tool not executed.".to_owned(),
            }
            .to_json(),
        );
    }
    session.metadata.remove(KEY);
    session.metadata.remove(PENDING_PERMISSION_APPROVAL_KEY);
    session.metadata.remove(PENDING_RECENT_RETRY_APPROVAL_KEY);
}

pub(super) fn preserve_checkpoint_boundary(session: &mut Session) {
    let action_checkpoint = session
        .metadata
        .get(RUNTIME_CHECKPOINT_KEY)
        .and_then(|checkpoint| checkpoint.get("assistant_message"))
        .is_some_and(|message| pending_call(std::slice::from_ref(message)).is_some());
    if action_checkpoint {
        session
            .metadata
            .entry(KEY.to_owned())
            .or_insert(Value::Null);
    }
}

impl AgentLoop<'_> {
    pub(super) fn finish_action_question(
        &self,
        session: &mut Session,
        message: &InboundMessage,
        result: &mut crate::runtime::AgentRunResult,
    ) -> Result<(), AgentLoopError> {
        let Some((call_id, draft)) = pending_call(&session.messages) else {
            if result.interrupt.is_none() {
                session.metadata.remove(KEY);
            }
            return Ok(());
        };
        let decision = ExternalActionDraft::parse(draft)
            .and_then(|draft| Decision::new(session, message, (call_id, draft, process_epoch())));
        match (decision, &mut result.interrupt) {
            (
                Some(decision),
                Some(RuntimeInterrupt::AskUser {
                    question, options, ..
                }),
            ) => {
                *question = decision.prompt();
                *options = vec![
                    format!("/action accept {}", decision.proposal_id),
                    format!("/action decline {}", decision.proposal_id),
                ];
                result.final_content = Some(question.clone());
                result.stop_reason = "entrusted_action_pending".to_owned();
                decision.save(session)?;
            }
            _ => {
                cancel(session);
                result.interrupt = None;
                result.stop_reason = "entrusted_action_closed".to_owned();
                result.final_content = Some("Action proposal is invalid, blocked, or not bound to current active entrusted work. No action was run.".to_owned());
            }
        }
        Ok(())
    }
}
