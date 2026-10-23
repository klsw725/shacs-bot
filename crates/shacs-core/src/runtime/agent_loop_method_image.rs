//! Image and composed requests are reported by the runtime. The model's text
//! about an image never reaches the user or the saved history, since it may
//! describe an image (or its content) that nothing checked; a composed reply
//! is shown only as a report whose lines carry the runtime's checks. Neither
//! lets the model use a tool that delivers its own words, or a workflow.

use super::*;
use crate::runtime::task_method::{MethodId, MethodRequest};
use crate::runtime::task_method_compose_answer::composed_answer;
use crate::runtime::task_method_image_answer::guarded_answer;
use crate::runtime::task_method_store::{preview, MethodStore};

#[path = "agent_loop_method_errors.rs"]
mod errors;
#[path = "agent_loop_method_held.rs"]
mod held;
#[path = "agent_loop_method_hold.rs"]
mod hold;
#[path = "agent_loop_method_quiet.rs"]
mod quiet;
#[cfg(test)]
#[path = "agent_loop_method_image_tests.rs"]
mod tests;

pub(in crate::runtime::agent_loop) use errors::{
    quiet_failure, quiet_provider_details, quiet_provider_error, quiet_run_error,
};
pub(in crate::runtime::agent_loop) use held::{
    hold_results, quiet_request_history, reported_start, restore_results, NOT_CLEARED,
};
pub(in crate::runtime::agent_loop) use hold::{
    hold_model_text, quiet_checkpoints, quiet_events, withhold_run_text,
};

const IMAGE_TOOL: &str = "image_generate";
/// Tools whose output is the model's own words, delivered outside the checked
/// answer: a question, a message, a subagent or an entrusted-work proposal.
const OWN_WORDS: [&str; 4] = ["ask_user", "message", "spawn", "propose_entrusted_work"];

/// The image method, or a composed request with an image part.
pub(in crate::runtime::agent_loop) fn has_image(request: &MethodRequest) -> bool {
    request
        .selection
        .parts()
        .contains(&MethodId::ImageGenerationReview)
}

/// The active request if the runtime reports it: an image or composed one.
fn reported(session: &Session) -> std::io::Result<Option<MethodRequest>> {
    Ok(MethodStore::load(session)?.active.filter(|request| {
        matches!(
            request.selection.method,
            MethodId::ImageGenerationReview | MethodId::Composed
        )
    }))
}

/// Whether the execution ledger keeps none of the model's words (see
/// `AgentRunSpec::quiet_ledger`): in an image or composed request, in one
/// that was such a request before a method change (it still holds what it
/// saved then), or when the request's method cannot be read.
pub(in crate::runtime::agent_loop) fn quiet_ledger(session: &Session) -> bool {
    let reported = reported(session).map_or(true, |request| request.is_some());
    reported || held::held_view(session).map_or(true, |held| held.is_some())
}

/// An image or composed request never runs as a workflow: workflow children
/// cannot call image_generate, and a workflow's answer is its children's own
/// words, which no part of the request checks.
pub(in crate::runtime::agent_loop) fn skips_workflow(session: &Session) -> std::io::Result<bool> {
    Ok(reported(session)?.is_some())
}

/// The tools the model may call: image_generate alone for an image request,
/// every tool but those delivering its own words for a composed one, and
/// `None` (all tools) for any other request.
pub(in crate::runtime::agent_loop) fn method_tools(
    session: &Session,
    tools: &ToolRegistry,
) -> std::io::Result<Option<ToolRegistry>> {
    let Some(request) = reported(session)? else {
        return Ok(None);
    };
    if request.selection.method == MethodId::ImageGenerationReview {
        let mut only = ToolRegistry::new();
        if let Some(tool) = tools.get(IMAGE_TOOL) {
            only.register_arc(tool);
        }
        return Ok(Some(only));
    }
    let mut offered = tools.clone();
    for name in OWN_WORDS {
        offered.unregister(name);
    }
    Ok(Some(offered))
}

impl AgentLoop<'_> {
    /// A user turn of an image or composed request never resumes a pending
    /// workflow: the workflow is cancelled, durably, so its children's words
    /// are never the answer.
    pub(in crate::runtime::agent_loop) fn cancel_reported_workflow(
        &mut self,
        message: &InboundMessage,
        session: &mut Session,
    ) -> Result<(), AgentLoopError> {
        if !super::user_turn(message) || pending_workflow(session).is_none() {
            return Ok(());
        }
        let Some(request) = reported(session)? else {
            return Ok(());
        };
        let reason = if request.selection.method == MethodId::Composed {
            ("composed_request", "composed-request")
        } else {
            ("image_request", "image-request")
        };
        self.cancel_pending_workflow(session, &request, reason)?;
        session.metadata.remove(PENDING_WORKFLOW_KEY);
        Ok(())
    }
}

/// Replaces the reply of request `id` with the runtime's answer for an image
/// or composed request, both in the result and as this turn's final history
/// message (`history_start` is where the turn's messages begin). A pending
/// approval or question is left as it is.
pub(in crate::runtime::agent_loop) fn answer(
    session: &mut Session,
    id: &str,
    result: &mut crate::runtime::AgentRunResult,
    history_start: usize,
) -> std::io::Result<()> {
    if result.interrupt.is_some() || result.stop_reason == "entrusted_work_proposed" {
        return Ok(());
    }
    let mut store = MethodStore::load(session)?;
    let Some(request) = store
        .active
        .iter_mut()
        .chain(store.closed.iter_mut())
        .find(|request| request.request_id == id)
    else {
        return Ok(());
    };
    let reply = result.final_content.clone().unwrap_or_default();
    let Some((answer, withheld)) = guarded_answer(request, &result.stop_reason)
        .map(|answer| (answer, true))
        .or_else(|| composed_answer(request, &reply, &result.stop_reason))
    else {
        return Ok(());
    };
    if withheld {
        request.image.withheld_reply = result.final_content.as_deref().map(preview);
        store.save(session)?;
    }
    let added = session
        .messages
        .get_mut(history_start..)
        .unwrap_or_default();
    if let Some(message) = added.iter_mut().rev().find(|message| {
        message.get("role").and_then(Value::as_str) == Some("assistant")
            && message
                .get("tool_calls")
                .and_then(Value::as_array)
                .map_or(true, Vec::is_empty)
    }) {
        message["content"] = Value::String(answer.clone());
    }
    result.final_content = Some(answer);
    Ok(())
}
