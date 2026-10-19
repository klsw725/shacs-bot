use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

use super::{DurableDispatchError, EntrustedWorkError, EntrustedWorkReadRequest};

pub const ENTRUSTED_OBSERVATION_WORK_KIND: &str = "entrusted.observation";
pub const ENTRUSTED_OBSERVATION_OUTCOME: &str = "entrusted_observation";
pub const MAX_OBSERVATION_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedObservationPlan {
    pub source: ObservationSource,
    pub cadence_seconds: u64,
    pub first_wake_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<super::ObservationComparisonPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationSource {
    LocalFile { relative_path: String },
}

impl EntrustedObservationPlan {
    pub fn validate(&self) -> Result<(), ObservationError> {
        if let Some(policy) = &self.comparison {
            policy.validate()?;
        }
        if !(5..=86_400).contains(&self.cadence_seconds) {
            return Err(ObservationError::InvalidInput(
                "cadence must be 5..86400 seconds",
            ));
        }
        let ObservationSource::LocalFile { relative_path } = &self.source;
        if relative_path.is_empty()
            || relative_path.len() > 512
            || relative_path.chars().any(char::is_control)
            || Path::new(relative_path)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(ObservationError::InvalidInput(
                "source must be a bounded workspace-relative file",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ConfigureObservationRequest {
    pub work: EntrustedWorkReadRequest,
    pub expected_revision: u64,
    pub plan: EntrustedObservationPlan,
    pub changed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationWake {
    pub entrusted_work_id: String,
    pub owner_session_id: String,
    pub goal_id: String,
    pub condition_revision: u64,
    pub goal_transition_id: String,
    pub plan: EntrustedObservationPlan,
    pub due_at_ms: u64,
}

impl ObservationWake {
    pub fn lineage_prefix(&self) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.goal_transition_id.as_bytes());
        format!(
            "observation:{}:{}:{digest:x}:",
            self.entrusted_work_id, self.condition_revision
        )
    }

    pub fn durable_work_id(&self) -> String {
        format!("{}{}", self.lineage_prefix(), self.due_at_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationBlock {
    GoalInactive,
    StaleLineage,
    ConsentMissing,
    SourceMissing,
    ReadFailed,
    UnsupportedSource,
    SourceTooLarge,
    SnapshotUnavailable,
    HookDenied,
    PermissionDenied,
    Cancelled,
    RecoveryNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ObservationResult {
    Observed {
        evidence_ref: String,
        byte_count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<super::IntegerObservationSummary>,
    },
    Unavailable {
        reason: ObservationBlock,
    },
    Blocked {
        reason: ObservationBlock,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationRecord {
    pub wake: ObservationWake,
    pub observed_at_ms: u64,
    pub result: ObservationResult,
    pub execution_snapshot_ref: Option<String>,
    pub next_wake_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<super::ObservationDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ObservationNextWake {
    NotConfigured,
    AwaitingEnqueue { work_id: String, due_at_ms: u64 },
    Queued { work_id: String, due_at_ms: u64 },
    Leased { work_id: String },
    Stopped { reason: ObservationBlock },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrustedObservationSnapshot {
    pub work_id: String,
    pub condition_revision: u64,
    pub plan: Option<EntrustedObservationPlan>,
    pub current_observation: Option<ObservationRecord>,
    pub history: Vec<ObservationRecord>,
    pub next_wake: ObservationNextWake,
    pub current_decision: Option<super::ObservationDecision>,
    pub comparison_baseline: Option<super::ObservationComparisonEvidence>,
    pub decisions: Vec<super::ObservationDecision>,
    pub notifications: Vec<super::EntrustedNotificationSnapshot>,
}

#[derive(Debug)]
pub enum ObservationError {
    Work(EntrustedWorkError),
    Durable(DurableDispatchError),
    Io(std::io::Error),
    InvalidInput(&'static str),
    Blocked(ObservationBlock),
}

impl std::fmt::Display for ObservationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Work(error) => error.fmt(formatter),
            Self::Durable(error) => error.fmt(formatter),
            Self::Io(error) => write!(formatter, "observation I/O failed: {error}"),
            Self::InvalidInput(reason) => write!(formatter, "invalid observation: {reason}"),
            Self::Blocked(reason) => write!(formatter, "observation blocked: {reason:?}"),
        }
    }
}
impl std::error::Error for ObservationError {}
impl From<EntrustedWorkError> for ObservationError {
    fn from(error: EntrustedWorkError) -> Self {
        Self::Work(error)
    }
}
impl From<DurableDispatchError> for ObservationError {
    fn from(error: DurableDispatchError) -> Self {
        Self::Durable(error)
    }
}
impl From<std::io::Error> for ObservationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
