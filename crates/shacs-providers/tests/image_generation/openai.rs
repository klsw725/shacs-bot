use serde_json::json;
use shacs_providers::{
    build_openai_image_generation_request, openai_image_generation_capability,
    parse_openai_image_generation_response, resolve_image_generation_api_base,
    ImageGenerationClient, ImageGenerationHttpResponse, ImageGenerationRequest,
    ImageGenerationRequestParts, OpenAiImageGenerationClient, ProviderError, ProviderRegistry,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

#[test]
fn openai_image_generation_request_builder_serializes_options() -> Result<(), Box<dyn Error>> {
    let mut request = ImageGenerationRequest::new("paint a small robot");
    request.size = Some("1024x1024".to_owned());
    request.quality = Some("high".to_owned());
    request.output_format = Some("webp".to_owned());
    request.background = Some("transparent".to_owned());
    request.count = Some(2);
    request
        .provider_options
        .insert("moderation".to_owned(), json!("auto"));

    let parts = build_openai_image_generation_request("sk-test", &request, "gpt-image-2");
    if parts.path != "/images/generations"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer sk-test")
        || parts.body
            != json!({
                "model": "gpt-image-2",
                "prompt": "paint a small robot",
                "size": "1024x1024",
                "quality": "high",
                "output_format": "webp",
                "background": "transparent",
                "n": 2,
                "moderation": "auto"
            })
    {
        return Err("image request builder drifted".into());
    }
    Ok(())
}

#[test]
fn image_generation_request_parts_debug_redacts_authorization() -> Result<(), Box<dyn Error>> {
    let parts = build_openai_image_generation_request(
        "sk-secret-value",
        &ImageGenerationRequest::new("paint a small robot"),
        "gpt-image-2",
    );
    let debug = format!("{parts:?}");
    if debug.contains("sk-secret-value") || !debug.contains("<redacted>") {
        return Err(format!("authorization header leaked in debug output: {debug}").into());
    }
    Ok(())
}

#[test]
fn openai_image_generation_parser_decodes_base64_images() -> Result<(), Box<dyn Error>> {
    let result = parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::from([("x-request-id".to_owned(), "req-123".to_owned())]),
            body: json!({
                "created": 1710000000,
                "usage": {"total_tokens": 12},
                "data": [{
                    "id": "img-1",
                    "b64_json": "aGVsbG8=",
                    "mime_type": "image/webp",
                    "revised_prompt": "paint a tiny robot"
                }]
            }),
        },
        "gpt-image-2",
    )?;
    let image = result.images.first().ok_or("missing decoded image")?;
    if result.provider_id != "openai"
        || result.model != "gpt-image-2"
        || !result
            .request_id
            .as_deref()
            .is_some_and(|value| value.starts_with("request_sha256_"))
        || serde_json::to_value(&result.usage)? != json!({"total_tokens": 12})
        || image.index != 0
        || image.mime_type.as_str() != "image/webp"
        || image.bytes != b"hello"
        || image.byte_len != 5
        || image.revised_prompt.as_deref() != Some("paint a tiny robot")
        || !image
            .provider_item_id
            .as_deref()
            .is_some_and(|value| value.starts_with("item_sha256_"))
    {
        return Err(format!("decoded image result drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn openai_image_generation_client_uses_transport_and_default_model() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<ImageGenerationRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = OpenAiImageGenerationClient::new(
        "sk-test",
        "https://api.openai.com/v1",
        "gpt-image-2",
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
                body: json!({"data": [{"b64_json": "aGk="}]}),
            })
        },
    );

    let mut request = ImageGenerationRequest::new("say hi");
    request.output_format = Some("webp".to_owned());

    let result = client.generate_image(request)?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let parts = captured.first().ok_or("missing image generation request")?;
    let image = result.images.first().ok_or("missing generated image")?;
    if parts.body.get("model") != Some(&json!("gpt-image-2"))
        || parts.body.get("prompt") != Some(&json!("say hi"))
        || image.bytes.as_slice() != b"hi"
        || image.mime_type.as_str() != "image/webp"
    {
        return Err(format!(
            "image generation client drifted: body={:?} result={result:?}",
            parts.body
        )
        .into());
    }
    Ok(())
}

#[test]
fn openai_image_generation_parser_rejects_malformed_response() -> Result<(), Box<dyn Error>> {
    let error = match parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"data": [{"url": "https://temporary.example/image.png"}]}),
        },
        "gpt-image-2",
    ) {
        Ok(value) => {
            return Err(format!("malformed response unexpectedly parsed: {value:?}").into())
        }
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status: Some(200),
            message,
            retryable: false,
            ..
        } if message.contains("missing base64 data") => {}
        other => return Err(format!("unexpected malformed response error: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn openai_image_generation_parser_rejects_decode_failure() -> Result<(), Box<dyn Error>> {
    let error = match parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"data": [{"b64_json": "not base64"}]}),
        },
        "gpt-image-2",
    ) {
        Ok(value) => return Err(format!("invalid base64 unexpectedly parsed: {value:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status: Some(200),
            message,
            retryable: false,
            ..
        } if message.contains("base64 decode failed") => {}
        other => return Err(format!("unexpected decode failure error: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn openai_image_generation_metadata_exposes_only_openai_capability() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let openai = registry
        .find_by_name("openai")
        .ok_or("openai spec missing")?;
    let groq = registry.find_by_name("groq").ok_or("groq spec missing")?;
    let capability = openai_image_generation_capability();
    let resolved_base = resolve_image_generation_api_base(
        Some("https://example.test/v1/"),
        None,
        "https://fallback.test/v1",
    );
    if !openai.supports_image_generation
        || groq.supports_image_generation
        || capability.provider_id != "openai"
        || capability.default_model != "gpt-image-2"
        || resolved_base != "https://example.test/v1"
    {
        return Err(format!(
            "image generation metadata drifted: {openai:?} {groq:?} {capability:?} {resolved_base}"
        )
        .into());
    }
    Ok(())
}
