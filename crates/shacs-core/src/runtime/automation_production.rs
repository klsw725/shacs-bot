use super::{
    AutomationJobResult, AutomationScheduleKind, AutomationSourceEventKind,
    AutomationWorkEnqueueInput, AutomationWorkEnvelope, DurableDispatchError,
    DurableWorkDispatcher,
};
use serde::{Deserialize, Serialize};
use shacs_eval::completion_boundary::EvaluatorRoute;
use shacs_eval::evaluator::{EvaluationTriggerSource, ProjectionSurface};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOutcomePolicy {
    Notify,
    Suppress,
    Continue,
    Escalate,
    Verify,
    RollbackCandidate,
}

impl From<AutomationOutcomePolicy> for EvaluatorRoute {
    fn from(policy: AutomationOutcomePolicy) -> Self {
        match policy {
            AutomationOutcomePolicy::Notify => Self::Notify,
            AutomationOutcomePolicy::Suppress => Self::Suppress,
            AutomationOutcomePolicy::Continue => Self::Continue,
            AutomationOutcomePolicy::Escalate => Self::Escalate,
            AutomationOutcomePolicy::Verify => Self::Verify,
            AutomationOutcomePolicy::RollbackCandidate => Self::RollbackCandidate,
        }
    }
}

impl From<EvaluatorRoute> for AutomationOutcomePolicy {
    fn from(route: EvaluatorRoute) -> Self {
        match route {
            EvaluatorRoute::Notify => Self::Notify,
            EvaluatorRoute::Suppress => Self::Suppress,
            EvaluatorRoute::Continue => Self::Continue,
            EvaluatorRoute::Escalate => Self::Escalate,
            EvaluatorRoute::Verify => Self::Verify,
            EvaluatorRoute::RollbackCandidate => Self::RollbackCandidate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AutomationProductionJob {
    pub work_id: String,
    pub envelope: AutomationWorkEnvelope,
    pub next_wake_at_ms: Option<u64>,
}

pub fn enqueue_production_automation(
    dispatcher: &mut DurableWorkDispatcher,
    job: AutomationProductionJob,
) -> Result<(), DurableDispatchError> {
    validate_production_job(&job)?;
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: job.work_id,
        envelope: job.envelope,
        next_wake_at_ms: job.next_wake_at_ms,
    })
}

fn validate_production_job(job: &AutomationProductionJob) -> Result<(), DurableDispatchError> {
    let valid_schedule = match (&job.envelope.event.source, &job.envelope.schedule) {
        (AutomationSourceEventKind::Heartbeat, AutomationScheduleKind::Recurring) => {
            job.next_wake_at_ms.is_some()
        }
        (AutomationSourceEventKind::Cron { .. }, AutomationScheduleKind::OneShot)
        | (AutomationSourceEventKind::Cron { .. }, AutomationScheduleKind::Recurring) => {
            job.next_wake_at_ms.is_some()
        }
        (
            AutomationSourceEventKind::SubagentResult { .. }
            | AutomationSourceEventKind::AppTaskResult { .. }
            | AutomationSourceEventKind::ChannelEvent { .. }
            | AutomationSourceEventKind::LocalApiBackground { .. },
            AutomationScheduleKind::OneShot,
        ) => job.next_wake_at_ms.is_none(),
        (AutomationSourceEventKind::ManualResume { .. }, _) => false,
        _ => false,
    };
    if !valid_schedule {
        return Err(DurableDispatchError::InvalidWork(
            "unsupported production automation adapter or schedule".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTaskOutcomeInput {
    pub work_id: String,
    pub session_key: String,
    pub result_ref: String,
    pub source: EvaluationTriggerSource,
    pub target_surface: Option<ProjectionSurface>,
    pub policy: AutomationOutcomePolicy,
    pub goal_id: Option<String>,
    pub continuation_budget_remaining: u32,
    pub user_interrupted: bool,
    pub owner_target_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTaskOutcomeDecision {
    pub route: EvaluatorRoute,
    pub evaluator_evidence_ref: String,
}

pub trait AutomationTaskOutcomeEvaluator: Send + Sync {
    fn evaluate(
        &self,
        input: &AutomationTaskOutcomeInput,
        result: &AutomationJobResult,
    ) -> AutomationTaskOutcomeDecision;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ConservativeAutomationTaskOutcomeEvaluator;

impl AutomationTaskOutcomeEvaluator for ConservativeAutomationTaskOutcomeEvaluator {
    fn evaluate(
        &self,
        input: &AutomationTaskOutcomeInput,
        result: &AutomationJobResult,
    ) -> AutomationTaskOutcomeDecision {
        let requested = EvaluatorRoute::from(input.policy);
        let route = match (requested, result) {
            (EvaluatorRoute::Continue, _)
                if input.user_interrupted
                    || input.goal_id.is_none()
                    || input.continuation_budget_remaining == 0 =>
            {
                EvaluatorRoute::Suppress
            }
            (EvaluatorRoute::Notify, AutomationJobResult::Succeeded { .. }) => {
                EvaluatorRoute::Notify
            }
            (EvaluatorRoute::Notify, _) => EvaluatorRoute::Suppress,
            (route, _) => route,
        };
        AutomationTaskOutcomeDecision {
            route,
            evaluator_evidence_ref: format!("task-outcome:{}:{}", input.work_id, input.result_ref),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationRouteEvidence {
    pub owner_evidence_ref: String,
    pub effect: AutomationOwnerEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOwnerEffect {
    DeliveryRequest,
    NoNotification,
    ContinuationEnqueue,
    DurableEscalation,
    VerificationRequest,
    RollbackCandidateRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTaskOutcomeRecord {
    pub route: EvaluatorRoute,
    pub evaluator_evidence_ref: String,
    pub owner_evidence_ref: String,
}

pub trait AutomationRouteOwners {
    fn notify(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String>;
    fn suppress(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String>;
    fn continue_task(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String>;
    fn escalate(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String>;
    fn verify(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String>;
    fn rollback_candidate(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String>;
}

pub fn route_task_outcome(
    decision: AutomationTaskOutcomeDecision,
    input: &AutomationTaskOutcomeInput,
    owners: &mut dyn AutomationRouteOwners,
) -> Result<AutomationTaskOutcomeRecord, String> {
    let (evidence, expected_effect) = match decision.route {
        EvaluatorRoute::Notify => (owners.notify(input), AutomationOwnerEffect::DeliveryRequest),
        EvaluatorRoute::Suppress => (
            owners.suppress(input),
            AutomationOwnerEffect::NoNotification,
        ),
        EvaluatorRoute::Continue => (
            owners.continue_task(input),
            AutomationOwnerEffect::ContinuationEnqueue,
        ),
        EvaluatorRoute::Escalate => (
            owners.escalate(input),
            AutomationOwnerEffect::DurableEscalation,
        ),
        EvaluatorRoute::Verify => (
            owners.verify(input),
            AutomationOwnerEffect::VerificationRequest,
        ),
        EvaluatorRoute::RollbackCandidate => (
            owners.rollback_candidate(input),
            AutomationOwnerEffect::RollbackCandidateRecord,
        ),
    };
    let evidence = evidence?;
    if decision.evaluator_evidence_ref.trim().is_empty()
        || evidence.owner_evidence_ref.trim().is_empty()
        || evidence.effect != expected_effect
    {
        return Err("task outcome route lacks supported durable effect evidence".to_owned());
    }
    Ok(AutomationTaskOutcomeRecord {
        route: decision.route,
        evaluator_evidence_ref: decision.evaluator_evidence_ref,
        owner_evidence_ref: evidence.owner_evidence_ref,
    })
}
