pub use crate::clients::image_generation_contract::{
    ImageGenerationItemId, ImageGenerationRequestId, ImageGenerationUsage, ImageMimeType,
    IMAGE_GENERATION_PROVIDER_ERROR_CODE, IMAGE_GENERATION_RESPONSE_BODY_TOO_LARGE_CODE,
    IMAGE_GENERATION_RESPONSE_MAX_BYTES, IMAGE_GENERATION_RESPONSE_READ_LIMIT,
};

mod clients;
mod helpers;
mod parsing;
mod requests;
mod resolution;
mod transport;

use crate::clients::image_operations::{
    ImageMultipartRequestParts, ImageOperationRequest, ImageOperationResult,
};
use crate::error::ProviderError;
use crate::{
    ProviderEvent, ProviderInvocation, ProviderMediaCandidateId, ProviderMediaOrigin,
    ProviderRemoteMedia, ProviderRemoteMediaCandidate,
};
use helpers::{
    api_error, is_retryable_status, non_empty_model, non_empty_option,
    openai_output_format_mime_type,
};
pub use parsing::{
    parse_openai_image_generation_response, parse_openrouter_image_generation_response,
};
pub use requests::{
    build_openai_image_generation_request, build_openrouter_image_generation_request,
};
pub use resolution::{
    image_generation_client_from_config, openai_image_generation_capability,
    openai_image_generation_client_from_config, openrouter_image_generation_client_from_config,
    resolve_image_generation_api_base, resolve_image_generation_client,
    resolve_image_generation_client_with_request, resolve_image_generation_provider,
    ImageGenerationResolutionRequest,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

const DEFAULT_IMAGE_GENERATION_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_OPENAI_IMAGE_GENERATION_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_OPENROUTER_IMAGE_GENERATION_BASE: &str = "https://openrouter.ai/api/v1";
const IMAGE_GENERATION_PATH: &str = "/images/generations";
const OPENROUTER_IMAGE_GENERATION_PATH: &str = "/chat/completions";
const IMAGE_GENERATION_CAPABILITY: &str = "image_generation";
const OPENAI_IMAGE_GENERATION_DEFAULT_MODEL: &str = "gpt-image-2";
const OPENROUTER_IMAGE_GENERATION_DEFAULT_MODEL: &str = "openai/gpt-5.4-image-2";

#[derive(Debug, Clone, PartialEq)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub output_format: Option<String>,
    pub background: Option<String>,
    pub count: Option<u32>,
    pub provider_options: Map<String, Value>,
}

impl ImageGenerationRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            size: None,
            quality: None,
            output_format: None,
            background: None,
            count: None,
            provider_options: Map::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GeneratedImage {
    pub index: usize,
    pub mime_type: ImageMimeType,
    pub bytes: Vec<u8>,
    pub byte_len: usize,
    pub revised_prompt: Option<String>,
    pub provider_item_id: Option<ImageGenerationItemId>,
}

impl fmt::Debug for GeneratedImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeneratedImage")
            .field("index", &self.index)
            .field("mime_type", &self.mime_type)
            .field("byte_len", &self.byte_len)
            .field(
                "revised_prompt",
                &self.revised_prompt.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "provider_item_id",
                &self.provider_item_id.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageGenerationResult {
    pub provider_id: String,
    pub model: String,
    pub images: Vec<GeneratedImage>,
    pub remote_images: Vec<ProviderRemoteMediaCandidate>,
    pub usage: Option<ImageGenerationUsage>,
    pub request_id: Option<ImageGenerationRequestId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageGenerationCapability {
    pub provider_id: String,
    pub supported_actions: Vec<String>,
    pub supported_formats: Vec<String>,
    pub supported_size_policy: String,
    pub default_model: String,
}

pub trait ImageGenerationClient: Send + Sync {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError>;

    fn generate_image_with_observer(
        &self,
        request: ImageGenerationRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.generate_image(request)
    }

    fn generate_image_with_invocation(
        &self,
        request: ImageGenerationRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        _invocation: &ProviderInvocation,
    ) -> Result<ImageGenerationResult, ProviderError> {
        self.generate_image_with_observer(request, on_event)
    }

    fn execute_image_operation(
        &self,
        request: ImageOperationRequest,
    ) -> Result<ImageOperationResult, ProviderError> {
        match request {
            ImageOperationRequest::Generate(request) => self
                .generate_image(request)
                .map(ImageOperationResult::Generate),
            other => Err(ProviderError::UnsupportedCapability {
                provider_id: "unknown".to_owned(),
                capability: other.operation().capability_name().to_owned(),
            }),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct ImageGenerationRequestParts {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

impl fmt::Debug for ImageGenerationRequestParts {
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
            .debug_struct("ImageGenerationRequestParts")
            .field("path", &self.path)
            .field("headers", &headers)
            .field("body", &self.body)
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct ImageGenerationHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

impl fmt::Debug for ImageGenerationHttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageGenerationHttpResponse")
            .field("status", &self.status)
            .field("headers", &"<redacted>")
            .field("body", &"<redacted>")
            .finish()
    }
}

pub trait ImageGenerationHttpTransport: Send + Sync {
    fn post_json(
        &self,
        request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError>;

    fn post_multipart(
        &self,
        _request: ImageMultipartRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        Err(ProviderError::UnsupportedCapability {
            provider_id: "transport".to_owned(),
            capability: "multipart".to_owned(),
        })
    }
}

impl<F> ImageGenerationHttpTransport for F
where
    F: Fn(ImageGenerationRequestParts) -> Result<ImageGenerationHttpResponse, ProviderError>
        + Send
        + Sync,
{
    fn post_json(
        &self,
        request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        self(request)
    }
}

#[derive(Clone)]
pub struct UreqImageGenerationHttpTransport {
    base_url: String,
    agent: ureq::Agent,
}

#[derive(Clone)]
pub struct OpenAiImageGenerationClient<T> {
    api_key: String,
    api_base: String,
    default_model: String,
    transport: T,
}

#[derive(Clone)]
pub struct OpenRouterImageGenerationClient<T> {
    api_key: String,
    api_base: String,
    default_model: String,
    transport: T,
}

pub struct DefaultModelImageGenerationClient {
    default_model: String,
    inner: Box<dyn ImageGenerationClient>,
}

pub struct ResolvedImageGenerationClient {
    pub provider_id: String,
    pub model: String,
    pub client: Box<dyn ImageGenerationClient>,
}
