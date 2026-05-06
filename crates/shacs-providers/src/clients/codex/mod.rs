use crate::clients::openai_compatible::{build_responses_request, parse_openai_responses_stream};
use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderRequest};
use crate::registry::ProviderSpec;
use crate::types::LlmResponse;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_CODEX_API_BASE: &str = "https://chatgpt.com/backend-api";
const DEFAULT_ORIGINATOR: &str = "shacs-bot";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexRequestParts {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexHttpStreamResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub trait CodexHttpTransport: Send + Sync {
    fn post_json_stream(
        &self,
        request: CodexRequestParts,
    ) -> Result<CodexHttpStreamResponse, ProviderError>;
}

impl<F> CodexHttpTransport for F
where
    F: Fn(CodexRequestParts) -> Result<CodexHttpStreamResponse, ProviderError> + Send + Sync,
{
    fn post_json_stream(
        &self,
        request: CodexRequestParts,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        self(request)
    }
}

#[derive(Clone)]
pub struct UreqCodexHttpTransport {
    base_url: String,
    agent: ureq::Agent,
}

impl UreqCodexHttpTransport {
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
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl CodexHttpTransport for UreqCodexHttpTransport {
    fn post_json_stream(
        &self,
        request: CodexRequestParts,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .header("Accept", "text/event-stream")
            .content_type("application/json");
        for (key, value) in &request.headers {
            if key.eq_ignore_ascii_case("accept") || key.eq_ignore_ascii_case("content-type") {
                continue;
            }
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body).map_err(|error| api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| ProviderError::Api {
                status: Some(status),
                message: error.to_string(),
                retryable: false,
                headers: headers.clone(),
                body: None,
            })?;
        Ok(CodexHttpStreamResponse {
            status,
            headers,
            body,
        })
    }
}

#[derive(Clone)]
pub struct CodexClient<T> {
    config: ProviderConfig,
    transport: T,
}

impl<T> CodexClient<T>
where
    T: CodexHttpTransport,
{
    pub fn new(config: ProviderConfig, transport: T) -> Self {
        Self { config, transport }
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T> ProviderClient for CodexClient<T>
where
    T: CodexHttpTransport,
{
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let mut ignored_events = |_| {};
        self.chat_stream(request, &mut ignored_events)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        let parts = build_codex_responses_request(&request, &self.config);
        let response = self.transport.post_json_stream(parts)?;
        parse_codex_stream_http_response(response, on_event)
    }
}

pub fn codex_client_from_config(
    config: ProviderConfig,
    spec: &ProviderSpec,
) -> Result<CodexClient<UreqCodexHttpTransport>, ProviderError> {
    ensure_codex_backend(spec)?;
    let base_url = config
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .or(spec.default_api_base)
        .unwrap_or(DEFAULT_CODEX_API_BASE)
        .to_owned();
    Ok(CodexClient::new(
        config,
        UreqCodexHttpTransport::new(base_url),
    ))
}

fn ensure_codex_backend(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.backend == "openai_codex" {
        return Ok(());
    }
    Err(api_error(
        None,
        format!("provider '{}' does not use OpenAI Codex backend", spec.name),
    ))
}

pub fn build_codex_responses_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
) -> CodexRequestParts {
    let mut codex_request = request.clone();
    codex_request.model = strip_codex_model_prefix(&codex_request.model);
    let mut parts = build_responses_request(&codex_request, &ProviderConfig::default(), true);
    let Some(body) = parts.body.as_object_mut() else {
        return CodexRequestParts {
            path: "/codex/responses".to_owned(),
            headers: build_codex_headers(config),
            body: parts.body,
        };
    };
    body.remove("max_output_tokens");
    body.remove("temperature");
    body.entry("instructions".to_owned())
        .or_insert_with(|| Value::String(String::new()));
    body.insert("store".to_owned(), Value::Bool(false));
    body.insert("stream".to_owned(), Value::Bool(true));
    body.insert("text".to_owned(), json!({ "verbosity": "medium" }));
    body.insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
    body.insert(
        "prompt_cache_key".to_owned(),
        Value::String(prompt_cache_key(&request.messages)),
    );
    body.insert(
        "tool_choice".to_owned(),
        request
            .tool_choice
            .clone()
            .unwrap_or_else(|| Value::String("auto".to_owned())),
    );
    body.insert("parallel_tool_calls".to_owned(), Value::Bool(true));
    if let Some(extra_body) = &config.extra_body {
        merge_json_objects(body, extra_body);
        body.insert("stream".to_owned(), Value::Bool(true));
    }

    CodexRequestParts {
        path: "/codex/responses".to_owned(),
        headers: build_codex_headers(config),
        body: parts.body,
    }
}

pub fn build_codex_headers(config: &ProviderConfig) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        (
            "OpenAI-Beta".to_owned(),
            "responses=experimental".to_owned(),
        ),
        ("originator".to_owned(), DEFAULT_ORIGINATOR.to_owned()),
        ("User-Agent".to_owned(), "shacs-bot (rust)".to_owned()),
        ("accept".to_owned(), "text/event-stream".to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ]);
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

pub fn parse_codex_stream(
    body: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    parse_openai_responses_stream(body, on_event)
}

fn parse_codex_stream_http_response(
    response: CodexHttpStreamResponse,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_codex_stream(&response.body, on_event);
    }
    Ok(codex_error_response(
        response.status,
        &response.headers,
        response.body,
    ))
}

fn codex_error_response(
    status: u16,
    headers: &BTreeMap<String, String>,
    body: String,
) -> LlmResponse {
    let content = if status == 429 {
        "ChatGPT usage quota exceeded or rate limit triggered. Please try again later.".to_owned()
    } else if body.trim().is_empty() {
        format!("HTTP {status}: provider error")
    } else {
        format!("HTTP {status}: {body}")
    };
    LlmResponse {
        content: Some(content),
        finish_reason: "error".to_owned(),
        error_status_code: Some(status),
        error_retry_after_s: retry_after_seconds(headers),
        error_should_retry: should_retry(headers),
        ..LlmResponse::default()
    }
}

fn strip_codex_model_prefix(model: &str) -> String {
    for prefix in ["openai-codex/", "openai_codex/", "openai/"] {
        if let Some(stripped) = model.strip_prefix(prefix) {
            return stripped.to_owned();
        }
    }
    model.to_owned()
}

fn prompt_cache_key(messages: &[Value]) -> String {
    let raw = python_json_dumps(&Value::Array(messages.to_vec()));
    let digest = Sha256::digest(raw.as_bytes());
    format!("{digest:x}")
}

fn python_json_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => python_json_number(value),
        Value::String(value) => python_json_string(value),
        Value::Array(items) => {
            let items = items.iter().map(python_json_dumps).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let entries = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}: {}",
                        python_json_string(key),
                        python_json_dumps(&object[key])
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", entries.join(", "))
        }
    }
}

fn python_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' || character == '\u{7f}' => {
                output.push_str(&format!("\\u{:04x}", u32::from(character)));
            }
            character if !character.is_ascii() => {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    output.push_str(&format!("\\u{unit:04x}"));
                }
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn python_json_number(value: &Number) -> String {
    let raw = value.to_string();
    let Some(exponent_index) = raw.find(['e', 'E']) else {
        return raw;
    };
    let mantissa = &raw[..exponent_index];
    let exponent = &raw[exponent_index + 1..];
    let (sign, digits) = match exponent.strip_prefix(['+', '-']) {
        Some(digits) if exponent.starts_with('-') => ("-", digits),
        Some(digits) => ("+", digits),
        None => ("+", exponent),
    };
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    format!("{mantissa}e{sign}{digits:0>2}")
}

fn merge_json_objects(target: &mut Map<String, Value>, source: &Map<String, Value>) {
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

fn retry_after_seconds(headers: &BTreeMap<String, String>) -> Option<f64> {
    header_value(headers, "retry-after-ms")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|milliseconds| milliseconds / 1000.0)
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| {
            header_value(headers, "retry-after")
                .and_then(|value| parse_retry_after_header(value.trim()))
        })
}

fn parse_retry_after_header(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .and_then(|time| time.duration_since(SystemTime::now()).ok())
                .map(|duration| duration.as_secs_f64())
                .filter(|seconds| *seconds > 0.0)
        })
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

fn join_base_and_path(base: &str, path: &str) -> Result<String, ProviderError> {
    if base.trim().is_empty() {
        return Err(api_error(None, "missing Codex base URL"));
    }
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    Ok(format!("{base}/{path}"))
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
        retryable: status.is_some_and(|status| status == 429 || status >= 500),
        headers: BTreeMap::new(),
        body: None,
    }
}
