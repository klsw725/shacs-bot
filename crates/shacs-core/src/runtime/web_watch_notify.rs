use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{EntrustedWorkReadRequest, WebCheckFailure, WebCondition, WebEvidence, WebWatchWake};

pub const WEB_NOTIFY_PREFIX: &str = "web-watch-notification:";
pub const WEB_NOTIFY_MAX_THRESHOLD_SECONDS: u64 = 7 * 86_400;
pub const WEB_NOTIFY_ENQUEUE_ATTEMPTS: u32 = 5;
pub const WEB_NOTIFY_ENQUEUE_RETRY_MS: u64 = 60_000;
const DEFAULT_THRESHOLD_FLOOR_SECONDS: u64 = 6 * 3_600;

/// 042 notification settings of one entrusted work. Their revision is separate from the
/// observation revision, so changing them never resets the comparison baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebNotifySettings {
    pub work_id: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_threshold_seconds: Option<u64>,
    pub changed_at_ms: u64,
}

impl WebNotifySettings {
    /// The chosen failure and gap threshold, or `max(2 x cadence, 6 h)`; never below 2 x cadence.
    pub fn threshold_seconds(&self, cadence_seconds: u64) -> u64 {
        let floor = cadence_seconds.saturating_mul(2);
        self.failure_threshold_seconds
            .unwrap_or(floor.max(DEFAULT_THRESHOLD_FLOOR_SECONDS))
            .max(floor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebNotifyKind {
    Change,
    Failure,
    Recovery,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebRecoveredInterval {
    pub interval_id: String,
    pub started_at_ms: u64,
    pub failure_count: u32,
}

/// What one notification says. Bounded values only: no page text, URL query or fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebNotifyContent {
    Change {
        condition: WebCondition,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phrase: Option<String>,
        previous: Box<WebEvidence>,
        current: Box<WebEvidence>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovered: Option<WebRecoveredInterval>,
    },
    Failure {
        failure: WebCheckFailure,
        started_at_ms: u64,
        failure_count: u32,
        failing_for_ms: u64,
        last_success_at_ms: Option<u64>,
        next_check_at_ms: Option<u64>,
        #[serde(default)]
        held_until_next_check: bool,
    },
    Recovery {
        started_at_ms: u64,
        failure_count: u32,
        recovered_at_ms: u64,
    },
    Gap {
        from_ms: u64,
        to_ms: u64,
        next_check_at_ms: Option<u64>,
    },
}

impl WebNotifyContent {
    pub fn kind(&self) -> WebNotifyKind {
        match self {
            Self::Change { .. } => WebNotifyKind::Change,
            Self::Failure { .. } => WebNotifyKind::Failure,
            Self::Recovery { .. } => WebNotifyKind::Recovery,
            Self::Gap { .. } => WebNotifyKind::Gap,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebNotifyIntent {
    pub correlation_id: String,
    pub owner_session_id: String,
    pub entrusted_work_id: String,
    pub target: String,
    pub settings_revision: u64,
    pub event_id: String,
    pub wake: WebWatchWake,
    pub url: String,
    pub url_digest: String,
    pub content: WebNotifyContent,
}

impl WebNotifyIntent {
    pub fn kind(&self) -> WebNotifyKind {
        self.content.kind()
    }

    pub(super) fn correlation(kind: WebNotifyKind, event_id: &str, revision: u64) -> String {
        let digest = Sha256::digest(format!("{kind:?}:{event_id}:{revision}").as_bytes());
        format!("{WEB_NOTIFY_PREFIX}{digest:x}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebNotifyStage {
    EnqueuePending,
    EnqueueFailed,
    Enqueued,
    SuppressedStale,
    Consuming,
    OutboundPending,
    DeliveryFailed,
    SentHint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebNotifyFact {
    pub intent: WebNotifyIntent,
    pub stage: WebNotifyStage,
    pub writer_instance_ref: String,
    /// Runtime clock time of a failed enqueue attempt; the next attempt waits from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebNotifyEnqueue {
    Pending,
    Failed,
    Enqueued,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebNotifyDelivery {
    NotRequested,
    Pending,
    Failed,
    Unknown,
    SentHint,
}

/// Enqueue and delivery facts of one notification, kept apart from the check that raised it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebNotificationSnapshot {
    pub correlation_id: String,
    pub kind: WebNotifyKind,
    pub event_id: String,
    pub settings_revision: u64,
    pub target: String,
    pub enqueue: WebNotifyEnqueue,
    pub enqueue_failures: u32,
    pub consumption_started: bool,
    pub suppressed_stale: bool,
    pub delivery: WebNotifyDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebNotifySettingsView {
    pub revision: u64,
    pub target: Option<String>,
    pub failure_threshold_seconds: Option<u64>,
    pub effective_threshold_seconds: u64,
    pub changed_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ConfigureWebNotifyRequest {
    pub work: EntrustedWorkReadRequest,
    /// `email:<address>`, or `None` to stop notifications.
    pub target: Option<String>,
    pub failure_threshold_seconds: Option<u64>,
    /// Whether the Email channel is enabled, consented and has SMTP settings.
    pub email_ready: bool,
    pub now_ms: u64,
}
