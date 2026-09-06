use crate::generated_media::{
    ArtifactHandlingPolicy, ArtifactId, ArtifactPublicationError, ArtifactPublicationMetadata,
    ArtifactPublisher, ArtifactStore, ArtifactStoreError, GeneratedArtifactRef,
    GenerationOptionsSummary, ProjectionDisclosure, ProviderMediaCandidateId, RetentionPolicy,
};

mod publication;
mod storage;
mod support;
mod tool;

use support::{
    optional_string, param_count, publication_options, render_provider_error,
    request_option_summary,
};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
use crate::tools::{
    IntegerSchema, JsonMap, SchemaFragment, StringSchema, Tool, ToolCallExecutionContext,
    ToolParameters, ToolResult,
};
use serde_json::{json, Value};
use shacs_providers::{
    GeneratedImage, ImageGenerationClient, ImageGenerationRequest, ImageGenerationResult,
    ImageMimeType,
};
use std::path::PathBuf;

const IMAGE_GENERATE_TOOL_NAME: &str = "image_generate";
const ALLOWED_PARAMS: &[&str] = &["prompt", "size", "quality", "format", "background", "count"];

pub struct ImageGenerateTool {
    client: Box<dyn ImageGenerationClient>,
    media_dir: PathBuf,
    config: ImageGenerateToolConfig,
    artifact_store: Option<ArtifactStore>,
}

struct StoredArtifact {
    value: Value,
    image_path: PathBuf,
    metadata_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageGenerateToolConfig {
    pub provider_id: String,
    pub model_id: String,
    pub default_format: String,
    pub max_count: u32,
    pub max_bytes: usize,
}

impl ImageGenerateTool {
    pub fn new(
        client: Box<dyn ImageGenerationClient>,
        media_dir: PathBuf,
        config: ImageGenerateToolConfig,
    ) -> Self {
        Self {
            client,
            media_dir,
            config,
            artifact_store: None,
        }
    }

    pub fn with_artifact_store(mut self, artifact_store: ArtifactStore) -> Self {
        self.artifact_store = Some(artifact_store);
        self
    }

    fn build_request(&self, params: &JsonMap) -> Result<ImageGenerationRequest, String> {
        let prompt = params
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing prompt".to_owned())?;
        let count = param_count(params).unwrap_or(1);
        if count > self.config.max_count {
            return Err(format!(
                "count {count} exceeds configured maxCount {}",
                self.config.max_count
            ));
        }
        let mut request = ImageGenerationRequest::new(prompt);
        request.model = Some(self.config.model_id.clone());
        request.size = optional_string(params, "size");
        request.quality = optional_string(params, "quality");
        request.output_format = optional_string(params, "format")
            .or_else(|| Some(self.config.default_format.clone()))
            .filter(|value| !value.is_empty());
        request.background = optional_string(params, "background");
        request.count = Some(count);
        Ok(request)
    }

    fn execute_inner(
        &self,
        params: JsonMap,
        invocation: Option<&shacs_providers::ProviderInvocation>,
    ) -> Result<Value, String> {
        let request = self.build_request(&params).map_err(|error| {
            format!("Error: Image generation unsupported/config failure: {error}")
        })?;
        let request_summary = request_option_summary(&request);
        let publication_options = publication_options(&request)?;
        let result = match invocation {
            Some(invocation) => {
                self.client
                    .generate_image_with_invocation(request, &mut |_| {}, invocation)
            }
            None => self
                .client
                .generate_image_with_observer(request, &mut |_| {}),
        }
        .map_err(render_provider_error)?;
        if result.images.len() > self.config.max_count as usize {
            return Err(format!(
                "Error: Image generation provider failure: provider returned {} images, exceeding configured maxCount {}",
                result.images.len(), self.config.max_count
            ));
        }
        let generated_artifacts = if result.provider_id == "openai_codex" {
            self.persist_generated_artifacts(&result, &publication_options)?
        } else {
            Vec::new()
        };
        let artifacts = if generated_artifacts.is_empty() {
            self.store_artifacts(&result, &request_summary)?
        } else {
            generated_artifacts
                .iter()
                .map(|artifact| serde_json::to_value(artifact).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(json!({
            "artifacts": artifacts,
            "generatedArtifacts": generated_artifacts,
            "warnings": [],
            "disclosure": {
                "rawContentPossible": true,
                "surfaces": ["toolOutput"],
                "source": "spec030",
            },
            "provider": {
                "configuredProviderId": self.config.provider_id,
                "providerId": result.provider_id,
                "modelId": result.model,
                "requestId": result.request_id,
                "usage": result.usage,
            },
            "retryable": false,
        }))
    }
}
