use serde::{Deserialize, Serialize};
use shacs_projection::{Spec034OwnerFactKind, Spec034PrimaryPrd, Spec034ReviewKind};
use std::path::PathBuf;
use std::time::Duration;

pub const RELEASE_SCHEMA: &str = "spec034.release_runner.v2";
pub const PUBLICATION_STATUS_SCHEMA: &str = "spec034.publication_status.v1";

#[path = "command_model.rs"]
mod command_model;
pub use command_model::{
    CommandStreamSummary, PortableCommandRecord, PortableProcessReceipt, PortableToolIdentity,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec034ReleaseConfig {
    pub run_id: String,
    pub repo_root: PathBuf,
    pub evidence_root: PathBuf,
    pub cache_root: Option<PathBuf>,
    pub mode: Spec034ReleaseMode,
    pub command_timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spec034ReleaseMode {
    SuccessFixture,
    CurrentWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestRow {
    pub locator: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub locator: String,
    pub digest: Option<String>,
    pub tracked: bool,
    pub modified: bool,
    pub state: SourceFileState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFileState {
    Present,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceManifest {
    pub repo_root: String,
    pub head_oid: String,
    pub worktree_dirty: bool,
    pub digest: String,
    pub files: Vec<SourceFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEvidence {
    pub kind: String,
    pub source_digest: String,
    pub tool: PortableToolIdentity,
    pub rustc: PortableToolIdentity,
    pub environment_policy: String,
    pub command: PortableCommandRecord,
    pub portable_process_receipt: PortableProcessReceipt,
    pub stdout_digest: String,
    pub stderr_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultsDocument {
    pub schema: String,
    pub run_id: String,
    pub mode: Spec034ReleaseMode,
    pub runner_passed: bool,
    pub closure_eligible: bool,
    pub execution_attested: bool,
    pub structural_only: bool,
    pub commands: Vec<CommandEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementRow {
    pub requirement_id: String,
    pub primary_prd: Spec034PrimaryPrd,
    pub command_kind: String,
    pub evidence: DigestRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockerRow {
    pub blocker: String,
    pub disposition: String,
    pub evidence: DigestRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageDocument {
    pub schema: String,
    pub run_id: String,
    pub requirements: Vec<RequirementRow>,
    pub blockers: Vec<BlockerRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRecord {
    pub record_id: String,
    pub kind: Spec034ReviewKind,
    pub final_review: bool,
    pub fixture_only: bool,
    pub evidence: DigestRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDocument {
    pub schema: String,
    pub run_id: String,
    pub records: Vec<ReviewRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAudit {
    pub kind: Spec034OwnerFactKind,
    pub status: String,
    pub evidence: DigestRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAuditDocument {
    pub schema: String,
    pub run_id: String,
    pub audits: Vec<OwnerAudit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupReceipt {
    pub schema: String,
    pub run_id: String,
    pub raw_evidence_cleaned: bool,
    pub leak_count: u8,
    pub leak_summary: Vec<String>,
    pub cleanup_binding_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationStatusDocument {
    pub schema: String,
    pub run_id: String,
    pub status: PublicationStatus,
    pub content_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    Validated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationsDocument {
    pub schema: String,
    pub run_id: String,
    pub source: SourceManifest,
    pub fixture_digests: Vec<DigestRow>,
    pub dirty_worktree_recorded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageDocument {
    pub schema: String,
    pub run_id: String,
    pub command_failures: Vec<String>,
    pub open_blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryDocument {
    pub schema: String,
    pub run_id: String,
    pub label: String,
    pub runner_passed: bool,
    pub closure_eligible: bool,
    pub execution_attested: bool,
    pub structural_only: bool,
    pub non_guarantees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034ReleaseManifest {
    pub schema: String,
    pub run_id: String,
    pub mode: Spec034ReleaseMode,
    pub repo_root: String,
    pub head_oid: String,
    pub source: SourceManifest,
    pub fixture_digests: Vec<DigestRow>,
    pub artifact_digests: Vec<DigestRow>,
    pub requirement_count: usize,
    pub blocker_count: usize,
    pub runner_passed: bool,
    pub runner_only: bool,
    pub closure_eligible: bool,
    pub execution_attested: bool,
    pub structural_only: bool,
    pub non_guarantees: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationStage {
    MarkerCreate,
    MarkerWrite,
    FileSync,
    DirectorySync,
    MarkerRename,
    DestinationIdentity,
    QuarantineFailure,
}

#[path = "model_error.rs"]
mod error;
pub use error::{Spec034ReleaseArtifactError, Spec034StructuralAudit};
