use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{EntrustedWorkError, ObservationError};

pub const WEB_WATCH_METADATA_KEY: &str = "web_watch";
pub const WEB_WATCH_WORK_KIND: &str = "web_watch.observation";
pub const WEB_WATCH_OUTCOME: &str = "web_watch_observation";
pub const WEB_TEXT_VERSION: &str = "normalized_static_text_v1";
pub const WEB_WATCH_MIN_CADENCE_SECONDS: u64 = 600;
pub const WEB_WATCH_MAX_CADENCE_SECONDS: u64 = 86_400;

/// One region between fixed anchors of the `normalized_static_text_v1` text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebExtraction {
    pub version: String,
    pub prefix: String,
    pub suffix: String,
    pub occurrence: u32,
    pub match_count: u32,
    pub value: WebValueKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebValueKind {
    Number {
        decimal_separator: char,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thousands_separator: Option<char>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_after: Option<String>,
    },
    Text {
        phrase: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebCondition {
    AtMost { value: String },
    AtLeast { value: String },
    TextPresent,
    TextAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebObservedValue {
    Number { value: String },
    Text { present: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebCheckFailure {
    NetworkError,
    GuardBlocked,
    HttpError,
    RateLimited,
    StaticContentUnavailable,
    AmbiguousValue,
    FormatMismatch,
}

/// Stored plan. Holds the full confirmed URL and never appears in outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebWatchPlan {
    pub url: String,
    pub extraction: WebExtraction,
    pub condition: WebCondition,
    pub cadence_seconds: u64,
    pub first_wake_at_ms: u64,
}

impl WebWatchPlan {
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        format!("sha256:{:x}", Sha256::digest(bytes))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebWatchEntry {
    pub work_id: String,
    pub plan: WebWatchPlan,
    pub baseline: WebEvidence,
    pub baseline_revision: u64,
    pub baseline_transition_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebWatchWake {
    pub entrusted_work_id: String,
    pub owner_session_id: String,
    pub goal_id: String,
    pub condition_revision: u64,
    pub goal_transition_id: String,
    pub plan_digest: String,
    pub due_at_ms: u64,
}

impl WebWatchWake {
    pub fn lineage_prefix(&self) -> String {
        let digest = Sha256::digest(self.goal_transition_id.as_bytes());
        format!(
            "web-watch:{}:{}:{digest:x}:",
            self.entrusted_work_id, self.condition_revision
        )
    }

    pub fn durable_work_id(&self) -> String {
        format!("{}{}", self.lineage_prefix(), self.due_at_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebResponseFacts {
    pub final_url: String,
    pub final_url_digest: String,
    pub http_status: u16,
    pub text_digest: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebEvidence {
    pub observation_ref: String,
    pub observed_at_ms: u64,
    pub value: WebObservedValue,
    pub condition_matches: bool,
    pub response: WebResponseFacts,
}

#[derive(Debug)]
pub enum WebWatchError {
    Observation(ObservationError),
    InvalidInput(String),
    Unreadable {
        failure: WebCheckFailure,
        http_status: Option<u16>,
    },
    ValueChanged {
        shown: WebObservedValue,
        current: WebObservedValue,
    },
}

impl std::fmt::Display for WebWatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Observation(error) => error.fmt(formatter),
            Self::InvalidInput(reason) => write!(formatter, "invalid web watch: {reason}"),
            Self::Unreadable {
                failure,
                http_status,
            } => match http_status {
                Some(status) => write!(
                    formatter,
                    "web page could not be read: {failure:?} (HTTP {status})"
                ),
                None => write!(formatter, "web page could not be read: {failure:?}"),
            },
            Self::ValueChanged { shown, current } => write!(
                formatter,
                "page value changed since the preview (shown {}, now {}); preview again",
                value_text(shown),
                value_text(current)
            ),
        }
    }
}

impl std::error::Error for WebWatchError {}

impl From<ObservationError> for WebWatchError {
    fn from(error: ObservationError) -> Self {
        Self::Observation(error)
    }
}

impl From<EntrustedWorkError> for WebWatchError {
    fn from(error: EntrustedWorkError) -> Self {
        Self::Observation(ObservationError::Work(error))
    }
}

impl From<std::io::Error> for WebWatchError {
    fn from(error: std::io::Error) -> Self {
        Self::Observation(ObservationError::Io(error))
    }
}

pub(super) fn value_text(value: &WebObservedValue) -> String {
    match value {
        WebObservedValue::Number { value } => value.clone(),
        WebObservedValue::Text { present } => format!("phrase present={present}"),
    }
}
