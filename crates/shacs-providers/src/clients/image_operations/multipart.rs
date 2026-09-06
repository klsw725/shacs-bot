use super::{ImageOperationContractError, ImageOperationOptions, ImageOperationRequest};
use crate::error::ProviderError;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

const OPENAI_IMAGE_EDIT_PATH: &str = "/images/edits";
const MAX_MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct ImageMultipartRequestParts {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl fmt::Debug for ImageMultipartRequestParts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers = self
            .headers
            .iter()
            .map(|(key, value)| {
                let value = if key.eq_ignore_ascii_case("authorization") {
                    "<redacted>".to_owned()
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect::<BTreeMap<_, _>>();
        formatter
            .debug_struct("ImageMultipartRequestParts")
            .field("path", &self.path)
            .field("headers", &headers)
            .field("content_type", &self.content_type)
            .field(
                "body",
                &format_args!("[REDACTED; {} bytes]", self.body.len()),
            )
            .finish()
    }
}

pub(crate) fn build_openai_multipart_request(
    api_key: &str,
    request: &ImageOperationRequest,
    model: &str,
) -> Result<ImageMultipartRequestParts, ProviderError> {
    let boundary = multipart_boundary();
    let mut encoder = MultipartEncoder::new(&boundary);
    match request {
        ImageOperationRequest::Edit(request) => {
            let (prompt, source, options) = request.parts();
            encoder.append_file("image", source);
            encoder.append_common_fields(prompt, model, options)?;
        }
        ImageOperationRequest::Mask(request) => {
            let (prompt, source, mask, options) = request.parts();
            encoder.append_file("image", source);
            encoder.append_file("mask", mask);
            encoder.append_common_fields(prompt, model, options)?;
        }
        ImageOperationRequest::Generate(_) | ImageOperationRequest::Variation(_) => {
            return Err(unsupported("openai", request));
        }
    }
    let body = encoder.finish();
    let payload_bytes = match request {
        ImageOperationRequest::Edit(request) => request.parts().1.parts().2.len(),
        ImageOperationRequest::Mask(request) => {
            request.parts().1.parts().2.len() + request.parts().2.parts().2.len()
        }
        ImageOperationRequest::Generate(_) | ImageOperationRequest::Variation(_) => 0,
    };
    if body.len() > payload_bytes.saturating_add(MAX_MULTIPART_OVERHEAD_BYTES) {
        return Err(contract_error(
            ImageOperationContractError::PayloadTooLarge {
                byte_len: body.len(),
                max_bytes: payload_bytes.saturating_add(MAX_MULTIPART_OVERHEAD_BYTES),
            },
        ));
    }
    Ok(ImageMultipartRequestParts {
        path: OPENAI_IMAGE_EDIT_PATH.to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {api_key}"))]),
        content_type: format!("multipart/form-data; boundary={boundary}"),
        body,
    })
}

struct MultipartEncoder<'a> {
    boundary: &'a str,
    body: Vec<u8>,
}

impl<'a> MultipartEncoder<'a> {
    fn new(boundary: &'a str) -> Self {
        Self {
            boundary,
            body: Vec::new(),
        }
    }

    fn append_common_fields(
        &mut self,
        prompt: &str,
        model: &str,
        options: &ImageOperationOptions,
    ) -> Result<(), ProviderError> {
        for (name, value) in &options.provider_options {
            if !valid_provider_option_name(name) {
                return Err(contract_error(ImageOperationContractError::MalformedPart));
            }
            let value = match value {
                Value::String(value) => value.clone(),
                Value::Bool(value) => value.to_string(),
                Value::Number(value) => value.to_string(),
                Value::Null | Value::Array(_) | Value::Object(_) => {
                    return Err(contract_error(ImageOperationContractError::MalformedPart));
                }
            };
            self.append_text(name, &value);
        }
        self.append_text("model", model);
        self.append_text("prompt", prompt);
        for (name, value) in [
            ("size", options.size.as_deref()),
            ("quality", options.quality.as_deref()),
            ("output_format", options.output_format.as_deref()),
            ("background", options.background.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                self.append_text(name, value);
            }
        }
        if let Some(count) = options.count {
            self.append_text("n", &count.to_string());
        }
        Ok(())
    }

    fn append_text(&mut self, name: &str, value: &str) {
        self.body
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
    }

    fn append_file(&mut self, name: &str, file: &super::ImageFileInput) {
        let (file_name, mime_type, bytes) = file.parts();
        self.body
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: {mime_type}\r\n\r\n"
            )
            .as_bytes(),
        );
        self.body.extend_from_slice(bytes);
        self.body.extend_from_slice(b"\r\n");
    }

    fn finish(mut self) -> Vec<u8> {
        self.body
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        self.body
    }
}

fn multipart_boundary() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("shacs-image-{:x}-{nanos:x}", process::id())
}

fn valid_provider_option_name(name: &str) -> bool {
    !matches!(
        name,
        "image"
            | "mask"
            | "model"
            | "prompt"
            | "n"
            | "size"
            | "quality"
            | "output_format"
            | "background"
    ) && !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn unsupported(provider_id: &str, request: &ImageOperationRequest) -> ProviderError {
    ProviderError::UnsupportedCapability {
        provider_id: provider_id.to_owned(),
        capability: request.operation().capability_name().to_owned(),
    }
}

fn contract_error(error: ImageOperationContractError) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: error.to_string(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
