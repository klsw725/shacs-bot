use crate::evaluator::{EvaluatorRequestEnvelope, EvaluatorVerdictEnvelope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorRoute {
    Notify,
    Suppress,
    Continue,
    Escalate,
    Verify,
    RollbackCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorRouteStopReason {
    UserInterrupted,
    ContinuationBudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OwnerResultLocator(String);

impl OwnerResultLocator {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskResultOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
    NotRequested,
    Pending,
    Delivered,
    Suppressed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluatorBoundaryContext {
    pub user_interrupted: bool,
    pub continuation_budget_remaining: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorBoundaryRecord {
    pub input: EvaluatorRequestEnvelope,
    pub output: EvaluatorVerdictEnvelope,
    pub route: EvaluatorRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_stop_reason: Option<EvaluatorRouteStopReason>,
    pub owner_result_locator: OwnerResultLocator,
    pub task_outcome: TaskResultOutcome,
    pub delivery_outcome: DeliveryOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorBoundaryRecordInput {
    pub input: EvaluatorRequestEnvelope,
    pub output: EvaluatorVerdictEnvelope,
    pub requested_route: EvaluatorRoute,
    pub owner_result_locator: OwnerResultLocator,
    pub task_outcome: TaskResultOutcome,
    pub delivery_outcome: DeliveryOutcome,
    pub context: EvaluatorBoundaryContext,
}

impl EvaluatorBoundaryRecord {
    pub fn grants_execution_authority(&self) -> bool {
        false
    }
}

pub fn record_evaluator_boundary(input: EvaluatorBoundaryRecordInput) -> EvaluatorBoundaryRecord {
    let (route, route_stop_reason) = match (input.requested_route, input.context) {
        (
            EvaluatorRoute::Continue,
            EvaluatorBoundaryContext {
                user_interrupted: true,
                ..
            },
        ) => (
            EvaluatorRoute::Suppress,
            Some(EvaluatorRouteStopReason::UserInterrupted),
        ),
        (
            EvaluatorRoute::Continue,
            EvaluatorBoundaryContext {
                continuation_budget_remaining: 0,
                ..
            },
        ) => (
            EvaluatorRoute::Suppress,
            Some(EvaluatorRouteStopReason::ContinuationBudgetExhausted),
        ),
        (route, _) => (route, None),
    };

    EvaluatorBoundaryRecord {
        input: input.input,
        output: input.output,
        route,
        route_stop_reason,
        owner_result_locator: input.owner_result_locator,
        task_outcome: input.task_outcome,
        delivery_outcome: input.delivery_outcome,
    }
}
