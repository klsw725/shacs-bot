use serde_json::json;
use shacs_providers::{
    build_openrouter_image_generation_request, parse_openrouter_image_generation_response,
    ImageGenerationClient, ImageGenerationHttpResponse, ImageGenerationRequest,
    ImageGenerationRequestParts, OpenRouterImageGenerationClient, ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

#[test]
fn openrouter_image_generation_request_builder_uses_chat_completions_contract(
) -> Result<(), Box<dyn Error>> {
    let mut request = ImageGenerationRequest::new("paint a small robot");
    request.size = Some("1024x1024".to_owned());
    request.quality = Some("high".to_owned());
    request.output_format = Some("png".to_owned());
    request.background = Some("transparent".to_owned());
    request.count = Some(2);
    request
        .provider_options
        .insert("style".to_owned(), json!("natural"));

    let parts = build_openrouter_image_generation_request(
        "sk-or-test",
        &request,
        "google/gemini-2.5-flash-image-preview",
    );
    if parts.path != "/chat/completions"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer sk-or-test")
        || parts.body
            != json!({
                "model": "google/gemini-2.5-flash-image-preview",
                "messages": [{"role": "user", "content": "paint a small robot"}],
                "modalities": ["image", "text"],
                "stream": false,
                "image_config": {
                    "size": "1024x1024",
                    "quality": "high",
                    "output_format": "png",
                    "background": "transparent",
                    "n": 2,
                    "style": "natural"
                }
            })
    {
        return Err(format!("OpenRouter image request builder drifted: {:?}", parts.body).into());
    }
    Ok(())
}

#[test]
fn openrouter_image_generation_parser_decodes_data_url_images() -> Result<(), Box<dyn Error>> {
    let result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::from([("x-request-id".to_owned(), "req-or-123".to_owned())]),
            body: json!({
                "id": "gen-1",
                "created": 1710000000,
                "usage": {"total_tokens": 12},
                "choices": [{
                    "id": "choice-1",
                    "message": {
                        "role": "assistant",
                        "content": "painted a tiny robot",
                        "images": [{
                            "image_url": {"url": "data:image/png;base64,aGVsbG8="}
                        }]
                    }
                }]
            }),
        },
        "google/gemini-2.5-flash-image-preview",
    )?;
    let image = result
        .images
        .first()
        .ok_or("missing decoded OpenRouter image")?;
    if result.provider_id != "openrouter"
        || result.model != "google/gemini-2.5-flash-image-preview"
        || !result
            .request_id
            .as_deref()
            .is_some_and(|value| value.starts_with("request_sha256_"))
        || serde_json::to_value(&result.usage)? != json!({"total_tokens": 12})
        || image.index != 0
        || image.mime_type.as_str() != "image/png"
        || image.bytes != b"hello"
        || image.byte_len != 5
        || image.revised_prompt.as_deref() != Some("painted a tiny robot")
        || !image
            .provider_item_id
            .as_deref()
            .is_some_and(|value| value.starts_with("item_sha256_"))
    {
        return Err(format!("decoded OpenRouter image result drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn openrouter_image_generation_parser_normalizes_remote_only_output() -> Result<(), Box<dyn Error>>
{
    let result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({
                "choices": [{
                    "message": {
                        "images": [{
                            "mime_type": "image/png",
                            "image_url": {"url": "https://temporary.example/image.png"}
                        }]
                    }
                }]
            }),
        },
        "google/gemini-2.5-flash-image-preview",
    )?;
    if !result.images.is_empty() || result.remote_images.len() != 1 {
        return Err(format!("remote-only result shape drifted: {result:?}").into());
    }
    let rendered = format!("{result:?}");
    assert!(!rendered.contains("temporary.example"));
    assert!(!rendered.contains("image.png"));
    Ok(())
}

#[test]
fn openrouter_image_generation_parser_preserves_mixed_local_and_remote_output(
) -> Result<(), Box<dyn Error>> {
    let result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({
                "choices": [{
                    "id": "choice-mixed",
                    "message": {
                        "images": [
                            {"image_url": {"url": "data:image/webp;base64,aGk="}},
                            {
                                "mime_type": "image/png",
                                "image_url": {"url": "https://temporary.example/mixed.png"}
                            }
                        ]
                    }
                }]
            }),
        },
        "google/gemini-2.5-flash-image-preview",
    )?;
    assert_eq!(result.images.len(), 1);
    assert_eq!(result.remote_images.len(), 1);
    assert!(!format!("{result:?}").contains("temporary.example"));
    Ok(())
}

#[test]
fn openrouter_image_generation_client_uses_transport_and_default_model(
) -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<ImageGenerationRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = OpenRouterImageGenerationClient::new(
        "sk-or-test",
        "https://openrouter.ai/api/v1",
        "google/gemini-2.5-flash-image-preview",
        move |request: ImageGenerationRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(ImageGenerationHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({
                    "choices": [{
                        "message": {
                            "images": [{"image_url": {"url": "data:image/webp;base64,aGk="}}]
                        }
                    }]
                }),
            })
        },
    );

    let result = client.generate_image(ImageGenerationRequest::new("say hi"))?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let parts = captured
        .first()
        .ok_or("missing OpenRouter image generation request")?;
    let image = result
        .images
        .first()
        .ok_or("missing generated OpenRouter image")?;
    if parts.path != "/chat/completions"
        || parts.body.get("model") != Some(&json!("google/gemini-2.5-flash-image-preview"))
        || parts.body.get("messages") != Some(&json!([{"role": "user", "content": "say hi"}]))
        || image.bytes.as_slice() != b"hi"
        || image.mime_type.as_str() != "image/webp"
    {
        return Err(format!(
            "OpenRouter image generation client drifted: body={:?} result={result:?}",
            parts.body
        )
        .into());
    }
    Ok(())
}
