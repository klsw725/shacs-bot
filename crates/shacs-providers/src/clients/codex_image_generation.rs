use super::image_generation::{
    parse_openai_image_generation_response, ImageGenerationClient, ImageGenerationHttpTransport,
    ImageGenerationRequest, ImageGenerationRequestParts, ImageGenerationResult,
    UreqImageGenerationHttpTransport,
};
use crate::{ProviderConfig, ProviderError};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_CODEX_IMAGE_GENERATION_BASE: &str = "https://chatgpt.com/backend-api";
const CODEX_IMAGE_GENERATION_PATH: &str = "/codex/images/generations";
const CODEX_IMAGE_GENERATION_DEFAULT_MODEL: &str = "gpt-image-2";
const DEFAULT_ORIGINATOR: &str = "shacs-bot";
static TURN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct CodexImageGenerationClient<T> {
    access_token: String,
    extra_headers: BTreeMap<String, String>,
    default_model: String,
    transport: T,
}

impl<T> CodexImageGenerationClient<T>
where
    T: ImageGenerationHttpTransport,
{
    pub fn new(
        access_token: impl Into<String>,
        extra_headers: BTreeMap<String, String>,
        default_model: impl Into<String>,
        transport: T,
    ) -> Self {
        Self {
            access_token: access_token.into(),
            extra_headers,
            default_model: default_model.into(),
            transport,
        }
    }
}

impl<T> ImageGenerationClient for CodexImageGenerationClient<T>
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
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.default_model)
            .to_owned();
        let parts = build_codex_image_generation_request(
            &self.access_token,
            &self.extra_headers,
            &next_turn_id(),
            &request,
            &model,
        );
        let mut response = self.transport.post_json(parts)?;
        if let Some(request_id) = response.headers.get("x-codex-imagegen-request-id").cloned() {
            response
                .headers
                .insert("x-request-id".to_owned(), request_id);
        }
        let mut result = parse_openai_image_generation_response(response, &model)?;
        result.provider_id = "openai_codex".to_owned();
        Ok(result)
    }
}

pub fn codex_image_generation_client_from_config(
    config: ProviderConfig,
) -> Result<CodexImageGenerationClient<UreqImageGenerationHttpTransport>, ProviderError> {
    let access_token = config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ProviderError::AuthRequired {
            provider_id: "openai_codex".to_owned(),
        })?;
    let api_base = config
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CODEX_IMAGE_GENERATION_BASE)
        .trim_end_matches('/')
        .to_owned();
    Ok(CodexImageGenerationClient::new(
        access_token,
        config.extra_headers.unwrap_or_default(),
        CODEX_IMAGE_GENERATION_DEFAULT_MODEL,
        UreqImageGenerationHttpTransport::new(api_base),
    ))
}

pub fn build_codex_image_generation_request(
    access_token: &str,
    extra_headers: &BTreeMap<String, String>,
    turn_id: &str,
    request: &ImageGenerationRequest,
    model: &str,
) -> ImageGenerationRequestParts {
    let mut headers = BTreeMap::from([
        ("Authorization".to_owned(), format!("Bearer {access_token}")),
        ("originator".to_owned(), DEFAULT_ORIGINATOR.to_owned()),
        ("x-codex-image-turn-id".to_owned(), turn_id.to_owned()),
    ]);
    headers.extend(extra_headers.clone());

    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(model.to_owned())),
        ("prompt".to_owned(), Value::String(request.prompt.clone())),
    ]);
    insert_non_empty(&mut body, "size", request.size.as_deref());
    insert_non_empty(&mut body, "quality", request.quality.as_deref());
    insert_non_empty(&mut body, "background", request.background.as_deref());
    if let Some(count) = request.count {
        body.insert("n".to_owned(), Value::Number(Number::from(count)));
    }

    ImageGenerationRequestParts {
        path: CODEX_IMAGE_GENERATION_PATH.to_owned(),
        headers,
        body: Value::Object(body),
    }
}

fn insert_non_empty(body: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        body.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn next_turn_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = TURN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("shacs-{}-{nanos}-{sequence}", std::process::id())
}
