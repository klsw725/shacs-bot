use super::goal::{PersistentGoal, PersistentGoalStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalObservedState {
    Unavailable,
    Active,
    Paused,
    Blocked,
    Done,
    Cleared,
}

impl From<PersistentGoalStatus> for GoalObservedState {
    fn from(status: PersistentGoalStatus) -> Self {
        match status {
            PersistentGoalStatus::Active => Self::Active,
            PersistentGoalStatus::Paused => Self::Paused,
            PersistentGoalStatus::Blocked => Self::Blocked,
            PersistentGoalStatus::Done => Self::Done,
            PersistentGoalStatus::Cleared => Self::Cleared,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStopReason {
    GoalSet,
    PausedByUser,
    ResumedByUser,
    ClearedByUser,
    MarkedDoneByUser,
    EvaluatorCompletionAccepted,
    BlockedByUser,
    EvaluatorBlocked,
    EvaluatorContinuationAccepted,
    UserInterrupted,
    ContinuationBudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalTransitionKind {
    Pause,
    Resume,
    Clear,
    MarkDoneByUser,
    BlockByUser,
    EvaluatorDone,
    EvaluatorBlocked,
    EvaluatorContinue,
    UserInterrupted,
    ContinuationBudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalTransitionError {
    prior_state: GoalObservedState,
    transition: GoalTransitionKind,
}

impl GoalTransitionError {
    pub const fn prior_state(&self) -> GoalObservedState {
        self.prior_state
    }
}

impl std::fmt::Display for GoalTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "illegal goal transition {:?} from {:?}",
            self.transition, self.prior_state
        )
    }
}

impl std::error::Error for GoalTransitionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEvidenceAvailability {
    Available,
    Unavailable,
    Unknown,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalBudgetAccounting {
    pub turn_budget: u32,
    pub turns_used: u32,
    pub remaining_turns: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalTransitionFact {
    pub goal_id: String,
    pub prior_state: GoalObservedState,
    pub current_state: GoalObservedState,
    pub stop_reason: GoalStopReason,
    pub budget: GoalBudgetAccounting,
    pub evidence: GoalEvidenceAvailability,
    pub user_interrupted: bool,
    pub observed_at: String,
}

pub(super) fn ensure_legal_transition(
    prior: PersistentGoalStatus,
    transition: GoalTransitionKind,
) -> Result<(), GoalTransitionError> {
    let legal = match transition {
        GoalTransitionKind::Pause => matches!(prior, PersistentGoalStatus::Active),
        GoalTransitionKind::Resume => {
            matches!(
                prior,
                PersistentGoalStatus::Paused | PersistentGoalStatus::Blocked
            )
        }
        GoalTransitionKind::Clear => !matches!(prior, PersistentGoalStatus::Cleared),
        GoalTransitionKind::MarkDoneByUser
        | GoalTransitionKind::BlockByUser
        | GoalTransitionKind::EvaluatorDone
        | GoalTransitionKind::EvaluatorBlocked
        | GoalTransitionKind::EvaluatorContinue
        | GoalTransitionKind::UserInterrupted
        | GoalTransitionKind::ContinuationBudgetExhausted => {
            matches!(prior, PersistentGoalStatus::Active)
        }
    };
    if legal {
        Ok(())
    } else {
        Err(GoalTransitionError {
            prior_state: prior.into(),
            transition,
        })
    }
}

pub(super) fn transition_fact(
    goal: &PersistentGoal,
    prior_state: GoalObservedState,
    stop_reason: GoalStopReason,
    observed_at: String,
) -> GoalTransitionFact {
    GoalTransitionFact {
        goal_id: goal.id.clone(),
        prior_state,
        current_state: goal.status.into(),
        stop_reason,
        budget: GoalBudgetAccounting {
            turn_budget: goal.turn_budget,
            turns_used: goal.turns_used,
            remaining_turns: goal.turn_budget.saturating_sub(goal.turns_used),
        },
        evidence: GoalEvidenceAvailability::Available,
        user_interrupted: false,
        observed_at,
    }
}
