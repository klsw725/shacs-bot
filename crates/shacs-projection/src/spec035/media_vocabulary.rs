use serde::{Deserialize, Serialize};

pub const SPEC035_MEDIA_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035MediaProjectionKind {
    MediaCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035MediaState {
    Included,
    Unsupported,
    ExtractionFailed,
    AnalyzerMissing,
    Truncated,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035MediaReasonCode {
    Included,
    Unsupported,
    ExtractionFailed,
    AnalyzerMissing,
    Truncated,
    Unavailable,
}

impl From<Spec035MediaState> for Spec035MediaReasonCode {
    fn from(state: Spec035MediaState) -> Self {
        match state {
            Spec035MediaState::Included => Self::Included,
            Spec035MediaState::Unsupported => Self::Unsupported,
            Spec035MediaState::ExtractionFailed => Self::ExtractionFailed,
            Spec035MediaState::AnalyzerMissing => Self::AnalyzerMissing,
            Spec035MediaState::Truncated => Self::Truncated,
            Spec035MediaState::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035MediaOwnerUnavailableReason {
    MissingAnalyzerOwnerRef,
    MissingSpec030OwnerFacts,
    MissingExecutionSnapshot,
    StaleOwnerFacts,
    OwnerFactsUnavailable,
    OwnerFreshnessUnknown,
    AnalyzerResourceMismatch,
    SnapshotResourceMissing,
    SnapshotProvenanceInvalid,
    SnapshotRefMalformed,
}
