use super::{
    CodexHttpStreamResponse, CODEX_ERROR_CODE_BODY_MAX_BYTES, CODEX_SSE_MAX_AGGREGATE_BYTES,
    CODEX_SSE_MAX_FRAME_BYTES, CODEX_SSE_MAX_LINE_BYTES,
};
use crate::clients::openai_compatible::parse_openai_responses_stream;
use crate::clients::sse::split_sse_frame_texts_bounded;
use crate::error::ProviderError;
use crate::provider::ProviderEvent;
use crate::types::LlmResponse;
use serde_json::Value;
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
    model: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_codex_stream(&response.body, on_event);
    }
    Ok(codex_error_response(
        response.status,
        &response.headers,
        &response.body,
        model,
    ))
}

fn codex_error_response(
    status: u16,
    headers: &BTreeMap<String, String>,
    body: &str,
    model: &str,
) -> LlmResponse {
    let content = if status == 429 {
        "ChatGPT usage quota exceeded or rate limit triggered. Please try again later.".to_owned()
    } else if let Some(explanation) = known_codex_error(body, model) {
        format!("HTTP {status}: {explanation}")
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

const KNOWN_CODEX_ERROR_CODES: [&str; 1] = ["model_not_found"];

pub(super) fn known_codex_error_code(body: &str) -> Option<&'static str> {
    if body.len() > CODEX_ERROR_CODE_BODY_MAX_BYTES {
        return None;
    }
    let body: Value = serde_json::from_str(body).ok()?;
    let code = body.get("error")?.get("code")?.as_str()?;
    KNOWN_CODEX_ERROR_CODES
        .into_iter()
        .find(|known| *known == code)
}

/// Our own sentence for an allowlisted `error.code`. Nothing the provider
/// wrote goes into it (Spec 019 PRD 000, Spec 034); the only other value is
/// the model name this request used.
fn known_codex_error(body: &str, model: &str) -> Option<String> {
    match known_codex_error_code(body)? {
        "model_not_found" => {
            let subject = if plain_model_name(model) {
                format!("the model '{model}'")
            } else {
                "the configured model".to_owned()
            };
            Some(format!(
                "{subject} is not available for this ChatGPT account (model_not_found). Set agents.defaults.model to a model the account offers, or run shacs-bot provider codex login again to pick one."
            ))
        }
        _ => None,
    }
}

fn plain_model_name(model: &str) -> bool {
    (1..=100).contains(&model.len())
        && model.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ':' | '/')
        })
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
