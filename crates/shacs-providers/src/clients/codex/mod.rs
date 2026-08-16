use crate::clients::openai_compatible::OpenAiResponsesStreamState;
use crate::clients::sse::split_sse_frame_texts_bounded;
use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderInvocation, ProviderRequest};
use crate::types::LlmResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_CODEX_API_BASE: &str = "https://chatgpt.com/backend-api";
const DEFAULT_ORIGINATOR: &str = "shacs-bot";
pub const CODEX_ERROR_RESPONSE_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const CODEX_ERROR_RESPONSE_READ_LIMIT: u64 = 32 * 1024 * 1024 + 1;

mod media;
mod request;
mod response;
mod transport;

pub use media::{
    parse_codex_media_stream, CODEX_SSE_MAX_AGGREGATE_BYTES, CODEX_SSE_MAX_FRAME_BYTES,
    CODEX_SSE_MAX_LINE_BYTES, CODEX_SSE_MAX_PARTIAL_IMAGES,
};
pub use request::{build_codex_headers, build_codex_responses_request, codex_client_from_config};
pub use response::parse_codex_stream;

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

    fn post_json_stream_frames(
        &self,
        request: CodexRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        self.post_json_stream_frames_bounded(request, on_frame, None)
    }

    fn post_json_stream_frames_bounded(
        &self,
        request: CodexRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
        _timeout: Option<Duration>,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        let response = self.post_json_stream(request)?;
        if (200..300).contains(&response.status) {
            for frame in split_sse_frame_texts_bounded(
                &response.body,
                CODEX_SSE_MAX_LINE_BYTES,
                CODEX_SSE_MAX_FRAME_BYTES,
                CODEX_SSE_MAX_AGGREGATE_BYTES,
            )
            .map_err(|error| api_error(None, error))?
            {
                if on_frame(&frame)? {
                    break;
                }
            }
        }
        Ok(response)
    }
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

    fn chat_with_invocation(
        &self,
        request: ProviderRequest,
        invocation: &ProviderInvocation,
    ) -> Result<LlmResponse, ProviderError> {
        let mut ignored_events = |_| {};
        self.chat_stream_with_invocation(request, &mut ignored_events, invocation)
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
            return Err(api_error(None, "provider invocation cancelled"));
        }
        self.chat_stream_bounded(request, on_event, invocation.remaining())
    }
}

impl<T> CodexClient<T>
where
    T: CodexHttpTransport,
{
    fn chat_stream_bounded(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
        timeout: Option<Duration>,
    ) -> Result<LlmResponse, ProviderError> {
        let parts = build_codex_responses_request(&request, &self.config);
        let mut stream = OpenAiResponsesStreamState::default();
        let response = self.transport.post_json_stream_frames_bounded(
            parts,
            &mut |frame| stream.process_frame_text(frame, on_event),
            timeout,
        )?;
        if (200..300).contains(&response.status) {
            stream.finish(on_event)
        } else {
            response::parse_codex_stream_http_response(response, on_event)
        }
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
