use super::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactStoreError, ArtifactWriteRequest,
    GeneratedArtifactDefinition, GeneratedArtifactMetadata, GeneratedArtifactRef,
    GeneratedMediaContractError, GeneratedMediaKind, GenerationOperation, GenerationOptionsSummary,
    ProviderMediaBytes, ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaOrigin,
    RemoteOutputDecision, RemoteRejectionReason, RetentionPolicy, SafeProviderId,
    ValidatedImageOperationCandidate, ValidatedLocalImage,
};
use chrono::DateTime;
use serde::Serialize;
use shacs_providers::{GeneratedImage, ImageGenerationResult};

const TIMESTAMP_MAX_CHARS: usize = 64;

#[derive(Debug, Clone)]
pub struct ArtifactPublicationMetadata {
    artifact_id: ArtifactId,
    options: GenerationOptionsSummary,
    handling: ArtifactHandlingPolicy,
    created_at: String,
}

impl ArtifactPublicationMetadata {
    pub fn try_new(
        artifact_id: ArtifactId,
        handling: ArtifactHandlingPolicy,
        created_at: impl Into<String>,
    ) -> Result<Self, ArtifactPublicationError> {
        let created_at = created_at.into();
        validate_timestamp(&created_at)?;
        validate_retention(&handling.retention)?;
        Ok(Self {
            artifact_id,
            options: GenerationOptionsSummary::default(),
            handling,
            created_at,
        })
    }

    pub fn with_options(mut self, options: GenerationOptionsSummary) -> Self {
        self.options = options;
        self
    }

    fn into_generated(
        self,
        operation: GenerationOperation,
        source_artifact_ids: Vec<ArtifactId>,
    ) -> GeneratedArtifactMetadata {
        GeneratedArtifactMetadata::new(
            self.artifact_id,
            GeneratedArtifactDefinition::new(GeneratedMediaKind::Image, operation, self.handling),
            self.created_at,
        )
        .with_sources(source_artifact_ids)
        .with_options(self.options)
    }
}

#[derive(Debug)]
pub enum ArtifactPublicationError {
    Artifact(ArtifactStoreError),
    Contract(GeneratedMediaContractError),
    InvalidMetadata,
    InvalidFinalCandidate,
}

impl std::fmt::Display for ArtifactPublicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Artifact(error) => write!(formatter, "artifact publication failed: {error}"),
            Self::Contract(error) => write!(formatter, "artifact publication rejected: {error}"),
            Self::InvalidMetadata => {
                formatter.write_str("artifact publication metadata is invalid")
            }
            Self::InvalidFinalCandidate => {
                formatter.write_str("artifact publication requires one validated final candidate")
            }
        }
    }
}

impl std::error::Error for ArtifactPublicationError {}

impl From<ArtifactStoreError> for ArtifactPublicationError {
    fn from(error: ArtifactStoreError) -> Self {
        Self::Artifact(error)
    }
}

impl From<GeneratedMediaContractError> for ArtifactPublicationError {
    fn from(error: GeneratedMediaContractError) -> Self {
        Self::Contract(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RemotePublicationReference {
    provider_id: SafeProviderId,
    domain: String,
    expires_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "value", rename_all = "snake_case")]
pub enum RemotePublicationOutcome {
    Persisted(GeneratedArtifactRef),
    Reference(RemotePublicationReference),
    Rejected(RemoteRejectionReason),
}

pub struct ArtifactPublisher<'a> {
    store: &'a ArtifactStore,
}

impl<'a> ArtifactPublisher<'a> {
    pub const fn new(store: &'a ArtifactStore) -> Self {
        Self { store }
    }

    pub fn publish_operation(
        &self,
        candidate: ValidatedImageOperationCandidate,
        metadata: ArtifactPublicationMetadata,
    ) -> Result<super::CommittedArtifact, ArtifactPublicationError> {
        let (operation, local_image, source_artifact_ids) = candidate.into_validated_parts();
        self.persist_validated_local_image(
            local_image,
            metadata.into_generated(operation, source_artifact_ids),
        )
    }

    pub fn publish_generation_candidate(
        &self,
        result: &ImageGenerationResult,
        image: &GeneratedImage,
        metadata: ArtifactPublicationMetadata,
    ) -> Result<super::CommittedArtifact, ArtifactPublicationError> {
        validate_final_image(image)?;
        self.persist_image(
            result,
            image,
            metadata.into_generated(GenerationOperation::Generate, Vec::new()),
        )
    }

    pub fn publish_remote(
        &self,
        decision: RemoteOutputDecision,
        metadata: ArtifactPublicationMetadata,
    ) -> Result<RemotePublicationOutcome, ArtifactPublicationError> {
        match decision {
            RemoteOutputDecision::ReadyToPersist(ready) => {
                let generated = metadata.into_generated(GenerationOperation::Generate, Vec::new());
                let committed = self
                    .store
                    .persist(ArtifactWriteRequest::new(ready.into_candidate(), generated))?;
                Ok(RemotePublicationOutcome::Persisted(
                    committed.artifact_ref(),
                ))
            }
            RemoteOutputDecision::Reference(reference) => Ok(RemotePublicationOutcome::Reference(
                RemotePublicationReference {
                    provider_id: reference.provider_id().clone(),
                    domain: reference.domain().to_owned(),
                    expires_at_unix_seconds: reference.expires_at_unix_seconds(),
                },
            )),
            RemoteOutputDecision::Rejected(rejection) => {
                Ok(RemotePublicationOutcome::Rejected(rejection.reason()))
            }
        }
    }

    fn persist_image(
        &self,
        result: &ImageGenerationResult,
        image: &GeneratedImage,
        metadata: GeneratedArtifactMetadata,
    ) -> Result<super::CommittedArtifact, ArtifactPublicationError> {
        validate_final_image(image)?;
        self.persist_image_candidate(
            ProviderMediaOrigin::new(&result.provider_id, &result.model),
            image,
            metadata,
        )
    }

    fn persist_image_candidate(
        &self,
        origin: ProviderMediaOrigin,
        image: &GeneratedImage,
        metadata: GeneratedArtifactMetadata,
    ) -> Result<super::CommittedArtifact, ArtifactPublicationError> {
        let candidate_id = image
            .provider_item_id
            .clone()
            .unwrap_or_else(|| {
                shacs_providers::ImageGenerationItemId::from_provider(&format!(
                    "image_{}",
                    image.index
                ))
            })
            .into_string();
        let candidate = ProviderMediaCandidate::bytes(
            ProviderMediaCandidateId::new(candidate_id)
                .map_err(|_| ArtifactPublicationError::InvalidFinalCandidate)?,
            origin,
            ProviderMediaBytes::new(image.mime_type, image.bytes.clone()),
        );
        Ok(self
            .store
            .persist(ArtifactWriteRequest::new(candidate, metadata))?)
    }

    fn persist_validated_local_image(
        &self,
        local_image: ValidatedLocalImage,
        metadata: GeneratedArtifactMetadata,
    ) -> Result<super::CommittedArtifact, ArtifactPublicationError> {
        let result = local_image.into_result();
        self.persist_image_candidate(
            ProviderMediaOrigin::new(&result.provider_id, &result.model),
            &result.images[0],
            metadata,
        )
    }
}

fn validate_final_image(image: &GeneratedImage) -> Result<(), ArtifactPublicationError> {
    if image.bytes.is_empty() || image.byte_len != image.bytes.len() {
        return Err(ArtifactPublicationError::InvalidFinalCandidate);
    }
    Ok(())
}

fn validate_timestamp(value: &str) -> Result<(), ArtifactPublicationError> {
    if value.chars().count() > TIMESTAMP_MAX_CHARS || DateTime::parse_from_rfc3339(value).is_err() {
        return Err(ArtifactPublicationError::InvalidMetadata);
    }
    Ok(())
}

fn validate_retention(retention: &RetentionPolicy) -> Result<(), ArtifactPublicationError> {
    match retention {
        RetentionPolicy::UserManaged | RetentionPolicy::Session => Ok(()),
        RetentionPolicy::ExpiresAt { expires_at } => validate_timestamp(expires_at),
    }
}
