use serde_json::json;
use shacs_providers::{
    parse_openai_image_generation_response, parse_openrouter_image_generation_response,
    ImageGenerationHttpResponse, ProviderError,
};
use std::collections::BTreeMap;
use std::error::Error;

#[test]
fn openai_image_generation_error_replaces_sensitive_message() -> Result<(), Box<dyn Error>> {
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
        ProviderError::Api {
            message,
            headers,
            body,
            ..
        } if message == "image_generation_provider_error"
            && headers.is_empty()
            && body.is_none() => {}
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
        } if message == "image_generation_provider_error" && body.is_none() => {}
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
        } if message == "image_generation_provider_error" && body.is_none() => {}
        other => return Err(format!("unexpected provider error fallback: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn openrouter_remote_media_error_projects_only_safe_facts() -> Result<(), Box<dyn Error>> {
    // Given
    let signed_url = "https://cdn.example/private.png?token=url-secret&signature=raw-signature";
    let error = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 503,
            headers: BTreeMap::from([
                ("Location".to_owned(), signed_url.to_owned()),
                ("Set-Cookie".to_owned(), "session=header-secret".to_owned()),
            ]),
            body: json!({
                "error": {"message": "download failed token=body-secret"},
                "url": signed_url,
                "raw": "provider-body-secret"
            }),
        },
        "image-model",
    )
    .expect_err("non-2xx provider response");

    // When
    let debug = format!("{error:?}");
    let display = error.to_string();

    // Then
    for projection in [&debug, &display] {
        for forbidden in [
            signed_url,
            "url-secret",
            "raw-signature",
            "header-secret",
            "body-secret",
            "provider-body-secret",
            "Set-Cookie",
            "Location",
        ] {
            assert!(
                !projection.contains(forbidden),
                "unsafe error: {projection}"
            );
        }
    }
    match error {
        ProviderError::Api {
            status: Some(503),
            message,
            retryable: true,
            headers,
            body: None,
        } if message == "image_generation_provider_error" && headers.is_empty() => {}
        other => return Err(format!("unexpected safe error projection: {other:?}").into()),
    }
    Ok(())
}
