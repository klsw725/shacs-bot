use super::support::hex_digest;
use super::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactPublicationError, ArtifactPublicationMetadata,
    ArtifactPublisher, ArtifactStoreError, GeneratedArtifactRef, GenerationOptionsSummary,
    ImageGenerateTool, ImageGenerationResult, ProjectionDisclosure, ProviderMediaCandidateId,
    RetentionPolicy,
};
use chrono::Utc;

impl ImageGenerateTool {
    pub(super) fn persist_generated_artifacts(
        &self,
        result: &ImageGenerationResult,
        publication_options: &GenerationOptionsSummary,
    ) -> Result<Vec<GeneratedArtifactRef>, String> {
        let store = self.artifact_store.as_ref().ok_or_else(|| {
            "Error: Image generation media write failure: generated artifact store unavailable"
                .to_owned()
        })?;
        let publisher = ArtifactPublisher::new(store);
        let mut artifacts = Vec::with_capacity(result.images.len());
        for image in &result.images {
            if image.byte_len > self.config.max_bytes || image.bytes.len() > self.config.max_bytes {
                return Err(format!(
                    "Error: Image generation media write failure: image {} exceeds configured maxBytes {}",
                    image.index, self.config.max_bytes
                ));
            }
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
            let candidate_id = ProviderMediaCandidateId::new(candidate_id)
                .map_err(|error| format!("Error: Image generation provider failure: {error}"))?;
            let artifact_digest = hex_digest(
                format!(
                    "{}\0{}\0{}\0{}",
                    result.provider_id,
                    result.model,
                    candidate_id.as_str(),
                    hex_digest(&image.bytes)
                )
                .as_bytes(),
            );
            let artifact_id = ArtifactId::new(format!("img-{}", &artifact_digest[..24]))
                .map_err(|error| format!("Error: Image generation media write failure: {error}"))?;
            let metadata = ArtifactPublicationMetadata::try_new(
                artifact_id.clone(),
                ArtifactHandlingPolicy::new(
                    RetentionPolicy::UserManaged,
                    ProjectionDisclosure::RawContentPossibleElsewhere,
                ),
                Utc::now().to_rfc3339(),
            )
            .map_err(|error| format!("Error: Image generation media write failure: {error}"))?
            .with_options(publication_options.clone());
            let committed = match publisher.publish_generation_candidate(result, image, metadata) {
                Ok(committed) => committed,
                Err(ArtifactPublicationError::Artifact(ArtifactStoreError::AlreadyExists)) => {
                    store.read(&artifact_id).map_err(|error| {
                        format!("Error: Image generation media write failure: {error}")
                    })?
                }
                Err(error) => {
                    return Err(format!(
                        "Error: Image generation media write failure: {error}"
                    ))
                }
            };
            artifacts.push(committed.artifact_ref());
        }
        Ok(artifacts)
    }
}
