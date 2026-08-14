use super::sse::{read_sse_frame_texts, split_sse_frame_texts};
use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderInvocation, ProviderRequest};
use crate::registry::ProviderSpec;
use crate::types::{finish_reason_from_openai_responses, LlmResponse, ToolCallRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(120);
const OPENROUTER_REFERER: &str = "https://github.com/HKUDS/shacs-bot";
const OPENROUTER_TITLE: &str = "shacs-bot";
const OPENROUTER_CATEGORIES: &str = "cli-agent,personal-agent";
const KIMI_THINKING_MODELS: &[&str] = &["kimi-k2.5", "kimi-k2.6", "k2.6-code-preview"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiApiKind {
    ChatCompletions,
    Responses,
}

impl OpenAiApiKind {
    pub fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/chat/completions",
            Self::Responses => "/responses",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiCompatibleRequestParts {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpStreamResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub trait OpenAiHttpTransport: Send + Sync {
    fn post_json(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpResponse, ProviderError>;

    fn post_json_bounded(
        &self,
        request: OpenAiCompatibleRequestParts,
        _timeout: Option<Duration>,
    ) -> Result<OpenAiHttpResponse, ProviderError> {
        self.post_json(request)
    }

    fn post_json_stream(
        &self,
        _request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpStreamResponse, ProviderError> {
        Err(ProviderError::Api {
            status: None,
            message: "OpenAI-compatible streaming transport is not implemented".to_owned(),
            retryable: false,
            headers: BTreeMap::new(),
            body: None,
        })
    }

    fn post_json_stream_frames(
        &self,
        request: OpenAiCompatibleRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
    ) -> Result<OpenAiHttpStreamResponse, ProviderError> {
        let response = self.post_json_stream(request)?;
        if (200..300).contains(&response.status) {
            for frame in split_sse_frame_texts(&response.body) {
                if on_frame(&frame)? {
                    break;
                }
            }
        }
        Ok(response)
    }

    fn post_json_stream_frames_bounded(
        &self,
        request: OpenAiCompatibleRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
        _timeout: Option<Duration>,
    ) -> Result<OpenAiHttpStreamResponse, ProviderError> {
        self.post_json_stream_frames(request, on_frame)
    }
}

impl<F> OpenAiHttpTransport for F
where
    F: Fn(OpenAiCompatibleRequestParts) -> Result<OpenAiHttpResponse, ProviderError> + Send + Sync,
{
    fn post_json(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpResponse, ProviderError> {
        self(request)
    }
}

#[derive(Clone)]
pub struct UreqOpenAiHttpTransport {
    base_url: String,
    agent: ureq::Agent,
    stream_agent: ureq::Agent,
}

impl UreqOpenAiHttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, DEFAULT_HTTP_TIMEOUT)
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .new_agent(),
            stream_agent: ureq::Agent::config_builder()
                .timeout_connect(Some(timeout))
                .timeout_recv_body(Some(timeout))
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl OpenAiHttpTransport for UreqOpenAiHttpTransport {
    fn post_json(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpResponse, ProviderError> {
        self.post_json_bounded(request, None)
    }

    fn post_json_bounded(
        &self,
        request: OpenAiCompatibleRequestParts,
        timeout: Option<Duration>,
    ) -> Result<OpenAiHttpResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .config()
            .timeout_global(timeout)
            .build()
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body).map_err(|error| api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body_text =
            response
                .body_mut()
                .read_to_string()
                .map_err(|error| ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                })?;
        Ok(OpenAiHttpResponse {
            status,
            headers,
            body: parse_http_body(body_text),
        })
    }

    fn post_json_stream(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpStreamResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .stream_agent
            .post(&url)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body).map_err(|error| api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body = if (200..300).contains(&status) {
            read_sse_frame_texts(
                response.body_mut().as_reader(),
                |_| Ok(false),
                |error| ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                },
            )?
        } else {
            response
                .body_mut()
                .read_to_string()
                .map_err(|error| ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                })?
        };
        Ok(OpenAiHttpStreamResponse {
            status,
            headers,
            body,
        })
    }

    fn post_json_stream_frames(
        &self,
        request: OpenAiCompatibleRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
    ) -> Result<OpenAiHttpStreamResponse, ProviderError> {
        self.post_json_stream_frames_bounded(request, on_frame, None)
    }

    fn post_json_stream_frames_bounded(
        &self,
        request: OpenAiCompatibleRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
        timeout: Option<Duration>,
    ) -> Result<OpenAiHttpStreamResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .stream_agent
            .post(&url)
            .config()
            .timeout_global(timeout)
            .build()
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body).map_err(|error| api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body = if (200..300).contains(&status) {
            read_sse_frame_texts(response.body_mut().as_reader(), on_frame, |error| {
                ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                }
            })?
        } else {
            response
                .body_mut()
                .read_to_string()
                .map_err(|error| ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                })?
        };
        Ok(OpenAiHttpStreamResponse {
            status,
            headers,
            body,
        })
    }
}

pub fn resolve_openai_compatible_api_base(
    config: &ProviderConfig,
    spec: &ProviderSpec,
) -> Result<String, ProviderError> {
    ensure_openai_compatible_backend(spec)?;
    config
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .or_else(|| {
            spec.default_api_base
                .map(str::trim)
                .filter(|base| !base.is_empty())
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            api_error(
                None,
                format!(
                    "missing OpenAI-compatible base URL for provider '{}'",
                    spec.name
                ),
            )
        })
}

pub fn openai_compatible_client_from_config(
    config: ProviderConfig,
    spec: &ProviderSpec,
) -> Result<OpenAiCompatibleClient<UreqOpenAiHttpTransport>, ProviderError> {
    let base_url = resolve_openai_compatible_api_base(&config, spec)?;
    Ok(OpenAiCompatibleClient::with_provider_spec(
        config,
        UreqOpenAiHttpTransport::new(base_url.clone()),
        *spec,
        base_url,
    ))
}

fn ensure_openai_compatible_backend(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.backend == "openai_compat" {
        return Ok(());
    }
    Err(api_error(
        None,
        format!(
            "provider '{}' does not use OpenAI-compatible backend",
            spec.name
        ),
    ))
}

fn join_base_and_path(base_url: &str, path: &str) -> Result<String, ProviderError> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err(api_error(None, "missing OpenAI-compatible base URL"));
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
    ProviderError::Api {
        status,
        message: error.to_string(),
        retryable,
        headers: BTreeMap::new(),
        body: None,
    }
}

fn api_error(status: Option<u16>, error: impl ToString) -> ProviderError {
    ProviderError::Api {
        status,
        message: error.to_string(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleClient<T> {
    config: ProviderConfig,
    transport: T,
    provider_name: Option<String>,
    api_base: Option<String>,
    spec: Option<ProviderSpec>,
}

impl<T> OpenAiCompatibleClient<T>
where
    T: OpenAiHttpTransport,
{
    pub fn new(config: ProviderConfig, transport: T) -> Self {
        Self {
            config,
            transport,
            provider_name: None,
            api_base: None,
            spec: None,
        }
    }

    pub fn with_provider_context(
        config: ProviderConfig,
        transport: T,
        provider_name: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Self {
        Self {
            config,
            transport,
            provider_name: Some(provider_name.into()),
            api_base: Some(api_base.into()),
            spec: None,
        }
    }

    pub fn with_provider_spec(
        config: ProviderConfig,
        transport: T,
        spec: ProviderSpec,
        api_base: impl Into<String>,
    ) -> Self {
        Self {
            config,
            transport,
            provider_name: Some(spec.name.to_owned()),
            api_base: Some(api_base.into()),
            spec: Some(spec),
        }
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T> ProviderClient for OpenAiCompatibleClient<T>
where
    T: OpenAiHttpTransport,
{
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.chat_bounded(request, None)
    }

    fn chat_with_invocation(
        &self,
        request: ProviderRequest,
        invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        if invocation.is_cancelled() {
            return Err(invocation_cancelled());
        }
        self.chat_bounded(request, invocation.remaining())
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat_stream_bounded(request, on_event, None)
    }

    fn chat_stream_with_invocation(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        if invocation.is_cancelled() {
            return Err(invocation_cancelled());
        }
        self.chat_stream_bounded(request, on_event, invocation.remaining())
    }
}

impl<T> OpenAiCompatibleClient<T>
where
    T: OpenAiHttpTransport,
{
    fn chat_bounded(
        &self,
        request: ProviderRequest,
        timeout: Option<Duration>,
    ) -> Result<LlmResponse, ProviderError> {
        if self.should_use_responses_api(&request) {
            let responses_request = normalize_request_for_provider(&request, self.spec.as_ref());
            let parts = build_responses_request(&responses_request, &self.config, false);
            let response = self.transport.post_json_bounded(parts, timeout)?;
            let parsed = parse_responses_http_response(response)?;
            if parsed.finish_reason != "error" || !should_fallback_from_responses_response(&parsed)
            {
                return Ok(parsed);
            }
        }
        let parts = build_provider_chat_completions_request(&request, &self.config, self.spec);
        let response = self.transport.post_json_bounded(parts, timeout)?;
        parse_http_response_with_spec(response, self.spec.as_ref())
    }

    fn chat_stream_bounded(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        timeout: Option<Duration>,
    ) -> Result<LlmResponse, ProviderError> {
        if self.should_use_responses_api(&request) {
            let responses_request = normalize_request_for_provider(&request, self.spec.as_ref());
            let parts = build_responses_request(&responses_request, &self.config, true);
            let mut stream = OpenAiResponsesStreamState::default();
            match self.transport.post_json_stream_frames_bounded(
                parts,
                &mut |frame| stream.process_frame_text(frame, on_event),
                timeout,
            ) {
                Ok(response) => {
                    let parsed = if (200..300).contains(&response.status) {
                        stream.finish(on_event)?
                    } else {
                        parse_responses_stream_http_response(response, on_event)?
                    };
                    if parsed.finish_reason != "error"
                        || !should_fallback_from_responses_response(&parsed)
                    {
                        return Ok(parsed);
                    }
                }
                Err(error) if !is_streaming_transport_unsupported(&error) => return Err(error),
                Err(_) => {}
            }
        }
        let parts =
            build_provider_chat_completions_stream_request(&request, &self.config, self.spec);
        let mut stream = ChatCompletionsStreamState::default();
        match self.transport.post_json_stream_frames_bounded(
            parts,
            &mut |frame| stream.process_frame_text(frame, on_event),
            timeout,
        ) {
            Ok(response) => {
                let parsed = if (200..300).contains(&response.status) {
                    stream.finish(on_event)?
                } else {
                    parse_chat_completions_stream_http_response(response, on_event)?
                };
                return Ok(apply_reasoning_as_content(parsed, self.spec.as_ref()));
            }
            Err(error) if !is_streaming_transport_unsupported(&error) => return Err(error),
            Err(_) => {}
        }
        let response = self.chat_bounded(request, timeout)?;
        if let Some(content) = response
            .content
            .as_deref()
            .filter(|content| !content.is_empty())
        {
            on_event(ProviderEvent::TextDelta {
                text: content.to_owned(),
            });
        }
        on_event(ProviderEvent::Finish {
            usage: serde_json::to_value(&response.usage).unwrap_or(Value::Null),
            reason: response.finish_reason.clone(),
        });
        Ok(response)
    }
}

fn invocation_cancelled() -> ProviderError {
    ProviderError::Api {
        status: None,
        message: "provider invocation cancelled".to_owned(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}

impl<T> OpenAiCompatibleClient<T>
where
    T: OpenAiHttpTransport,
{
    fn should_use_responses_api(&self, request: &ProviderRequest) -> bool {
        if self.provider_name.as_deref() != Some("openai") {
            return false;
        }
        let direct_openai = self
            .api_base
            .as_deref()
            .or(self.config.api_base.as_deref())
            .is_some_and(is_direct_openai_base);
        if !direct_openai {
            return false;
        }
        request
            .settings
            .reasoning_effort
            .as_deref()
            .is_some_and(|effort| !effort.eq_ignore_ascii_case("none"))
            || should_route_model_to_responses(&request.model)
    }
}

fn parse_http_response(response: OpenAiHttpResponse) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_chat_completions_response(&response.body);
    }
    let mut body = match response.body {
        Value::Object(mut body) => {
            let error = body
                .remove("error")
                .unwrap_or_else(|| error_from_non_success_body(&body));
            body.insert("error".to_owned(), error);
            body
        }
        other => Map::from_iter([("error".to_owned(), error_from_non_object_body(&other))]),
    };
    body.entry("status".to_owned())
        .or_insert_with(|| Value::Number(Number::from(response.status)));
    if let Some(retry_after) = retry_after_seconds(&response.headers) {
        body.insert(
            "retry_after".to_owned(),
            Number::from_f64(retry_after)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(should_retry) = should_retry(&response.headers) {
        body.insert("should_retry".to_owned(), Value::Bool(should_retry));
    }
    parse_chat_completions_response(&Value::Object(body))
}

fn parse_http_response_with_spec(
    response: OpenAiHttpResponse,
    spec: Option<&ProviderSpec>,
) -> Result<LlmResponse, ProviderError> {
    parse_http_response(response).map(|response| apply_reasoning_as_content(response, spec))
}

fn apply_reasoning_as_content(
    mut response: LlmResponse,
    spec: Option<&ProviderSpec>,
) -> LlmResponse {
    if spec.is_some_and(|spec| spec.reasoning_as_content)
        && response.content.as_deref().unwrap_or_default().is_empty()
    {
        response.content = response.reasoning_content.clone();
    }
    response
}

fn error_from_non_success_body(body: &Map<String, Value>) -> Value {
    let message = body
        .get("message")
        .or_else(|| body.get("error_description"))
        .or_else(|| body.get("content"))
        .or_else(|| body.get("output_text"))
        .and_then(value_to_text)
        .unwrap_or_else(|| "provider error".to_owned());
    json!({ "message": message })
}

fn error_from_non_object_body(body: &Value) -> Value {
    match body {
        Value::String(message) => json!({ "message": message }),
        Value::Null => json!({ "message": "provider error" }),
        other => json!({ "message": other.to_string() }),
    }
}

pub fn build_chat_completions_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
) -> OpenAiCompatibleRequestParts {
    let mut body = Map::new();
    body.insert("model".to_owned(), Value::String(request.model.clone()));
    body.insert(
        "messages".to_owned(),
        Value::Array(request.messages.clone()),
    );
    body.insert(
        "max_tokens".to_owned(),
        Value::Number(Number::from(request.settings.max_tokens.max(1))),
    );
    if supports_temperature(&request.model, request.settings.reasoning_effort.as_deref()) {
        if let Some(number) = Number::from_f64(request.settings.temperature) {
            body.insert("temperature".to_owned(), Value::Number(number));
        }
    }
    if let Some(reasoning_effort) = request
        .settings
        .reasoning_effort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .filter(|value| value != "none")
    {
        body.insert(
            "reasoning_effort".to_owned(),
            Value::String(reasoning_effort),
        );
    }
    if !request.tools.is_empty() {
        body.insert("tools".to_owned(), Value::Array(request.tools.clone()));
        body.insert(
            "tool_choice".to_owned(),
            request
                .tool_choice
                .clone()
                .unwrap_or_else(|| Value::String("auto".to_owned())),
        );
    }
    if let Some(extra_body) = &config.extra_body {
        merge_json_objects(&mut body, extra_body);
    }

    OpenAiCompatibleRequestParts {
        path: OpenAiApiKind::ChatCompletions.path().to_owned(),
        headers: build_headers(config),
        body: Value::Object(body),
    }
}

pub fn build_chat_completions_stream_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
) -> OpenAiCompatibleRequestParts {
    let mut parts = build_chat_completions_request(request, config);
    if let Value::Object(body) = &mut parts.body {
        body.insert("stream".to_owned(), Value::Bool(true));
        body.insert(
            "stream_options".to_owned(),
            json!({ "include_usage": true }),
        );
    }
    parts
}

fn build_provider_chat_completions_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
    spec: Option<ProviderSpec>,
) -> OpenAiCompatibleRequestParts {
    let Some(spec) = spec else {
        return build_chat_completions_request(request, config);
    };
    let normalized_request = normalize_request_for_provider(request, Some(&spec));
    let mut parts = build_chat_completions_request(&normalized_request, config);
    parts.headers = build_provider_headers(config, &spec);
    if let Value::Object(body) = &mut parts.body {
        apply_provider_body_metadata(body, &normalized_request, &spec, config);
    }
    parts
}

fn normalize_request_for_provider(
    request: &ProviderRequest,
    spec: Option<&ProviderSpec>,
) -> ProviderRequest {
    let Some(spec) = spec else {
        return request.clone();
    };
    let mut normalized = request.clone();
    let mut messages = request.messages.clone();
    let mut tools = request.tools.clone();
    if spec.supports_prompt_caching && model_uses_prompt_cache_blocks(&request.model) {
        apply_openai_compat_cache_control(&mut messages, &mut tools);
    }
    if spec.name == "deepseek" {
        messages = drop_deepseek_incomplete_reasoning_history(
            messages,
            request.settings.reasoning_effort.as_deref(),
        );
    }
    messages = sanitize_openai_compat_messages(
        messages,
        spec,
        request.settings.reasoning_effort.as_deref(),
        &request.model,
    );
    if thinking_is_active(
        spec,
        &request.model,
        request.settings.reasoning_effort.as_deref(),
    ) {
        backfill_assistant_reasoning_content(&mut messages);
    }
    normalized.messages = messages;
    normalized.tools = tools;
    normalized
}

fn model_uses_prompt_cache_blocks(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.starts_with("anthropic/") || model.starts_with("claude")
}

fn apply_openai_compat_cache_control(messages: &mut [Value], tools: &mut [Value]) {
    if messages
        .first()
        .and_then(Value::as_object)
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("system")
    {
        add_cache_control_to_openai_message(&mut messages[0]);
    }
    if messages.len() >= 3 {
        let index = messages.len() - 2;
        add_cache_control_to_openai_message(&mut messages[index]);
    }
    for index in tool_cache_marker_indices(tools) {
        if let Some(tool) = tools.get_mut(index).and_then(Value::as_object_mut) {
            tool.entry("cache_control".to_owned())
                .or_insert_with(cache_control_marker);
        }
    }
}

fn add_cache_control_to_openai_message(message: &mut Value) {
    let Some(message) = message.as_object_mut() else {
        return;
    };
    match message.get_mut("content") {
        Some(Value::String(text)) if !text.is_empty() => {
            let text = text.clone();
            message.insert(
                "content".to_owned(),
                json!([{ "type": "text", "text": text, "cache_control": cache_control_marker() }]),
            );
        }
        Some(Value::Array(blocks)) => {
            if let Some(block) = blocks.last_mut().and_then(Value::as_object_mut) {
                block.insert("cache_control".to_owned(), cache_control_marker());
            }
        }
        _ => {}
    }
}

fn tool_cache_marker_indices(tools: &[Value]) -> Vec<usize> {
    if tools.is_empty() {
        return Vec::new();
    }
    let tail = tools.len() - 1;
    let last_builtin = tools
        .iter()
        .enumerate()
        .rev()
        .find(|(_, tool)| !tool_name(tool).starts_with("mcp_"))
        .map(|(index, _)| index);
    let mut indices = Vec::new();
    for index in [last_builtin, Some(tail)].into_iter().flatten() {
        if !indices.contains(&index) {
            indices.push(index);
        }
    }
    indices
}

fn tool_name(tool: &Value) -> String {
    let Some(object) = tool.as_object() else {
        return String::new();
    };
    object
        .get("name")
        .or_else(|| {
            object
                .get("function")
                .and_then(Value::as_object)
                .and_then(|function| function.get("name"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn cache_control_marker() -> Value {
    json!({ "type": "ephemeral" })
}

fn sanitize_openai_compat_messages(
    messages: Vec<Value>,
    spec: &ProviderSpec,
    reasoning_effort: Option<&str>,
    model: &str,
) -> Vec<Value> {
    let force_string_content = spec.name == "deepseek";
    let mut id_map = BTreeMap::new();
    let sanitized = messages
        .into_iter()
        .filter_map(|message| {
            sanitize_openai_compat_message(message, force_string_content, &mut id_map)
        })
        .collect::<Vec<_>>();
    let mut sanitized = enforce_role_alternation(sanitized);
    if thinking_is_active(spec, model, reasoning_effort) {
        backfill_assistant_reasoning_content(&mut sanitized);
    }
    sanitized
}

fn sanitize_openai_compat_message(
    message: Value,
    force_string_content: bool,
    id_map: &mut BTreeMap<String, String>,
) -> Option<Value> {
    let object = message.as_object()?;
    let mut clean = Map::new();
    for key in [
        "role",
        "content",
        "tool_calls",
        "tool_call_id",
        "name",
        "reasoning_content",
        "extra_content",
    ] {
        if let Some(value) = object.get(key) {
            clean.insert(key.to_owned(), value.clone());
        }
    }
    sanitize_empty_content(&mut clean);

    if let Some(Value::Array(tool_calls)) = clean.get_mut("tool_calls") {
        for tool_call in tool_calls {
            normalize_outbound_tool_call(tool_call, id_map);
        }
        if clean.get("role").and_then(Value::as_str) == Some("assistant") {
            clean.insert("content".to_owned(), Value::Null);
        }
    }
    if let Some(tool_call_id) = clean.get("tool_call_id").and_then(Value::as_str) {
        clean.insert(
            "tool_call_id".to_owned(),
            Value::String(mapped_tool_id(tool_call_id, id_map)),
        );
    }
    if force_string_content
        && !(clean.get("role").and_then(Value::as_str) == Some("assistant")
            && clean.get("tool_calls").is_some())
    {
        let content = clean.get("content").cloned().unwrap_or(Value::Null);
        clean.insert("content".to_owned(), coerce_content_to_string(content));
    }
    Some(Value::Object(clean))
}

fn sanitize_empty_content(message: &mut Map<String, Value>) {
    let empty_replacement = empty_content_replacement(message);
    let Some(content) = message.get_mut("content") else {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            message.insert("content".to_owned(), Value::Null);
        }
        return;
    };
    match content {
        Value::String(text) if text.is_empty() => {
            *content = empty_replacement;
        }
        Value::Array(items) => {
            let mut changed = false;
            let mut new_items = Vec::new();
            for item in items.iter() {
                let Some(object) = item.as_object() else {
                    new_items.push(item.clone());
                    continue;
                };
                let is_empty_text = object
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| matches!(kind, "text" | "input_text" | "output_text"))
                    && object
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .is_empty();
                if is_empty_text {
                    changed = true;
                    continue;
                }
                if object.contains_key("_meta") {
                    let mut clean = object.clone();
                    clean.remove("_meta");
                    new_items.push(Value::Object(clean));
                    changed = true;
                } else {
                    new_items.push(item.clone());
                }
            }
            if changed {
                *content = if new_items.is_empty() {
                    empty_replacement
                } else {
                    Value::Array(new_items)
                };
            }
        }
        Value::Object(object) => {
            *content = Value::Array(vec![Value::Object(object.clone())]);
        }
        _ => {}
    }
}

fn empty_content_replacement(message: &Map<String, Value>) -> Value {
    if message.get("role").and_then(Value::as_str) == Some("assistant")
        && message.get("tool_calls").is_some()
    {
        Value::Null
    } else {
        Value::String("(empty)".to_owned())
    }
}

fn normalize_outbound_tool_call(tool_call: &mut Value, id_map: &mut BTreeMap<String, String>) {
    let Some(object) = tool_call.as_object_mut() else {
        return;
    };
    if let Some(id) = object.get("id").and_then(Value::as_str) {
        object.insert("id".to_owned(), Value::String(mapped_tool_id(id, id_map)));
    }
    if let Some(function) = object.get_mut("function").and_then(Value::as_object_mut) {
        let arguments = normalize_tool_call_arguments(function.get("arguments"));
        function.insert("arguments".to_owned(), Value::String(arguments));
    }
}

fn normalize_tool_call_arguments(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(raw)) if raw.trim().is_empty() => "{}".to_owned(),
        Some(Value::String(raw)) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|value| match value {
                Value::Object(arguments) => Some(Value::Object(arguments).to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "{}".to_owned()),
        Some(Value::Object(arguments)) => Value::Object(arguments.clone()).to_string(),
        _ => "{}".to_owned(),
    }
}

fn mapped_tool_id(id: &str, id_map: &mut BTreeMap<String, String>) -> String {
    if let Some(mapped) = id_map.get(id) {
        return mapped.clone();
    }
    let mapped = normalize_tool_call_id(id);
    id_map.insert(id.to_owned(), mapped.clone());
    mapped
}

fn normalize_tool_call_id(id: &str) -> String {
    if id.len() == 9
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return id.to_owned();
    }
    let digest = Sha256::digest(id.as_bytes());
    format!("{digest:x}").chars().take(9).collect()
}

fn coerce_content_to_string(content: Value) -> Value {
    match content {
        Value::Null | Value::String(_) => content,
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|object| object.get("text"))
                        .and_then(Value::as_str)
                })
                .collect::<String>();
            if text.is_empty() {
                Value::String(Value::Array(items).to_string())
            } else {
                Value::String(text)
            }
        }
        other => Value::String(other.to_string()),
    }
}

fn enforce_role_alternation(messages: Vec<Value>) -> Vec<Value> {
    if messages.is_empty() {
        return messages;
    }
    let mut merged: Vec<Value> = Vec::new();
    for message in messages {
        let role = message_role(&message).unwrap_or_default().to_owned();
        let should_merge = merged
            .last()
            .and_then(message_role)
            .is_some_and(|previous| previous == role)
            && matches!(role.as_str(), "user" | "assistant");
        if should_merge {
            let current_has_tools = message_has_tool_calls(&message);
            let previous_has_tools = merged.last().is_some_and(message_has_tool_calls);
            if role == "assistant" && current_has_tools {
                if let Some(last) = merged.last_mut() {
                    *last = message;
                }
                continue;
            }
            if role == "assistant" && previous_has_tools {
                continue;
            }
            merge_message_content(merged.last_mut(), message);
        } else {
            merged.push(message);
        }
    }

    let mut last_popped = None;
    while merged.last().and_then(message_role) == Some("assistant") {
        last_popped = merged.pop();
    }
    if !merged.is_empty()
        && last_popped.is_some()
        && !merged
            .iter()
            .any(|message| matches!(message_role(message), Some("user" | "tool")))
    {
        if let Some(mut recovered) = last_popped {
            if let Some(object) = recovered.as_object_mut() {
                object.insert("role".to_owned(), Value::String("user".to_owned()));
            }
            merged.push(recovered);
        }
    }
    if let Some(index) = merged
        .iter()
        .position(|message| message_role(message) != Some("system"))
    {
        if message_role(&merged[index]) == Some("assistant")
            && !message_has_tool_calls(&merged[index])
        {
            merged.insert(
                index,
                json!({"role": "user", "content": "(conversation continued)"}),
            );
        }
    }
    merged
}

fn message_role(message: &Value) -> Option<&str> {
    message
        .as_object()
        .and_then(|object| object.get("role"))
        .and_then(Value::as_str)
}

fn message_has_tool_calls(message: &Value) -> bool {
    message
        .as_object()
        .and_then(|object| object.get("tool_calls"))
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty())
}

fn merge_message_content(last: Option<&mut Value>, current: Value) {
    let Some(last) = last.and_then(Value::as_object_mut) else {
        return;
    };
    let Some(current) = current.as_object() else {
        return;
    };
    let previous_content = last.get("content").and_then(Value::as_str);
    let current_content = current.get("content").and_then(Value::as_str);
    match (previous_content, current_content) {
        (Some(previous), Some(current)) => {
            last.insert(
                "content".to_owned(),
                Value::String(format!("{previous}\n\n{current}").trim().to_owned()),
            );
        }
        _ => {
            for (key, value) in current {
                last.insert(key.clone(), value.clone());
            }
        }
    }
}

fn drop_deepseek_incomplete_reasoning_history(
    messages: Vec<Value>,
    reasoning_effort: Option<&str>,
) -> Vec<Value> {
    if !reasoning_effort.is_some_and(|effort| !effort.eq_ignore_ascii_case("none")) {
        return messages;
    }
    let bad_index = messages.iter().rposition(|message| {
        message_role(message) == Some("assistant")
            && message_has_tool_calls(message)
            && message
                .as_object()
                .and_then(|object| object.get("reasoning_content"))
                .is_none()
    });
    let Some(bad_index) = bad_index else {
        return messages;
    };
    let keep_from = messages
        .iter()
        .enumerate()
        .skip(bad_index + 1)
        .find(|(_, message)| message_role(message) == Some("user"))
        .map(|(index, _)| index);
    let Some(keep_from) = keep_from else {
        return messages.into_iter().take(bad_index).collect();
    };
    messages
        .iter()
        .take(keep_from)
        .filter(|message| message_role(message) == Some("system"))
        .chain(messages.iter().skip(keep_from))
        .cloned()
        .collect()
}

fn thinking_is_active(spec: &ProviderSpec, model: &str, reasoning_effort: Option<&str>) -> bool {
    let Some(effort) = reasoning_effort.map(semantic_reasoning_effort) else {
        return false;
    };
    !matches!(effort.as_str(), "none" | "minimal")
        && (spec.thinking_style.is_some() || is_kimi_thinking_model(model))
}

fn backfill_assistant_reasoning_content(messages: &mut [Value]) {
    for message in messages {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        if object.get("role").and_then(Value::as_str) == Some("assistant") {
            object
                .entry("reasoning_content".to_owned())
                .or_insert_with(|| Value::String(String::new()));
        }
    }
}

fn build_provider_chat_completions_stream_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
    spec: Option<ProviderSpec>,
) -> OpenAiCompatibleRequestParts {
    let mut parts = build_provider_chat_completions_request(request, config, spec);
    if let Value::Object(body) = &mut parts.body {
        body.insert("stream".to_owned(), Value::Bool(true));
        body.insert(
            "stream_options".to_owned(),
            json!({ "include_usage": true }),
        );
    }
    parts
}

fn build_provider_headers(
    config: &ProviderConfig,
    spec: &ProviderSpec,
) -> BTreeMap<String, String> {
    let mut headers = build_headers(config);
    headers
        .entry("x-session-affinity".to_owned())
        .or_insert_with(default_session_affinity);
    if spec.name == "openrouter" {
        headers
            .entry("HTTP-Referer".to_owned())
            .or_insert_with(|| OPENROUTER_REFERER.to_owned());
        headers
            .entry("X-OpenRouter-Title".to_owned())
            .or_insert_with(|| OPENROUTER_TITLE.to_owned());
        headers
            .entry("X-OpenRouter-Categories".to_owned())
            .or_insert_with(|| OPENROUTER_CATEGORIES.to_owned());
    }
    headers
}

fn apply_provider_body_metadata(
    body: &mut Map<String, Value>,
    request: &ProviderRequest,
    spec: &ProviderSpec,
    config: &ProviderConfig,
) {
    if spec.supports_max_completion_tokens {
        if let Some(max_tokens) = body.remove("max_tokens") {
            body.insert("max_completion_tokens".to_owned(), max_tokens);
        }
    }

    apply_model_overrides(body, &request.model, spec);
    apply_reasoning_wire_parity(body, request, spec);

    if let Some(extra_body) = &config.extra_body {
        merge_json_objects(body, extra_body);
    }
}

fn apply_model_overrides(body: &mut Map<String, Value>, model: &str, spec: &ProviderSpec) {
    let model_lower = model.to_ascii_lowercase();
    let Some((_, override_json)) = spec
        .model_overrides
        .iter()
        .find(|(pattern, _)| model_lower.contains(&pattern.to_ascii_lowercase()))
    else {
        return;
    };
    let Ok(Value::Object(overrides)) = serde_json::from_str::<Value>(override_json) else {
        return;
    };
    merge_json_objects(body, &overrides);
}

fn apply_reasoning_wire_parity(
    body: &mut Map<String, Value>,
    request: &ProviderRequest,
    spec: &ProviderSpec,
) {
    let Some(raw_effort) = request.settings.reasoning_effort.as_deref() else {
        return;
    };
    let semantic_effort = semantic_reasoning_effort(raw_effort);

    if spec.name == "dashscope" && semantic_effort == "minimal" {
        body.insert(
            "reasoning_effort".to_owned(),
            Value::String("minimum".to_owned()),
        );
    }

    let thinking_enabled = !matches!(semantic_effort.as_str(), "none" | "minimal");
    if let Some(style) = spec.thinking_style {
        if let Some(extra) = thinking_style_body(style, thinking_enabled) {
            merge_json_objects(body, &extra);
        }
    }
    if is_kimi_thinking_model(&request.model) {
        merge_json_objects(body, &thinking_type_body(thinking_enabled));
    }
}

fn semantic_reasoning_effort(raw_effort: &str) -> String {
    match raw_effort.to_ascii_lowercase().as_str() {
        "minimum" => "minimal".to_owned(),
        effort => effort.to_owned(),
    }
}

fn thinking_style_body(style: &str, enabled: bool) -> Option<Map<String, Value>> {
    match style {
        "thinking_type" => Some(thinking_type_body(enabled)),
        "enable_thinking" => object_from_value(json!({ "enable_thinking": enabled })),
        "reasoning_split" => object_from_value(json!({ "reasoning_split": enabled })),
        _ => None,
    }
}

fn thinking_type_body(enabled: bool) -> Map<String, Value> {
    object_from_value(json!({
        "thinking": { "type": if enabled { "enabled" } else { "disabled" } }
    }))
    .unwrap_or_default()
}

fn object_from_value(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

fn is_kimi_thinking_model(model: &str) -> bool {
    let name = model.to_ascii_lowercase();
    KIMI_THINKING_MODELS.iter().any(|candidate| {
        name == *candidate
            || name
                .rsplit_once('/')
                .is_some_and(|(_, slug)| slug == *candidate)
    })
}

fn default_session_affinity() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}{:x}", process::id(), nanos)
}

pub fn build_responses_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
    stream: bool,
) -> OpenAiCompatibleRequestParts {
    let (instructions, input) = convert_messages_to_responses_input(&request.messages);
    let mut body = Map::new();
    body.insert("model".to_owned(), Value::String(request.model.clone()));
    if let Some(instructions) = non_empty_text(Some(instructions)) {
        body.insert("instructions".to_owned(), Value::String(instructions));
    }
    body.insert("input".to_owned(), Value::Array(input));
    body.insert(
        "max_output_tokens".to_owned(),
        Value::Number(Number::from(request.settings.max_tokens.max(1))),
    );
    body.insert("store".to_owned(), Value::Bool(false));
    body.insert("stream".to_owned(), Value::Bool(stream));
    if supports_temperature(&request.model, request.settings.reasoning_effort.as_deref()) {
        if let Some(number) = Number::from_f64(request.settings.temperature) {
            body.insert("temperature".to_owned(), Value::Number(number));
        }
    }
    if let Some(reasoning_effort) = request
        .settings
        .reasoning_effort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .filter(|value| value != "none")
    {
        body.insert(
            "reasoning".to_owned(),
            json!({ "effort": reasoning_effort }),
        );
        body.insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(convert_tools_to_responses_tools(&request.tools)),
        );
        body.insert(
            "tool_choice".to_owned(),
            request
                .tool_choice
                .clone()
                .unwrap_or_else(|| Value::String("auto".to_owned())),
        );
    }
    if let Some(extra_body) = &config.extra_body {
        merge_json_objects(&mut body, extra_body);
        body.insert("stream".to_owned(), Value::Bool(stream));
    }

    OpenAiCompatibleRequestParts {
        path: OpenAiApiKind::Responses.path().to_owned(),
        headers: build_headers(config),
        body: Value::Object(body),
    }
}

pub fn build_headers(config: &ProviderConfig) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    if let Some(api_key) = config.api_key.as_deref().filter(|value| !value.is_empty()) {
        headers.insert("Authorization".to_owned(), format!("Bearer {api_key}"));
    }
    if let Some(extra_headers) = &config.extra_headers {
        for (key, value) in extra_headers {
            headers.insert(key.clone(), value.clone());
        }
    }
    headers
}

pub fn chat_completions_tool(
    name: impl Into<String>,
    description: impl Into<String>,
    parameters: Value,
) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name.into(),
            "description": description.into(),
            "parameters": parameters,
        }
    })
}

pub fn merge_json_objects(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(target_object)), Value::Object(source_object)) => {
                merge_json_objects(target_object, source_object);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn convert_messages_to_responses_input(messages: &[Value]) -> (String, Vec<Value>) {
    let mut instructions = Vec::new();
    let mut input = Vec::new();
    let mut omitted_function_call_ids = BTreeSet::new();
    for (index, message) in messages.iter().filter_map(Value::as_object).enumerate() {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match role {
            "system" => {
                if let Some(text) = message.get("content").and_then(value_to_text) {
                    if !text.is_empty() {
                        instructions.push(text);
                    }
                }
            }
            "user" => input.push(convert_user_message_to_responses_item(
                message.get("content"),
            )),
            "assistant" => {
                if let Some(content) = message
                    .get("content")
                    .and_then(value_to_text)
                    .filter(|content| !content.is_empty())
                {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": content}],
                        "status": "completed",
                        "id": format!("msg_{index}"),
                    }));
                }
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls.iter().filter_map(Value::as_object) {
                        let (call_id, item_id) = split_responses_tool_call_id(
                            call.get("id").and_then(Value::as_str).unwrap_or_default(),
                        );
                        let function = call.get("function").and_then(Value::as_object);
                        let call_id = if call_id.is_empty() {
                            format!("call_{index}")
                        } else {
                            call_id
                        };
                        let name = function
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if name.is_empty() {
                            omitted_function_call_ids.insert(call_id);
                            continue;
                        }
                        input.push(json!({
                            "type": "function_call",
                            "id": item_id.unwrap_or_else(|| format!("fc_{index}")),
                            "call_id": call_id,
                            "name": name,
                            "arguments": function.and_then(|function| function.get("arguments")).and_then(Value::as_str).unwrap_or("{}"),
                        }));
                    }
                }
            }
            "tool" => {
                let (call_id, _) = split_responses_tool_call_id(
                    message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("call_0"),
                );
                if omitted_function_call_ids.contains(&call_id) {
                    continue;
                }
                let output = message
                    .get("content")
                    .and_then(value_to_text)
                    .unwrap_or_else(|| message.get("content").unwrap_or(&Value::Null).to_string());
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }));
            }
            _ => {}
        }
    }
    (instructions.join("\n"), input)
}

fn convert_user_message_to_responses_item(content: Option<&Value>) -> Value {
    let Some(content) = content else {
        return json!({"role": "user", "content": [{"type": "input_text", "text": ""}]});
    };
    if let Some(text) = content.as_str() {
        return json!({"role": "user", "content": [{"type": "input_text", "text": text}]});
    }
    if let Some(items) = content.as_array() {
        let converted = items
            .iter()
            .filter_map(|item| {
                let object = item.as_object()?;
                match object.get("type").and_then(Value::as_str) {
                    Some("text") => Some(json!({
                        "type": "input_text",
                        "text": object.get("text").and_then(Value::as_str).unwrap_or_default(),
                    })),
                    Some("image_url") => object
                        .get("image_url")
                        .and_then(Value::as_object)
                        .and_then(|image| image.get("url"))
                        .and_then(Value::as_str)
                        .map(|url| json!({"type": "input_image", "image_url": url, "detail": "auto"})),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if !converted.is_empty() {
            return json!({"role": "user", "content": converted});
        }
    }
    json!({"role": "user", "content": [{"type": "input_text", "text": ""}]})
}

fn convert_tools_to_responses_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let object = tool.as_object()?;
            let function = if object.get("type").and_then(Value::as_str) == Some("function") {
                object.get("function").and_then(Value::as_object)?
            } else {
                object
            };
            let name = function.get("name").and_then(Value::as_str)?;
            Some(json!({
                "type": "function",
                "name": name,
                "description": function.get("description").and_then(Value::as_str).unwrap_or_default(),
                "parameters": function.get("parameters").and_then(Value::as_object).cloned().unwrap_or_default(),
            }))
        })
        .collect()
}

fn split_responses_tool_call_id(tool_call_id: &str) -> (String, Option<String>) {
    if let Some((call_id, item_id)) = tool_call_id.split_once('|') {
        return (
            call_id.to_owned(),
            (!item_id.is_empty()).then(|| item_id.to_owned()),
        );
    }
    (tool_call_id.to_owned(), None)
}

pub fn parse_chat_completions_response(response: &Value) -> Result<LlmResponse, ProviderError> {
    if let Some(text) = response.as_str() {
        return Ok(LlmResponse {
            content: Some(text.to_owned()),
            ..LlmResponse::default()
        });
    }

    let Some(object) = response.as_object() else {
        return Ok(LlmResponse {
            content: Some("Error: API returned an invalid response.".to_owned()),
            finish_reason: "error".to_owned(),
            ..LlmResponse::default()
        });
    };

    if let Some(error) = object.get("error") {
        return Ok(parse_error_response(error, object));
    }

    let Some(choices) = object.get("choices").and_then(Value::as_array) else {
        if let Some(content) = object
            .get("content")
            .or_else(|| object.get("output_text"))
            .and_then(value_to_text)
        {
            return Ok(LlmResponse {
                content: Some(content),
                usage: parse_usage(object.get("usage")),
                ..LlmResponse::default()
            });
        }
        return Ok(LlmResponse {
            content: Some("Error: API returned empty choices.".to_owned()),
            finish_reason: "error".to_owned(),
            usage: parse_usage(object.get("usage")),
            ..LlmResponse::default()
        });
    };

    if choices.is_empty() {
        return Ok(LlmResponse {
            content: Some("Error: API returned empty choices.".to_owned()),
            finish_reason: "error".to_owned(),
            usage: parse_usage(object.get("usage")),
            ..LlmResponse::default()
        });
    }

    let mut content = None;
    let mut reasoning_content = None;
    let mut tool_calls = Vec::new();
    let mut preferred_finish_reason = None;

    for choice in choices.iter().filter_map(Value::as_object) {
        let message = choice.get("message").and_then(Value::as_object);
        if content.is_none() {
            content = non_empty_text(
                message
                    .and_then(|message| message.get("content"))
                    .and_then(value_to_text),
            );
        }
        if reasoning_content.is_none() {
            reasoning_content = message.and_then(|message| {
                non_empty_text(message.get("reasoning_content").and_then(value_to_text))
                    .or_else(|| non_empty_text(message.get("reasoning").and_then(value_to_text)))
            });
        }
        if let Some(calls) = message
            .and_then(|message| message.get("tool_calls"))
            .and_then(Value::as_array)
        {
            for call in calls {
                tool_calls.push(parse_tool_call(call)?);
            }
        }
        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        if preferred_finish_reason.is_none()
            || matches!(finish_reason, Some("tool_calls" | "function_call" | "stop"))
        {
            preferred_finish_reason = finish_reason.map(str::to_owned);
        }
    }

    let finish_reason =
        normalize_chat_finish_reason(preferred_finish_reason.as_deref(), !tool_calls.is_empty());
    Ok(LlmResponse {
        content,
        tool_calls,
        finish_reason,
        usage: parse_usage(object.get("usage")),
        reasoning_content,
        ..LlmResponse::default()
    })
}

fn parse_chat_completions_stream_http_response(
    response: OpenAiHttpStreamResponse,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_chat_completions_stream(&response.body, on_event);
    }
    parse_http_response(OpenAiHttpResponse {
        status: response.status,
        headers: response.headers,
        body: parse_http_body(response.body),
    })
}

pub fn parse_chat_completions_stream(
    body: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    let mut stream = ChatCompletionsStreamState::default();
    for frame in parse_sse_frames(body) {
        if stream.process_frame(frame, on_event)? {
            break;
        }
    }
    stream.finish(on_event)
}

fn parse_responses_http_response(
    response: OpenAiHttpResponse,
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_openai_responses_response(&response.body);
    }
    parse_http_response(response)
}

fn parse_responses_stream_http_response(
    response: OpenAiHttpStreamResponse,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_openai_responses_stream(&response.body, on_event);
    }
    parse_http_response(OpenAiHttpResponse {
        status: response.status,
        headers: response.headers,
        body: parse_http_body(response.body),
    })
}

pub fn parse_openai_responses_response(response: &Value) -> Result<LlmResponse, ProviderError> {
    let Some(object) = response.as_object() else {
        return Ok(LlmResponse {
            content: Some("Error: API returned an invalid response.".to_owned()),
            finish_reason: "error".to_owned(),
            ..LlmResponse::default()
        });
    };
    if let Some(error) = object.get("error").filter(|error| !error.is_null()) {
        let mut parsed = parse_error_response(error, object);
        parsed.finish_reason =
            finish_reason_from_openai_responses(object.get("status").and_then(Value::as_str))
                .to_owned();
        return Ok(parsed);
    }
    let mut content = String::new();
    let mut reasoning_content = String::new();
    let mut tool_calls = Vec::new();
    for item in object
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => collect_responses_message_text(item, &mut content),
            Some("reasoning") => collect_responses_reasoning(item, &mut reasoning_content),
            Some("function_call") => tool_calls.push(parse_responses_function_call(item)?),
            _ => {}
        }
    }
    Ok(LlmResponse {
        content: (!content.is_empty()).then_some(content),
        tool_calls: tool_calls.clone(),
        finish_reason: finish_reason_from_openai_responses(
            object.get("status").and_then(Value::as_str),
        )
        .to_owned(),
        usage: parse_responses_usage(object.get("usage")),
        reasoning_content: (!reasoning_content.is_empty()).then_some(reasoning_content),
        ..LlmResponse::default()
    })
}

pub fn parse_openai_responses_stream(
    body: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    let mut stream = OpenAiResponsesStreamState::default();
    for frame in parse_sse_frames(body) {
        if stream.process_frame(frame, on_event)? {
            break;
        }
    }
    stream.finish(on_event)
}

#[derive(Debug, Default)]
struct ChatCompletionsStreamState {
    content: String,
    reasoning_content: String,
    usage: BTreeMap<String, u64>,
    finish_reason: Option<String>,
    tool_buffers: BTreeMap<(u64, u64), StreamToolCallBuffer>,
    terminal_response: Option<LlmResponse>,
    done: bool,
}

impl ChatCompletionsStreamState {
    fn process_frame_text(
        &mut self,
        frame_text: &str,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<bool, ProviderError> {
        for frame in parse_sse_frames(frame_text) {
            if self.process_frame(frame, on_event)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn process_frame(
        &mut self,
        frame: SseFrame,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<bool, ProviderError> {
        if self.done {
            return Ok(true);
        }
        if frame.data.trim() == "[DONE]" {
            self.done = true;
            return Ok(true);
        }
        let value = parse_sse_json(&frame.data)?;
        if frame.event.as_deref() == Some("error") || value.get("error").is_some() {
            self.terminal_response = Some(parse_error_stream_value(&value));
            self.done = true;
            return Ok(true);
        }
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            if let Some(frame_usage) = value.get("usage") {
                self.usage = parse_usage(Some(frame_usage));
            }
            return Ok(false);
        };
        if choices.is_empty() {
            if let Some(frame_usage) = value.get("usage") {
                self.usage = parse_usage(Some(frame_usage));
            }
            return Ok(false);
        }
        for choice in choices.iter().filter_map(Value::as_object) {
            let choice_index = choice.get("index").and_then(Value::as_u64).unwrap_or(0);
            if let Some(delta) = choice.get("delta").and_then(Value::as_object) {
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        self.content.push_str(text);
                        on_event(ProviderEvent::TextDelta {
                            text: text.to_owned(),
                        });
                    }
                }
                if let Some(reasoning) = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .and_then(Value::as_str)
                {
                    if !reasoning.is_empty() {
                        self.reasoning_content.push_str(reasoning);
                        on_event(ProviderEvent::ReasoningDelta {
                            text: reasoning.to_owned(),
                        });
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    merge_chat_stream_tool_calls(
                        choice_index,
                        calls,
                        &mut self.tool_buffers,
                        on_event,
                    );
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(reason.to_owned());
            }
        }
        if let Some(frame_usage) = value.get("usage") {
            self.usage = parse_usage(Some(frame_usage));
        }
        Ok(false)
    }

    fn finish(self, on_event: &mut dyn FnMut(ProviderEvent)) -> Result<LlmResponse, ProviderError> {
        if let Some(response) = self.terminal_response {
            return Ok(response);
        }
        let tool_calls = finalize_stream_tool_calls(self.tool_buffers)?;
        for call in &tool_calls {
            on_event(ProviderEvent::ToolCallReady {
                id: call.id.clone(),
                name: call.name.clone(),
                input: Value::Object(call.arguments.clone()),
            });
        }
        let finish_reason =
            normalize_chat_finish_reason(self.finish_reason.as_deref(), !tool_calls.is_empty());
        on_event(ProviderEvent::Finish {
            usage: serde_json::to_value(&self.usage).unwrap_or(Value::Null),
            reason: finish_reason.clone(),
        });
        Ok(LlmResponse {
            content: (!self.content.is_empty()).then_some(self.content),
            tool_calls,
            finish_reason,
            usage: self.usage,
            reasoning_content: (!self.reasoning_content.is_empty())
                .then_some(self.reasoning_content),
            ..LlmResponse::default()
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct OpenAiResponsesStreamState {
    content: String,
    reasoning_content: String,
    finish_reason: String,
    usage: BTreeMap<String, u64>,
    tool_buffers: BTreeMap<String, StreamToolCallBuffer>,
    terminal_response: Option<LlmResponse>,
    done: bool,
}

impl OpenAiResponsesStreamState {
    pub(crate) fn process_frame_text(
        &mut self,
        frame_text: &str,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<bool, ProviderError> {
        if self.finish_reason.is_empty() {
            self.finish_reason = "stop".to_owned();
        }
        for frame in parse_sse_frames(frame_text) {
            if self.process_frame(frame, on_event)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn process_frame(
        &mut self,
        frame: SseFrame,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<bool, ProviderError> {
        if self.done {
            return Ok(true);
        }
        if frame.data.trim() == "[DONE]" {
            self.done = true;
            return Ok(true);
        }
        let value = parse_sse_json(&frame.data)?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .or(frame.event.as_deref());
        match event_type {
            Some("error" | "response.failed") => {
                self.terminal_response = Some(parse_error_stream_value(&value));
                self.done = true;
                return Ok(true);
            }
            Some("response.output_item.added") => {
                if let Some(item) = value.get("item").and_then(Value::as_object) {
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or("call_0")
                            .to_owned();
                        let buffer = self.tool_buffers.entry(call_id.clone()).or_default();
                        buffer.id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("fc_0")
                            .to_owned();
                        buffer.name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        buffer.arguments.push_str(
                            item.get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        );
                        on_event(ProviderEvent::ToolCallStart {
                            id: format!("{}|{}", call_id, buffer.id),
                            name: buffer.name.clone(),
                        });
                    }
                }
            }
            Some("response.output_text.delta") => {
                if let Some(delta) = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .filter(|delta| !delta.is_empty())
                {
                    self.content.push_str(delta);
                    on_event(ProviderEvent::TextDelta {
                        text: delta.to_owned(),
                    });
                }
            }
            Some("response.reasoning_text.delta" | "response.reasoning_summary_text.delta") => {
                if let Some(delta) = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .filter(|delta| !delta.is_empty())
                {
                    self.reasoning_content.push_str(delta);
                    on_event(ProviderEvent::ReasoningDelta {
                        text: delta.to_owned(),
                    });
                }
            }
            Some("response.function_call_arguments.delta") => {
                let call_id = value
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("call_0")
                    .to_owned();
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !delta.is_empty() {
                    let event_id = {
                        let buffer = self.tool_buffers.entry(call_id.clone()).or_default();
                        buffer.arguments.push_str(delta);
                        responses_stream_event_tool_id(&call_id, buffer)
                    };
                    on_event(ProviderEvent::ToolCallDelta {
                        id: event_id,
                        delta: delta.to_owned(),
                    });
                }
            }
            Some("response.function_call_arguments.done") => {
                if let Some(call_id) = value.get("call_id").and_then(Value::as_str) {
                    if let Some(buffer) = self.tool_buffers.get_mut(call_id) {
                        buffer.arguments = value
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                    }
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = value.get("item").and_then(Value::as_object) {
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or("call_0")
                            .to_owned();
                        let buffer = self.tool_buffers.entry(call_id).or_default();
                        if buffer.id.is_empty() {
                            buffer.id = item
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("fc_0")
                                .to_owned();
                        }
                        if buffer.name.is_empty() {
                            buffer.name = item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                        }
                        if buffer.arguments.is_empty() {
                            buffer.arguments = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or("{}")
                                .to_owned();
                        }
                    }
                }
            }
            Some("response.completed" | "response.incomplete") => {
                if let Some(response) = value.get("response").and_then(Value::as_object) {
                    self.finish_reason = finish_reason_from_openai_responses(
                        response.get("status").and_then(Value::as_str),
                    )
                    .to_owned();
                    self.usage = parse_responses_usage(response.get("usage"));
                    collect_responses_output_reasoning(response, &mut self.reasoning_content);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    pub(crate) fn finish(
        mut self,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        if self.finish_reason.is_empty() {
            self.finish_reason = "stop".to_owned();
        }
        if let Some(response) = self.terminal_response {
            return Ok(response);
        }
        let tool_calls = finalize_responses_stream_tool_calls(self.tool_buffers)?;
        for call in &tool_calls {
            on_event(ProviderEvent::ToolCallReady {
                id: call.id.clone(),
                name: call.name.clone(),
                input: Value::Object(call.arguments.clone()),
            });
        }
        on_event(ProviderEvent::Finish {
            usage: serde_json::to_value(&self.usage).unwrap_or(Value::Null),
            reason: self.finish_reason.clone(),
        });
        Ok(LlmResponse {
            content: (!self.content.is_empty()).then_some(self.content),
            tool_calls,
            finish_reason: self.finish_reason,
            usage: self.usage,
            reasoning_content: (!self.reasoning_content.is_empty())
                .then_some(self.reasoning_content),
            ..LlmResponse::default()
        })
    }
}

#[derive(Debug, Clone, Default)]
struct StreamToolCallBuffer {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SseFrame {
    event: Option<String>,
    data: String,
}

fn parse_sse_frames(body: &str) -> Vec<SseFrame> {
    let normalized = body.replace("\r\n", "\n");
    let mut frames = Vec::new();
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in normalized.lines() {
        if line.is_empty() {
            if !data_lines.is_empty() {
                frames.push(SseFrame {
                    event: event.take(),
                    data: data_lines.join("\n"),
                });
                data_lines.clear();
            } else {
                event = None;
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_owned());
        }
    }
    if !data_lines.is_empty() {
        frames.push(SseFrame {
            event,
            data: data_lines.join("\n"),
        });
    }
    frames
}

fn parse_sse_json(data: &str) -> Result<Value, ProviderError> {
    serde_json::from_str(data).map_err(|error| parse_error(format!("invalid SSE JSON: {error}")))
}

fn parse_error_stream_value(value: &Value) -> LlmResponse {
    if let Some(object) = value.as_object() {
        if let Some(error) = object.get("error") {
            return parse_error_response(error, object);
        }
        if let Some(message) = object.get("message").and_then(Value::as_str) {
            return LlmResponse {
                content: Some(format!("Error: {message}")),
                finish_reason: "error".to_owned(),
                ..LlmResponse::default()
            };
        }
    }
    LlmResponse {
        content: Some(format!("Error: {value}")),
        finish_reason: "error".to_owned(),
        ..LlmResponse::default()
    }
}

fn merge_chat_stream_tool_calls(
    choice_index: u64,
    calls: &[Value],
    buffers: &mut BTreeMap<(u64, u64), StreamToolCallBuffer>,
    on_event: &mut dyn FnMut(ProviderEvent),
) {
    for call in calls.iter().filter_map(Value::as_object) {
        let call_index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
        let key = (choice_index, call_index);
        let buffer = buffers.entry(key).or_default();
        let was_empty =
            buffer.id.is_empty() && buffer.name.is_empty() && buffer.arguments.is_empty();
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            buffer.id = id.to_owned();
        }
        let function = call.get("function").and_then(Value::as_object);
        if let Some(name) = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
        {
            buffer.name = name.to_owned();
        }
        if was_empty && !buffer.id.is_empty() && !buffer.name.is_empty() {
            on_event(ProviderEvent::ToolCallStart {
                id: buffer.id.clone(),
                name: buffer.name.clone(),
            });
        }
        if let Some(arguments) = function
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
        {
            if !arguments.is_empty() {
                buffer.arguments.push_str(arguments);
                on_event(ProviderEvent::ToolCallDelta {
                    id: buffer.id.clone(),
                    delta: arguments.to_owned(),
                });
            }
        }
    }
}

fn finalize_stream_tool_calls(
    buffers: BTreeMap<(u64, u64), StreamToolCallBuffer>,
) -> Result<Vec<ToolCallRequest>, ProviderError> {
    buffers
        .into_values()
        .map(tool_call_from_stream_buffer)
        .collect()
}

fn finalize_responses_stream_tool_calls(
    buffers: BTreeMap<String, StreamToolCallBuffer>,
) -> Result<Vec<ToolCallRequest>, ProviderError> {
    buffers
        .into_iter()
        .map(|(call_id, mut buffer)| {
            if buffer.id.is_empty() {
                buffer.id = "fc_0".to_owned();
            }
            buffer.id = format!("{}|{}", call_id, buffer.id);
            tool_call_from_stream_buffer(buffer)
        })
        .collect()
}

fn responses_stream_event_tool_id(call_id: &str, buffer: &StreamToolCallBuffer) -> String {
    if buffer.id.is_empty() {
        call_id.to_owned()
    } else {
        format!("{}|{}", call_id, buffer.id)
    }
}

fn tool_call_from_stream_buffer(
    buffer: StreamToolCallBuffer,
) -> Result<ToolCallRequest, ProviderError> {
    let arguments_value = if buffer.arguments.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(&buffer.arguments)
            .unwrap_or_else(|_| json!({"raw": buffer.arguments}))
    };
    let arguments = match arguments_value {
        Value::Object(arguments) => arguments,
        _ => Map::new(),
    };
    Ok(ToolCallRequest::new(buffer.id, buffer.name, arguments))
}

fn collect_responses_message_text(item: &Map<String, Value>, content: &mut String) {
    for block in item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        if block.get("type").and_then(Value::as_str) == Some("output_text") {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                content.push_str(text);
            }
        }
    }
}

fn collect_responses_reasoning(item: &Map<String, Value>, reasoning: &mut String) {
    for summary in item
        .get("summary")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        if summary.get("type").and_then(Value::as_str) == Some("summary_text") {
            if let Some(text) = summary.get("text").and_then(Value::as_str) {
                reasoning.push_str(text);
            }
        }
    }
}

fn collect_responses_output_reasoning(response: &Map<String, Value>, reasoning: &mut String) {
    for item in response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        if item.get("type").and_then(Value::as_str) == Some("reasoning") {
            collect_responses_reasoning(item, reasoning);
        }
    }
}

fn parse_responses_function_call(
    item: &Map<String, Value>,
) -> Result<ToolCallRequest, ProviderError> {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("fc_0");
    let arguments_raw = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let arguments = if arguments_raw.trim().is_empty() {
        Map::new()
    } else {
        match serde_json::from_str::<Value>(arguments_raw)
            .map_err(|error| parse_error(format!("invalid tool arguments JSON: {error}")))?
        {
            Value::Object(arguments) => arguments,
            _ => Map::new(),
        }
    };
    Ok(ToolCallRequest::new(
        format!("{call_id}|{item_id}"),
        item.get("name").and_then(Value::as_str).unwrap_or_default(),
        arguments,
    ))
}

fn parse_responses_usage(value: Option<&Value>) -> BTreeMap<String, u64> {
    let Some(object) = value.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    BTreeMap::from([
        (
            "prompt_tokens".to_owned(),
            object
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        (
            "completion_tokens".to_owned(),
            object
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
        (
            "total_tokens".to_owned(),
            object
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        ),
    ])
}

fn is_streaming_transport_unsupported(error: &ProviderError) -> bool {
    error
        .to_string()
        .contains("OpenAI-compatible streaming transport is not implemented")
}

fn is_direct_openai_base(base: &str) -> bool {
    let base = base.trim_end_matches('/').to_ascii_lowercase();
    base == "https://api.openai.com/v1" || base == "https://api.openai.com"
}

fn should_route_model_to_responses(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    ["gpt-5", "o1", "o3", "o4"]
        .iter()
        .any(|needle| model.contains(needle))
}

fn supports_temperature(model: &str, reasoning_effort: Option<&str>) -> bool {
    if reasoning_effort.is_some_and(|effort| !effort.eq_ignore_ascii_case("none")) {
        return false;
    }
    !should_route_model_to_responses(model)
}

fn should_fallback_from_responses_response(response: &LlmResponse) -> bool {
    if !matches!(response.error_status_code, Some(400 | 404 | 422)) {
        return false;
    }
    let text = response
        .content
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "responses",
        "response api",
        "max_output_tokens",
        "instructions",
        "previous_response",
        "unsupported",
        "not supported",
        "unknown parameter",
        "unrecognized request argument",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

pub fn normalize_chat_finish_reason(raw: Option<&str>, has_tool_calls: bool) -> String {
    match raw {
        Some("tool_calls" | "function_call") => "tool_calls".to_owned(),
        Some("stop") => "stop".to_owned(),
        Some("length") => "length".to_owned(),
        Some("content_filter") => "content_filter".to_owned(),
        Some(other) if !other.is_empty() => other.to_owned(),
        _ if has_tool_calls => "tool_calls".to_owned(),
        _ => "stop".to_owned(),
    }
}

fn parse_tool_call(value: &Value) -> Result<ToolCallRequest, ProviderError> {
    let Some(object) = value.as_object() else {
        return Err(parse_error("invalid tool call object"));
    };
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let arguments = parse_tool_arguments(function.and_then(|function| function.get("arguments")))?;
    let extra_content = object
        .get("extra_content")
        .and_then(Value::as_object)
        .cloned();
    let provider_specific_fields =
        collect_provider_fields(object, &["id", "type", "function", "extra_content"]);
    let function_provider_specific_fields = function
        .map(|function| collect_provider_fields(function, &["name", "arguments"]))
        .unwrap_or_default();

    let mut request = ToolCallRequest::new(id, name, arguments);
    request.extra_content = extra_content;
    request.provider_specific_fields =
        (!provider_specific_fields.is_empty()).then_some(provider_specific_fields);
    request.function_provider_specific_fields = (!function_provider_specific_fields.is_empty())
        .then_some(function_provider_specific_fields);
    Ok(request)
}

fn parse_tool_arguments(value: Option<&Value>) -> Result<Map<String, Value>, ProviderError> {
    match value {
        Some(Value::String(raw)) if raw.trim().is_empty() => Ok(Map::new()),
        Some(Value::String(raw)) => match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Ok(Map::new()),
            Err(error) => Err(parse_error(format!("invalid tool arguments JSON: {error}"))),
        },
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) | None => Ok(Map::new()),
    }
}

fn collect_provider_fields(
    object: &Map<String, Value>,
    standard_keys: &[&str],
) -> Map<String, Value> {
    object
        .iter()
        .filter(|(key, _)| !standard_keys.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn parse_usage(value: Option<&Value>) -> BTreeMap<String, u64> {
    let mut usage = BTreeMap::from([
        ("prompt_tokens".to_owned(), 0),
        ("completion_tokens".to_owned(), 0),
        ("total_tokens".to_owned(), 0),
    ]);
    let Some(object) = value.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
        if let Some(value) = object.get(key).and_then(Value::as_u64) {
            usage.insert(key.to_owned(), value);
        }
    }
    let cached_tokens = object
        .get("prompt_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| object.get("cached_tokens").and_then(Value::as_u64))
        .or_else(|| {
            object
                .get("prompt_cache_hit_tokens")
                .and_then(Value::as_u64)
        });
    if let Some(cached_tokens) = cached_tokens.filter(|value| *value > 0) {
        usage.insert("cached_tokens".to_owned(), cached_tokens);
    }
    usage
}

fn parse_error_response(error: &Value, object: &Map<String, Value>) -> LlmResponse {
    let error_object = error.as_object();
    let message = error_object
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("provider error");
    LlmResponse {
        content: Some(format!("Error: {message}")),
        finish_reason: "error".to_owned(),
        error_status_code: object
            .get("status")
            .or_else(|| object.get("status_code"))
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok()),
        error_type: error_object
            .and_then(|error| error.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        error_code: error_object
            .and_then(|error| error.get("code"))
            .and_then(error_code_to_string),
        error_retry_after_s: object.get("retry_after").and_then(Value::as_f64),
        error_should_retry: object.get("should_retry").and_then(Value::as_bool),
        ..LlmResponse::default()
    }
}

fn retry_after_seconds(headers: &BTreeMap<String, String>) -> Option<f64> {
    header_value(headers, "retry-after-ms")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|milliseconds| milliseconds / 1000.0)
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| header_value(headers, "retry-after").and_then(parse_retry_after))
}

fn should_retry(headers: &BTreeMap<String, String>) -> Option<bool> {
    header_value(headers, "x-should-retry").and_then(|value| {
        match value.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

fn header_value<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn parse_retry_after(value: &str) -> Option<f64> {
    let value = value.trim();
    value
        .parse::<f64>()
        .ok()
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| {
            let retry_at = httpdate::parse_http_date(value).ok()?;
            retry_at
                .duration_since(SystemTime::now())
                .ok()
                .map(|duration| duration.as_secs_f64())
                .filter(|seconds| *seconds > 0.0)
        })
}

fn error_code_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(code) => Some(code.clone()),
        Value::Number(code) => Some(code.to_string()),
        _ => None,
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|object| object.get("text"))
                        .and_then(Value::as_str)
                })
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn non_empty_text(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

fn parse_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
