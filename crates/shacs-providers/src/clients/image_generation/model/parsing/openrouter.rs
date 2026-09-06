use super::super::{
    api_error, is_retryable_status, GeneratedImage, ImageGenerationHttpResponse,
    ImageGenerationItemId, ImageGenerationRequestId, ImageGenerationResult, ImageGenerationUsage,
    ImageMimeType, ProviderMediaCandidateId, ProviderMediaOrigin, ProviderRemoteMedia,
    ProviderRemoteMediaCandidate, IMAGE_GENERATION_PROVIDER_ERROR_CODE,
};
use crate::error::ProviderError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn parse_openrouter_image_generation_response(
    response: ImageGenerationHttpResponse,
    model: &str,
) -> Result<ImageGenerationResult, ProviderError> {
    if !(200..300).contains(&response.status) {
        return Err(api_error(
            Some(response.status),
            IMAGE_GENERATION_PROVIDER_ERROR_CODE,
            is_retryable_status(response.status),
            BTreeMap::new(),
            None,
        ));
    }
    let object = response.body.as_object().ok_or_else(|| {
        malformed_openrouter_response(
            Some(response.status),
            "OpenRouter image generation response must be a JSON object",
        )
    })?;
    let choices = object
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            malformed_openrouter_response(Some(response.status), "missing choices array")
        })?;
    let mut images = Vec::new();
    let mut remote_images = Vec::new();
    for choice in choices {
        let Some(choice_object) = choice.as_object() else {
            continue;
        };
        let Some(message) = choice_object.get("message").and_then(Value::as_object) else {
            continue;
        };
        let Some(message_images) = message.get("images").and_then(Value::as_array) else {
            continue;
        };
        for item in message_images {
            let Some(url) = item
                .get("image_url")
                .and_then(|image_url| image_url.get("url"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !url.starts_with("data:") {
                let mime_type = item
                    .get("mime_type")
                    .and_then(Value::as_str)
                    .map_or(Some(ImageMimeType::Png), ImageMimeType::parse_provider)
                    .ok_or_else(|| {
                        malformed_openrouter_response(
                            Some(response.status),
                            "unsupported image MIME type",
                        )
                    })?;
                let candidate_id =
                    ProviderMediaCandidateId::new(format!("remote_image_{}", remote_images.len()))
                        .map_err(|error| {
                            api_error(Some(response.status), error, false, BTreeMap::new(), None)
                        })?;
                remote_images.push(ProviderRemoteMediaCandidate::new(
                    candidate_id,
                    ProviderMediaOrigin::new("openrouter", model),
                    ProviderRemoteMedia::new(mime_type.as_str(), url),
                ));
                continue;
            }
            let (mime_type, bytes) = decode_data_url(url, response.status)?;
            let byte_len = bytes.len();
            images.push(GeneratedImage {
                index: images.len(),
                mime_type,
                bytes,
                byte_len,
                revised_prompt: message
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                provider_item_id: choice_object
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ImageGenerationItemId::from_provider),
            });
        }
    }
    if images.is_empty() && remote_images.is_empty() {
        return Err(malformed_openrouter_response(
            Some(response.status),
            "missing generated image output",
        ));
    }
    let request_id = response
        .headers
        .get("x-request-id")
        .or_else(|| response.headers.get("X-Request-Id"))
        .map(String::as_str)
        .or_else(|| object.get("id").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(ImageGenerationRequestId::from_provider);
    Ok(ImageGenerationResult {
        provider_id: "openrouter".to_owned(),
        model: model.to_owned(),
        images,
        remote_images,
        usage: object
            .get("usage")
            .and_then(ImageGenerationUsage::from_provider),
        request_id,
    })
}

fn malformed_openrouter_response(status: Option<u16>, message: &str) -> ProviderError {
    api_error(
        status,
        format!("OpenRouter image generation response was malformed: {message}"),
        false,
        BTreeMap::new(),
        None,
    )
}

fn decode_data_url(url: &str, status: u16) -> Result<(ImageMimeType, Vec<u8>), ProviderError> {
    let (metadata, encoded) = url.split_once(',').ok_or_else(|| {
        malformed_openrouter_response(Some(status), "data URL is missing payload")
    })?;
    let mime_type = metadata
        .strip_prefix("data:")
        .and_then(|metadata| metadata.split(';').next())
        .filter(|mime_type| !mime_type.is_empty())
        .ok_or_else(|| {
            malformed_openrouter_response(Some(status), "data URL is missing mime type")
        })?;
    let mime_type = ImageMimeType::parse_provider(mime_type).ok_or_else(|| {
        malformed_openrouter_response(Some(status), "unsupported image MIME type")
    })?;
    if !metadata
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        return Err(malformed_openrouter_response(
            Some(status),
            "data URL is not base64 encoded",
        ));
    }
    let bytes = STANDARD.decode(encoded).map_err(|error| {
        api_error(
            Some(status),
            format!("OpenRouter image generation data URL decode failed: {error}"),
            false,
            BTreeMap::new(),
            None,
        )
    })?;
    Ok((mime_type, bytes))
}
