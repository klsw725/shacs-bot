use crate::config::ProviderConfig;
use crate::error::ProviderError;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TRANSCRIPTION_TIMEOUT: Duration = Duration::from_secs(60);
const OPENAI_TRANSCRIPTION_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const GROQ_TRANSCRIPTION_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const TRANSCRIPTION_PATH: &str = "/audio/transcriptions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest {
    pub file_path: PathBuf,
    pub model: Option<String>,
    pub language: Option<String>,
    pub prompt: Option<String>,
}

impl TranscriptionRequest {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
            model: None,
            language: None,
            prompt: None,
        }
    }
}

pub trait TranscriptionClient: Send + Sync {
    fn transcribe(&self, request: TranscriptionRequest) -> Result<String, ProviderError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTranscriptionRequestParts {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTranscriptionHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub trait AudioTranscriptionHttpTransport: Send + Sync {
    fn post_multipart(
        &self,
        request: AudioTranscriptionRequestParts,
    ) -> Result<AudioTranscriptionHttpResponse, ProviderError>;
}

impl<F> AudioTranscriptionHttpTransport for F
where
    F: Fn(AudioTranscriptionRequestParts) -> Result<AudioTranscriptionHttpResponse, ProviderError>
        + Send
        + Sync,
{
    fn post_multipart(
        &self,
        request: AudioTranscriptionRequestParts,
    ) -> Result<AudioTranscriptionHttpResponse, ProviderError> {
        self(request)
    }
}

#[derive(Clone)]
pub struct UreqAudioTranscriptionHttpTransport {
    agent: ureq::Agent,
}

impl Default for UreqAudioTranscriptionHttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqAudioTranscriptionHttpTransport {
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_TRANSCRIPTION_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }
}

impl AudioTranscriptionHttpTransport for UreqAudioTranscriptionHttpTransport {
    fn post_multipart(
        &self,
        request: AudioTranscriptionRequestParts,
    ) -> Result<AudioTranscriptionHttpResponse, ProviderError> {
        let mut http_request = self
            .agent
            .post(&request.url)
            .header("Accept", "application/json")
            .header("Content-Type", &request.content_type);
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let mut response = http_request.send(request.body).map_err(map_ureq_error)?;
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
        Ok(AudioTranscriptionHttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[derive(Clone)]
pub struct OpenAiTranscriptionClient<T> {
    api_key: String,
    api_url: String,
    language: Option<String>,
    transport: T,
}

#[derive(Clone)]
pub struct GroqTranscriptionClient<T> {
    api_key: String,
    api_url: String,
    language: Option<String>,
    transport: T,
}

impl<T> OpenAiTranscriptionClient<T>
where
    T: AudioTranscriptionHttpTransport,
{
    pub fn new(
        api_key: impl Into<String>,
        api_url: impl Into<String>,
        language: Option<String>,
        transport: T,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_url: api_url.into(),
            language,
            transport,
        }
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}

impl<T> GroqTranscriptionClient<T>
where
    T: AudioTranscriptionHttpTransport,
{
    pub fn new(
        api_key: impl Into<String>,
        api_url: impl Into<String>,
        language: Option<String>,
        transport: T,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_url: api_url.into(),
            language,
            transport,
        }
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}

impl<T> TranscriptionClient for OpenAiTranscriptionClient<T>
where
    T: AudioTranscriptionHttpTransport,
{
    fn transcribe(&self, request: TranscriptionRequest) -> Result<String, ProviderError> {
        transcribe_with_transport(
            &self.transport,
            &self.api_url,
            &self.api_key,
            request,
            self.language.as_deref(),
            "whisper-1",
        )
    }
}

impl<T> TranscriptionClient for GroqTranscriptionClient<T>
where
    T: AudioTranscriptionHttpTransport,
{
    fn transcribe(&self, request: TranscriptionRequest) -> Result<String, ProviderError> {
        transcribe_with_transport(
            &self.transport,
            &self.api_url,
            &self.api_key,
            request,
            self.language.as_deref(),
            "whisper-large-v3",
        )
    }
}

pub fn openai_transcription_client_from_config(
    config: ProviderConfig,
    language: Option<String>,
) -> Result<OpenAiTranscriptionClient<UreqAudioTranscriptionHttpTransport>, ProviderError> {
    let api_key = api_key_from_config_or_env(&config, "OPENAI_API_KEY", "openai")?;
    let api_url = resolve_transcription_api_url(
        config.api_base.as_deref(),
        env::var("OPENAI_TRANSCRIPTION_BASE_URL").ok().as_deref(),
        OPENAI_TRANSCRIPTION_URL,
    );
    Ok(OpenAiTranscriptionClient::new(
        api_key,
        api_url,
        language,
        UreqAudioTranscriptionHttpTransport::new(),
    ))
}

pub fn groq_transcription_client_from_config(
    config: ProviderConfig,
    language: Option<String>,
) -> Result<GroqTranscriptionClient<UreqAudioTranscriptionHttpTransport>, ProviderError> {
    let api_key = api_key_from_config_or_env(&config, "GROQ_API_KEY", "groq")?;
    let api_url = resolve_transcription_api_url(
        config.api_base.as_deref(),
        env::var("GROQ_BASE_URL").ok().as_deref(),
        GROQ_TRANSCRIPTION_URL,
    );
    Ok(GroqTranscriptionClient::new(
        api_key,
        api_url,
        language,
        UreqAudioTranscriptionHttpTransport::new(),
    ))
}

pub fn transcription_client_from_config(
    provider_id: &str,
    config: ProviderConfig,
    language: Option<String>,
) -> Result<Box<dyn TranscriptionClient>, ProviderError> {
    match provider_id {
        "openai" => Ok(Box::new(openai_transcription_client_from_config(
            config, language,
        )?)),
        "groq" => Ok(Box::new(groq_transcription_client_from_config(
            config, language,
        )?)),
        other => Err(ProviderError::ProviderNotFound {
            provider_id: other.to_owned(),
            suggestions: vec!["groq".to_owned(), "openai".to_owned()],
        }),
    }
}

pub fn resolve_transcription_api_url(
    configured: Option<&str>,
    env_override: Option<&str>,
    default_url: &str,
) -> String {
    let selected = configured
        .or(env_override)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_url);
    if selected
        .trim_end_matches('/')
        .ends_with(TRANSCRIPTION_PATH.trim_start_matches('/'))
    {
        selected.to_owned()
    } else {
        format!(
            "{}/{}",
            selected.trim_end_matches('/'),
            TRANSCRIPTION_PATH.trim_start_matches('/')
        )
    }
}

pub fn parse_transcription_response(
    response: AudioTranscriptionHttpResponse,
) -> Result<String, ProviderError> {
    if (200..300).contains(&response.status) {
        if response.body.trim().is_empty() {
            return Ok(String::new());
        }
        let parsed = serde_json::from_str::<Value>(&response.body).map_err(|error| {
            api_error(Some(response.status), error, false, response.headers, None)
        })?;
        return Ok(parsed
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned());
    }
    let message = error_message_from_body(&response.body);
    Err(api_error(
        Some(response.status),
        message,
        is_retryable_status(response.status),
        response.headers,
        Some(response.body),
    ))
}

fn transcribe_with_transport<T>(
    transport: &T,
    api_url: &str,
    api_key: &str,
    request: TranscriptionRequest,
    client_language: Option<&str>,
    default_model: &str,
) -> Result<String, ProviderError>
where
    T: AudioTranscriptionHttpTransport,
{
    let file_bytes = fs::read(&request.file_path).map_err(|error| {
        api_error(
            None,
            format!(
                "audio file '{}' could not be read: {error}",
                request.file_path.display()
            ),
            false,
            BTreeMap::new(),
            None,
        )
    })?;
    let file_name = request
        .file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio");
    let model = request.model.as_deref().unwrap_or(default_model);
    let language = request.language.as_deref().or(client_language);
    let parts = build_audio_transcription_request(
        api_url,
        api_key,
        file_name,
        &file_bytes,
        model,
        language,
        request.prompt.as_deref(),
    );
    parse_transcription_response(transport.post_multipart(parts)?)
}

pub fn build_audio_transcription_request(
    api_url: &str,
    api_key: &str,
    file_name: &str,
    file_bytes: &[u8],
    model: &str,
    language: Option<&str>,
    prompt: Option<&str>,
) -> AudioTranscriptionRequestParts {
    let boundary = multipart_boundary();
    let mut body = Vec::new();
    append_file_field(&mut body, &boundary, "file", file_name, file_bytes);
    append_text_field(&mut body, &boundary, "model", model);
    append_text_field(&mut body, &boundary, "response_format", "json");
    if let Some(language) = language.filter(|value| !value.is_empty()) {
        append_text_field(&mut body, &boundary, "language", language);
    }
    if let Some(prompt) = prompt.filter(|value| !value.is_empty()) {
        append_text_field(&mut body, &boundary, "prompt", prompt);
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    AudioTranscriptionRequestParts {
        url: api_url.to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {api_key}"))]),
        content_type: format!("multipart/form-data; boundary={boundary}"),
        body,
    }
}

fn append_text_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
            escape_multipart_value(name)
        )
        .as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn append_file_field(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    file_name: &str,
    file_bytes: &[u8],
) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
            escape_multipart_value(name),
            escape_multipart_value(file_name)
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(b"\r\n");
}

fn escape_multipart_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], " ")
}

fn multipart_boundary() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("shacs-bot-{:x}-{nanos:x}", process::id())
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

fn error_message_from_body(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return if body.trim().is_empty() {
            "transcription provider error".to_owned()
        } else {
            body.to_owned()
        };
    };
    value
        .get("error")
        .and_then(|error| match error {
            Value::Object(object) => object.get("message").and_then(Value::as_str),
            Value::String(message) => Some(message.as_str()),
            _ => None,
        })
        .or_else(|| value.get("message").and_then(Value::as_str))
        .unwrap_or("transcription provider error")
        .to_owned()
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || status >= 500
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
