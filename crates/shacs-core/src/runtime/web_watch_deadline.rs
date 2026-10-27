use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{EntrustedWorkReadRequest, WebCondition, WebEvidence};

pub const WEB_DEADLINE_WORK_KIND: &str = "web_watch.deadline";
pub const WEB_DEADLINE_OUTCOME: &str = "web_watch.deadline.v1";
pub const WEB_DEADLINE_DEFAULT_REMIND_BEFORE_SECONDS: u64 = 86_400;
pub const WEB_DEADLINE_MIN_REMIND_BEFORE_SECONDS: u64 = 60;
pub const WEB_DEADLINE_MAX_REMIND_BEFORE_SECONDS: u64 = 30 * 86_400;

/// A decision deadline: the RFC 3339 text as the user gave it, its instant, and when to remind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDeadline {
    pub at_ms: u64,
    pub at_text: String,
    pub remind_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_before_seconds: Option<u64>,
    #[serde(default)]
    pub remind_now: bool,
}

/// The deadline of one entrusted work under its own revision, apart from the observation
/// revision, so changing it never resets the comparison baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDeadlineSettings {
    pub work_id: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<WebDeadline>,
    pub changed_at_ms: u64,
}

/// The one wake of a deadline revision at its reminder time, in one goal lineage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDeadlineWake {
    pub entrusted_work_id: String,
    pub owner_session_id: String,
    pub goal_id: String,
    pub goal_transition_id: String,
    pub deadline_revision: u64,
    pub due_at_ms: u64,
}

impl WebDeadlineWake {
    pub fn durable_work_id(&self) -> String {
        let digest = Sha256::digest(self.goal_transition_id.as_bytes());
        format!(
            "web-deadline:{}:{}:{digest:x}",
            self.entrusted_work_id, self.deadline_revision
        )
    }
}

/// What the wake found at the reminder time: the deadline, the goal, the condition and the
/// latest successful read of the current plan, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDeadlineRecord {
    pub wake: WebDeadlineWake,
    pub reached_at_ms: u64,
    pub deadline: WebDeadline,
    pub goal: String,
    pub url: String,
    pub url_digest: String,
    pub condition_revision: u64,
    pub condition: WebCondition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<WebEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebDeadlineView {
    pub revision: u64,
    pub deadline: Option<WebDeadline>,
    pub changed_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ConfigureWebDeadlineRequest {
    pub work: EntrustedWorkReadRequest,
    /// The deadline revision the caller saw; 0 when none was set yet.
    pub expected_revision: u64,
    /// `None` clears the deadline.
    pub deadline: Option<WebDeadlineInput>,
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDeadlineInput {
    /// RFC 3339 with an explicit offset, kept as given.
    pub at: String,
    pub remind_before_seconds: Option<u64>,
    /// Remind right after setting when the reminder time has already passed.
    pub remind_now: bool,
}
