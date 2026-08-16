use super::ImageMimeType;
use crate::error::ProviderError;
use std::collections::BTreeMap;

pub(super) fn api_error(
    status: Option<u16>,
    error: impl ToString,
    retryable: bool,
    headers: BTreeMap<String, String>,
    body: Option<String>,
) -> ProviderError {
    ProviderError::Api {
        status,
        message: error.to_string(),
        retryable,
        headers,
        body,
    }
}

pub(super) fn is_retryable_status(status: u16) -> bool {
    status == 408 || status == 409 || status == 429 || (500..600).contains(&status)
}

pub(super) fn openai_output_format_mime_type(format: &str) -> Option<ImageMimeType> {
    match format.trim().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some(ImageMimeType::Jpeg),
        "png" => Some(ImageMimeType::Png),
        "webp" => Some(ImageMimeType::Webp),
        _ => None,
    }
}

pub(super) fn non_empty_model(model: &str) -> Option<&str> {
    let model = model.trim();
    if model.is_empty() {
        None
    } else {
        Some(model)
    }
}

pub(super) fn non_empty_option(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
