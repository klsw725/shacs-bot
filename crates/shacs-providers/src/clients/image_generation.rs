use crate::clients::codex_image_generation::codex_image_generation_client_from_config;
use crate::config::{ProviderConfig, ProvidersConfig};
use crate::error::ProviderError;
use crate::registry::{ProviderRegistry, ProviderSpec};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use regex::Regex;
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::env;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImage {
    pub index: usize,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub byte_len: usize,
    pub revised_prompt: Option<String>,
    pub provider_item_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageGenerationResult {
    pub provider_id: String,
    pub model: String,
    pub images: Vec<GeneratedImage>,
    pub usage: Option<Value>,
    pub request_id: Option<String>,
    pub provider_metadata: Map<String, Value>,
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
                let value = if key.eq_ignore_ascii_case("authorization")
                    || key.eq_ignore_ascii_case("chatgpt-account-id")
                {
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

#[derive(Debug, Clone, PartialEq)]
pub struct ImageGenerationHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

pub trait ImageGenerationHttpTransport: Send + Sync {
    fn post_json(
        &self,
        request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError>;
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

impl UreqImageGenerationHttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, DEFAULT_IMAGE_GENERATION_TIMEOUT)
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl ImageGenerationHttpTransport for UreqImageGenerationHttpTransport {
    fn post_json(
        &self,
        request: ImageGenerationRequestParts,
    ) -> Result<ImageGenerationHttpResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body)
            .map_err(|error| api_error(None, error, false, BTreeMap::new(), None))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body_text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| api_error(Some(status), error, false, headers.clone(), None))?;
        Ok(ImageGenerationHttpResponse {
            status,
            headers,
            body: parse_http_body(body_text),
        })
    }
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

impl<T> OpenAiImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    pub fn new(
        api_key: impl Into<String>,
        api_base: impl Into<String>,
        default_model: impl Into<String>,
        transport: T,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_base: api_base.into(),
            default_model: default_model.into(),
            transport,
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }
}

impl<T> ImageGenerationClient for OpenAiImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        let model = request
            .model
            .as_deref()
            .and_then(non_empty_model)
            .unwrap_or(&self.default_model)
            .to_owned();
        let output_format = request
            .output_format
            .as_deref()
            .and_then(openai_output_format_mime_type);
        let parts = build_openai_image_generation_request(&self.api_key, &request, &model);
        parse_openai_image_generation_response_with_format(
            self.transport.post_json(parts)?,
            &model,
            output_format,
        )
    }
}

impl<T> OpenRouterImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    pub fn new(
        api_key: impl Into<String>,
        api_base: impl Into<String>,
        default_model: impl Into<String>,
        transport: T,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_base: api_base.into(),
            default_model: default_model.into(),
            transport,
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }
}

impl<T> ImageGenerationClient for OpenRouterImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        let model = request
            .model
            .as_deref()
            .and_then(non_empty_model)
            .unwrap_or(&self.default_model)
            .to_owned();
        let parts = build_openrouter_image_generation_request(&self.api_key, &request, &model);
        parse_openrouter_image_generation_response(self.transport.post_json(parts)?, &model)
    }
}

pub struct DefaultModelImageGenerationClient {
    default_model: String,
    inner: Box<dyn ImageGenerationClient>,
}

impl DefaultModelImageGenerationClient {
    pub fn new(default_model: impl Into<String>, inner: Box<dyn ImageGenerationClient>) -> Self {
        Self {
            default_model: default_model.into(),
            inner,
        }
    }
}

impl ImageGenerationClient for DefaultModelImageGenerationClient {
    fn generate_image(
        &self,
        mut request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        if request.model.as_deref().and_then(non_empty_model).is_none() {
            request.model = Some(self.default_model.clone());
        }
        self.inner.generate_image(request)
    }
}

pub struct ResolvedImageGenerationClient {
    pub provider_id: String,
    pub model: String,
    pub client: Box<dyn ImageGenerationClient>,
}

pub fn resolve_image_generation_client(
    registry: &ProviderRegistry,
    requested_provider: &str,
    model: &str,
    providers: &ProvidersConfig,
) -> Result<ResolvedImageGenerationClient, ProviderError> {
    let spec = if requested_provider == "auto" {
        resolve_auto_image_generation_provider(registry, providers)
            .ok_or_else(|| unsupported_image_generation(requested_provider))?
    } else {
        registry.find_by_name(requested_provider).ok_or_else(|| {
            ProviderError::ProviderNotFound {
                provider_id: requested_provider.to_owned(),
                suggestions: registry
                    .specs()
                    .iter()
                    .map(|spec| spec.name.to_owned())
                    .collect(),
            }
        })?
    };
    ensure_image_generation_supported(spec)?;
    let config = providers
        .get(spec.name)
        .cloned()
        .ok_or_else(|| ProviderError::AuthRequired {
            provider_id: spec.name.to_owned(),
        })?;
    let selected_model = default_image_generation_model(spec, model);
    let client = image_generation_client_from_config(spec.name, config)?;
    Ok(ResolvedImageGenerationClient {
        provider_id: spec.name.to_owned(),
        model: selected_model.to_owned(),
        client: Box::new(DefaultModelImageGenerationClient::new(
            selected_model,
            client,
        )),
    })
}

pub fn image_generation_client_from_config(
    provider_id: &str,
    config: ProviderConfig,
) -> Result<Box<dyn ImageGenerationClient>, ProviderError> {
    match provider_id {
        "openai_codex" => Ok(Box::new(codex_image_generation_client_from_config(config)?)),
        "openai" => Ok(Box::new(openai_image_generation_client_from_config(
            config,
        )?)),
        "openrouter" => Ok(Box::new(openrouter_image_generation_client_from_config(
            config,
        )?)),
        other => Err(unsupported_image_generation(other)),
    }
}

pub fn openai_image_generation_client_from_config(
    config: ProviderConfig,
) -> Result<OpenAiImageGenerationClient<UreqImageGenerationHttpTransport>, ProviderError> {
    let api_key = api_key_from_config_or_env(&config, "OPENAI_API_KEY", "openai")?;
    let api_base = resolve_image_generation_api_base(
        config.api_base.as_deref(),
        env::var("OPENAI_IMAGE_GENERATION_BASE_URL").ok().as_deref(),
        DEFAULT_OPENAI_IMAGE_GENERATION_BASE,
    );
    Ok(OpenAiImageGenerationClient::new(
        api_key,
        api_base.clone(),
        OPENAI_IMAGE_GENERATION_DEFAULT_MODEL,
        UreqImageGenerationHttpTransport::new(api_base),
    ))
}

pub fn openai_image_generation_capability() -> ImageGenerationCapability {
    ImageGenerationCapability {
        provider_id: "openai".to_owned(),
        supported_actions: vec!["text_to_image".to_owned()],
        supported_formats: vec!["png".to_owned(), "jpeg".to_owned(), "webp".to_owned()],
        supported_size_policy: "provider_defined".to_owned(),
        default_model: OPENAI_IMAGE_GENERATION_DEFAULT_MODEL.to_owned(),
    }
}

pub fn openrouter_image_generation_client_from_config(
    config: ProviderConfig,
) -> Result<OpenRouterImageGenerationClient<UreqImageGenerationHttpTransport>, ProviderError> {
    let api_key = api_key_from_config_or_env(&config, "OPENROUTER_API_KEY", "openrouter")?;
    let api_base = resolve_image_generation_api_base(
        config.api_base.as_deref(),
        env::var("OPENROUTER_IMAGE_GENERATION_BASE_URL")
            .ok()
            .as_deref(),
        DEFAULT_OPENROUTER_IMAGE_GENERATION_BASE,
    );
    Ok(OpenRouterImageGenerationClient::new(
        api_key,
        api_base.clone(),
        OPENROUTER_IMAGE_GENERATION_DEFAULT_MODEL,
        UreqImageGenerationHttpTransport::new(api_base),
    ))
}

pub fn resolve_image_generation_api_base(
    configured: Option<&str>,
    env_override: Option<&str>,
    default_base: &str,
) -> String {
    configured
        .or(env_override)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base)
        .trim_end_matches('/')
        .to_owned()
}

pub fn build_openai_image_generation_request(
    api_key: &str,
    request: &ImageGenerationRequest,
    model: &str,
) -> ImageGenerationRequestParts {
    let mut body = request.provider_options.clone();
    body.insert("model".to_owned(), Value::String(model.to_owned()));
    body.insert("prompt".to_owned(), Value::String(request.prompt.clone()));
    if let Some(size) = non_empty_option(request.size.as_deref()) {
        body.insert("size".to_owned(), Value::String(size.to_owned()));
    }
    if let Some(quality) = non_empty_option(request.quality.as_deref()) {
        body.insert("quality".to_owned(), Value::String(quality.to_owned()));
    }
    if let Some(output_format) = non_empty_option(request.output_format.as_deref()) {
        body.insert(
            "output_format".to_owned(),
            Value::String(output_format.to_owned()),
        );
    }
    if let Some(background) = non_empty_option(request.background.as_deref()) {
        body.insert(
            "background".to_owned(),
            Value::String(background.to_owned()),
        );
    }
    if let Some(count) = request.count {
        body.insert("n".to_owned(), Value::Number(Number::from(count)));
    }
    ImageGenerationRequestParts {
        path: IMAGE_GENERATION_PATH.to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {api_key}"))]),
        body: Value::Object(body),
    }
}

pub fn build_openrouter_image_generation_request(
    api_key: &str,
    request: &ImageGenerationRequest,
    model: &str,
) -> ImageGenerationRequestParts {
    let mut body = Map::new();
    body.insert("model".to_owned(), Value::String(model.to_owned()));
    body.insert(
        "messages".to_owned(),
        Value::Array(vec![Value::Object(Map::from_iter([
            ("role".to_owned(), Value::String("user".to_owned())),
            ("content".to_owned(), Value::String(request.prompt.clone())),
        ]))]),
    );
    body.insert(
        "modalities".to_owned(),
        Value::Array(vec![
            Value::String("image".to_owned()),
            Value::String("text".to_owned()),
        ]),
    );
    body.insert("stream".to_owned(), Value::Bool(false));

    let mut image_config = request.provider_options.clone();
    if let Some(size) = non_empty_option(request.size.as_deref()) {
        image_config.insert("size".to_owned(), Value::String(size.to_owned()));
    }
    if let Some(quality) = non_empty_option(request.quality.as_deref()) {
        image_config.insert("quality".to_owned(), Value::String(quality.to_owned()));
    }
    if let Some(output_format) = non_empty_option(request.output_format.as_deref()) {
        image_config.insert(
            "output_format".to_owned(),
            Value::String(output_format.to_owned()),
        );
    }
    if let Some(background) = non_empty_option(request.background.as_deref()) {
        image_config.insert(
            "background".to_owned(),
            Value::String(background.to_owned()),
        );
    }
    if let Some(count) = request.count {
        image_config.insert("n".to_owned(), Value::Number(Number::from(count)));
    }
    if !image_config.is_empty() {
        body.insert("image_config".to_owned(), Value::Object(image_config));
    }

    ImageGenerationRequestParts {
        path: OPENROUTER_IMAGE_GENERATION_PATH.to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {api_key}"))]),
        body: Value::Object(body),
    }
}

pub fn parse_openai_image_generation_response(
    response: ImageGenerationHttpResponse,
    model: &str,
) -> Result<ImageGenerationResult, ProviderError> {
    parse_openai_image_generation_response_with_format(response, model, None)
}

fn parse_openai_image_generation_response_with_format(
    response: ImageGenerationHttpResponse,
    model: &str,
    output_format: Option<&'static str>,
) -> Result<ImageGenerationResult, ProviderError> {
    if !(200..300).contains(&response.status) {
        let message = error_message_from_body(&response.body);
        return Err(api_error(
            Some(response.status),
            message,
            is_retryable_status(response.status),
            response.headers,
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
        let mime_type = item_object
            .get("mime_type")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| output_format.map(str::to_owned))
            .unwrap_or_else(|| "image/png".to_owned());
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
                .map(str::to_owned),
        });
    }

    let request_id = response
        .headers
        .get("x-request-id")
        .or_else(|| response.headers.get("X-Request-Id"))
        .map(String::to_owned)
        .or_else(|| object.get("id").and_then(Value::as_str).map(str::to_owned));
    let mut provider_metadata = Map::new();
    if let Some(created) = object.get("created") {
        provider_metadata.insert("created".to_owned(), created.clone());
    }

    Ok(ImageGenerationResult {
        provider_id: "openai".to_owned(),
        model: model.to_owned(),
        images,
        usage: object.get("usage").cloned(),
        request_id,
        provider_metadata,
    })
}

pub fn parse_openrouter_image_generation_response(
    response: ImageGenerationHttpResponse,
    model: &str,
) -> Result<ImageGenerationResult, ProviderError> {
    if !(200..300).contains(&response.status) {
        let message = error_message_from_body(&response.body);
        return Err(api_error(
            Some(response.status),
            message,
            is_retryable_status(response.status),
            response.headers,
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
    let mut saw_remote_url = false;
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
                saw_remote_url = true;
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
                    .map(str::to_owned),
            });
        }
    }

    if images.is_empty() {
        if saw_remote_url {
            return Err(api_error(
                Some(response.status),
                "OpenRouter image generation returned remote image URLs, which are not persisted artifacts",
                false,
                BTreeMap::new(),
                None,
            ));
        }
        return Err(malformed_openrouter_response(
            Some(response.status),
            "missing generated image data URLs",
        ));
    }

    let request_id = response
        .headers
        .get("x-request-id")
        .or_else(|| response.headers.get("X-Request-Id"))
        .map(String::to_owned)
        .or_else(|| object.get("id").and_then(Value::as_str).map(str::to_owned));
    let mut provider_metadata = Map::new();
    if let Some(created) = object.get("created") {
        provider_metadata.insert("created".to_owned(), created.clone());
    }

    Ok(ImageGenerationResult {
        provider_id: "openrouter".to_owned(),
        model: model.to_owned(),
        images,
        usage: object.get("usage").cloned(),
        request_id,
        provider_metadata,
    })
}

fn default_image_generation_model<'a>(spec: &ProviderSpec, model: &'a str) -> &'a str {
    match (spec.name, non_empty_model(model)) {
        ("openrouter", None | Some(OPENAI_IMAGE_GENERATION_DEFAULT_MODEL)) => {
            OPENROUTER_IMAGE_GENERATION_DEFAULT_MODEL
        }
        (_, Some(model)) => model,
        _ => OPENAI_IMAGE_GENERATION_DEFAULT_MODEL,
    }
}

fn resolve_auto_image_generation_provider<'a>(
    registry: &'a ProviderRegistry,
    providers: &ProvidersConfig,
) -> Option<&'a ProviderSpec> {
    registry
        .find_by_name("openai")
        .filter(|spec| spec.supports_image_generation && providers.contains_key(spec.name))
        .or_else(|| {
            registry
                .specs()
                .iter()
                .find(|spec| spec.supports_image_generation && providers.contains_key(spec.name))
        })
}

fn ensure_image_generation_supported(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.supports_image_generation {
        return Ok(());
    }
    Err(unsupported_image_generation(spec.name))
}

fn unsupported_image_generation(provider_id: &str) -> ProviderError {
    ProviderError::UnsupportedCapability {
        provider_id: provider_id.to_owned(),
        capability: IMAGE_GENERATION_CAPABILITY.to_owned(),
    }
}

fn api_key_from_config_or_env(
    config: &ProviderConfig,
    env_key: &str,
    provider_id: &str,
) -> Result<String, ProviderError> {
    config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            env::var(env_key)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| ProviderError::AuthRequired {
            provider_id: provider_id.to_owned(),
        })
}

fn join_base_and_path(base_url: &str, path: &str) -> Result<String, ProviderError> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err(api_error(
            None,
            "missing OpenAI image generation base URL",
            false,
            BTreeMap::new(),
            None,
        ));
    }
    Ok(format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
}

fn response_headers(headers: &ureq::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_http_body(body: String) -> Value {
    if body.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&body).unwrap_or(Value::String(body))
}

fn map_ureq_error(error: ureq::Error) -> ProviderError {
    let retryable = matches!(
        error,
        ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Io(_)
    );
    let status = match error {
        ureq::Error::StatusCode(status) => Some(status),
        _ => None,
    };
    api_error(status, error, retryable, BTreeMap::new(), None)
}

fn error_message_from_body(body: &Value) -> String {
    body.get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .map(redact_sensitive_text)
        .unwrap_or_else(|| "OpenAI image generation request failed".to_owned())
}

fn redact_sensitive_text(message: &str) -> String {
    let mut redacted = message.to_owned();
    for (pattern, replacement) in [
        (r"sk-[A-Za-z0-9_-]+", "sk-[redacted]"),
        (r"(?i)bearer\s+[A-Za-z0-9._~+/=-]+", "Bearer [redacted]"),
        (r"(?i)(api[_ -]?key\s*[:=]\s*)[^\s,;]+", "${1}[redacted]"),
        (r"[A-Za-z0-9+/]{80,}={0,2}", "[redacted]"),
    ] {
        if let Ok(regex) = Regex::new(pattern) {
            redacted = regex.replace_all(&redacted, replacement).into_owned();
        }
    }
    redacted
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

fn malformed_openrouter_response(status: Option<u16>, message: &str) -> ProviderError {
    api_error(
        status,
        format!("OpenRouter image generation response was malformed: {message}"),
        false,
        BTreeMap::new(),
        None,
    )
}

fn decode_data_url(url: &str, status: u16) -> Result<(String, Vec<u8>), ProviderError> {
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
    Ok((mime_type.to_owned(), bytes))
}

fn api_error(
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

fn is_retryable_status(status: u16) -> bool {
    status == 408 || status == 409 || status == 429 || (500..600).contains(&status)
}

fn openai_output_format_mime_type(format: &str) -> Option<&'static str> {
    match format.trim().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn non_empty_model(model: &str) -> Option<&str> {
    let model = model.trim();
    if model.is_empty() {
        None
    } else {
        Some(model)
    }
}

fn non_empty_option(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
