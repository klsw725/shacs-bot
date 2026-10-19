use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ObservationBlock, ObservationError, ObservationSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationComparisonPolicy {
    pub criterion: ObservationCriterion,
    pub max_age_seconds: u64,
    #[serde(default)]
    pub regular_summary: bool,
}

impl ObservationComparisonPolicy {
    pub fn validate(&self) -> Result<(), ObservationError> {
        if !(5..=86_400).contains(&self.max_age_seconds) {
            return Err(ObservationError::InvalidInput(
                "comparison max age must be 5..86400 seconds",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationCriterion {
    IntegerAtMost { value: i64 },
}

impl ObservationCriterion {
    pub fn matches(&self, observed: i64) -> bool {
        match self {
            Self::IntegerAtMost { value } => observed <= *value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSummarySchema {
    IntegerV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegerObservationSummary {
    pub schema: ObservationSummarySchema,
    pub value: i64,
    pub as_of_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationVerdict {
    Baseline,
    Suppress,
    NotifyCandidate,
    Unknown,
    Blocked,
}

impl ObservationVerdict {
    pub(super) fn establishes_baseline(self) -> bool {
        match self {
            Self::Baseline | Self::Suppress | Self::NotifyCandidate => true,
            Self::Unknown | Self::Blocked => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ObservationComparisonReason {
    FirstComparableObservation,
    LineageReset,
    EquivalentValue,
    SameConditionOutcome,
    ConditionOutcomeChanged,
    CriteriaNotConfigured,
    SummaryUnavailable,
    BaselineUnavailable,
    SourceStale,
    SourceFromFuture,
    SourceOutOfOrder,
    SourceTimestampConflict,
    BaselineStale,
    ObservationUnavailable { reason: ObservationBlock },
    ObservationBlocked { reason: ObservationBlock },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationComparisonEvidence {
    pub observation_work_id: String,
    pub evidence_ref: String,
    pub byte_count: u64,
    pub observed_at_ms: u64,
    pub summary: IntegerObservationSummary,
    pub condition_matches: bool,
    pub execution_snapshot_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationComparisonLimit {
    UserOwnedLocalSourceOnly,
    SourceAssertedTimestamp,
    IntegerConditionOnly,
    NoExternalVerification,
    NoDeliveryOrAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservationDecisionBasis {
    ObservationTerminal,
    LegacyObservationUncompared,
    CancellationFact { event_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationDecision {
    pub basis: ObservationDecisionBasis,
    pub decision_id: String,
    pub observation_work_id: String,
    pub entrusted_work_id: String,
    pub condition_revision: u64,
    pub goal_transition_id: String,
    pub source: ObservationSource,
    pub policy: Option<ObservationComparisonPolicy>,
    pub evaluated_at_ms: u64,
    pub previous: Option<ObservationComparisonEvidence>,
    pub current: Option<ObservationComparisonEvidence>,
    pub verdict: ObservationVerdict,
    pub reason: ObservationComparisonReason,
    pub regular_summary_eligible: bool,
    pub limitations: Vec<ObservationComparisonLimit>,
}

pub(super) fn decision_id(observation_work_id: &str) -> String {
    format!(
        "comparison:v1:{:x}",
        Sha256::digest(observation_work_id.as_bytes())
    )
}
