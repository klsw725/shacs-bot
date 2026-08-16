#[path = "image_generation_security/support.rs"]
mod support;

use self::support::{
    assert_no_provider_secret, serve_oversized_json, EXPECTED_RESPONSE_BODY_LIMIT,
};
use serde_json::{json, Value};
use shacs_providers::{
    parse_openai_image_generation_response, parse_openrouter_image_generation_response,
    ImageGenerationHttpResponse, ImageGenerationHttpTransport, ImageGenerationRequestParts,
    ImageMultipartRequestParts, ProviderError, UreqImageGenerationHttpTransport,
};
use std::collections::BTreeMap;
use std::error::Error;

#[test]
fn success_metadata_is_bounded_and_contains_only_numeric_accounting() -> Result<(), Box<dyn Error>>
{
    // Given
    let signed_id = "https://provider.example/request?token=query-secret&signature=signed-secret";
    let response = ImageGenerationHttpResponse {
        status: 200,
        headers: BTreeMap::from([
            ("x-request-id".to_owned(), signed_id.to_owned()),
            ("Set-Cookie".to_owned(), "session=cookie-secret".to_owned()),
        ]),
        body: json!({
            "created": "https://provider.example/created?token=created-secret",
            "usage": {
                "input_tokens": 3,
                "output_tokens": 5,
                "total_tokens": 8,
                "nested": {"token": "nested-secret"},
                "string_tokens": "credential-secret",
                "url": "https://provider.example/usage?token=usage-secret"
            },
            "data": [{"b64_json": "aW1hZ2U="}]
        }),
    };

    // When
    let result = parse_openai_image_generation_response(response, "gpt-image-2")?;
    let rendered = format!("{result:?}");

    // Then
    let request_id = result
        .request_id
        .as_deref()
        .ok_or("missing safe request id")?;
    assert!(request_id.starts_with("request_sha256_"));
    assert_eq!(request_id.len(), "request_sha256_".len() + 64);
    assert_eq!(
        serde_json::to_value(&result.usage)?,
        json!({
            "input_tokens": 3,
            "output_tokens": 5,
            "total_tokens": 8
        })
    );
    assert_no_provider_secret(&rendered);
    Ok(())
}

#[test]
fn ordinary_request_id_is_digested_and_numeric_usage_is_preserved() -> Result<(), Box<dyn Error>> {
    // Given
    let response = ImageGenerationHttpResponse {
        status: 200,
        headers: BTreeMap::from([("x-request-id".to_owned(), "req-ordinary_123".to_owned())]),
        body: json!({
            "usage": {"prompt_tokens": 2, "completion_tokens": 7, "total_tokens": 9},
            "choices": [{
                "message": {"images": [{"image_url": {"url": "data:image/png;base64,aQ=="}}]}
            }]
        }),
    };

    // When
    let result = parse_openrouter_image_generation_response(response, "image-model")?;

    // Then
    let request_id = result.request_id.as_deref().ok_or("request ID missing")?;
    assert!(request_id.starts_with("request_sha256_"));
    assert!(!request_id.contains("req-ordinary_123"));
    assert_eq!(
        serde_json::to_value(&result.usage)?,
        json!({
            "prompt_tokens": 2,
            "completion_tokens": 7,
            "total_tokens": 9
        })
    );
    Ok(())
}

#[test]
fn signed_body_request_id_is_digested_when_header_is_absent() -> Result<(), Box<dyn Error>> {
    // Given
    let signed_id = "https://provider.example/body-id?token=body-id-secret";
    let response = ImageGenerationHttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: json!({
            "id": signed_id,
            "choices": [{
                "message": {"images": [{"image_url": {"url": "data:image/png;base64,aQ=="}}]}
            }]
        }),
    };

    // When
    let result = parse_openrouter_image_generation_response(response, "image-model")?;

    // Then
    let request_id = result
        .request_id
        .as_deref()
        .ok_or("missing safe request id")?;
    assert!(request_id.starts_with("request_sha256_"));
    assert!(!format!("{result:?}").contains("body-id-secret"));
    Ok(())
}

#[test]
fn credential_shaped_request_id_is_digested() -> Result<(), Box<dyn Error>> {
    // Given
    let response = ImageGenerationHttpResponse {
        status: 200,
        headers: BTreeMap::from([("x-request-id".to_owned(), "sk-provider-secret".to_owned())]),
        body: json!({"data": [{"b64_json": "aQ=="}]}),
    };

    // When
    let result = parse_openai_image_generation_response(response, "gpt-image-2")?;

    // Then
    let request_id = result
        .request_id
        .as_deref()
        .ok_or("missing safe request id")?;
    assert!(request_id.starts_with("request_sha256_"));
    assert!(!format!("{result:?}").contains("sk-provider-secret"));
    Ok(())
}

#[test]
fn provider_error_exposes_only_stable_status_and_retryability() -> Result<(), Box<dyn Error>> {
    // Given
    let signed_url = "https://provider.example/error?token=body-token&signature=body-signature";
    let response = ImageGenerationHttpResponse {
        status: 503,
        headers: BTreeMap::from([
            ("Location".to_owned(), signed_url.to_owned()),
            ("Set-Cookie".to_owned(), "session=cookie-secret".to_owned()),
        ]),
        body: json!({
            "error": {
                "code": "provider-secret-code",
                "message": format!("download {signed_url} with credential=body-secret")
            }
        }),
    };

    // When
    let error = parse_openai_image_generation_response(response, "gpt-image-2")
        .expect_err("non-2xx response must fail");
    let debug = format!("{error:?}");
    let display = error.to_string();

    // Then
    assert_no_provider_secret(&debug);
    assert_no_provider_secret(&display);
    match error {
        ProviderError::Api {
            status: Some(503),
            message,
            retryable: true,
            headers,
            body: None,
        } if message == "image_generation_provider_error" && headers.is_empty() => Ok(()),
        other => Err(format!("unexpected safe provider error: {other:?}").into()),
    }
}

#[test]
fn transport_rejects_response_before_reading_past_hard_body_limit() -> Result<(), Box<dyn Error>> {
    // Given
    let (base_url, handle) = serve_oversized_json(EXPECTED_RESPONSE_BODY_LIMIT + 1)?;
    let transport = UreqImageGenerationHttpTransport::new(base_url);

    // When
    let error = transport
        .post_json(ImageGenerationRequestParts {
            path: "/images/generations".to_owned(),
            headers: BTreeMap::new(),
            body: Value::Object(Default::default()),
        })
        .expect_err("oversized response must fail at the transport boundary");

    // Then
    handle.join().map_err(|_| "fixture server panicked")??;
    match error {
        ProviderError::Api {
            status: Some(200),
            message,
            retryable: false,
            headers,
            body: None,
        } if message == "image_generation_response_body_too_large" && headers.is_empty() => Ok(()),
        other => Err(format!("unexpected oversized response error: {other:?}").into()),
    }
}

#[test]
fn multipart_transport_uses_the_same_hard_response_body_limit() -> Result<(), Box<dyn Error>> {
    // Given
    let (base_url, handle) = serve_oversized_json(EXPECTED_RESPONSE_BODY_LIMIT + 1)?;
    let transport = UreqImageGenerationHttpTransport::new(base_url);

    // When
    let error = transport
        .post_multipart(ImageMultipartRequestParts {
            path: "/images/edits".to_owned(),
            headers: BTreeMap::new(),
            content_type: "multipart/form-data; boundary=safe".to_owned(),
            body: b"--safe--\r\n".to_vec(),
        })
        .expect_err("oversized multipart response must fail at the transport boundary");

    // Then
    handle.join().map_err(|_| "fixture server panicked")??;
    match error {
        ProviderError::Api {
            status: Some(200),
            message,
            retryable: false,
            headers,
            body: None,
        } if message == "image_generation_response_body_too_large" && headers.is_empty() => Ok(()),
        other => Err(format!("unexpected multipart response error: {other:?}").into()),
    }
}
