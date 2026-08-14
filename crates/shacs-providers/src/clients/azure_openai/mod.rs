use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderInvocation, ProviderRequest};
use crate::registry::ProviderSpec;
use crate::types::LlmResponse;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::openai_compatible::{
    build_headers, build_responses_request, parse_openai_responses_response,
    parse_openai_responses_stream, OpenAiCompatibleRequestParts, OpenAiHttpResponse,
    OpenAiHttpStreamResponse, OpenAiHttpTransport, OpenAiResponsesStreamState,
    UreqOpenAiHttpTransport,
};

const STREAMING_NOT_IMPLEMENTED: &str = "OpenAI-compatible streaming transport is not implemented";

#[derive(Clone)]
pub struct AzureOpenAiClient<T> {
    config: ProviderConfig,
    transport: T,
    session_affinity: String,
}

impl<T> AzureOpenAiClient<T>
where
    T: OpenAiHttpTransport,
{
    pub fn new(config: ProviderConfig, transport: T) -> Self {
        Self::with_session_affinity(config, transport, default_session_affinity())
    }

    pub fn with_session_affinity(
        config: ProviderConfig,
        transport: T,
        session_affinity: impl Into<String>,
    ) -> Self {
        Self {
            config,
            transport,
            session_affinity: session_affinity.into(),
        }
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T> ProviderClient for AzureOpenAiClient<T>
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
            return Err(api_error("provider invocation cancelled"));
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
            return Err(api_error("provider invocation cancelled"));
        }
        self.chat_stream_bounded(request, on_event, invocation.remaining())
    }
}

impl<T> AzureOpenAiClient<T>
where
    T: OpenAiHttpTransport,
{
    fn chat_bounded(
        &self,
        request: ProviderRequest,
        timeout: Option<Duration>,
    ) -> Result<LlmResponse, ProviderError> {
        let parts = build_azure_openai_responses_request(
            &request,
            &self.config,
            false,
            &self.session_affinity,
        );
        parse_azure_http_response(self.transport.post_json_bounded(parts, timeout)?)
    }

    fn chat_stream_bounded(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        timeout: Option<Duration>,
    ) -> Result<LlmResponse, ProviderError> {
        let parts = build_azure_openai_responses_request(
            &request,
            &self.config,
            true,
            &self.session_affinity,
        );
        let mut stream = OpenAiResponsesStreamState::default();
        match self.transport.post_json_stream_frames_bounded(
            parts,
            &mut |frame| stream.process_frame_text(frame, on_event),
            timeout,
        ) {
            Ok(response) => {
                if (200..300).contains(&response.status) {
                    stream.finish(on_event)
                } else {
                    parse_azure_stream_response(response, on_event)
                }
            }
            Err(error) if error.to_string().contains(STREAMING_NOT_IMPLEMENTED) => {
                self.chat_bounded(request, timeout)
            }
            Err(error) => Err(error),
        }
    }
}

pub fn azure_openai_client_from_config(
    config: ProviderConfig,
    spec: &ProviderSpec,
) -> Result<AzureOpenAiClient<UreqOpenAiHttpTransport>, ProviderError> {
    ensure_azure_openai_backend(spec)?;
    ensure_non_empty(config.api_key.as_deref(), "api_key")?;
    let base_url = resolve_azure_openai_api_base(&config)?;
    Ok(AzureOpenAiClient::new(
        config,
        UreqOpenAiHttpTransport::new(base_url),
    ))
}

pub fn resolve_azure_openai_api_base(config: &ProviderConfig) -> Result<String, ProviderError> {
    let api_base = ensure_non_empty(config.api_base.as_deref(), "api_base")?;
    Ok(format!(
        "{}/openai/v1/",
        api_base.trim().trim_end_matches('/')
    ))
}

pub fn build_azure_openai_responses_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
    stream: bool,
    session_affinity: &str,
) -> OpenAiCompatibleRequestParts {
    let mut parts = build_responses_request(request, config, stream);
    parts.headers = build_azure_openai_headers(config, session_affinity);
    parts
}

pub fn build_azure_openai_headers(
    config: &ProviderConfig,
    session_affinity: &str,
) -> BTreeMap<String, String> {
    let mut headers = build_headers(config);
    headers
        .entry("x-session-affinity".to_owned())
        .or_insert_with(|| session_affinity.to_owned());
    headers
}

fn ensure_azure_openai_backend(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.backend == "azure_openai" {
        return Ok(());
    }
    Err(api_error(format!(
        "provider '{}' does not use Azure OpenAI backend",
        spec.name
    )))
}

fn ensure_non_empty<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, ProviderError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| api_error(format!("Azure OpenAI {field} is required")))
}

fn parse_azure_http_response(response: OpenAiHttpResponse) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_openai_responses_response(&response.body);
    }
    Ok(error_response_from_body(
        response.status,
        response.headers,
        response.body,
    ))
}

fn parse_azure_stream_response(
    response: OpenAiHttpStreamResponse,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_openai_responses_stream(&response.body, on_event);
    }
    let body =
        serde_json::from_str::<Value>(&response.body).unwrap_or(Value::String(response.body));
    Ok(error_response_from_body(
        response.status,
        response.headers,
        body,
    ))
}

fn error_response_from_body(
    status: u16,
    headers: BTreeMap<String, String>,
    body: Value,
) -> LlmResponse {
    let error = match &body {
        Value::Object(object) => object
            .get("error")
            .cloned()
            .unwrap_or_else(|| error_from_body(&body)),
        _ => error_from_body(&body),
    };
    let error_object = error.as_object();
    let message = error_object
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("provider error");
    LlmResponse {
        content: Some(format!("Error: {message}")),
        finish_reason: "error".to_owned(),
        error_status_code: Some(status),
        error_type: error_object
            .and_then(|error| error.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        error_code: error_object
            .and_then(|error| error.get("code"))
            .and_then(error_code_to_string),
        error_retry_after_s: retry_after_seconds(&headers),
        error_should_retry: should_retry(&headers),
        retry_after: retry_after_seconds(&headers),
        ..LlmResponse::default()
    }
}

fn error_from_body(body: &Value) -> Value {
    match body {
        Value::Object(object) => object
            .get("message")
            .or_else(|| object.get("error_description"))
            .or_else(|| object.get("content"))
            .and_then(value_to_text)
            .map(|message| json!({ "message": message }))
            .unwrap_or_else(|| json!({ "message": "provider error" })),
        Value::String(message) => json!({ "message": message }),
        Value::Null => json!({ "message": "provider error" }),
        other => json!({ "message": other.to_string() }),
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
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

fn default_session_affinity() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}{:x}", process::id(), nanos)
}

fn api_error(error: impl ToString) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: error.to_string(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
