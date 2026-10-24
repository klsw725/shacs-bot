use super::task_method_calc::bounded;
use serde::{Deserialize, Serialize};

pub(crate) const ATTEMPT_LIMIT: usize = 16;
pub(crate) const ARTIFACT_LIMIT: usize = 8;
pub(crate) const TEXT_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Generated,
    CommitUncertain,
    Unavailable,
    Unsupported,
    Failed,
    WriteFailed,
    Denied,
    Cancelled,
    Blocked,
}

impl AttemptStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Generated => "generated and committed",
            Self::CommitUncertain => "reported, but the saved file could not be verified",
            Self::Unavailable => "provider or credentials unavailable",
            Self::Unsupported => "unsupported",
            Self::Failed => "generation failed",
            Self::WriteFailed => "image could not be saved",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled before it ran",
            Self::Blocked => "blocked before it ran",
        }
    }
}

/// One generated image as committed by `image_generate`, checked on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageArtifact {
    pub artifact_id: String,
    pub reference: String,
    pub sha256: String,
    pub committed: bool,
    pub format: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_len: Option<u64>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAttempt {
    pub seq: u64,
    pub call_id: String,
    pub selection_revision: u64,
    pub status: AttemptStatus,
    pub options: Vec<String>,
    pub prompt: Option<String>,
    pub artifacts: Vec<ImageArtifact>,
    pub error: Option<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Size,
    Format,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Met,
    NotMet,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub kind: RequirementKind,
    pub expected: String,
    pub state: CheckState,
    pub observed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageState {
    NotRun,
    Unavailable,
    Unsupported,
    Failed,
    Uncertain,
    Generated,
}

impl ImageState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotRun => "not generated",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
            Self::Uncertain => "save not confirmed",
            Self::Generated => "generated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageVerdict {
    pub selection_revision: u64,
    pub state: ImageState,
    pub reason: String,
    pub result: Option<u64>,
    pub requirements: Vec<Requirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImageEvidence {
    pub version: u64,
    pub revision_of: Option<String>,
    pub purpose: Option<String>,
    pub previous: Vec<ImageArtifact>,
    pub attempts: Vec<ImageAttempt>,
    pub next_seq: u64,
    pub omitted_attempts: usize,
    pub verdict: Option<ImageVerdict>,
    /// Images of the last call that saved and confirmed them, kept even when
    /// older calls fall out of the retention limit.
    pub last_saved: Vec<ImageArtifact>,
    pub last_saved_revision: Option<u64>,
    /// The model's last reply, which is never shown; kept as a diagnostic preview.
    pub withheld_reply: Option<String>,
    /// The first method revision whose calls still count: calls carry over a
    /// method change that keeps the image part and are judged again.
    pub since: u64,
}

impl Default for ImageEvidence {
    fn default() -> Self {
        Self {
            version: 1,
            revision_of: None,
            purpose: None,
            previous: Vec::new(),
            attempts: Vec::new(),
            next_seq: 0,
            omitted_attempts: 0,
            verdict: None,
            last_saved: Vec::new(),
            last_saved_revision: None,
            withheld_reply: None,
            since: 0,
        }
    }
}

impl ImageEvidence {
    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty()
            && self.verdict.is_none()
            && self.revision_of.is_none()
            && self.omitted_attempts == 0
    }

    pub(crate) fn push_attempt(&mut self, mut attempt: ImageAttempt, revision: u64) {
        self.next_seq = self.next_seq.saturating_add(1);
        attempt.seq = self.next_seq;
        attempt.selection_revision = revision;
        attempt.artifacts.truncate(ARTIFACT_LIMIT);
        if attempt.status == AttemptStatus::Generated {
            self.last_saved = attempt
                .artifacts
                .iter()
                .filter(|a| a.committed)
                .cloned()
                .collect();
            self.last_saved_revision = Some(revision);
        }
        self.attempts.push(attempt);
        let removed = self.attempts.len().saturating_sub(ATTEMPT_LIMIT);
        self.attempts.drain(..removed);
        self.omitted_attempts = self.omitted_attempts.saturating_add(removed);
    }

    /// The committed images of the attempt the verdict chose as the result.
    /// Whether a call or save recorded in method revision `recorded` counts
    /// for `revision`.
    pub fn counts(&self, recorded: u64, revision: u64) -> bool {
        (self.since..=revision).contains(&recorded)
    }

    pub fn result_artifacts(&self) -> Vec<&ImageArtifact> {
        let Some(seq) = self.verdict.as_ref().and_then(|verdict| verdict.result) else {
            return Vec::new();
        };
        self.attempts
            .iter()
            .filter(|attempt| attempt.seq == seq)
            .flat_map(|attempt| &attempt.artifacts)
            .filter(|artifact| artifact.committed)
            .collect()
    }

    /// The images of the last call that saved and confirmed them, current or not.
    pub fn saved_artifacts(&self) -> Vec<&ImageArtifact> {
        self.last_saved.iter().collect()
    }

    pub(crate) fn next_version(previous: &ImageEvidence, request_id: &str, goal: &str) -> Self {
        let mut earlier: Vec<ImageArtifact> =
            previous.saved_artifacts().into_iter().cloned().collect();
        if earlier.is_empty() {
            earlier = previous.previous.clone();
        }
        earlier.truncate(ARTIFACT_LIMIT);
        Self {
            version: previous.version.saturating_add(1),
            revision_of: Some(bounded(request_id, TEXT_LIMIT)),
            purpose: Some(
                previous
                    .purpose
                    .clone()
                    .unwrap_or_else(|| bounded(goal, 512)),
            ),
            previous: earlier,
            ..Self::default()
        }
    }
}
