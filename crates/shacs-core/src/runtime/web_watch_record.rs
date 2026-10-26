use serde::{Deserialize, Serialize};

use super::{
    EntrustedWorkReadRequest, ObservationBlock, ObservationNextWake, ObservationVerdict,
    PersistentGoalStatus, WebCheckFailure, WebCondition, WebDeadlineView, WebEvidence,
    WebExtraction, WebNotificationSnapshot, WebNotifySettingsView, WebObservedValue,
    WebResponseFacts, WebWatchWake,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WebCheckResult {
    Observed {
        response: WebResponseFacts,
        value: WebObservedValue,
    },
    Failed {
        failure: WebCheckFailure,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<WebResponseFacts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
    Blocked {
        reason: ObservationBlock,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebRuntimeGap {
    pub from_ms: u64,
    pub to_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WebDecisionReason {
    ConfirmedBaseline,
    LineageReset,
    EquivalentValue,
    SameConditionOutcome,
    ConditionOutcomeChanged,
    CheckFailed { failure: WebCheckFailure },
    ObservationBlocked { reason: ObservationBlock },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebDecision {
    pub decision_id: String,
    pub verdict: ObservationVerdict,
    pub reason: WebDecisionReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<WebEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<WebEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebObservationRecord {
    pub wake: WebWatchWake,
    pub observed_at_ms: u64,
    pub request_url: String,
    pub request_url_digest: String,
    pub text_version: String,
    pub result: WebCheckResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_gap: Option<WebRuntimeGap>,
    pub next_wake_at_ms: Option<u64>,
    pub execution_snapshot_ref: Option<String>,
    pub decision: WebDecision,
    /// The 042 notification settings revision in force when this check was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_revision: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebIntervalCloseReason {
    Recovered,
    LineageChanged,
    GoalInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebIntervalClosure {
    pub reason: WebIntervalCloseReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFailureInterval {
    pub interval_id: String,
    pub condition_revision: u64,
    pub goal_transition_id: String,
    pub started_at_ms: u64,
    pub last_failure_at_ms: u64,
    pub failure_count: u32,
    pub first_failure: WebCheckFailure,
    pub last_failure: WebCheckFailure,
    /// Runtime gaps inside the interval; they do not count towards its length.
    #[serde(default)]
    pub excluded_gap_ms: u64,
    /// The next check the last failure scheduled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_check_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<WebIntervalClosure>,
}

impl WebFailureInterval {
    /// Failing time without runtime gaps: up to the success that closed the interval, or up to
    /// the last failure while it is open.
    pub fn failing_for_ms(&self) -> u64 {
        self.closed
            .filter(|closure| closure.reason == WebIntervalCloseReason::Recovered)
            .and_then(|closure| closure.at_ms)
            .unwrap_or(self.last_failure_at_ms)
            .saturating_sub(self.started_at_ms)
            .saturating_sub(self.excluded_gap_ms)
    }

    pub(super) fn exclude(&mut self, gap_ms: u64) {
        self.excluded_gap_ms = self.excluded_gap_ms.saturating_add(gap_ms);
    }

    /// Failing time a notification judges. When the last failure put the next check later
    /// than one cadence (429/503 backoff, `Retry-After`), nothing can recover before it, so the
    /// interval counts up to that check instead of staying silent until then.
    pub fn held_failing_ms(&self, cadence_ms: u64) -> u64 {
        let held = self
            .next_check_at_ms
            .filter(|next| next.saturating_sub(self.last_failure_at_ms) > cadence_ms)
            .map_or(0, |next| {
                next.saturating_sub(self.started_at_ms)
                    .saturating_sub(self.excluded_gap_ms)
            });
        self.failing_for_ms().max(held)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebWatchPlanView {
    pub url: String,
    pub url_digest: String,
    pub extraction: WebExtraction,
    pub condition: WebCondition,
    pub cadence_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebWatchSnapshot {
    pub work_id: String,
    pub condition_revision: u64,
    pub goal_status: PersistentGoalStatus,
    pub plan: Option<WebWatchPlanView>,
    pub baseline: Option<WebEvidence>,
    pub last_success_at_ms: Option<u64>,
    pub current_decision: Option<WebDecision>,
    pub failure_interval: Option<WebFailureInterval>,
    pub failure_intervals: Vec<WebFailureInterval>,
    pub runtime_gaps: Vec<WebRuntimeGap>,
    pub next_wake: ObservationNextWake,
    pub history: Vec<WebObservationRecord>,
    pub notify: Option<WebNotifySettingsView>,
    #[serde(default)]
    pub deadline: Option<WebDeadlineView>,
    pub notifications: Vec<WebNotificationSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebPreviewTarget {
    Number {
        label: String,
        decimal_comma: bool,
    },
    Text {
        phrase: String,
        area: Option<WebTextArea>,
    },
}

/// The text between `label` and the next `until`, offered whether or not the phrase is in
/// it. Without an area the phrase must already be on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebTextArea {
    pub label: String,
    pub until: String,
}

#[derive(Debug, Clone)]
pub struct WebPreviewRequest {
    pub work: EntrustedWorkReadRequest,
    pub url: String,
    pub target: WebPreviewTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebCandidate {
    pub token: String,
    pub shown_text: String,
    pub value: WebObservedValue,
    pub extraction: WebExtraction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPreview {
    pub url: String,
    pub final_url: String,
    pub http_status: u16,
    pub truncated: bool,
    pub text_digest: String,
    pub candidates: Vec<WebCandidate>,
}

#[derive(Debug, Clone)]
pub struct ConfirmWebWatchRequest {
    pub work: EntrustedWorkReadRequest,
    pub expected_revision: u64,
    pub url: String,
    pub candidate_token: String,
    pub condition: WebCondition,
    pub cadence_seconds: u64,
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebWatchConfigured {
    pub work_id: String,
    pub condition_revision: u64,
    pub plan: WebWatchPlanView,
    pub baseline: WebEvidence,
    pub first_wake_at_ms: u64,
}
