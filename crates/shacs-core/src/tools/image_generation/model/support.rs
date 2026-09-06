use super::{GenerationOptionsSummary, ImageGenerationRequest, ImageMimeType, JsonMap};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_providers::ProviderError;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn optional_string(params: &JsonMap, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(super) fn param_count(params: &JsonMap) -> Option<u32> {
    params
        .get("count")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

pub(super) fn request_option_summary(request: &ImageGenerationRequest) -> Value {
    json!({
        "model": request.model, "size": request.size, "quality": request.quality,
        "format": request.output_format, "background": request.background, "count": request.count,
    })
}

pub(super) fn publication_options(
    request: &ImageGenerationRequest,
) -> Result<GenerationOptionsSummary, String> {
    let mut options = std::collections::BTreeMap::new();
    for (name, value) in [
        ("model", request.model.clone()),
        ("size", request.size.clone()),
        ("quality", request.quality.clone()),
        ("format", request.output_format.clone()),
        ("background", request.background.clone()),
        ("count", request.count.map(|count| count.to_string())),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            options.insert(name.to_owned(), value);
        }
    }
    GenerationOptionsSummary::new(options)
        .map_err(|error| format!("Error: Image generation media write failure: {error}"))
}

pub(super) fn revised_prompt_summary(prompt: Option<&str>) -> Value {
    match prompt {
        Some(prompt) => json!({"sha256": hex_digest(prompt.as_bytes()), "redacted": true}),
        None => json!({"sha256": Value::Null, "redacted": false}),
    }
}

pub(super) fn render_provider_error(error: ProviderError) -> String {
    match error {
        ProviderError::UnsupportedCapability { provider_id, capability } => format!(
            "Error: Image generation unsupported/config failure: provider {provider_id} does not support {capability}"
        ),
        ProviderError::AuthRequired { provider_id } => format!(
            "Error: Image generation unsupported/config failure: provider {provider_id} requires configured authentication"
        ),
        ProviderError::ProviderNotFound { provider_id, .. } => format!(
            "Error: Image generation unsupported/config failure: provider {provider_id} was not found"
        ),
        ProviderError::ModelNotFound { provider_id, model_id, .. } => format!(
            "Error: Image generation unsupported/config failure: model {provider_id}/{model_id} was not found"
        ),
        ProviderError::Api { status, retryable, .. } => format!(
            "Error: Image generation provider failure: code=image_generation_provider_error status={} retryable={retryable} message=Image generation provider request failed",
            status.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_owned())
        ),
    }
}

pub(super) const fn image_extension(mime_type: ImageMimeType) -> &'static str {
    match mime_type {
        ImageMimeType::Jpeg => "jpg",
        ImageMimeType::Webp => "webp",
        ImageMimeType::Png => "png",
    }
}

pub(super) fn reject_symlink_components(path: &Path) -> Result<(), String> {
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

pub(super) fn ensure_child_path(media_dir: &Path, path: &Path) -> Result<(), String> {
    if path.starts_with(media_dir) {
        Ok(())
    } else {
        Err(
            "Error: Image generation media write failure: artifact path escapes media directory"
                .to_owned(),
        )
    }
}

pub(super) fn cleanup_written_artifacts(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = fs::remove_file(path);
    }
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
