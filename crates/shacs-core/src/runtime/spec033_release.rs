use super::{
    replay_recorded_trajectory, RecordedTrajectoryReplayError, RecordedTrajectoryReplayReceipt,
    RecordedTrajectoryStore, RecordedTrajectoryStoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

mod release_runner;
mod release_runner_model;

const SPEC033_REDACTION_TRANSFORM_SCHEMA: &str = "spec033.redaction_transform.v1";

pub use release_runner::{
    run_spec033_release_runner, validate_spec033_release_artifacts,
    validate_spec033_release_artifacts_against, validate_spec033_release_coverage,
};
pub use release_runner_model::{
    Spec033ReleaseArtifactError, Spec033ReleaseCommandEvidence, Spec033ReleaseConfig,
    Spec033ReleaseManifest, Spec033ReleaseMode, Spec033SourceManifest, Spec033TrajectoryProvenance,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033ReleaseCheck {
    AutomationDispatch,
    GoalAccounting,
    SnapshotReplay,
    SelfImprovement,
    ReviewArtifacts,
}

impl Spec033ReleaseCheck {
    pub const fn required() -> [Self; 5] {
        [
            Self::AutomationDispatch,
            Self::GoalAccounting,
            Self::SnapshotReplay,
            Self::SelfImprovement,
            Self::ReviewArtifacts,
        ]
    }

    pub fn cargo_args(self) -> Vec<String> {
        let (package, target) = match self {
            Self::AutomationDispatch => ("shacs-core", "spec033_automation_dispatch"),
            Self::GoalAccounting => ("shacs-core", "spec033_goal_accounting"),
            Self::SnapshotReplay => ("shacs-core", "spec033_snapshot_replay"),
            Self::SelfImprovement => ("shacs-core", "spec033_self_improvement_live"),
            Self::ReviewArtifacts => ("shacs-projection", "spec033_review_artifacts"),
        };
        [
            "test",
            "--manifest-path",
            "crates/Cargo.toml",
            "-p",
            package,
            "--test",
            target,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033RedactionReceipt {
    pub schema: String,
    pub source_digest: String,
    pub output_digest: String,
    pub source_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Debug)]
pub enum Spec033ReleaseEvidenceError {
    SourceTooLarge,
    Io(std::io::Error),
    Store(RecordedTrajectoryStoreError),
    Replay(RecordedTrajectoryReplayError),
}

pub fn redact_spec033_artifact(
    source: &Path,
    output: &Path,
    byte_limit: u64,
) -> Result<Spec033RedactionReceipt, Spec033ReleaseEvidenceError> {
    let bytes = std::fs::read(source).map_err(Spec033ReleaseEvidenceError::Io)?;
    let source_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if source_bytes > byte_limit {
        return Err(Spec033ReleaseEvidenceError::SourceTooLarge);
    }
    let text = String::from_utf8_lossy(&bytes);
    let redacted = redact_host_paths(&shacs_redaction::redact_string(&text));
    std::fs::write(output, redacted.as_bytes()).map_err(Spec033ReleaseEvidenceError::Io)?;
    Ok(Spec033RedactionReceipt {
        schema: SPEC033_REDACTION_TRANSFORM_SCHEMA.to_owned(),
        source_digest: digest(&bytes),
        output_digest: digest(redacted.as_bytes()),
        source_bytes,
        output_bytes: u64::try_from(redacted.len()).unwrap_or(u64::MAX),
    })
}

fn redact_host_paths(value: &str) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(start) = next_host_path_start(value, cursor) {
        redacted.push_str(&value[cursor..start]);
        redacted.push_str("[REDACTED_PATH]");
        cursor = value[start..]
            .char_indices()
            .find_map(|(offset, character)| {
                (offset > 0 && is_path_terminator(character)).then_some(start + offset)
            })
            .unwrap_or(value.len());
    }
    redacted.push_str(&value[cursor..]);
    redacted
}

fn next_host_path_start(value: &str, cursor: usize) -> Option<usize> {
    let remaining = &value[cursor..];
    let unix = ["/Users/", "/home/", "/private/var/"]
        .into_iter()
        .filter_map(|prefix| remaining.find(prefix))
        .min();
    let windows = remaining.as_bytes().windows(3).position(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    });
    unix.into_iter()
        .chain(windows)
        .min()
        .map(|offset| cursor + offset)
}

fn is_path_terminator(character: char) -> bool {
    character.is_whitespace() || matches!(character, ')' | ']' | '}' | '"' | '\'' | ',' | ';')
}

pub fn collect_spec033_replay_evidence(
    trajectory_root: &Path,
    receipt_root: &Path,
    trajectory_id: &str,
    run_id: &str,
) -> Result<RecordedTrajectoryReplayReceipt, Spec033ReleaseEvidenceError> {
    let store = RecordedTrajectoryStore::open(trajectory_root)
        .map_err(Spec033ReleaseEvidenceError::Store)?;
    let receipt = replay_recorded_trajectory(&store, trajectory_id, run_id)
        .map_err(Spec033ReleaseEvidenceError::Replay)?;
    std::fs::create_dir_all(receipt_root).map_err(Spec033ReleaseEvidenceError::Io)?;
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| {
        Spec033ReleaseEvidenceError::Io(std::io::Error::other(error.to_string()))
    })?;
    std::fs::write(receipt_root.join(format!("{run_id}.json")), bytes)
        .map_err(Spec033ReleaseEvidenceError::Io)?;
    Ok(receipt)
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

impl std::fmt::Display for Spec033ReleaseEvidenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec033ReleaseEvidenceError {}
