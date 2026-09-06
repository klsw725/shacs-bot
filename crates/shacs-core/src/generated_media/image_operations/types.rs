use super::super::{ArtifactId, ArtifactStoreError, GeneratedArtifactRef, GenerationOperation};
use shacs_providers::{
    ImageGenerationResult, ImageOperationContractError, ImageOperationOptions, ProviderError,
};

#[derive(Debug, Clone, PartialEq)]
pub enum ArtifactImageOperationRequest {
    Edit {
        prompt: String,
        source: GeneratedArtifactRef,
        options: ImageOperationOptions,
    },
    Mask {
        prompt: String,
        source: GeneratedArtifactRef,
        mask: Option<GeneratedArtifactRef>,
        options: ImageOperationOptions,
    },
    Variation {
        source: GeneratedArtifactRef,
        options: ImageOperationOptions,
    },
}

impl ArtifactImageOperationRequest {
    pub fn edit(prompt: impl Into<String>, source: GeneratedArtifactRef) -> Self {
        Self::Edit {
            prompt: prompt.into(),
            source,
            options: ImageOperationOptions::default(),
        }
    }

    pub fn mask(
        prompt: impl Into<String>,
        source: GeneratedArtifactRef,
        mask: Option<GeneratedArtifactRef>,
    ) -> Self {
        Self::Mask {
            prompt: prompt.into(),
            source,
            mask,
            options: ImageOperationOptions::default(),
        }
    }

    pub fn variation(source: GeneratedArtifactRef) -> Self {
        Self::Variation {
            source,
            options: ImageOperationOptions::default(),
        }
    }
}

#[derive(Debug)]
pub enum ImageOperationAdmissionError {
    Artifact(ArtifactStoreError),
    ReferenceMismatch,
    MissingMask,
    InvalidSourceKind,
    InvalidSourceMime,
    SourceTooLarge,
    MimeMismatch,
    Provider(ProviderError),
    InvalidProviderResult,
}

impl std::fmt::Display for ImageOperationAdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Artifact(error) => {
                write!(formatter, "image source artifact failed proof: {error}")
            }
            Self::ReferenceMismatch => {
                formatter.write_str("image source ref does not match committed proof")
            }
            Self::MissingMask => formatter.write_str("image mask source is required"),
            Self::InvalidSourceKind => formatter.write_str("image source artifact kind is invalid"),
            Self::InvalidSourceMime => formatter.write_str("image source MIME is unsupported"),
            Self::SourceTooLarge => formatter.write_str("image source exceeds the byte limit"),
            Self::MimeMismatch => formatter.write_str("image source bytes do not match MIME"),
            Self::Provider(error) => write!(formatter, "image operation provider failed: {error}"),
            Self::InvalidProviderResult => {
                formatter.write_str("image operation provider returned an invalid result")
            }
        }
    }
}

impl std::error::Error for ImageOperationAdmissionError {}

impl From<ArtifactStoreError> for ImageOperationAdmissionError {
    fn from(error: ArtifactStoreError) -> Self {
        match error {
            ArtifactStoreError::ReferenceMismatch => Self::ReferenceMismatch,
            other => Self::Artifact(other),
        }
    }
}

impl From<ProviderError> for ImageOperationAdmissionError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl From<ImageOperationContractError> for ImageOperationAdmissionError {
    fn from(_: ImageOperationContractError) -> Self {
        Self::InvalidSourceMime
    }
}

pub struct AdmittedImageOperation {
    request: ArtifactImageOperationRequest,
    committed_refs: Vec<GeneratedArtifactRef>,
}

impl AdmittedImageOperation {
    pub(super) fn new(
        request: ArtifactImageOperationRequest,
        committed_refs: Vec<GeneratedArtifactRef>,
    ) -> Self {
        Self {
            request,
            committed_refs,
        }
    }

    pub(super) fn into_parts(self) -> (ArtifactImageOperationRequest, Vec<GeneratedArtifactRef>) {
        (self.request, self.committed_refs)
    }
}

pub struct ValidatedImageOperationCandidate {
    operation: GenerationOperation,
    local_image: ValidatedLocalImage,
    source_artifact_ids: Vec<ArtifactId>,
}

pub struct ValidatedLocalImage {
    pub(super) result: ImageGenerationResult,
}

impl ValidatedLocalImage {
    pub fn image(&self) -> &shacs_providers::GeneratedImage {
        &self.result.images[0]
    }

    pub fn result(&self) -> &ImageGenerationResult {
        &self.result
    }

    pub(crate) fn into_result(self) -> ImageGenerationResult {
        self.result
    }
}

impl ValidatedImageOperationCandidate {
    pub(super) fn new(
        operation: GenerationOperation,
        local_image: ValidatedLocalImage,
        source_artifact_ids: Vec<ArtifactId>,
    ) -> Self {
        Self {
            operation,
            local_image,
            source_artifact_ids,
        }
    }

    pub const fn operation(&self) -> GenerationOperation {
        self.operation
    }

    pub fn result(&self) -> &ImageGenerationResult {
        self.local_image.result()
    }

    pub const fn local_image(&self) -> &ValidatedLocalImage {
        &self.local_image
    }

    pub fn source_artifact_ids(&self) -> &[ArtifactId] {
        &self.source_artifact_ids
    }

    pub fn into_parts(self) -> (GenerationOperation, ImageGenerationResult, Vec<ArtifactId>) {
        (
            self.operation,
            self.local_image.into_result(),
            self.source_artifact_ids,
        )
    }

    pub(crate) fn into_validated_parts(
        self,
    ) -> (GenerationOperation, ValidatedLocalImage, Vec<ArtifactId>) {
        (self.operation, self.local_image, self.source_artifact_ids)
    }
}
