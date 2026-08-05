use super::model::Spec031ReleaseArtifactError;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031CoverageRequirementKind {
    ParentMustHave,
    AcceptanceCriterion,
    ClosureEvidence,
    PrdTask,
    RequiredCommand,
    RequiredArtifact,
    ExternalOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031CoverageStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031CoverageEvidenceKind {
    ImplementedArtifact,
    CommandTranscript,
    CleanupReceipt,
    ExternalAudit,
    PlannedProse,
    Screenshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ArtifactMediaType {
    Json,
    Markdown,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031TypedEvidenceClass {
    ManifestJson,
    CoverageMatrixJson,
    CommandResultsJson,
    FailureTriageJson,
    SummaryMarkdown,
    CommandStdout,
    ExternalAuditMarkdown,
    CleanupReceiptJson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExternalOwnerId {
    Spec029,
    Spec030,
    Spec032,
    Spec033,
    Spec034,
    Spec035,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExternalAuditStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec031ReleaseCoverageEntry {
    pub requirement_id: String,
    pub kind: Spec031CoverageRequirementKind,
    pub source_locator: String,
    pub owner: String,
    pub status: Spec031CoverageStatus,
    pub evidence_kind: Spec031CoverageEvidenceKind,
    pub evidence_class: Spec031TypedEvidenceClass,
    pub artifact_media_type: Spec031ArtifactMediaType,
    pub artifact: String,
    pub artifact_hash: String,
    pub command_result_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec031ExternalAuditRow {
    pub owner: Spec031ExternalOwnerId,
    pub status: Spec031ExternalAuditStatus,
    pub source_locator: String,
    pub source_status_locator: String,
    pub implementation_artifacts: Vec<String>,
    pub command_result_ids: Vec<String>,
    pub artifact: String,
    pub artifact_media_type: Spec031ArtifactMediaType,
    pub evidence_class: Spec031TypedEvidenceClass,
    pub artifact_hash: String,
    pub reason: String,
}

pub(super) fn artifact_hash(
    root: &Path,
    relative: &str,
) -> Result<String, Spec031ReleaseArtifactError> {
    let path = super::validate::require_safe_file(root, relative)?;
    let bytes =
        std::fs::read(path).map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("fnv64:{:016x}", hasher.finish()))
}
