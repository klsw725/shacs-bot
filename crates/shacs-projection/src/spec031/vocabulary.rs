use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031Availability {
    Ready,
    Degraded,
    Blocked,
    Unavailable,
    Unknown,
}

impl Spec031Availability {
    pub const ALL: [Self; 5] = [
        Self::Ready,
        Self::Degraded,
        Self::Blocked,
        Self::Unavailable,
        Self::Unknown,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ApprovalState {
    Pending,
    Allowed,
    Denied,
    Expired,
    Skipped,
    RetryConsumed,
}

impl Spec031ApprovalState {
    pub const ALL: [Self; 6] = [
        Self::Pending,
        Self::Allowed,
        Self::Denied,
        Self::Expired,
        Self::Skipped,
        Self::RetryConsumed,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031InclusionReason {
    Included,
    Skipped,
    Blocked,
    Degraded,
    Missing,
    Unsupported,
    ExtractionFailed,
}

impl Spec031InclusionReason {
    pub const ALL: [Self; 7] = [
        Self::Included,
        Self::Skipped,
        Self::Blocked,
        Self::Degraded,
        Self::Missing,
        Self::Unsupported,
        Self::ExtractionFailed,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ReasonCode {
    Included,
    Skipped,
    Blocked,
    Degraded,
    Missing,
    Unsupported,
    ExtractionFailed,
    MissingExternalOwnerEvidence,
    Requested,
    Completed,
    Progress,
    Final,
    Interrupted,
    RecoveryRequested,
    RecoveryCompleted,
    RepeatedInterruption,
    PendingFollowUp,
    RetryConsumed,
}

impl Spec031ReasonCode {
    pub const ALL: [Self; 18] = [
        Self::Included,
        Self::Skipped,
        Self::Blocked,
        Self::Degraded,
        Self::Missing,
        Self::Unsupported,
        Self::ExtractionFailed,
        Self::MissingExternalOwnerEvidence,
        Self::Requested,
        Self::Completed,
        Self::Progress,
        Self::Final,
        Self::Interrupted,
        Self::RecoveryRequested,
        Self::RecoveryCompleted,
        Self::RepeatedInterruption,
        Self::PendingFollowUp,
        Self::RetryConsumed,
    ];
}

impl From<Spec031InclusionReason> for Spec031ReasonCode {
    fn from(value: Spec031InclusionReason) -> Self {
        match value {
            Spec031InclusionReason::Included => Self::Included,
            Spec031InclusionReason::Skipped => Self::Skipped,
            Spec031InclusionReason::Blocked => Self::Blocked,
            Spec031InclusionReason::Degraded => Self::Degraded,
            Spec031InclusionReason::Missing => Self::Missing,
            Spec031InclusionReason::Unsupported => Self::Unsupported,
            Spec031InclusionReason::ExtractionFailed => Self::ExtractionFailed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ProgressDelivery {
    Live,
    Coalesced,
    Dropped,
    Reconnected,
    FinalDelivered,
    FinalPending,
    FinalFailed,
    FinalUnknown,
}

impl Spec031ProgressDelivery {
    pub const ALL: [Self; 8] = [
        Self::Live,
        Self::Coalesced,
        Self::Dropped,
        Self::Reconnected,
        Self::FinalDelivered,
        Self::FinalPending,
        Self::FinalFailed,
        Self::FinalUnknown,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ProjectionKind {
    Session,
    Turn,
    Subagent,
    Approval,
    Tool,
    Context,
    Plugin,
    App,
    Media,
    Diagnostics,
    ReleaseEvidence,
    Readiness,
    Progress,
}

impl Spec031ProjectionKind {
    pub const ALL: [Self; 13] = [
        Self::Session,
        Self::Turn,
        Self::Subagent,
        Self::Approval,
        Self::Tool,
        Self::Context,
        Self::Plugin,
        Self::App,
        Self::Media,
        Self::Diagnostics,
        Self::ReleaseEvidence,
        Self::Readiness,
        Self::Progress,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Spec031Severity {
    pub const ALL: [Self; 4] = [Self::Info, Self::Warning, Self::Error, Self::Critical];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031Freshness {
    Current,
    Stale,
    Unavailable,
    Unknown,
}

impl Spec031Freshness {
    pub const ALL: [Self; 4] = [Self::Current, Self::Stale, Self::Unavailable, Self::Unknown];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031SourceOwner {
    Spec029,
    Spec030,
    Spec031,
    Spec032,
    Spec033,
    Spec034,
    Spec035,
    Session,
    Workflow,
    Channel,
    Projection,
}

impl Spec031SourceOwner {
    pub const ALL: [Self; 11] = [
        Self::Spec029,
        Self::Spec030,
        Self::Spec031,
        Self::Spec032,
        Self::Spec033,
        Self::Spec034,
        Self::Spec035,
        Self::Session,
        Self::Workflow,
        Self::Channel,
        Self::Projection,
    ];
}
