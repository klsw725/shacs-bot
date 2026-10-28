use super::{
    lock, now_iso, pending_ask_user_id, pending_permission_approval, pending_recent_retry_approval,
    pending_workflow, AgentLoop, RuntimeInterrupt, RuntimeToolCall, RuntimeToolExecutionReport,
    Session,
};
use crate::runtime::task_method::{
    EffectOutcome, EvidenceFact, ExternalEffectFact, Gate, MethodId, RequestState, UserDecision,
};
use crate::runtime::task_method_apply::apply_observation;
use crate::runtime::task_method_calc::run;
use crate::runtime::task_method_compose::{is_calc_line, is_image_line};
use crate::runtime::task_method_image_inspect::image_call;
use crate::runtime::task_method_observe::{
    digest, exec_command, failure_outcome, MethodObservation,
};
use crate::runtime::task_method_observe_report::{record_result, report_call};
use crate::runtime::task_method_render::footer;
use crate::runtime::task_method_store::{preview, MethodStore};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[cfg(test)]
#[path = "agent_loop_method_finish_tests.rs"]
mod tests;

pub(super) fn record_decision(session: &mut Session, answer: &str) -> std::io::Result<()> {
    let mut store = MethodStore::load(session)?;
    if let (Some(request), Some(id)) = (
        store.active.as_mut(),
        pending_ask_user_id(&session.messages),
    ) {
        if let Some(question) = super::method::cancel::pending_question(session, &id) {
            request.user_decisions.push(UserDecision {
                question: preview(&question),
                answer: preview(answer),
            });
        }
    }
    store.save(session)
}

impl AgentLoop<'_> {
    pub(super) fn observe_method_approved(
        &self,
        observation: &Arc<Mutex<MethodObservation>>,
        call: &RuntimeToolCall,
        report: &RuntimeToolExecutionReport,
    ) {
        let mut observation = lock(observation);
        let (name, arguments) = if call.name == "tool_call" {
            (
                call.arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&call.name),
                call.arguments.get("arguments").unwrap_or(&call.arguments),
            )
        } else {
            (call.name.as_str(), &call.arguments)
        };
        let Some(result) = report
            .messages
            .iter()
            .find(|message| message.tool_call_id == call.id)
        else {
            return;
        };
        record_result(
            &mut observation,
            report_call(name, arguments).as_ref(),
            image_call(name, arguments).as_ref(),
            &call.id,
            &result.content,
            Some(&self.config.workspace),
            None,
        );
        for action in &report.permissioned_actions {
            if self
                .tools
                .get(&action.tool_name)
                .is_some_and(|tool| tool.read_only())
            {
                continue;
            }
            let outcome = if result.content.starts_with("Error") {
                failure_outcome(&result.content)
            } else {
                EffectOutcome::Completed
            };
            if let Some(command) = exec_command(&action.tool_name, arguments) {
                observation
                    .calculation_runs
                    .push(run(&call.id, &command, &result.content));
            }
            observation.effects.push(ExternalEffectFact {
                tool_name: preview(&action.tool_name),
                outcome,
                argument_digest: digest(arguments),
                selection_revision: 0,
                turn_id: preview(&action.turn_id),
                observed_at: now_iso(),
            });
            if outcome == EffectOutcome::Completed
                && matches!(action.tool_name.as_str(), "write_file" | "edit_file")
            {
                if let Some(path) = arguments.get("path").and_then(Value::as_str) {
                    observation.evidence.push(EvidenceFact::WrittenFile {
                        path: preview(path),
                    });
                }
            }
        }
    }
}

pub(super) fn finish(
    session: &mut Session,
    observation: &Arc<Mutex<MethodObservation>>,
    state: RequestState,
) -> std::io::Result<String> {
    let mut store = MethodStore::load(session)?;
    let Some(request) = store.active.as_mut() else {
        return Ok(String::new());
    };
    let mut observed = lock(observation);
    for mut fact in observed.effects.drain(..) {
        fact.selection_revision = request.selection.revision;
        request.external_effect_facts.push(fact);
    }
    request.evidence_facts.append(&mut observed.evidence);
    apply_observation(request, &mut observed);
    request.state = state;
    request.selection.gate = if pending_permission_approval(session).is_some()
        || pending_recent_retry_approval(session).is_some()
    {
        Gate::WaitingApproval
    } else {
        Gate::NotEvaluated
    };
    let suffix = footer(request);
    match state {
        RequestState::Answered
        | RequestState::Failed
        | RequestState::Cancelled
        | RequestState::Abandoned => store.close(state),
        RequestState::InProgress | RequestState::AwaitingContinuation | RequestState::Blocked => {}
    }
    store.save(session)?;
    Ok(suffix)
}

pub(super) fn finish_run(
    session: &mut Session,
    observation: &Arc<Mutex<MethodObservation>>,
    result: &mut crate::runtime::AgentRunResult,
    history_start: usize,
) -> std::io::Result<String> {
    if let Some(answer) = report_answer(result) {
        // `[image]` lines, and `[calculation]` lines of a composed request, are
        // placeholders the runtime fills, not report text.
        let active = MethodStore::load(session)?.active;
        let image = active.as_ref().is_some_and(super::method::image::has_image);
        let composed = active
            .as_ref()
            .is_some_and(|request| request.selection.method == MethodId::Composed);
        let text: Vec<&str> = answer
            .lines()
            .filter(|line| !(image && is_image_line(line)) && !(composed && is_calc_line(line)))
            .collect();
        lock(observation)
            .report_outputs
            .push(crate::runtime::task_method_report_parse::output(
                crate::runtime::task_method_report::OutputKind::Answer,
                "answer",
                &text.join("\n"),
            ));
    }
    for artifact in &result.generated_artifacts {
        lock(observation)
            .evidence
            .push(EvidenceFact::GeneratedArtifact {
                reference: preview(
                    &serde_json::to_string(artifact).map_err(std::io::Error::other)?,
                ),
            });
    }
    let state = if result.interrupt.is_some()
        || pending_workflow(session).is_some()
        || pending_ask_user_id(&session.messages).is_some()
    {
        RequestState::InProgress
    } else if result.stop_reason == "cancelled" {
        RequestState::Cancelled
    } else if result.error.is_some() {
        RequestState::Failed
    } else {
        RequestState::Answered
    };
    let active = MethodStore::load(session)?
        .active
        .map(|request| request.request_id);
    let suffix = finish(session, observation, state)?;
    if let Some(id) = active {
        super::method::image::answer(session, &id, result, history_start)?;
    }
    Ok(suffix)
}

/// The provider's own final answer. Runtime texts for failures, cancellation,
/// the iteration limit and pending questions or approvals are never report text.
pub(super) fn report_answer(result: &crate::runtime::AgentRunResult) -> Option<&str> {
    let completed =
        result.stop_reason == "completed" && result.error.is_none() && result.interrupt.is_none();
    completed
        .then_some(result.final_content.as_deref())
        .flatten()
}

pub(super) fn append_footer(result: &mut crate::runtime::AgentRunResult, suffix: &str) {
    if suffix.is_empty() {
        return;
    }
    result
        .final_content
        .get_or_insert_with(String::new)
        .push_str(suffix);
    match &mut result.interrupt {
        Some(
            RuntimeInterrupt::AskUser { question, .. }
            | RuntimeInterrupt::PermissionApproval { question, .. },
        ) => question.push_str(suffix),
        None => {}
    }
}

pub(super) fn command_footer(session: &mut Session, reason: &str) -> std::io::Result<String> {
    let state = match reason {
        "permission_denied_by_user"
        | "permission_recent_retry_denied"
        | "permission_recent_retry_rejected"
        | "entrusted_action_declined"
        | "entrusted_action_closed"
        | "permission_recent_retry_closed" => RequestState::Cancelled,
        "tool_error" | "workflow_blocked" | "work_routing_error" => RequestState::Failed,
        "permission_approval_pending"
        | "permission_recent_retry_pending"
        | "entrusted_action_pending"
        | "workflow_ask_user"
        | "permission_approval_executing"
        | "permission_recent_retry_executing" => RequestState::InProgress,
        _ => return Ok(String::new()),
    };
    finish(
        session,
        &Arc::new(Mutex::new(MethodObservation::default())),
        state,
    )
}
