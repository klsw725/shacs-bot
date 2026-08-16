use super::support::{
    cleanup_written_artifacts, ensure_child_path, hex_digest, image_extension,
    reject_symlink_components, revised_prompt_summary,
};
use super::{GeneratedImage, ImageGenerateTool, ImageGenerationResult, StoredArtifact, Value};
use chrono::Utc;
use serde_json::json;
use std::fs;
use std::path::Path;

impl ImageGenerateTool {
    pub(super) fn store_artifacts(
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
        let extension = image_extension(image.mime_type);
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
            "mimeType": image.mime_type, "byteLen": byte_len, "sha256": sha256,
            "providerId": result.provider_id, "modelId": result.model,
            "createdAt": created_at.to_rfc3339(), "requestOptionSummary": request_summary,
            "revisedPrompt": revised_prompt_summary(image.revised_prompt.as_deref()),
            "providerRequestId": result.request_id, "providerItemId": image.provider_item_id,
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
                "artifactId": artifact_id, "mediaRef": format!("media/image-generation/{filename}"),
                "path": image_path.to_string_lossy(),
                "metadataRef": format!("media/image-generation/{artifact_id}.json"),
                "metadataPath": metadata_path.to_string_lossy(), "mimeType": image.mime_type,
                "byteLen": byte_len, "sha256": sha256, "providerId": result.provider_id,
                "modelId": result.model, "createdAt": created_at.to_rfc3339(),
            }),
            image_path,
            metadata_path,
        })
    }
}
