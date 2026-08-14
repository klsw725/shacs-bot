use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub const SPEC033_REVIEW_ARTIFACT_SCHEMA: &str = "spec033.review_artifacts.v3";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033ReviewKind {
    Qa,
    Goal,
    Code,
    Security,
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033ReviewVerdict {
    Pass,
    Fail,
    Inconclusive,
}

impl Spec033ReviewKind {
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Qa => "reviews/qa.json",
            Self::Goal => "reviews/goal.json",
            Self::Code => "reviews/code.json",
            Self::Security => "reviews/security.json",
            Self::Docs => "reviews/docs.json",
        }
    }

    pub const fn required() -> [Self; 5] {
        [Self::Qa, Self::Goal, Self::Code, Self::Security, Self::Docs]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033CargoPackage {
    Core,
    Projection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033TestTarget {
    SnapshotReplay,
    ReviewArtifacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033CargoCommand {
    pub package: Spec033CargoPackage,
    pub test_target: Spec033TestTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033ArtifactRef {
    pub locator: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033CargoCommandResult {
    pub command: Spec033CargoCommand,
    pub extra_arguments: Vec<String>,
    pub exit_code: i32,
    pub passed: bool,
    pub evidence: Spec033ArtifactRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033ReviewRecord {
    pub kind: Spec033ReviewKind,
    pub verdict: Spec033ReviewVerdict,
    pub final_review: bool,
    pub evidence: Vec<Spec033ArtifactRef>,
    pub safe_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033CoverageEntry {
    pub spec_id: String,
    pub artifacts: Vec<Spec033ArtifactRef>,
    pub waivers: Vec<String>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spec033ArtifactInput {
    pub source_artifact_root: PathBuf,
    pub run_id: String,
    pub trajectory_id: String,
    pub execution_snapshot_id: String,
    pub execution_snapshot: Spec033ArtifactRef,
    pub replay_result: Spec033ArtifactRef,
    pub redaction_evidence: Option<Spec033ArtifactRef>,
    pub safe_summary: String,
    pub reviews: Vec<Spec033ReviewRecord>,
    pub cargo_commands: Vec<Spec033CargoCommandResult>,
    pub coverage: Spec033CoverageEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033ArtifactManifest {
    pub schema: String,
    pub run_id: String,
    pub trajectory_id: String,
    pub execution_snapshot_id: String,
    pub execution_snapshot: Spec033ArtifactRef,
    pub replay_result: Spec033ArtifactRef,
    pub redaction_evidence: Spec033ArtifactRef,
    pub safe_summary: String,
    pub reviews: Vec<Spec033ReviewRecord>,
    pub cargo_commands: Vec<Spec033CargoCommandResult>,
    pub coverage: Spec033CoverageEntry,
    pub artifact_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec033ArtifactTransformError {
    MissingRedactionEvidence,
    MissingReviewEvidence,
    InvalidCoverageEntry,
    MissingEvidenceArtifact,
    EvidenceDigestMismatch,
    InvalidReviewCommand,
    ReviewCommandFailed,
    ReviewVerdictFailed,
    ForbiddenWaiver,
    ForbiddenBlocker,
    UnsafePersistedString,
    Io,
    Json,
}

impl Display for Spec033ArtifactTransformError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec033ArtifactTransformError {}
