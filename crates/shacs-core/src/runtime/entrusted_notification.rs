use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ObservationDecision, ObservationRecord, ObservationVerdict};

pub const ENTRUSTED_NOTIFICATION_PREFIX: &str = "entrusted-notification:";
pub const ENTRUSTED_NOTIFICATION_CORRELATION: &str = "entrusted_notification_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrustedNotificationKind {
    Change,
    RegularSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedNotificationIntent {
    pub correlation_id: String,
    pub owner_session_id: String,
    pub entrusted_work_id: String,
    pub observation_work_id: String,
    pub decision_id: String,
    pub condition_revision: u64,
    pub goal_transition_id: String,
    pub kind: EntrustedNotificationKind,
}

impl EntrustedNotificationIntent {
    pub(super) fn from_record(record: &ObservationRecord) -> Option<Self> {
        let decision = record.decision.as_ref()?;
        let kind = notification_kind(decision)?;
        let digest = Sha256::digest(format!("{}:{kind:?}", decision.decision_id).as_bytes());
        Some(Self {
            correlation_id: format!("{ENTRUSTED_NOTIFICATION_PREFIX}{digest:x}"),
            owner_session_id: record.wake.owner_session_id.clone(),
            entrusted_work_id: decision.entrusted_work_id.clone(),
            observation_work_id: decision.observation_work_id.clone(),
            decision_id: decision.decision_id.clone(),
            condition_revision: decision.condition_revision,
            goal_transition_id: decision.goal_transition_id.clone(),
            kind,
        })
    }
}

fn notification_kind(decision: &ObservationDecision) -> Option<EntrustedNotificationKind> {
    match decision.verdict {
        ObservationVerdict::NotifyCandidate => Some(EntrustedNotificationKind::Change),
        ObservationVerdict::Baseline | ObservationVerdict::Suppress => decision
            .regular_summary_eligible
            .then_some(EntrustedNotificationKind::RegularSummary),
        ObservationVerdict::Unknown | ObservationVerdict::Blocked => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrustedNotificationStage {
    EnqueuePending,
    EnqueueFailed,
    Enqueued,
    UnsupportedTarget,
    SuppressedStale,
    Consuming,
    OutboundPending,
    DeliveryFailed,
    SentHint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntrustedNotificationFact {
    pub intent: EntrustedNotificationIntent,
    pub stage: EntrustedNotificationStage,
    pub writer_instance_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrustedNotificationEnqueueState {
    Pending,
    Failed,
    Enqueued,
    Suppressed,
    UnsupportedTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrustedNotificationDeliveryState {
    NotRequested,
    Pending,
    Failed,
    Unknown,
    SentHint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrustedNotificationSnapshot {
    pub intent: EntrustedNotificationIntent,
    pub enqueue: EntrustedNotificationEnqueueState,
    pub consumption_started: bool,
    pub suppressed_stale: bool,
    pub delivery: EntrustedNotificationDeliveryState,
}
