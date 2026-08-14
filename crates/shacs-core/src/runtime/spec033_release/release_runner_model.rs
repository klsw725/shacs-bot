use super::{Spec033RedactionReceipt, Spec033ReleaseCheck};
use serde::{Deserialize, Serialize};
use shacs_projection::Spec031ReleaseCommandRecord;
use std::path::PathBuf;
use std::time::Duration;

pub const SPEC033_RELEASE_SCHEMA: &str = "spec033.release_evidence.v5";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec033ReleaseConfig {
    pub run_id: String,
    pub repo_root: PathBuf,
    pub evidence_root: PathBuf,
    pub trajectory_root: PathBuf,
    pub data_dir: PathBuf,
    pub trajectory_id: String,
    pub mode: Spec033ReleaseMode,
    pub command_timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033ReleaseMode {
    CurrentWorktree,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033TrajectoryProvenance {
    pub record_path: String,
    pub record_digest: String,
    pub source_id: String,
    pub origin: crate::runtime::RecordedTrajectoryOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033ReleaseCommandEvidence {
    pub kind: Spec033ReleaseCheck,
    pub command: Spec031ReleaseCommandRecord,
    pub stdout_digest: String,
    pub stderr_digest: String,
    pub redacted_stdout: String,
    pub redacted_stderr: String,
    pub stdout_transform: Spec033RedactionReceipt,
    pub stderr_transform: Spec033RedactionReceipt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033ReleaseManifest {
    pub schema: String,
    pub run_id: String,
    pub trajectory_id: String,
    pub mode: Spec033ReleaseMode,
    pub trajectory: Spec033TrajectoryProvenance,
    pub source_manifest: Spec033SourceManifest,
    pub commands: Vec<Spec033ReleaseCommandEvidence>,
    pub edge_commands: Vec<Spec033EdgeCommandEvidence>,
    pub replay: super::RecordedTrajectoryReplayReceipt,
    pub coverage: Vec<Spec033CoverageRow>,
    pub blocker_coverage: Vec<Spec033BlockerCoverageRow>,
    pub artifact_digests: Vec<Spec033DigestRow>,
    pub blocked_non_guarantees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033SourceManifest {
    pub digest: String,
    pub files: Vec<Spec033DigestRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033EdgeCommandEvidence {
    pub blocker: String,
    pub test_id: String,
    pub command: Spec031ReleaseCommandRecord,
    pub artifact: String,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033CoverageRow {
    pub requirement: String,
    pub code_path: String,
    pub test_command: String,
    pub artifact: String,
    pub artifact_digest: String,
    pub evidence_source: String,
    pub status: String,
    pub non_guarantee: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033BlockerCoverageRow {
    pub blocker: String,
    pub code_path: String,
    pub test_command: String,
    pub artifact: String,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033DigestRow {
    pub locator: String,
    pub digest: String,
}

#[derive(Debug)]
pub enum Spec033ReleaseArtifactError {
    InvalidConfig,
    CommandFailed,
    MissingGuarantee,
    ForbiddenWaiver,
    DigestMismatch,
    Io(std::io::Error),
    Json(serde_json::Error),
    Command(shacs_projection::Spec031ReleaseArtifactError),
    Replay(super::Spec033ReleaseEvidenceError),
}

impl std::fmt::Display for Spec033ReleaseArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec033ReleaseArtifactError {}
