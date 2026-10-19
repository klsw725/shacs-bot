use serde::{Deserialize, Serialize};

use super::{GoalMetadataError, GoalTransitionError, PersistentGoalStatus};

pub const ENTRUSTED_WORK_METADATA_KEY: &str = "entrusted_work";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrustedWorkConsentKind {
    ExplicitRequest,
    ConfirmedProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedWorkConsentProof {
    pub kind: EntrustedWorkConsentKind,
    pub evidence_ref: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedWorkConditions {
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<String>,
    #[serde(default)]
    pub prior_decisions: Vec<String>,
    #[serde(default)]
    pub remaining_questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedWorkRevision {
    pub revision: u64,
    pub conditions: EntrustedWorkConditions,
    #[serde(default)]
    pub linked_targets: Vec<String>,
    pub changed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedWork {
    pub work_id: String,
    pub owner_session_id: String,
    #[serde(default)]
    pub linked_targets: Vec<String>,
    pub goal_id: String,
    pub consent: EntrustedWorkConsentProof,
    pub current_revision: u64,
    pub revisions: Vec<EntrustedWorkRevision>,
    #[serde(default)]
    pub goal_transition_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_plan: Option<super::EntrustedObservationPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrustedWorkCreateRequest {
    pub owner_session_id: String,
    pub goal_text: String,
    pub turn_budget: u32,
    pub consent: EntrustedWorkConsentProof,
    pub conditions: EntrustedWorkConditions,
    pub linked_targets: Vec<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrustedWorkReadRequest {
    pub owner_session_id: String,
    pub requester_target: String,
    pub work_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrustedWorkReviseRequest {
    pub owner_session_id: String,
    pub requester_target: String,
    pub work_id: String,
    pub expected_revision: u64,
    pub conditions: EntrustedWorkConditions,
    pub linked_targets: Option<Vec<String>>,
    pub changed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrustedWorkLifecycleRequest {
    pub owner_session_id: String,
    pub requester_target: String,
    pub work_id: String,
    pub action: EntrustedWorkLifecycleAction,
    pub observed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrustedWorkLifecycleAction {
    Pause,
    Resume,
    Done,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrustedWorkSnapshot {
    pub work: EntrustedWork,
    pub goal_status: PersistentGoalStatus,
    pub latest_goal_transition_id: String,
}

impl EntrustedWork {
    pub fn current_conditions(&self) -> Option<&EntrustedWorkConditions> {
        self.revisions
            .iter()
            .find(|revision| revision.revision == self.current_revision)
            .map(|revision| &revision.conditions)
    }

    pub fn is_accessible_from(&self, target: &str) -> bool {
        self.owner_session_id == target || self.linked_targets.iter().any(|linked| linked == target)
    }
}

#[derive(Debug)]
pub enum EntrustedWorkMetadataError {
    Deserialize(serde_json::Error),
    Serialize(serde_json::Error),
}

impl std::fmt::Display for EntrustedWorkMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deserialize(error) => {
                write!(formatter, "entrusted work metadata is malformed: {error}")
            }
            Self::Serialize(error) => {
                write!(
                    formatter,
                    "entrusted work metadata could not serialize: {error}"
                )
            }
        }
    }
}

impl std::error::Error for EntrustedWorkMetadataError {}

#[derive(Debug)]
pub enum EntrustedWorkError {
    Io(std::io::Error),
    SessionMissing,
    WorkMissing,
    AccessDenied,
    WorkAlreadyExists,
    GoalAlreadyActive,
    GoalBindingMismatch,
    InvalidInput(&'static str),
    RevisionConflict { expected: u64, actual: u64 },
    Metadata(EntrustedWorkMetadataError),
    GoalMetadata(GoalMetadataError),
    GoalTransition(GoalTransitionError),
    MissingGoalTransitionId,
}

impl std::fmt::Display for EntrustedWorkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "entrusted work session I/O failed: {error}"),
            Self::SessionMissing => {
                write!(formatter, "entrusted work owner session does not exist")
            }
            Self::WorkMissing => write!(formatter, "entrusted work is not set"),
            Self::AccessDenied => write!(formatter, "target is not linked to entrusted work"),
            Self::WorkAlreadyExists => write!(formatter, "entrusted work already exists"),
            Self::GoalAlreadyActive => write!(formatter, "a nonterminal goal already exists"),
            Self::GoalBindingMismatch => write!(formatter, "entrusted work goal binding is stale"),
            Self::InvalidInput(field) => write!(formatter, "entrusted work {field} is invalid"),
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "entrusted work revision conflict: expected {expected}, actual {actual}"
            ),
            Self::Metadata(error) => write!(formatter, "{error}"),
            Self::GoalMetadata(error) => write!(formatter, "{error}"),
            Self::GoalTransition(error) => write!(formatter, "{error}"),
            Self::MissingGoalTransitionId => {
                write!(
                    formatter,
                    "new goal transition did not record an identifier"
                )
            }
        }
    }
}

impl std::error::Error for EntrustedWorkError {}

impl From<std::io::Error> for EntrustedWorkError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
