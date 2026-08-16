use super::super::{VideoAnalyzerProjection, VideoAnalyzerSnapshotProjection};
use crate::generated_media::{
    GeneratedArtifactRecord, GeneratedMediaKind, GenerationOperation, RetentionPolicy,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_projection::{DataDisclosureProjection, DataSurface, Spec031Freshness};
use std::fmt::{Display, Formatter};

pub(super) const MEDIA_EVIDENCE_SCHEMA: &str = "shacs.spec034.media-evidence.v1";

pub struct MediaEvidenceDiagnosticsInput<'a> {
    pub artifacts: &'a [GeneratedArtifactRecord],
    pub analyzer: &'a VideoAnalyzerProjection,
    pub disclosure: &'a DataDisclosureProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MediaEvidenceDiagnostics {
    pub schema: String,
    pub availability: MediaEvidenceAvailability,
    pub artifacts: Vec<ArtifactEvidenceSummary>,
    pub analyzer: AnalyzerEvidenceSummary,
    pub disclosure: MediaDisclosureSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<VideoAnalyzerSnapshotProjection>,
    pub freshness: Spec031Freshness,
    pub facts_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaEvidenceAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtifactEvidenceSummary {
    pub artifact_id: String,
    pub kind: GeneratedMediaKind,
    pub operation: GenerationOperation,
    pub byte_len: u64,
    pub sha256: String,
    pub retention: RetentionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AnalyzerEvidenceSummary {
    pub status: RecordedAnalyzerStatus,
    pub evidence_available: bool,
    pub evidence_digest: Option<String>,
    pub component_failure_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedAnalyzerStatus {
    Configured,
    AnalyzerMissing,
    Unsupported,
    ExtractionFailed,
    Included,
    Truncated,
    DurationCap,
    Cancelled,
    Timeout,
}

impl RecordedAnalyzerStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::AnalyzerMissing => "analyzer_missing",
            Self::Unsupported => "unsupported",
            Self::ExtractionFailed => "extraction_failed",
            Self::Included => "included",
            Self::Truncated => "truncated",
            Self::DurationCap => "duration_cap",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedArtifactStatus {
    Recorded,
    Unavailable,
}

impl RecordedArtifactStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MediaDisclosureSummary {
    pub raw_content_possible: bool,
    pub surfaces: Vec<DataSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvidenceProjectionError {
    InvalidRecord,
}

pub trait MediaEvidenceReplayDependencies {
    fn request_network(&self);
    fn resolve_credential(&self);
    fn invoke_analyzer(&self);
    fn resolve_resource(&self);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaEvidenceReplaySource {
    RecordedMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MediaEvidenceReplayReceipt {
    pub source: MediaEvidenceReplaySource,
    pub artifact_status: RecordedArtifactStatus,
    pub artifact_count: usize,
    pub analyzer_status: RecordedAnalyzerStatus,
    pub disclosure: MediaDisclosureSummary,
    pub snapshot: VideoAnalyzerSnapshotProjection,
    pub facts_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEvidenceReplayError {
    Malformed,
    UnknownSchema,
    DigestMismatch,
    StaleFacts,
    UnavailableFacts,
    InvalidAnalyzerState,
}

pub(super) fn projection_digest(
    projection: &MediaEvidenceDiagnostics,
) -> Result<String, MediaEvidenceProjectionError> {
    let mut digestible = projection.clone();
    digestible.facts_digest.clear();
    let bytes =
        serde_json::to_vec(&digestible).map_err(|_| MediaEvidenceProjectionError::InvalidRecord)?;
    Ok(sha256(&bytes))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(super) fn safe_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

pub(super) fn safe_snapshot(snapshot: &VideoAnalyzerSnapshotProjection) -> bool {
    !snapshot.snapshot_id.is_empty()
        && snapshot.snapshot_id.len() <= 160
        && snapshot
            .snapshot_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
        && safe_digest(&snapshot.provenance_digest)
}

impl Display for MediaEvidenceProjectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MediaEvidenceProjectionError {}

impl Display for MediaEvidenceReplayError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MediaEvidenceReplayError {}
