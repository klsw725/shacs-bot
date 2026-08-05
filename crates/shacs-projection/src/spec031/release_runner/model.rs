use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

use super::coverage::{Spec031ExternalAuditRow, Spec031ReleaseCoverageEntry};

pub const SPEC031_RELEASE_RUNNER_SCHEMA: &str = "spec031.release_runner.v1";
const MAX_SAFE_ID_LEN: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spec031ReleaseRunId(String);

impl Spec031ReleaseRunId {
    pub fn try_new(value: &str) -> Result<Self, Spec031ReleaseArtifactError> {
        let safe = !value.is_empty()
            && value.len() <= MAX_SAFE_ID_LEN
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            });
        if safe {
            Ok(Self(value.to_owned()))
        } else {
            Err(Spec031ReleaseArtifactError::InvalidRunId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ReleaseGateKind {
    FocusedCargoTest,
    FullCargoGate,
    SurfaceSmoke,
    FailureInjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ReleaseCommandStatus {
    Passed,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec031ReleaseTestCounts {
    pub tests_run: u64,
    pub tests_failed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec031ReleaseCommandRecord {
    pub id: String,
    pub gate: Spec031ReleaseGateKind,
    pub package: Option<String>,
    pub filter: Option<String>,
    pub argv: Vec<String>,
    pub cwd: String,
    pub status: Spec031ReleaseCommandStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_path: String,
    pub stderr_path: String,
    pub tests: Option<Spec031ReleaseTestCounts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec031ReleaseRunArtifacts {
    pub schema: String,
    pub run_id: Spec031ReleaseRunId,
    pub evidence_root: String,
    pub fixture_registry: Vec<String>,
    pub command_registry: Vec<Spec031ReleaseCommandRecord>,
    pub cleanup_registry: Vec<String>,
    pub manifest_files: Vec<String>,
    pub coverage_matrix: Vec<Spec031ReleaseCoverageEntry>,
    pub external_audits: Vec<Spec031ExternalAuditRow>,
    pub failure_triage: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec031ReleaseArtifactError {
    UnsupportedSchema,
    InvalidRunId,
    InvalidArtifactPath,
    ArtifactMismatch,
    InvalidCommandEvidence,
    MissingRequiredArtifact,
    MissingCleanupReceipt,
    ZeroTestsRun,
    NonzeroTestsFailed,
    CommandFailed,
    CommandTimedOut,
    DirtyWorktree,
    BlockedExternalEvidence,
    InvalidCoverageEvidence,
    UnknownCoverageRequirement,
    DuplicateCoverageRequirement,
    UnmappedCoverageRequirement,
    BlockedAsPass,
    Io,
    EmptyCommand,
}

impl Display for Spec031ReleaseArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec031ReleaseArtifactError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031ReleaseRunnerMode {
    SuccessFixture,
    CurrentWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031ReleaseRunnerConfig {
    pub run_id: Spec031ReleaseRunId,
    pub evidence_root: PathBuf,
    pub repo_root: PathBuf,
    pub mode: Spec031ReleaseRunnerMode,
    pub command_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031ReleaseCommandSpec {
    pub id: String,
    pub gate: Spec031ReleaseGateKind,
    pub package: Option<String>,
    pub filter: Option<String>,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
}
