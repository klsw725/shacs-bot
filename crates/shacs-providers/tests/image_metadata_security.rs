use serde_json::json;
use shacs_providers::{
    parse_openai_image_generation_response, parse_openrouter_image_generation_response,
    ImageGenerationHttpResponse,
};
use std::collections::BTreeMap;
use std::error::Error;

const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJwcm92aWRlciJ9.signature";
const GITHUB_TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
const AWS_ACCESS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const SIGNED_ITEM_ID: &str = "https://provider.example/item?token=item-secret&sig=item-signature";

#[test]
fn every_request_id_is_a_fixed_opaque_digest() -> Result<(), Box<dyn Error>> {
    // Given / When / Then
    for raw in ["req-ordinary_123", JWT, GITHUB_TOKEN, AWS_ACCESS_KEY] {
        let result = parse_openai_image_generation_response(
            ImageGenerationHttpResponse {
                status: 200,
                headers: BTreeMap::from([("x-request-id".to_owned(), raw.to_owned())]),
                body: json!({"data": [{"b64_json": "aQ=="}]}),
            },
            "gpt-image-2",
        )?;
        let projected = result.request_id.as_deref().ok_or("request ID missing")?;
        assert!(projected.starts_with("request_sha256_"));
        assert_eq!(projected.len(), "request_sha256_".len() + 64);
        assert!(!format!("{result:?}").contains(raw));
    }
    Ok(())
}

#[test]
fn provider_item_id_is_a_fixed_opaque_digest() -> Result<(), Box<dyn Error>> {
    // Given
    let response = ImageGenerationHttpResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: json!({
            "metadata": {"nested": SIGNED_ITEM_ID},
            "data": [{
                "id": SIGNED_ITEM_ID,
                "mime_type": "image/png",
                "b64_json": "aQ=="
            }]
        }),
    };

    // When
    let result = parse_openai_image_generation_response(response, "gpt-image-2")?;

    // Then
    let item_id = result.images[0]
        .provider_item_id
        .as_deref()
        .ok_or("provider item ID missing")?;
    assert!(item_id.starts_with("item_sha256_"));
    assert_eq!(item_id.len(), "item_sha256_".len() + 64);
    assert!(!format!("{result:?}").contains(SIGNED_ITEM_ID));
    Ok(())
}

#[test]
fn malicious_or_unknown_openai_mime_is_rejected_before_result() {
    // Given
    let malicious_mime = "image/png?token=mime-secret&url=https://provider.example/signed";

    // When
    let result = parse_openai_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"data": [{"mime_type": malicious_mime, "b64_json": "aQ=="}]}),
        },
        "gpt-image-2",
    );

    // Then
    let error = result.expect_err("malicious MIME must be rejected");
    assert!(!format!("{error:?}").contains("mime-secret"));
}

#[test]
fn huge_unknown_remote_mime_is_rejected_before_candidate() {
    // Given
    let huge_mime = format!("image/{}", "x".repeat(32 * 1024));

    // When
    let result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"choices": [{"message": {"images": [{
                "mime_type": huge_mime,
                "image_url": {"url": "https://provider.example/image"}
            }]}}]}),
        },
        "image-model",
    );

    // Then
    assert!(result.is_err(), "unknown MIME reached a remote candidate");
}

#[test]
fn non_image_data_url_mime_is_rejected_before_result() {
    // Given / When
    let result = parse_openrouter_image_generation_response(
        ImageGenerationHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"choices": [{"message": {"images": [{
                "image_url": {"url": "data:text/html;base64,aQ=="}
            }]}}]}),
        },
        "image-model",
    );

    // Then
    assert!(
        result.is_err(),
        "non-image MIME reached ImageGenerationResult"
    );
}
