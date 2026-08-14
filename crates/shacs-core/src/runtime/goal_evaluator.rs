use shacs_eval::completion_boundary::{
    DeliveryOutcome, EvaluatorBoundaryContext, EvaluatorBoundaryRecord,
    EvaluatorBoundaryRecordInput, EvaluatorRoute, OwnerResultLocator, TaskResultOutcome,
};
use shacs_eval::evaluator::{
    EvaluatorVerdictEnvelope, RedactionStatus, SuggestedNextAction, VerdictKind,
};

use super::goal::{GoalCompletionVerdict, GoalEvaluationRequest};

pub const GOAL_EVALUATOR_BOUNDARY_METADATA_KEY: &str = "goal_evaluator_boundaries";

#[derive(Debug, Clone, PartialEq)]
pub struct GoalEvaluatorOutcome {
    pub output: EvaluatorVerdictEnvelope,
    pub requested_route: EvaluatorRoute,
    pub owner_result_locator: OwnerResultLocator,
    pub advisory_verdict: Option<GoalCompletionVerdict>,
}

#[derive(Debug, Default)]
pub struct ConservativeGoalCompletionEvaluator;

impl GoalCompletionEvaluator for ConservativeGoalCompletionEvaluator {
    fn evaluate(&self, request: &GoalEvaluationRequest) -> Result<GoalEvaluatorOutcome, String> {
        Ok(GoalEvaluatorOutcome {
            output: EvaluatorVerdictEnvelope {
                verdict_kind: VerdictKind::LowConfidence,
                reason: "goal completion evidence is insufficient".to_owned(),
                confidence: 0.0,
                evidence_refs: Vec::new(),
                suggested_next_action: SuggestedNextAction::None,
                expires_at_ms: None,
                redaction_status: RedactionStatus::AlreadySafe,
                evaluator_version: "conservative-local-v1".to_owned(),
            },
            requested_route: EvaluatorRoute::Notify,
            owner_result_locator: OwnerResultLocator::new(format!(
                "session:{}",
                request.request.session_id.as_deref().unwrap_or("unknown")
            )),
            advisory_verdict: None,
        })
    }
}

pub trait GoalCompletionEvaluator: Send + Sync {
    fn evaluate(&self, request: &GoalEvaluationRequest) -> Result<GoalEvaluatorOutcome, String>;
}

impl<F> GoalCompletionEvaluator for F
where
    F: Fn(&GoalEvaluationRequest) -> Result<GoalEvaluatorOutcome, String> + Send + Sync,
{
    fn evaluate(&self, request: &GoalEvaluationRequest) -> Result<GoalEvaluatorOutcome, String> {
        self(request)
    }
}

pub(super) fn boundary_record(
    request: GoalEvaluationRequest,
    outcome: &GoalEvaluatorOutcome,
    user_interrupted: bool,
    continuation_budget_remaining: u32,
) -> EvaluatorBoundaryRecord {
    shacs_eval::completion_boundary::record_evaluator_boundary(EvaluatorBoundaryRecordInput {
        input: request.request,
        output: outcome.output.clone(),
        requested_route: outcome.requested_route,
        owner_result_locator: outcome.owner_result_locator.clone(),
        task_outcome: TaskResultOutcome::Succeeded,
        delivery_outcome: DeliveryOutcome::NotRequested,
        context: EvaluatorBoundaryContext {
            user_interrupted,
            continuation_budget_remaining,
        },
    })
}
