use crate::tools::{
    IntegerSchema, JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolResult,
};
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_providers::{
    GeneratedImage, ImageGenerationClient, ImageGenerationRequest, ImageGenerationResult,
    ProviderError,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const IMAGE_GENERATE_TOOL_NAME: &str = "image_generate";
const ALLOWED_PARAMS: &[&str] = &["prompt", "size", "quality", "format", "background", "count"];

pub struct ImageGenerateTool {
    client: Box<dyn ImageGenerationClient>,
    media_dir: PathBuf,
    config: ImageGenerateToolConfig,
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
        }
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

    fn execute_inner(&self, params: JsonMap) -> Result<Value, String> {
        let request = self.build_request(&params).map_err(|error| {
            format!("Error: Image generation unsupported/config failure: {error}")
        })?;
        let request_summary = request_option_summary(&request);
        let result = self
            .client
            .generate_image(request)
            .map_err(render_provider_error)?;
        if result.images.len() > self.config.max_count as usize {
            return Err(format!(
                "Error: Image generation provider failure: provider returned {} images, exceeding configured maxCount {}",
                result.images.len(), self.config.max_count
            ));
        }
        let artifacts = self.store_artifacts(&result, &request_summary)?;
        Ok(json!({
            "artifacts": artifacts,
            "warnings": [],
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

    fn store_artifacts(
        &self,
        result: &ImageGenerationResult,
        request_summary: &Value,
    ) -> Result<Vec<Value>, String> {
        fs::create_dir_all(&self.media_dir).map_err(|error| {
            format!("Error: Image generation media write failure: could not create media directory: {error}")
        })?;
        reject_symlink_components(&self.media_dir)?;
        let media_dir = fs::canonicalize(&self.media_dir).map_err(|error| {
            format!("Error: Image generation media write failure: could not resolve media directory: {error}")
        })?;
        let mut artifacts = Vec::new();
        let mut written_paths = Vec::new();
        for image in &result.images {
            if image.byte_len > self.config.max_bytes || image.bytes.len() > self.config.max_bytes {
                cleanup_written_artifacts(&written_paths);
                return Err(format!(
                    "Error: Image generation media write failure: image {} exceeds configured maxBytes {}",
                    image.index, self.config.max_bytes
                ));
            }
            match self.store_artifact(&media_dir, image, result, request_summary) {
                Ok(stored) => {
                    written_paths.push(stored.image_path);
                    written_paths.push(stored.metadata_path);
                    artifacts.push(stored.value);
                }
                Err(error) => {
                    cleanup_written_artifacts(&written_paths);
                    return Err(error);
                }
            }
        }
        Ok(artifacts)
    }

    fn store_artifact(
        &self,
        media_dir: &Path,
        image: &GeneratedImage,
        result: &ImageGenerationResult,
        request_summary: &Value,
    ) -> Result<StoredArtifact, String> {
        let created_at = Utc::now();
        let sha256 = hex_digest(&image.bytes);
        let byte_len = image.bytes.len();
        let digest_short = sha256.get(0..16).unwrap_or(&sha256);
        let extension = image_extension(&image.mime_type, self.config.default_format.as_str());
        let timestamp = created_at.format("%Y%m%dT%H%M%SZ");
        let artifact_id = format!("img-{timestamp}-{digest_short}-{}", image.index);
        let filename = format!("{artifact_id}.{extension}");
        let image_path = media_dir.join(&filename);
        let metadata_path = media_dir.join(format!("{artifact_id}.json"));
        ensure_child_path(media_dir, &image_path)?;
        ensure_child_path(media_dir, &metadata_path)?;
        fs::write(&image_path, &image.bytes).map_err(|error| {
            format!("Error: Image generation media write failure: could not write image artifact: {error}")
        })?;
        let metadata = json!({
            "artifactId": artifact_id,
            "mediaRef": format!("media/image-generation/{filename}"),
            "path": image_path.to_string_lossy(),
            "mimeType": image.mime_type,
            "byteLen": byte_len,
            "sha256": sha256,
            "providerId": result.provider_id,
            "modelId": result.model,
            "createdAt": created_at.to_rfc3339(),
            "requestOptionSummary": request_summary,
            "revisedPrompt": revised_prompt_summary(image.revised_prompt.as_deref()),
            "providerRequestId": result.request_id,
            "providerItemId": image.provider_item_id,
        });
        let metadata_bytes = serde_json::to_vec_pretty(&metadata).map_err(|error| {
            format!(
                "Error: Image generation media write failure: could not encode metadata: {error}"
            )
        })?;
        if let Err(error) = fs::write(&metadata_path, metadata_bytes) {
            let _ = fs::remove_file(&image_path);
            return Err(format!(
                "Error: Image generation media write failure: could not write metadata: {error}"
            ));
        }
        Ok(StoredArtifact {
            value: json!({
                "artifactId": artifact_id,
                "mediaRef": format!("media/image-generation/{filename}"),
                "path": image_path.to_string_lossy(),
                "metadataRef": format!("media/image-generation/{artifact_id}.json"),
                "metadataPath": metadata_path.to_string_lossy(),
                "mimeType": image.mime_type,
                "byteLen": byte_len,
                "sha256": sha256,
                "providerId": result.provider_id,
                "modelId": result.model,
                "createdAt": created_at.to_rfc3339(),
            }),
            image_path,
            metadata_path,
        })
    }
}

impl Tool for ImageGenerateTool {
    fn name(&self) -> &str {
        IMAGE_GENERATE_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Generate images through the configured image generation provider and store local media artifact references."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                "prompt",
                StringSchema::new("Image generation prompt").min_length(1),
            )
            .property("size", StringSchema::new("Provider-supported image size"))
            .property(
                "quality",
                StringSchema::new("Provider-supported quality setting"),
            )
            .property(
                "format",
                StringSchema::new("Output image format").enum_values([
                    Value::String("png".to_owned()),
                    Value::String("jpeg".to_owned()),
                    Value::String("webp".to_owned()),
                ]),
            )
            .property(
                "background",
                StringSchema::new("Provider-supported background setting"),
            )
            .property(
                "count",
                IntegerSchema::new("Number of images to generate")
                    .minimum(1)
                    .maximum(i64::from(self.config.max_count)),
            )
            .required(["prompt"])
            .to_json_schema()
    }

    fn read_only(&self) -> bool {
        false
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        match self.execute_inner(params) {
            Ok(value) => ToolResult::Json(value),
            Err(error) => ToolResult::Text(error),
        }
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<crate::tools::ValidationError> {
        let allowed: BTreeSet<&str> = ALLOWED_PARAMS.iter().copied().collect();
        let mut errors = super::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        );
        for key in params.keys() {
            if !allowed.contains(key.as_str()) {
                errors.push(crate::tools::ValidationError::new(
                    key,
                    "is not an allowed parameter",
                ));
            }
        }
        errors
    }
}

fn optional_string(params: &JsonMap, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn param_count(params: &JsonMap) -> Option<u32> {
    params
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn request_option_summary(request: &ImageGenerationRequest) -> Value {
    json!({
        "model": request.model,
        "size": request.size,
        "quality": request.quality,
        "format": request.output_format,
        "background": request.background,
        "count": request.count,
    })
}

fn revised_prompt_summary(prompt: Option<&str>) -> Value {
    match prompt {
        Some(prompt) => json!({
            "sha256": hex_digest(prompt.as_bytes()),
            "redacted": true,
        }),
        None => json!({
            "sha256": Value::Null,
            "redacted": false,
        }),
    }
}

fn render_provider_error(error: ProviderError) -> String {
    match error {
        ProviderError::UnsupportedCapability {
            provider_id,
            capability,
        } => format!(
            "Error: Image generation unsupported/config failure: provider {provider_id} does not support {capability}"
        ),
        ProviderError::AuthRequired { provider_id } => format!(
            "Error: Image generation unsupported/config failure: provider {provider_id} requires configured authentication"
        ),
        ProviderError::ProviderNotFound { provider_id, .. } => format!(
            "Error: Image generation unsupported/config failure: provider {provider_id} was not found"
        ),
        ProviderError::ModelNotFound {
            provider_id,
            model_id,
            ..
        } => format!(
            "Error: Image generation unsupported/config failure: model {provider_id}/{model_id} was not found"
        ),
        ProviderError::Api {
            status,
            message,
            retryable,
            ..
        } => format!(
            "Error: Image generation provider failure: status={} retryable={retryable}: {message}",
            status
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ),
    }
}

fn image_extension(mime_type: &str, requested_format: &str) -> String {
    match mime_type {
        "image/jpeg" => "jpg".to_owned(),
        "image/webp" => "webp".to_owned(),
        "image/png" => "png".to_owned(),
        _ => match requested_format.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => "jpg".to_owned(),
            "webp" => "webp".to_owned(),
            _ => "png".to_owned(),
        },
    }
}

fn reject_symlink_components(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Error: Image generation media write failure: could not inspect media path: {error}"
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Error: Image generation media write failure: media path is a symlink: {}",
            path.display()
        ));
    }
    Ok(())
}

fn ensure_child_path(media_dir: &Path, path: &Path) -> Result<(), String> {
    if path.starts_with(media_dir) {
        Ok(())
    } else {
        Err(
            "Error: Image generation media write failure: artifact path escapes media directory"
                .to_owned(),
        )
    }
}

fn cleanup_written_artifacts(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = fs::remove_file(path);
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use shacs_providers::ProviderError;
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::Mutex;

    struct CapturingClient {
        requests: Arc<Mutex<Vec<ImageGenerationRequest>>>,
        response: Result<ImageGenerationResult, ProviderError>,
    }

    impl CapturingClient {
        fn success() -> (Self, Arc<Mutex<Vec<ImageGenerationRequest>>>) {
            let requests = Arc::new(Mutex::new(Vec::new()));
            Self {
                requests: requests.clone(),
                response: Ok(ImageGenerationResult {
                    provider_id: "openai".to_owned(),
                    model: "gpt-image-2".to_owned(),
                    images: vec![GeneratedImage {
                        index: 0,
                        mime_type: "image/png".to_owned(),
                        bytes: b"not real png".to_vec(),
                        byte_len: b"not real png".len(),
                        revised_prompt: Some("expanded secret prompt".to_owned()),
                        provider_item_id: Some("item_1".to_owned()),
                    }],
                    usage: None,
                    request_id: Some("req_1".to_owned()),
                    provider_metadata: Map::new(),
                }),
            }
            .with_requests(requests)
        }

        fn with_requests(
            self,
            requests: Arc<Mutex<Vec<ImageGenerationRequest>>>,
        ) -> (Self, Arc<Mutex<Vec<ImageGenerationRequest>>>) {
            (self, requests)
        }
    }

    impl ImageGenerationClient for CapturingClient {
        fn generate_image(
            &self,
            request: ImageGenerationRequest,
        ) -> Result<ImageGenerationResult, ProviderError> {
            self.requests
                .lock()
                .map_err(|_| ProviderError::Api {
                    status: None,
                    message: "capture lock poisoned".to_owned(),
                    retryable: false,
                    headers: Default::default(),
                    body: None,
                })?
                .push(request);
            self.response.clone()
        }
    }

    fn tool_with_client(client: CapturingClient, media_dir: PathBuf) -> ImageGenerateTool {
        ImageGenerateTool::new(
            Box::new(client),
            media_dir,
            ImageGenerateToolConfig {
                provider_id: "openai".to_owned(),
                model_id: "gpt-image-2".to_owned(),
                default_format: "png".to_owned(),
                max_count: 2,
                max_bytes: 1024,
            },
        )
    }

    #[test]
    fn schema_validation_rejects_unknown_provider_auth_and_endpoint_params(
    ) -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let (client, _) = CapturingClient::success();
        let tool = tool_with_client(client, dir.path().join("media"));
        let schema = tool.parameters();
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or("schema properties missing")?;
        for forbidden in [
            "provider",
            "apiKey",
            "endpoint",
            "baseUrl",
            "providerOptions",
        ] {
            if properties.contains_key(forbidden) {
                return Err(format!("forbidden parameter exposed in schema: {forbidden}").into());
            }
        }
        for forbidden in ["provider", "apiKey", "endpoint"] {
            let mut params = Map::new();
            params.insert("prompt".to_owned(), Value::String("draw a cat".to_owned()));
            params.insert(forbidden.to_owned(), Value::String("secret".to_owned()));
            let errors = tool.validate_params(&params);
            if errors.is_empty() {
                return Err(format!("{forbidden} was accepted").into());
            }
        }
        Ok(())
    }

    #[test]
    fn maps_prompt_and_options_into_image_generation_request() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let (client, captured) = CapturingClient::success();
        let tool = tool_with_client(client, dir.path().join("media"));
        let result = tool.execute(json_map(json!({
            "prompt": "draw a quiet forest",
            "size": "1024x1024",
            "quality": "high",
            "format": "webp",
            "background": "transparent",
            "count": 2
        }))?);
        if !matches!(result, ToolResult::Json(_)) {
            return Err("tool did not return JSON success".into());
        }
        let requests = captured
            .lock()
            .map_err(|_| "captured requests lock poisoned")?
            .clone();
        let request = requests.first().ok_or("request was not captured")?;
        assert_eq!(request.prompt, "draw a quiet forest");
        assert_eq!(request.model.as_deref(), Some("gpt-image-2"));
        assert_eq!(request.size.as_deref(), Some("1024x1024"));
        assert_eq!(request.quality.as_deref(), Some("high"));
        assert_eq!(request.output_format.as_deref(), Some("webp"));
        assert_eq!(request.background.as_deref(), Some("transparent"));
        assert_eq!(request.count, Some(2));
        Ok(())
    }

    #[test]
    fn writes_image_and_metadata_without_raw_bytes_base64_or_prompt() -> Result<(), Box<dyn Error>>
    {
        let dir = tempfile::tempdir()?;
        let prompt = "draw a private prompt that must not persist";
        let (client, _) = CapturingClient::success();
        let tool = tool_with_client(client, dir.path().join("media"));
        let result = tool.execute(json_map(json!({"prompt": prompt}))?);
        let ToolResult::Json(value) = result else {
            return Err("tool did not return JSON success".into());
        };
        let rendered = value.to_string();
        if rendered.contains("not real png")
            || rendered.contains(prompt)
            || rendered.contains("expanded secret prompt")
        {
            return Err(format!("result leaked raw data or prompt: {rendered}").into());
        }
        let artifact = value
            .get("artifacts")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .ok_or("missing artifact")?;
        let image_path = PathBuf::from(
            artifact
                .get("path")
                .and_then(Value::as_str)
                .ok_or("missing image path")?,
        );
        let metadata_path = PathBuf::from(
            artifact
                .get("metadataPath")
                .and_then(Value::as_str)
                .ok_or("missing metadata path")?,
        );
        let media_dir = fs::canonicalize(dir.path().join("media"))?;
        if !image_path.starts_with(&media_dir) || !metadata_path.starts_with(&media_dir) {
            return Err("artifact escaped media subtree".into());
        }
        if !image_path.is_file() || !metadata_path.is_file() {
            return Err("artifact files were not written".into());
        }
        let metadata = fs::read_to_string(metadata_path)?;
        if metadata.contains(prompt)
            || metadata.contains("expanded secret prompt")
            || metadata.contains("not real png")
        {
            return Err(format!("metadata leaked raw data or prompt: {metadata}").into());
        }
        if !metadata.contains("requestOptionSummary") || !metadata.contains("revisedPrompt") {
            return Err(format!("metadata missing required summary: {metadata}").into());
        }
        Ok(())
    }

    #[test]
    fn stores_artifact_extension_from_generated_mime_type() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let client = CapturingClient {
            requests: Arc::new(Mutex::new(Vec::new())),
            response: Ok(ImageGenerationResult {
                provider_id: "openai".to_owned(),
                model: "gpt-image-2".to_owned(),
                images: vec![GeneratedImage {
                    index: 0,
                    mime_type: "image/webp".to_owned(),
                    bytes: b"webp bytes".to_vec(),
                    byte_len: b"webp bytes".len(),
                    revised_prompt: None,
                    provider_item_id: None,
                }],
                usage: None,
                request_id: None,
                provider_metadata: Map::new(),
            }),
        };
        let tool = tool_with_client(client, dir.path().join("media"));
        let result = tool.execute(json_map(json!({
            "prompt": "draw",
            "format": "webp"
        }))?);
        let ToolResult::Json(value) = result else {
            return Err("tool did not return JSON success".into());
        };
        let media_ref = value
            .pointer("/artifacts/0/mediaRef")
            .and_then(Value::as_str)
            .ok_or("missing media ref")?;
        if !media_ref.ends_with(".webp") {
            return Err(format!("media ref did not use webp extension: {media_ref}").into());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_media_directory() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("target");
        fs::create_dir_all(&target)?;
        let link = dir.path().join("media-link");
        std::os::unix::fs::symlink(&target, &link)?;
        let (client, _) = CapturingClient::success();
        let tool = tool_with_client(client, link);
        let result = tool
            .execute(json_map(json!({"prompt": "draw"}))?)
            .into_text();
        if !result.contains("media write failure") || !result.contains("symlink") {
            return Err(format!("symlink media dir was not rejected: {result}").into());
        }
        Ok(())
    }

    #[test]
    fn distinguishes_provider_failure_from_media_write_failure() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let provider_tool = tool_with_client(
            CapturingClient {
                requests: Arc::new(Mutex::new(Vec::new())),
                response: Err(ProviderError::Api {
                    status: Some(500),
                    message: "upstream unavailable".to_owned(),
                    retryable: true,
                    headers: Default::default(),
                    body: Some("secret body".to_owned()),
                }),
            },
            dir.path().join("provider"),
        );
        let provider_result = provider_tool
            .execute(json_map(json!({"prompt": "draw"}))?)
            .into_text();
        if !provider_result.contains("provider failure") || provider_result.contains("secret body")
        {
            return Err(
                format!("provider error was not distinct/redacted: {provider_result}").into(),
            );
        }

        let file_media_dir = dir.path().join("not-a-directory");
        fs::write(&file_media_dir, b"occupied")?;
        let (client, _) = CapturingClient::success();
        let media_tool = tool_with_client(client, file_media_dir);
        let media_result = media_tool
            .execute(json_map(json!({"prompt": "draw"}))?)
            .into_text();
        if !media_result.contains("media write failure") {
            return Err(format!("media error was not distinct: {media_result}").into());
        }
        Ok(())
    }

    fn json_map(value: Value) -> Result<JsonMap, Box<dyn Error>> {
        match value {
            Value::Object(map) => Ok(map),
            _ => Err("expected object".into()),
        }
    }
}
