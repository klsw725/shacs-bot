use super::{
    CodexHttpStreamResponse, CODEX_SSE_MAX_AGGREGATE_BYTES, CODEX_SSE_MAX_FRAME_BYTES,
    CODEX_SSE_MAX_LINE_BYTES,
};
use crate::clients::openai_compatible::parse_openai_responses_stream;
use crate::clients::sse::split_sse_frame_texts_bounded;
use crate::error::ProviderError;
use crate::provider::ProviderEvent;
use crate::types::LlmResponse;
use std::collections::BTreeMap;
use std::time::SystemTime;

pub fn parse_codex_stream(
    body: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    split_sse_frame_texts_bounded(
        body,
        CODEX_SSE_MAX_LINE_BYTES,
        CODEX_SSE_MAX_FRAME_BYTES,
        CODEX_SSE_MAX_AGGREGATE_BYTES,
    )
    .map_err(|error| super::api_error(None, error))?;
    parse_openai_responses_stream(body, on_event)
}

pub(super) fn parse_codex_stream_http_response(
    response: CodexHttpStreamResponse,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_codex_stream(&response.body, on_event);
    }
    Ok(codex_error_response(response.status, &response.headers))
}

fn codex_error_response(status: u16, headers: &BTreeMap<String, String>) -> LlmResponse {
    let content = if status == 429 {
        "ChatGPT usage quota exceeded or rate limit triggered. Please try again later.".to_owned()
    } else {
        format!("HTTP {status}: provider error")
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
