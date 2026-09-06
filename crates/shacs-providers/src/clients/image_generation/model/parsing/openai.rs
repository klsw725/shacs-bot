use super::super::{
    api_error, is_retryable_status, GeneratedImage, ImageGenerationHttpResponse,
    ImageGenerationItemId, ImageGenerationRequestId, ImageGenerationResult, ImageGenerationUsage,
    ImageMimeType, IMAGE_GENERATION_PROVIDER_ERROR_CODE,
};
use crate::error::ProviderError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn parse_openai_image_generation_response(
    response: ImageGenerationHttpResponse,
    model: &str,
) -> Result<ImageGenerationResult, ProviderError> {
    parse_openai_image_generation_response_with_format(response, model, None)
}

pub(crate) fn parse_openai_image_generation_response_with_format(
    response: ImageGenerationHttpResponse,
    model: &str,
    output_format: Option<ImageMimeType>,
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
        malformed_response(
            Some(response.status),
            "OpenAI image generation response must be a JSON object",
        )
    })?;
    let data = object
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed_response(Some(response.status), "missing image data array"))?;
    if data.is_empty() {
        return Err(malformed_response(
            Some(response.status),
            "image data array is empty",
        ));
    }
    let mut images = Vec::with_capacity(data.len());
    for (index, item) in data.iter().enumerate() {
        let item_object = item.as_object().ok_or_else(|| {
            malformed_response(Some(response.status), "image data item must be an object")
        })?;
        let encoded = item_object
            .get("b64_json")
            .or_else(|| item_object.get("base64"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                malformed_response(
                    Some(response.status),
                    "image data item is missing base64 data",
                )
            })?;
        let bytes = STANDARD.decode(encoded).map_err(|error| {
            api_error(
                Some(response.status),
                format!("OpenAI image generation base64 decode failed: {error}"),
                false,
                BTreeMap::new(),
                None,
            )
        })?;
        let mime_type = match item_object.get("mime_type").and_then(Value::as_str) {
            Some(value) => ImageMimeType::parse_provider(value).ok_or_else(|| {
                malformed_response(Some(response.status), "unsupported image MIME type")
            })?,
            None => output_format.unwrap_or(ImageMimeType::Png),
        };
        let byte_len = bytes.len();
        images.push(GeneratedImage {
            index,
            mime_type,
            bytes,
            byte_len,
            revised_prompt: item_object
                .get("revised_prompt")
                .and_then(Value::as_str)
                .map(str::to_owned),
            provider_item_id: item_object
                .get("id")
                .and_then(Value::as_str)
                .map(ImageGenerationItemId::from_provider),
        });
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
        provider_id: "openai".to_owned(),
        model: model.to_owned(),
        images,
        remote_images: Vec::new(),
        usage: object
            .get("usage")
            .and_then(ImageGenerationUsage::from_provider),
        request_id,
    })
}

fn malformed_response(status: Option<u16>, message: &str) -> ProviderError {
    api_error(
        status,
        format!("OpenAI image generation response was malformed: {message}"),
        false,
        BTreeMap::new(),
        None,
    )
}
