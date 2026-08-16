use serde_json::json;
use shacs_core::tools::{ImageGenerateTool, ImageGenerateToolConfig, Tool, ToolResult};
use shacs_providers::{
    GeneratedImage, ImageGenerationClient, ImageGenerationItemId, ImageGenerationRequest,
    ImageGenerationResult, ImageMimeType, ProviderError,
};
use std::error::Error;

struct LegacyImageClient;

const RAW_REQUEST_ID: &str = "ghp_persisted_request_secret";
const RAW_ITEM_ID: &str = "https://provider.example/item?token=sidecar-secret";

impl ImageGenerationClient for LegacyImageClient {
    fn generate_image(
        &self,
        _request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        Ok(ImageGenerationResult {
            provider_id: "openai".to_owned(),
            model: "gpt-image-2".to_owned(),
            images: vec![GeneratedImage {
                index: 0,
                mime_type: ImageMimeType::Png,
                bytes: b"legacy-png".to_vec(),
                byte_len: b"legacy-png".len(),
                revised_prompt: None,
                provider_item_id: Some(ImageGenerationItemId::from_provider(RAW_ITEM_ID)),
            }],
            remote_images: Vec::new(),
            usage: None,
            request_id: Some(shacs_providers::ImageGenerationRequestId::from_provider(
                RAW_REQUEST_ID,
            )),
        })
    }
}

#[test]
fn direct_tool_keeps_legacy_absolute_path_return_without_new_durable_record(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let media_dir = root.path().join("media");
    let tool = ImageGenerateTool::new(
        Box::new(LegacyImageClient),
        media_dir.clone(),
        ImageGenerateToolConfig {
            provider_id: "openai".to_owned(),
            model_id: "gpt-image-2".to_owned(),
            default_format: "png".to_owned(),
            max_count: 1,
            max_bytes: 1024,
        },
    );

    // When
    let ToolResult::Json(result) = tool.execute(
        json!({"prompt": "draw"})
            .as_object()
            .ok_or("fixture parameters must be an object")?
            .clone(),
    ) else {
        return Err("legacy direct tool did not return JSON".into());
    };

    // Then
    let path = result
        .pointer("/artifacts/0/path")
        .and_then(serde_json::Value::as_str)
        .ok_or("legacy path missing")?;
    assert!(std::path::Path::new(path).is_absolute());
    assert!(std::path::Path::new(path).starts_with(media_dir.canonicalize()?));
    let metadata_path = result
        .pointer("/artifacts/0/metadataPath")
        .and_then(serde_json::Value::as_str)
        .ok_or("legacy metadata path missing")?;
    assert!(std::path::Path::new(metadata_path).is_absolute());
    let durable_metadata = std::fs::read_to_string(metadata_path)?;
    let result_rendered = result.to_string();
    for forbidden in [
        RAW_REQUEST_ID,
        RAW_ITEM_ID,
        "sidecar-secret",
        "provider.example",
    ] {
        assert!(!durable_metadata.contains(forbidden));
        assert!(!result_rendered.contains(forbidden));
    }
    let durable_value: serde_json::Value = serde_json::from_str(&durable_metadata)?;
    assert!(durable_value.get("path").is_none());
    let durable_media_ref = durable_value
        .get("mediaRef")
        .and_then(serde_json::Value::as_str)
        .ok_or("durable relative media ref missing")?;
    assert!(!std::path::Path::new(durable_media_ref).is_absolute());
    assert!(!durable_media_ref.contains(".."));
    assert_eq!(
        Some(durable_media_ref),
        result
            .pointer("/artifacts/0/mediaRef")
            .and_then(serde_json::Value::as_str)
    );
    assert!(!durable_metadata.contains(&media_dir.to_string_lossy().to_string()));
    assert_eq!(
        result
            .get("generatedArtifacts")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        result.pointer("/disclosure/rawContentPossible"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        result
            .pointer("/disclosure/source")
            .and_then(serde_json::Value::as_str),
        Some("spec030")
    );
    assert!(!root.path().join("artifacts").exists());
    Ok(())
}
