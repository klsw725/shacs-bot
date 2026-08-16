use super::{
    CodexHttpStreamResponse, CodexHttpTransport, CodexRequestParts, UreqCodexHttpTransport,
    CODEX_ERROR_RESPONSE_MAX_BYTES, CODEX_ERROR_RESPONSE_READ_LIMIT, CODEX_SSE_MAX_AGGREGATE_BYTES,
    CODEX_SSE_MAX_FRAME_BYTES, CODEX_SSE_MAX_LINE_BYTES, DEFAULT_HTTP_TIMEOUT,
};
use crate::clients::sse::read_sse_frame_texts_bounded;
use crate::error::ProviderError;
use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

impl UreqCodexHttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, DEFAULT_HTTP_TIMEOUT)
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            agent: ureq::Agent::config_builder()
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

impl CodexHttpTransport for UreqCodexHttpTransport {
    fn post_json_stream(
        &self,
        request: CodexRequestParts,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        let mut ignored_frames = |_frame: &str| -> Result<bool, ProviderError> { Ok(false) };
        self.post_json_stream_frames_bounded(request, &mut ignored_frames, None)
    }

    fn post_json_stream_frames_bounded(
        &self,
        request: CodexRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
        timeout: Option<Duration>,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .config()
            .timeout_global(timeout)
            .build()
            .header("Accept", "text/event-stream")
            .content_type("application/json");
        for (key, value) in &request.headers {
            if key.eq_ignore_ascii_case("accept") || key.eq_ignore_ascii_case("content-type") {
                continue;
            }
            http_request = http_request.header(key, value);
        }
        let body =
            serde_json::to_string(&request.body).map_err(|error| super::api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body = if (200..300).contains(&status) {
            read_sse_frame_texts_bounded(
                response.body_mut().as_reader(),
                on_frame,
                |error| ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                },
                CODEX_SSE_MAX_LINE_BYTES,
                CODEX_SSE_MAX_FRAME_BYTES,
                CODEX_SSE_MAX_AGGREGATE_BYTES,
            )?
        } else {
            read_codex_error_body_bounded(&mut response, status)?;
            String::new()
        };
        Ok(CodexHttpStreamResponse {
            status,
            headers,
            body,
        })
    }
}

fn response_headers(headers: &ureq::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str().to_ascii_lowercase().as_str(),
                "retry-after" | "retry-after-ms" | "x-should-retry"
            )
        })
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

fn read_codex_error_body_bounded(
    response: &mut ureq::http::Response<ureq::Body>,
    status: u16,
) -> Result<(), ProviderError> {
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > CODEX_ERROR_RESPONSE_MAX_BYTES)
    {
        return Err(codex_error_body_error(status));
    }
    let mut body = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit(CODEX_ERROR_RESPONSE_READ_LIMIT)
        .reader()
        .take(CODEX_ERROR_RESPONSE_READ_LIMIT)
        .read_to_end(&mut body)
        .map_err(|_| ProviderError::Api {
            status: Some(status),
            message: "codex_error_response_read_failed".to_owned(),
            retryable: retryable_status(status),
            headers: BTreeMap::new(),
            body: None,
        })?;
    if body.len() > CODEX_ERROR_RESPONSE_MAX_BYTES {
        return Err(codex_error_body_error(status));
    }
    Ok(())
}

fn codex_error_body_error(status: u16) -> ProviderError {
    ProviderError::Api {
        status: Some(status),
        message: "codex_error_response_body_too_large".to_owned(),
        retryable: retryable_status(status),
        headers: BTreeMap::new(),
        body: None,
    }
}

fn retryable_status(status: u16) -> bool {
    status == 408 || status == 409 || status == 429 || status >= 500
}

fn join_base_and_path(base: &str, path: &str) -> Result<String, ProviderError> {
    if base.trim().is_empty() {
        return Err(super::api_error(None, "missing Codex base URL"));
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
