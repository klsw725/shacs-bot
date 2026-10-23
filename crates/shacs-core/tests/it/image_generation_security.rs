use serde_json::{json, Map, Value};
use shacs_core::tools::{ImageGenerateTool, ImageGenerateToolConfig, Tool, ToolResult};
use shacs_providers::{
    parse_openai_image_generation_response, ImageGenerationClient, ImageGenerationHttpResponse,
    ImageGenerationRequest, ImageGenerationResult, ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;

struct ProviderResponseClient {
    response: ImageGenerationHttpResponse,
}

impl ImageGenerationClient for ProviderResponseClient {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        parse_openai_image_generation_response(
            self.response.clone(),
            request.model.as_deref().unwrap_or("gpt-image-2"),
        )
    }
}

struct ProviderErrorClient;

impl ImageGenerationClient for ProviderErrorClient {
    fn generate_image(
        &self,
        _request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        Err(ProviderError::Api {
            status: Some(429),
            message: "https://provider.example/error?token=tool-secret".to_owned(),
            retryable: true,
            headers: BTreeMap::from([(
                "Set-Cookie".to_owned(),
                "session=tool-cookie-secret".to_owned(),
            )]),
            body: Some("credential=tool-body-secret".to_owned()),
        })
    }
}

#[test]
fn tool_result_contains_only_safe_provider_metadata() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let signed_id = "https://provider.example/request?token=request-secret&signature=signed-secret";
    let signed_item_id = "https://provider.example/item?token=item-secret&signature=item-signature";
    let tool = tool_with_client(
        Box::new(ProviderResponseClient {
            response: ImageGenerationHttpResponse {
                status: 200,
                headers: BTreeMap::from([("x-request-id".to_owned(), signed_id.to_owned())]),
                body: json!({
                    "usage": {
                        "input_tokens": 4,
                        "output_tokens": 6,
                        "total_tokens": 10,
                        "nested": {"secret": "usage-secret"},
                        "text": "credential-secret"
                    },
                    "metadata": {"nested": signed_item_id},
                    "data": [{
                        "id": signed_item_id,
                        "mime_type": "image/png",
                        "b64_json": "aW1hZ2U="
                    }]
                }),
            },
        }),
        dir.path().join("media"),
    );

    // When
    let ToolResult::Json(value) = tool.execute(prompt_params()) else {
        return Err("image tool did not return JSON".into());
    };

    // Then
    let provider = value.get("provider").ok_or("provider facts missing")?;
    assert_eq!(
        provider.get("usage"),
        Some(&json!({"input_tokens": 4, "output_tokens": 6, "total_tokens": 10}))
    );
    let request_id = provider
        .get("requestId")
        .and_then(Value::as_str)
        .ok_or("request ID missing")?;
    assert!(request_id.starts_with("request_sha256_"));
    assert_eq!(request_id.len(), "request_sha256_".len() + 64);
    let metadata_path = value
        .pointer("/artifacts/0/metadataPath")
        .and_then(Value::as_str)
        .ok_or("artifact metadata path missing")?;
    let metadata: Value = serde_json::from_slice(&std::fs::read(metadata_path)?)?;
    let item_id = metadata
        .get("providerItemId")
        .and_then(Value::as_str)
        .ok_or("provider item ID missing from durable metadata")?;
    assert!(item_id.starts_with("item_sha256_"));
    assert_eq!(item_id.len(), "item_sha256_".len() + 64);
    assert_eq!(
        value
            .pointer("/artifacts/0/mimeType")
            .and_then(Value::as_str),
        Some("image/png")
    );
    assert_no_tool_secret(&value.to_string());
    assert_no_tool_secret(&metadata.to_string());
    Ok(())
}

#[test]
fn tool_error_uses_stable_code_status_and_retryability_only() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tool = tool_with_client(Box::new(ProviderErrorClient), dir.path().join("media"));

    // When
    let output = tool.execute(prompt_params()).into_text();

    // Then
    assert_eq!(
        output,
        "Error: Image generation provider failure: code=image_generation_provider_error status=429 retryable=true message=Image generation provider request failed"
    );
    assert_no_tool_secret(&output);
    Ok(())
}

fn tool_with_client(
    client: Box<dyn ImageGenerationClient>,
    media_dir: PathBuf,
) -> ImageGenerateTool {
    ImageGenerateTool::new(
        client,
        media_dir,
        ImageGenerateToolConfig {
            provider_id: "openai".to_owned(),
            model_id: "gpt-image-2".to_owned(),
            default_format: "png".to_owned(),
            max_count: 1,
            max_bytes: 1024,
        },
    )
}

fn prompt_params() -> Map<String, Value> {
    Map::from_iter([("prompt".to_owned(), Value::String("draw".to_owned()))])
}

fn assert_no_tool_secret(rendered: &str) {
    for forbidden in [
        "provider.example",
        "request-secret",
        "signed-secret",
        "item-secret",
        "item-signature",
        "usage-secret",
        "credential-secret",
        "tool-secret",
        "tool-cookie-secret",
        "tool-body-secret",
        "Set-Cookie",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "tool result leaked: {rendered}"
        );
    }
}
