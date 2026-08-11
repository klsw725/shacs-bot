use super::semantic::Spec030SurfaceAssertions;
use super::source_manifest::Spec030SourceManifest;
use crate::Spec031ReleaseCommandRecord;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

pub const SPEC030_RELEASE_RUNNER_SCHEMA: &str = "spec030.release_runner.v4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spec030ReleaseRunId(String);

impl Spec030ReleaseRunId {
    pub fn try_new(value: &str) -> Result<Self, Spec030ReleaseArtifactError> {
        let valid = !value.is_empty()
            && value.len() <= 80
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            });
        valid
            .then(|| Self(value.to_owned()))
            .ok_or(Spec030ReleaseArtifactError::InvalidRunId)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec030ReleaseRunnerMode {
    SuccessFixture,
    CurrentWorktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec030CommandEvidenceMode {
    SuccessFixture,
    ExternalRecord,
    LinuxCurrentWorktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Spec030ReleaseVerdict {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030CoverageRow {
    pub prd: String,
    pub owner_surface: String,
    pub command_ids: Vec<String>,
    pub evidence: Vec<String>,
    pub assertions: Vec<Spec030CoverageAssertion>,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030CoverageAssertion {
    pub id: String,
    pub evidence: String,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030OwnerAudit {
    pub owner: String,
    pub fact: String,
    pub source_locator: String,
    pub command_ids: Vec<String>,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030CapturedFact {
    pub id: String,
    pub status: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030SurfaceArtifact {
    pub surface: String,
    pub command_id: String,
    pub artifact: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec030SurfaceOwnerReadiness {
    Observed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec030SurfaceOwnerShutdown {
    Requested,
    Reaped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030SurfaceOwnerSpawnSpec {
    pub executable: String,
    pub config_path: String,
    pub workspace_path: String,
    pub bind: String,
    pub allow_api_side_effects: bool,
}

impl Spec030SurfaceOwnerSpawnSpec {
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec![
            self.executable.clone(),
            "serve".to_owned(),
            "--config".to_owned(),
            self.config_path.clone(),
            "--workspace".to_owned(),
            self.workspace_path.clone(),
            "--bind".to_owned(),
            self.bind.clone(),
        ];
        if self.allow_api_side_effects {
            argv.push("--allow-api-side-effects".to_owned());
        }
        argv
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030SurfaceOwnerEvidence {
    pub schema: String,
    pub production_owner: bool,
    pub owner_pid: u32,
    pub spawn: Spec030SurfaceOwnerSpawnSpec,
    pub argv: Vec<String>,
    pub bind_host: String,
    pub requested_port: u16,
    pub bound_port: u16,
    pub readiness: Spec030SurfaceOwnerReadiness,
    pub shutdown: Spec030SurfaceOwnerShutdown,
    pub temp_root: String,
    pub temp_root_removed: bool,
    pub stdout_path: String,
    pub stderr_path: String,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ExternalEvidence {
    pub kind: String,
    pub artifact: String,
    pub artifact_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ReleaseBlocker {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ReleaseRunArtifacts {
    pub schema: String,
    pub run_id: Spec030ReleaseRunId,
    pub evidence_root: String,
    pub repo_root: String,
    pub mode: Spec030ReleaseRunnerMode,
    pub command_evidence_mode: Spec030CommandEvidenceMode,
    pub source_manifest: Spec030SourceManifest,
    pub verdict: Spec030ReleaseVerdict,
    pub coverage: Vec<Spec030CoverageRow>,
    pub owner_audits: Vec<Spec030OwnerAudit>,
    pub facts: Vec<Spec030CapturedFact>,
    pub surfaces: Vec<Spec030SurfaceArtifact>,
    pub surface_owner: Spec030SurfaceOwnerEvidence,
    pub surface_assertions: Spec030SurfaceAssertions,
    pub external_evidence: Vec<Spec030ExternalEvidence>,
    pub commands: Vec<Spec031ReleaseCommandRecord>,
    pub cleanup_records: Vec<String>,
    pub manual_records: Vec<String>,
    pub blockers: Vec<Spec030ReleaseBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec030ReleaseRunnerConfig {
    pub run_id: Spec030ReleaseRunId,
    pub evidence_root: PathBuf,
    pub repo_root: PathBuf,
    pub mode: Spec030ReleaseRunnerMode,
    pub command_timeout: Duration,
    pub manual_records: Vec<PathBuf>,
    pub bwrap_record: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030ReleaseArtifactError {
    Io,
    InvalidRunId,
    UnsupportedSchema,
    MissingCoverageRow,
    InvalidOwnerAudit,
    InvalidCoverageEvidence,
    ArtifactMismatch,
    ManifestMismatch,
    SourceMismatch,
    ZeroTestsRun,
    CommandFailed,
    RawCredentialMaterial,
    FalseSupportedClaim,
    MissingCleanupRecord,
    MissingManualRecord,
    InvalidArtifactPath,
    InvalidManualRecord,
    InvalidCleanupRecord,
    InvalidSurfaceEvidence,
}

impl Display for Spec030ReleaseArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec030ReleaseArtifactError {}
