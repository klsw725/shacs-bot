mod durable_owners;
mod improvement_owner;
mod owner_support;
mod verifier;

use durable_owners::DurableRouteOwners;
use shacs_core::runtime::{
    persistent_goal_from_session, route_task_outcome, AutomationDispatchRequest,
    AutomationJobResult, AutomationTaskOutcomeEvaluator, AutomationTaskOutcomeInput,
    AutomationTaskOutcomeRecord, ConservativeAutomationTaskOutcomeEvaluator,
};
use shacs_eval::evaluator::{AutomationRunTriggerKind, EvaluationTriggerSource, ProjectionSurface};
use shacs_session::{SessionManager, SessionMutationGuard};
use std::path::Path;
pub(super) use verifier::consume_verification_requests;

pub(super) fn evaluate_and_route(
    data_dir: &Path,
    workspace: &Path,
    request: &AutomationDispatchRequest,
    result: &AutomationJobResult,
) -> Result<AutomationTaskOutcomeRecord, String> {
    let input = outcome_input(workspace, request, result)?;
    let decision = ConservativeAutomationTaskOutcomeEvaluator.evaluate(&input, result);
    route_task_outcome(
        decision,
        &input,
        &mut DurableRouteOwners::new(data_dir, workspace),
    )
}

fn outcome_input(
    workspace: &Path,
    request: &AutomationDispatchRequest,
    result: &AutomationJobResult,
) -> Result<AutomationTaskOutcomeInput, String> {
    let _guard = SessionMutationGuard::acquire(workspace, &request.session_key)
        .map_err(|error| error.to_string())?;
    let goal = SessionManager::open_existing(workspace)
        .map_err(|error| error.to_string())?
        .and_then(|manager| manager.load_existing(&request.session_key))
        .and_then(|session| persistent_goal_from_session(&session));
    let latest = goal.as_ref().and_then(|goal| goal.last_transition.as_ref());
    Ok(AutomationTaskOutcomeInput {
        work_id: request.work_id.clone(),
        session_key: request.session_key.clone(),
        result_ref: result_ref(result).to_owned(),
        source: trigger_source(&request.run.trigger_kind),
        target_surface: target_surface(&request.session_key),
        policy: request.outcome_policy,
        goal_id: goal.as_ref().map(|goal| goal.id.clone()),
        continuation_budget_remaining: goal
            .as_ref()
            .map_or(0, |goal| goal.turn_budget.saturating_sub(goal.turns_used)),
        user_interrupted: latest.is_some_and(|fact| fact.user_interrupted),
        owner_target_ref: request.owner_target_ref.clone(),
    })
}

fn target_surface(session_key: &str) -> Option<ProjectionSurface> {
    match session_key.split_once(':').map(|(channel, _)| channel) {
        Some("api") => Some(ProjectionSurface::LocalApi),
        Some("cli") => Some(ProjectionSurface::Cli),
        Some("tui") => Some(ProjectionSurface::Tui),
        Some(_) => Some(ProjectionSurface::Channel),
        None => None,
    }
}

fn result_ref(result: &AutomationJobResult) -> &str {
    match result {
        AutomationJobResult::Pending => "pending",
        AutomationJobResult::Succeeded { result_ref } => result_ref,
        AutomationJobResult::Failed { reason_ref } => reason_ref,
        AutomationJobResult::TimedOut { timeout_ref } => timeout_ref,
        AutomationJobResult::Cancelled { reason_ref } => reason_ref,
    }
}

const fn trigger_source(trigger: &AutomationRunTriggerKind) -> EvaluationTriggerSource {
    match trigger {
        AutomationRunTriggerKind::Heartbeat => EvaluationTriggerSource::Heartbeat,
        AutomationRunTriggerKind::Cron => EvaluationTriggerSource::ScheduledJob,
        AutomationRunTriggerKind::SubagentResult => EvaluationTriggerSource::Subagent,
        AutomationRunTriggerKind::AppTaskResult => EvaluationTriggerSource::AppTask,
        AutomationRunTriggerKind::ChannelEvent => EvaluationTriggerSource::Channel,
        AutomationRunTriggerKind::LocalApiBackground => EvaluationTriggerSource::LocalApi,
        AutomationRunTriggerKind::ManualResume => EvaluationTriggerSource::ManualReplay,
    }
}

#[cfg(test)]
mod tests;
