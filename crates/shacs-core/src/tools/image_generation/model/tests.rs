use super::test_support::{tool_with_client, CapturingClient};
use super::*;
use serde_json::Map;
use shacs_providers::ProviderError;
use std::error::Error;
use std::fs;
use std::sync::{Arc, Mutex};

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
        assert!(!properties.contains_key(forbidden));
    }
    for forbidden in ["provider", "apiKey", "endpoint"] {
        let mut params = Map::new();
        params.insert("prompt".to_owned(), Value::String("draw a cat".to_owned()));
        params.insert(forbidden.to_owned(), Value::String("secret".to_owned()));
        assert!(!tool.validate_params(&params).is_empty());
    }
    Ok(())
}

#[test]
fn maps_prompt_and_options_into_image_generation_request() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let (client, captured) = CapturingClient::success();
    let tool = tool_with_client(client, dir.path().join("media"));
    assert!(matches!(
        tool.execute(json_map(json!({
            "prompt": "draw a quiet forest", "size": "1024x1024", "quality": "high",
            "format": "webp", "background": "transparent", "count": 2
        }))?),
        ToolResult::Json(_)
    ));
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
fn writes_image_and_metadata_without_raw_bytes_base64_or_prompt() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let prompt = "draw a private prompt that must not persist";
    let (client, _) = CapturingClient::success();
    let tool = tool_with_client(client, dir.path().join("media"));
    let ToolResult::Json(value) = tool.execute(json_map(json!({"prompt": prompt}))?) else {
        return Err("tool did not return JSON success".into());
    };
    let rendered = value.to_string();
    for forbidden in ["not real png", prompt, "expanded secret prompt"] {
        assert!(!rendered.contains(forbidden));
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
    assert!(image_path.starts_with(&media_dir) && metadata_path.starts_with(&media_dir));
    assert!(image_path.is_file() && metadata_path.is_file());
    let metadata = fs::read_to_string(metadata_path)?;
    for forbidden in [prompt, "expanded secret prompt", "not real png"] {
        assert!(!metadata.contains(forbidden));
    }
    assert!(metadata.contains("requestOptionSummary"));
    assert!(metadata.contains("revisedPrompt"));
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
                mime_type: ImageMimeType::Webp,
                bytes: b"webp bytes".to_vec(),
                byte_len: b"webp bytes".len(),
                revised_prompt: None,
                provider_item_id: None,
            }],
            remote_images: Vec::new(),
            usage: None,
            request_id: None,
        }),
    };
    let tool = tool_with_client(client, dir.path().join("media"));
    let ToolResult::Json(value) =
        tool.execute(json_map(json!({"prompt": "draw", "format": "webp"}))?)
    else {
        return Err("tool did not return JSON success".into());
    };
    assert!(value
        .pointer("/artifacts/0/mediaRef")
        .and_then(Value::as_str)
        .is_some_and(|media_ref| media_ref.ends_with(".webp")));
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
    let result = tool_with_client(client, link)
        .execute(json_map(json!({"prompt": "draw"}))?)
        .into_text();
    assert!(result.contains("media write failure") && result.contains("symlink"));
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
    assert!(provider_result.contains("provider failure"));
    assert!(!provider_result.contains("secret body"));
    let file_media_dir = dir.path().join("not-a-directory");
    fs::write(&file_media_dir, b"occupied")?;
    let (client, _) = CapturingClient::success();
    let media_result = tool_with_client(client, file_media_dir)
        .execute(json_map(json!({"prompt": "draw"}))?)
        .into_text();
    assert!(media_result.contains("media write failure"));
    Ok(())
}

fn json_map(value: Value) -> Result<JsonMap, Box<dyn Error>> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err("expected object".into()),
    }
}
