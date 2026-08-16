use super::{
    ArtifactId, CandidateId, GenerationOptionsSummary, MediaRootRelativePath, SafeModelId,
    SafeProviderId, Sha256Digest,
};
use serde::{Deserialize, Serialize};

const GENERATED_ARTIFACT_SCHEMA: &str = "shacs.generated-artifact.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedMediaKind {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationOperation {
    Generate,
    Edit,
    Variation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "policy",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RetentionPolicy {
    UserManaged,
    Session,
    ExpiresAt { expires_at: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDisclosure {
    RawContentPossibleElsewhere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactHandlingPolicy {
    pub retention: RetentionPolicy,
    pub disclosure: ProjectionDisclosure,
}

impl ArtifactHandlingPolicy {
    pub const fn new(retention: RetentionPolicy, disclosure: ProjectionDisclosure) -> Self {
        Self {
            retention,
            disclosure,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifactDefinition {
    pub kind: GeneratedMediaKind,
    pub operation: GenerationOperation,
    pub handling: ArtifactHandlingPolicy,
}

impl GeneratedArtifactDefinition {
    pub const fn new(
        kind: GeneratedMediaKind,
        operation: GenerationOperation,
        handling: ArtifactHandlingPolicy,
    ) -> Self {
        Self {
            kind,
            operation,
            handling,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedProvenanceKind {
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GeneratedProvenance {
    pub kind: GeneratedProvenanceKind,
    pub provider_id: SafeProviderId,
    pub model_id: SafeModelId,
    pub operation: GenerationOperation,
    pub source_artifact_ids: Vec<ArtifactId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundProvenanceKind {
    InboundAttachment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InboundProvenance {
    pub kind: InboundProvenanceKind,
    pub attachment_id: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GeneratedArtifactRef {
    pub artifact_id: ArtifactId,
    pub media_root_relative_path: MediaRootRelativePath,
    pub sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GeneratedArtifactRecord {
    pub schema: String,
    pub artifact_id: ArtifactId,
    pub candidate_id: CandidateId,
    pub kind: GeneratedMediaKind,
    pub media_root_relative_path: MediaRootRelativePath,
    pub mime_type: String,
    pub byte_len: u64,
    pub sha256: Sha256Digest,
    pub provenance: GeneratedProvenance,
    pub generation_options_summary: GenerationOptionsSummary,
    pub created_at: String,
    pub retention: RetentionPolicy,
    pub disclosure: ProjectionDisclosure,
}

impl GeneratedArtifactRecord {
    pub fn artifact_ref(&self) -> GeneratedArtifactRef {
        GeneratedArtifactRef {
            artifact_id: self.artifact_id.clone(),
            media_root_relative_path: self.media_root_relative_path.clone(),
            sha256: self.sha256.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CommittedArtifact(GeneratedArtifactRecord);

impl CommittedArtifact {
    pub(crate) const fn new(record: GeneratedArtifactRecord) -> Self {
        Self(record)
    }

    pub fn record(&self) -> &GeneratedArtifactRecord {
        &self.0
    }

    pub fn artifact_ref(&self) -> GeneratedArtifactRef {
        self.0.artifact_ref()
    }
}

impl std::ops::Deref for CommittedArtifact {
    type Target = GeneratedArtifactRecord;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifactMetadata {
    pub artifact_id: ArtifactId,
    pub kind: GeneratedMediaKind,
    pub operation: GenerationOperation,
    pub source_artifact_ids: Vec<ArtifactId>,
    pub generation_options_summary: GenerationOptionsSummary,
    pub retention: RetentionPolicy,
    pub disclosure: ProjectionDisclosure,
    pub created_at: String,
}

impl GeneratedArtifactMetadata {
    pub fn new(
        artifact_id: ArtifactId,
        definition: GeneratedArtifactDefinition,
        created_at: impl Into<String>,
    ) -> Self {
        Self {
            artifact_id,
            kind: definition.kind,
            operation: definition.operation,
            source_artifact_ids: Vec::new(),
            generation_options_summary: GenerationOptionsSummary::default(),
            retention: definition.handling.retention,
            disclosure: definition.handling.disclosure,
            created_at: created_at.into(),
        }
    }

    pub fn with_sources(mut self, source_artifact_ids: Vec<ArtifactId>) -> Self {
        self.source_artifact_ids = source_artifact_ids;
        self
    }

    pub fn with_options(mut self, options: GenerationOptionsSummary) -> Self {
        self.generation_options_summary = options;
        self
    }
}

pub(crate) fn artifact_schema() -> String {
    GENERATED_ARTIFACT_SCHEMA.to_owned()
}
