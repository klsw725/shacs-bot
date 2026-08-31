use serde_json::{json, Map};
use shacs_providers::{
    build_codex_image_generation_request, build_openai_image_generation_request,
    build_openrouter_image_generation_request, find_by_name, image_generation_client_from_config,
    openai_compatible_client_from_config, openai_image_generation_capability,
    parse_openai_image_generation_response, parse_openrouter_image_generation_response,
    resolve_image_generation_api_base, resolve_image_generation_client, CodexImageGenerationClient,
    DefaultModelImageGenerationClient, GeneratedImage, ImageGenerationClient,
    ImageGenerationHttpResponse, ImageGenerationRequest, ImageGenerationRequestParts,
    ImageGenerationResult, OpenAiImageGenerationClient, OpenRouterImageGenerationClient,
    ProviderConfig, ProviderError, ProviderRegistry, ProvidersConfig,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::{Arc, Mutex};

struct CapturingImageGenerationClient {
    models: Arc<Mutex<Vec<Option<String>>>>,
}

impl ImageGenerationClient for CapturingImageGenerationClient {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.models
            .lock()
            .map_err(|error| ProviderError::Api {
                status: None,
                message: error.to_string(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            })?
            .push(request.model);
        Ok(ImageGenerationResult {
            provider_id: "test".to_owned(),
            model: "captured".to_owned(),
            images: vec![GeneratedImage {
                index: 0,
                mime_type: "image/png".to_owned(),
                bytes: vec![1],
                byte_len: 1,
                revised_prompt: None,
                provider_item_id: None,
            }],
            usage: None,
            request_id: None,
            provider_metadata: Map::new(),
        })
    }
}

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
fn codex_image_generation_request_builder_uses_codex_contract() -> Result<(), Box<dyn Error>> {
    let mut request = ImageGenerationRequest::new("paint a small robot");
    request.size = Some("1024x1024".to_owned());
    request.quality = Some("high".to_owned());
    request.output_format = Some("webp".to_owned());
    request.background = Some("opaque".to_owned());
    request.count = Some(1);
    let extra_headers = BTreeMap::from([
        ("ChatGPT-Account-Id".to_owned(), "acct-test".to_owned()),
        ("originator".to_owned(), "shacs-bot-test".to_owned()),
    ]);

    let parts = build_codex_image_generation_request(
        "oauth-test",
        &extra_headers,
        "turn-test",
        &request,
        "gpt-image-2",
    );

    if parts.path != "/codex/images/generations"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer oauth-test")
        || parts.headers.get("ChatGPT-Account-Id").map(String::as_str) != Some("acct-test")
        || parts
            .headers
            .get("x-codex-image-turn-id")
            .map(String::as_str)
            != Some("turn-test")
        || parts.headers.get("originator").map(String::as_str) != Some("shacs-bot-test")
        || parts.body
            != json!({
                "model": "gpt-image-2",
                "prompt": "paint a small robot",
                "size": "1024x1024",
                "quality": "high",
                "background": "opaque",
                "n": 1
            })
    {
        return Err(format!("Codex image request builder drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn codex_image_generation_request_debug_redacts_auth_identity() -> Result<(), Box<dyn Error>> {
    let parts = build_codex_image_generation_request(
        "oauth-secret",
        &BTreeMap::from([("ChatGPT-Account-Id".to_owned(), "acct-secret".to_owned())]),
        "turn-test",
        &ImageGenerationRequest::new("paint a small robot"),
        "gpt-image-2",
    );

    let debug = format!("{parts:?}");
    if debug.contains("oauth-secret")
        || debug.contains("acct-secret")
        || debug.matches("<redacted>").count() != 2
    {
        return Err(format!("Codex image auth identity leaked in debug output: {debug}").into());
    }
    Ok(())
}

#[test]
fn codex_image_generation_client_normalizes_codex_response() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<ImageGenerationRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = CodexImageGenerationClient::new(
        "oauth-test",
        BTreeMap::from([("ChatGPT-Account-Id".to_owned(), "acct-test".to_owned())]),
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
                headers: BTreeMap::from([(
                    "x-codex-imagegen-request-id".to_owned(),
                    "codex-req-1".to_owned(),
                )]),
                body: json!({"data": [{"b64_json": "aGk="}]}),
            })
        },
    );

    let result = client.generate_image(ImageGenerationRequest::new("say hi"))?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let parts = captured.first().ok_or("missing Codex image request")?;
    let image = result.images.first().ok_or("missing generated image")?;
    if parts.path != "/codex/images/generations"
        || parts
            .headers
            .get("x-codex-image-turn-id")
            .is_none_or(String::is_empty)
        || result.provider_id != "openai_codex"
        || result.request_id.as_deref() != Some("codex-req-1")
        || image.bytes.as_slice() != b"hi"
        || image.mime_type != "image/png"
    {
        return Err(
            format!("Codex image client drifted: parts={parts:?} result={result:?}").into(),
        );
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
        || result.request_id.as_deref() != Some("req-123")
        || result.usage != Some(json!({"total_tokens": 12}))
        || image.index != 0
        || image.mime_type != "image/webp"
        || image.bytes != b"hello"
        || image.byte_len != 5
        || image.revised_prompt.as_deref() != Some("paint a tiny robot")
        || image.provider_item_id.as_deref() != Some("img-1")
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
        || image.mime_type != "image/webp"
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
        || result.request_id.as_deref() != Some("req-or-123")
        || result.usage != Some(json!({"total_tokens": 12}))
        || image.index != 0
        || image.mime_type != "image/png"
        || image.bytes != b"hello"
        || image.byte_len != 5
        || image.revised_prompt.as_deref() != Some("painted a tiny robot")
        || image.provider_item_id.as_deref() != Some("choice-1")
    {
        return Err(format!("decoded OpenRouter image result drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn openrouter_image_generation_parser_rejects_remote_urls() -> Result<(), Box<dyn Error>> {
    let error = match parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({
                "choices": [{
                    "message": {
                        "images": [{
                            "image_url": {"url": "https://temporary.example/image.png"}
                        }]
                    }
                }]
            }),
        },
        "google/gemini-2.5-flash-image-preview",
    ) {
        Ok(value) => return Err(format!("remote URL unexpectedly parsed: {value:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status: Some(200),
            message,
            retryable: false,
            ..
        } if message.contains("remote image URLs") => {}
        other => return Err(format!("unexpected remote URL rejection: {other:?}").into()),
    }
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
        || image.mime_type != "image/webp"
    {
        return Err(format!(
            "OpenRouter image generation client drifted: body={:?} result={result:?}",
            parts.body
        )
        .into());
    }
    Ok(())
}
#[test]
fn default_model_image_generation_client_injects_missing_model() -> Result<(), Box<dyn Error>> {
    let models = Arc::new(Mutex::new(Vec::new()));
    let client = DefaultModelImageGenerationClient::new(
        "custom-image-model",
        Box::new(CapturingImageGenerationClient {
            models: models.clone(),
        }),
    );

    client.generate_image(ImageGenerationRequest::new("say hi"))?;

    let models = models.lock().map_err(|error| error.to_string())?;
    if models.as_slice() != [Some("custom-image-model".to_owned())] {
        return Err(format!("default model wrapper drifted: {models:?}").into());
    }
    Ok(())
}

#[test]
fn openai_image_generation_error_redacts_sensitive_message() -> Result<(), Box<dyn Error>> {
    let raw_image = "a".repeat(96);
    let error = match parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 401,
            headers: BTreeMap::new(),
            body: json!({
                "error": {
                    "message": format!(
                        "Incorrect API key provided: sk-secret-value with Bearer token-value and payload {raw_image}"
                    )
                }
            }),
        },
        "gpt-image-2",
    ) {
        Ok(value) => return Err(format!("provider error unexpectedly parsed: {value:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api { message, body, .. }
            if body.is_none()
                && !message.contains("sk-secret-value")
                && !message.contains("token-value")
                && !message.contains(raw_image.as_str())
                && message.contains("sk-[redacted]") => {}
        other => return Err(format!("unexpected sensitive error redaction: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn openai_image_generation_error_redacts_provider_body() -> Result<(), Box<dyn Error>> {
    let error = match parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 400,
            headers: BTreeMap::new(),
            body: json!({
                "error": {"message": "policy rejected"},
                "b64_json": "raw-image-payload",
                "api_key": "sk-secret-value"
            }),
        },
        "gpt-image-2",
    ) {
        Ok(value) => return Err(format!("provider error unexpectedly parsed: {value:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status: Some(400),
            message,
            body,
            ..
        } if message == "policy rejected" && body.is_none() => {}
        other => return Err(format!("unexpected provider error redaction: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn openai_image_generation_error_uses_generic_fallback() -> Result<(), Box<dyn Error>> {
    let error = match parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 500,
            headers: BTreeMap::new(),
            body: json!({"b64_json": "raw-image-payload", "api_key": "sk-secret-value"}),
        },
        "gpt-image-2",
    ) {
        Ok(value) => return Err(format!("provider error unexpectedly parsed: {value:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status: Some(500),
            message,
            body,
            retryable: true,
            ..
        } if message == "OpenAI image generation request failed" && body.is_none() => {}
        other => return Err(format!("unexpected provider error fallback: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn image_generation_resolver_rejects_unsupported_provider() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();
    let error =
        match resolve_image_generation_client(&registry, "anthropic", "gpt-image-2", &providers) {
            Ok(_) => return Err("unsupported provider unexpectedly resolved".into()),
            Err(error) => error,
        };
    match error {
        ProviderError::UnsupportedCapability {
            provider_id,
            capability,
        } if provider_id == "anthropic" && capability == "image_generation" => {}
        other => return Err(format!("unexpected unsupported provider error: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn image_generation_resolver_requires_configured_auth() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::new();
    let error =
        match resolve_image_generation_client(&registry, "openai", "gpt-image-2", &providers) {
            Ok(_) => return Err("missing provider config unexpectedly resolved".into()),
            Err(error) => error,
        };
    match error {
        ProviderError::AuthRequired { provider_id } if provider_id == "openai" => {}
        other => return Err(format!("unexpected missing auth error: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn image_generation_resolver_returns_selected_model() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("sk-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved =
        resolve_image_generation_client(&registry, "openai", "custom-image-model", &providers)?;
    if resolved.provider_id != "openai" || resolved.model != "custom-image-model" {
        return Err(format!(
            "unexpected resolver success result: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_resolver_accepts_codex_oauth_config() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let providers = ProvidersConfig::from([(
        "openai_codex".to_owned(),
        ProviderConfig {
            api_key: Some("oauth-test".to_owned()),
            extra_headers: Some(BTreeMap::from([(
                "ChatGPT-Account-Id".to_owned(),
                "acct-test".to_owned(),
            )])),
            ..ProviderConfig::default()
        },
    )]);

    let resolved =
        resolve_image_generation_client(&registry, "openai_codex", "gpt-image-2", &providers)?;

    if resolved.provider_id != "openai_codex" || resolved.model != "gpt-image-2" {
        return Err(format!(
            "unexpected Codex resolver result: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_resolver_returns_selected_openrouter_model() -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved = resolve_image_generation_client(
        &registry,
        "openrouter",
        "google/gemini-2.5-flash-image-preview",
        &providers,
    )?;
    if resolved.provider_id != "openrouter"
        || resolved.model != "google/gemini-2.5-flash-image-preview"
    {
        return Err(format!(
            "unexpected OpenRouter resolver success result: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_resolver_maps_openrouter_openai_default_to_openrouter_default(
) -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved =
        resolve_image_generation_client(&registry, "openrouter", "gpt-image-2", &providers)?;
    if resolved.provider_id != "openrouter" || resolved.model != "openai/gpt-5.4-image-2" {
        return Err(format!(
            "OpenRouter default model mapping drifted: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_auto_prefers_openai_when_openrouter_is_also_configured(
) -> Result<(), Box<dyn Error>> {
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );
    providers.insert(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some("sk-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved = resolve_image_generation_client(&registry, "auto", "gpt-image-2", &providers)?;
    if resolved.provider_id != "openai" || resolved.model != "gpt-image-2" {
        return Err(format!(
            "auto image resolver should prefer OpenAI: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn image_generation_auto_uses_openrouter_when_openai_is_unconfigured() -> Result<(), Box<dyn Error>>
{
    let registry = ProviderRegistry::new();
    let mut providers = ProvidersConfig::new();
    providers.insert(
        "openrouter".to_owned(),
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            ..ProviderConfig::default()
        },
    );

    let resolved = resolve_image_generation_client(&registry, "auto", "gpt-image-2", &providers)?;
    if resolved.provider_id != "openrouter" || resolved.model != "openai/gpt-5.4-image-2" {
        return Err(format!(
            "auto image resolver should fallback to OpenRouter: provider={} model={}",
            resolved.provider_id, resolved.model
        )
        .into());
    }
    Ok(())
}

#[test]
fn openrouter_chat_registry_remains_openai_compatible() -> Result<(), Box<dyn Error>> {
    let openrouter = find_by_name("openrouter").ok_or("openrouter spec missing")?;
    let client = openai_compatible_client_from_config(ProviderConfig::default(), openrouter)?;
    if openrouter.backend != "openai_compat"
        || !openrouter.supports_image_generation
        || client.transport().base_url() != "https://openrouter.ai/api/v1"
    {
        return Err(format!(
            "OpenRouter registry drifted: backend={} image={} base={}",
            openrouter.backend,
            openrouter.supports_image_generation,
            client.transport().base_url()
        )
        .into());
    }
    Ok(())
}
#[test]
fn image_generation_factory_requires_api_key() -> Result<(), Box<dyn Error>> {
    let previous = std::env::var("OPENAI_API_KEY").ok();
    std::env::remove_var("OPENAI_API_KEY");
    let error = match image_generation_client_from_config("openai", ProviderConfig::default()) {
        Ok(_) => return Err("missing api key unexpectedly created a client".into()),
        Err(error) => error,
    };
    if let Some(previous) = previous {
        std::env::set_var("OPENAI_API_KEY", previous);
    }
    match error {
        ProviderError::AuthRequired { provider_id } if provider_id == "openai" => {}
        other => return Err(format!("unexpected missing api key error: {other:?}").into()),
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
