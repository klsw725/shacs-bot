use crate::state::RuntimeSession;
use shacs_projection::{
    Spec033AutomationJobStatus, Spec033DeliveryStatus, Spec033EvaluatorRoute, Spec033GoalStatus,
    Spec033HookConfirmationFact, Spec033ReplayStatus,
};

pub(crate) fn spec033_lines(session: &RuntimeSession) -> Vec<String> {
    let projection = &session.spec033;
    let mut lines = Vec::new();
    if let Some(goal) = projection.goal.fact.as_ref() {
        lines.push(format!(
            "task goal: status={} stop={} budget={}/{} remaining={}",
            goal_status_label(goal.status),
            goal.stop_reason.as_deref().unwrap_or("none"),
            goal.budget.turns_used,
            goal.budget.turn_budget,
            goal.budget.remaining_turns
        ));
    } else {
        lines.push("task goal: unavailable".to_owned());
    }
    if let Some(evaluator) = projection.evaluator.fact.as_ref() {
        lines.push(format!(
            "evaluator: verdict={} route={}",
            evaluator.verdict,
            evaluator_route_label(evaluator.route)
        ));
    } else {
        lines.push("evaluator: unavailable".to_owned());
    }
    if let Some(automation) = projection.automation.fact.as_ref() {
        lines.push(format!(
            "automation: job={} delivery={}",
            automation_job_label(automation.job_status),
            delivery_label(automation.delivery_status)
        ));
    } else {
        lines.push("automation: unavailable".to_owned());
    }
    if let Some(confirmation) = projection.hook_confirmation.fact {
        lines.push(format!(
            "hook confirmation: {}",
            confirmation_label(confirmation)
        ));
    } else {
        lines.push("hook confirmation: unavailable".to_owned());
    }
    if let Some(improvement) = projection.self_improvement.fact.as_ref() {
        lines.push(format!(
            "workspace improvement: proposal={} applied={} rolled_back={}",
            improvement.proposal_id, improvement.applied, improvement.rolled_back
        ));
    } else {
        lines.push("workspace improvement: unavailable".to_owned());
    }
    if let Some(verify) = projection.verify.fact.as_ref() {
        lines.push(format!("workspace verify: passed={}", verify.passed));
    } else {
        lines.push("workspace verify: unavailable".to_owned());
    }
    if let Some(candidate) = projection.rollback_candidate.fact.as_ref() {
        lines.push(format!(
            "workspace rollback candidate: {}",
            candidate.verify_failure_ref
        ));
    } else {
        lines.push("workspace rollback candidate: unavailable".to_owned());
    }
    if let Some(replay) = projection.replay.fact.as_ref() {
        lines.push(format!(
            "workspace replay: result={}",
            replay_label(replay.status)
        ));
    } else {
        lines.push("workspace replay: unavailable".to_owned());
    }
    lines
}

const fn evaluator_route_label(value: Spec033EvaluatorRoute) -> &'static str {
    match value {
        Spec033EvaluatorRoute::Notify => "notify",
        Spec033EvaluatorRoute::Suppress => "suppress",
        Spec033EvaluatorRoute::Continue => "continue",
        Spec033EvaluatorRoute::Escalate => "escalate",
        Spec033EvaluatorRoute::Verify => "verify",
        Spec033EvaluatorRoute::RollbackCandidate => "rollback_candidate",
    }
}

const fn goal_status_label(value: Spec033GoalStatus) -> &'static str {
    match value {
        Spec033GoalStatus::Unavailable => "unavailable",
        Spec033GoalStatus::Active => "active",
        Spec033GoalStatus::Paused => "paused",
        Spec033GoalStatus::Blocked => "blocked",
        Spec033GoalStatus::Done => "done",
        Spec033GoalStatus::Cleared => "cleared",
    }
}

const fn automation_job_label(value: Spec033AutomationJobStatus) -> &'static str {
    match value {
        Spec033AutomationJobStatus::Pending => "pending",
        Spec033AutomationJobStatus::Succeeded => "succeeded",
        Spec033AutomationJobStatus::Failed => "failed",
        Spec033AutomationJobStatus::TimedOut => "timed_out",
        Spec033AutomationJobStatus::Cancelled => "cancelled",
        Spec033AutomationJobStatus::Suppressed => "suppressed",
    }
}

const fn delivery_label(value: Spec033DeliveryStatus) -> &'static str {
    match value {
        Spec033DeliveryStatus::NotRequested => "not_requested",
        Spec033DeliveryStatus::Pending => "pending",
        Spec033DeliveryStatus::Succeeded => "succeeded",
        Spec033DeliveryStatus::Failed => "failed",
    }
}

const fn confirmation_label(value: Spec033HookConfirmationFact) -> &'static str {
    match value {
        Spec033HookConfirmationFact::NotRequired => "not_required",
        Spec033HookConfirmationFact::Confirmed => "confirmed",
        Spec033HookConfirmationFact::Denied => "denied",
        Spec033HookConfirmationFact::HeadlessDenied => "headless_denied",
        Spec033HookConfirmationFact::Vetoed => "vetoed",
        Spec033HookConfirmationFact::Failed => "failed",
    }
}

const fn replay_label(value: Spec033ReplayStatus) -> &'static str {
    match value {
        Spec033ReplayStatus::Passed => "passed",
        Spec033ReplayStatus::Failed => "failed",
        Spec033ReplayStatus::Blocked => "blocked",
    }
}
