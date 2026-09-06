use shacs_core::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactStore, ArtifactWriteRequest,
    GeneratedArtifactDefinition, GeneratedArtifactMetadata, GeneratedArtifactRef,
    GeneratedMediaKind, GenerationOperation, ProjectionDisclosure, ProviderMediaBytes,
    ProviderMediaCandidate, ProviderMediaCandidateId, ProviderMediaOrigin, RetentionPolicy,
};
use shacs_providers::{
    GeneratedImage, ImageGenerationClient, ImageGenerationItemId, ImageGenerationRequest,
    ImageGenerationResult, ImageMimeType, ImageOperationRequest, ImageOperationResult,
    ProviderError,
};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\nsource";

pub fn persist_image(
    store: &ArtifactStore,
    id: &str,
    bytes: &[u8],
) -> Result<GeneratedArtifactRef, Box<dyn Error>> {
    persist_media(store, id, "image/png", bytes, GeneratedMediaKind::Image)
}

pub fn persist_media(
    store: &ArtifactStore,
    id: &str,
    mime_type: &str,
    bytes: &[u8],
    kind: GeneratedMediaKind,
) -> Result<GeneratedArtifactRef, Box<dyn Error>> {
    let candidate = ProviderMediaCandidate::bytes(
        ProviderMediaCandidateId::new(format!("candidate-{id}"))?,
        ProviderMediaOrigin::new("openai", "gpt-image-2"),
        ProviderMediaBytes::new(mime_type, bytes.to_vec()),
    );
    let metadata = GeneratedArtifactMetadata::new(
        ArtifactId::new(id)?,
        GeneratedArtifactDefinition::new(
            kind,
            GenerationOperation::Generate,
            ArtifactHandlingPolicy::new(
                RetentionPolicy::UserManaged,
                ProjectionDisclosure::RawContentPossibleElsewhere,
            ),
        ),
        "2026-08-15T00:00:00Z",
    );
    Ok(store
        .persist(ArtifactWriteRequest::new(candidate, metadata))?
        .artifact_ref())
}

pub struct CountingClient {
    calls: AtomicUsize,
    returns_image: bool,
}

impl CountingClient {
    pub const fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            returns_image: true,
        }
    }

    pub const fn misleading_success() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            returns_image: false,
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ImageGenerationClient for CountingClient {
    fn generate_image(
        &self,
        _request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        unreachable!("Todo7 tests execute only image operations")
    }

    fn execute_image_operation(
        &self,
        request: ImageOperationRequest,
    ) -> Result<ImageOperationResult, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = ImageGenerationResult {
            provider_id: "openai".to_owned(),
            model: "gpt-image-2".to_owned(),
            images: self
                .returns_image
                .then(|| GeneratedImage {
                    index: 0,
                    mime_type: ImageMimeType::Png,
                    bytes: PNG.to_vec(),
                    byte_len: PNG.len(),
                    revised_prompt: None,
                    provider_item_id: Some(ImageGenerationItemId::from_provider("edit-result")),
                })
                .into_iter()
                .collect(),
            remote_images: Vec::new(),
            usage: None,
            request_id: None,
        };
        Ok(match request {
            ImageOperationRequest::Edit(_) => ImageOperationResult::Edit(result),
            ImageOperationRequest::Mask(_) => ImageOperationResult::Mask(result),
            ImageOperationRequest::Variation(_) => ImageOperationResult::Variation(result),
            ImageOperationRequest::Generate(_) => ImageOperationResult::Generate(result),
        })
    }
}

pub fn artifact_count(root: &std::path::Path) -> Result<usize, Box<dyn Error>> {
    Ok(std::fs::read_dir(root.join("artifacts"))?.count())
}
