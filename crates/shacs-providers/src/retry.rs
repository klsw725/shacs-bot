use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderRequest};
use crate::types::LlmResponse;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

const STANDARD_DELAYS: [f64; 3] = [1.0, 2.0, 4.0];
const PERSISTENT_MAX_DELAY_S: f64 = 60.0;
const PERSISTENT_IDENTICAL_ERROR_LIMIT: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRetryMode {
    Standard,
    Persistent,
}

impl ProviderRetryMode {
    pub fn from_config(value: &str) -> Self {
        if value.eq_ignore_ascii_case("persistent") {
            Self::Persistent
        } else {
            Self::Standard
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryStopReason {
    NotError,
    NonTransient,
    AttemptsExhausted,
    IdenticalTransientErrorLimit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRetryDecision {
    pub should_retry: bool,
    pub delay_s: Option<f64>,
    pub stop_reason: Option<RetryStopReason>,
}

pub trait ProviderRetryWaiter {
    fn wait(&mut self, delay_s: f64, message: &str);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadRetryWaiter;

impl ProviderRetryWaiter for ThreadRetryWaiter {
    fn wait(&mut self, delay_s: f64, _message: &str) {
        thread::sleep(Duration::from_secs_f64(delay_s.max(0.0)));
    }
}

pub fn chat_with_retry(
    client: &dyn ProviderClient,
    request: ProviderRequest,
    mode: ProviderRetryMode,
) -> Result<LlmResponse, ProviderError> {
    let mut waiter = ThreadRetryWaiter;
    chat_with_retry_using_waiter(client, request, mode, &mut waiter)
}

pub fn chat_with_retry_using_waiter(
    client: &dyn ProviderClient,
    mut request: ProviderRequest,
    mode: ProviderRetryMode,
    waiter: &mut dyn ProviderRetryWaiter,
) -> Result<LlmResponse, ProviderError> {
    let mut attempt = 0;
    let mut last_error_key = None;
    let mut identical_error_count = 0;
    let mut image_fallback_used = false;
    loop {
        attempt += 1;
        match client.chat(request.clone()) {
            Ok(response) if response.finish_reason != "error" => return Ok(response),
            Ok(response) => {
                update_identical_error_count(
                    response.content.as_deref(),
                    &mut last_error_key,
                    &mut identical_error_count,
                );
                let decision =
                    retry_decision_for_response(&response, attempt, mode, identical_error_count);
                if !decision.should_retry {
                    if decision.stop_reason == Some(RetryStopReason::NonTransient)
                        && !image_fallback_used
                    {
                        if let Some(stripped_request) = request_with_stripped_images(&request) {
                            request = stripped_request;
                            image_fallback_used = true;
                            continue;
                        }
                    }
                    return Ok(response);
                }
                let delay_s = decision
                    .delay_s
                    .unwrap_or_else(|| base_delay(attempt, mode));
                waiter.wait(
                    delay_s,
                    &retry_wait_message(attempt, mode, delay_s, response.content.as_deref()),
                );
            }
            Err(error) => {
                let error_text = error.to_string();
                update_identical_error_count(
                    Some(&error_text),
                    &mut last_error_key,
                    &mut identical_error_count,
                );
                let decision = retry_decision_for_error_with_identical_count(
                    &error,
                    attempt,
                    mode,
                    identical_error_count,
                );
                if !decision.should_retry {
                    return Err(error);
                }
                let delay_s = decision
                    .delay_s
                    .unwrap_or_else(|| base_delay(attempt, mode));
                waiter.wait(
                    delay_s,
                    &retry_wait_message(attempt, mode, delay_s, Some(&error_text)),
                );
            }
        }
    }
}

pub fn chat_stream_with_retry(
    client: &dyn ProviderClient,
    request: ProviderRequest,
    mode: ProviderRetryMode,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    let mut waiter = ThreadRetryWaiter;
    chat_stream_with_retry_using_waiter(client, request, mode, on_event, &mut waiter)
}

pub fn chat_stream_with_retry_using_waiter(
    client: &dyn ProviderClient,
    mut request: ProviderRequest,
    mode: ProviderRetryMode,
    on_event: &mut dyn FnMut(ProviderEvent),
    waiter: &mut dyn ProviderRetryWaiter,
) -> Result<LlmResponse, ProviderError> {
    let mut attempt = 0;
    let mut last_error_key = None;
    let mut identical_error_count = 0;
    let mut image_fallback_used = false;
    loop {
        attempt += 1;
        let outcome = client.chat_stream(request.clone(), on_event);
        match outcome {
            Ok(response) if response.finish_reason != "error" => {
                return Ok(response);
            }
            Ok(response) => {
                update_identical_error_count(
                    response.content.as_deref(),
                    &mut last_error_key,
                    &mut identical_error_count,
                );
                let decision =
                    retry_decision_for_response(&response, attempt, mode, identical_error_count);
                if !decision.should_retry {
                    if decision.stop_reason == Some(RetryStopReason::NonTransient)
                        && !image_fallback_used
                    {
                        if let Some(stripped_request) = request_with_stripped_images(&request) {
                            request = stripped_request;
                            image_fallback_used = true;
                            continue;
                        }
                    }
                    return Ok(response);
                }
                let delay_s = decision
                    .delay_s
                    .unwrap_or_else(|| base_delay(attempt, mode));
                waiter.wait(
                    delay_s,
                    &retry_wait_message(attempt, mode, delay_s, response.content.as_deref()),
                );
            }
            Err(error) => {
                let error_text = error.to_string();
                update_identical_error_count(
                    Some(&error_text),
                    &mut last_error_key,
                    &mut identical_error_count,
                );
                let decision = retry_decision_for_error_with_identical_count(
                    &error,
                    attempt,
                    mode,
                    identical_error_count,
                );
                if !decision.should_retry {
                    return Err(error);
                }
                let delay_s = decision
                    .delay_s
                    .unwrap_or_else(|| base_delay(attempt, mode));
                waiter.wait(
                    delay_s,
                    &retry_wait_message(attempt, mode, delay_s, Some(&error_text)),
                );
            }
        }
    }
}

fn update_identical_error_count(
    content: Option<&str>,
    last_error_key: &mut Option<String>,
    identical_error_count: &mut u32,
) {
    let error_key = content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(str::to_ascii_lowercase);
    if error_key.is_some() && error_key == *last_error_key {
        *identical_error_count += 1;
    } else {
        *last_error_key = error_key;
        *identical_error_count = u32::from(last_error_key.is_some());
    }
}

fn retry_wait_message(
    attempt: u32,
    mode: ProviderRetryMode,
    delay_s: f64,
    content: Option<&str>,
) -> String {
    let mode_label = if mode == ProviderRetryMode::Persistent {
        "persistent retry"
    } else {
        "retry"
    };
    let detail = content
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect::<String>();
    format!(
        "LLM transient error ({mode_label}, attempt {attempt}), retrying in {delay_s:.1}s: {detail}"
    )
}

fn request_with_stripped_images(request: &ProviderRequest) -> Option<ProviderRequest> {
    let mut found = false;
    let messages = request
        .messages
        .iter()
        .map(|message| strip_images_from_message(message, &mut found))
        .collect::<Vec<_>>();
    found.then(|| ProviderRequest {
        messages,
        tools: request.tools.clone(),
        model: request.model.clone(),
        settings: request.settings.clone(),
        tool_choice: request.tool_choice.clone(),
    })
}

fn strip_images_from_message(message: &Value, found: &mut bool) -> Value {
    let Some(object) = message.as_object() else {
        return message.clone();
    };
    let Some(content) = object.get("content").and_then(Value::as_array) else {
        return message.clone();
    };
    let mut stripped = object.clone();
    stripped.insert(
        "content".to_owned(),
        Value::Array(
            content
                .iter()
                .map(|block| strip_image_block(block, found))
                .collect(),
        ),
    );
    Value::Object(stripped)
}

fn strip_image_block(block: &Value, found: &mut bool) -> Value {
    let Some(object) = block.as_object() else {
        return block.clone();
    };
    if object.get("type").and_then(Value::as_str) != Some("image_url") {
        return block.clone();
    }
    *found = true;
    json!({
        "type": "text",
        "text": image_placeholder_text(
            object
                .get("_meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("path"))
                .and_then(Value::as_str)
        ),
    })
}

fn image_placeholder_text(path: Option<&str>) -> String {
    path.filter(|path| !path.is_empty())
        .map(|path| format!("[image: {path}]"))
        .unwrap_or_else(|| "[image omitted]".to_owned())
}

impl ProviderRetryDecision {
    fn retry(delay_s: f64) -> Self {
        Self {
            should_retry: true,
            delay_s: Some(delay_s),
            stop_reason: None,
        }
    }

    fn stop(reason: RetryStopReason) -> Self {
        Self {
            should_retry: false,
            delay_s: None,
            stop_reason: Some(reason),
        }
    }
}

pub fn retry_decision_for_response(
    response: &LlmResponse,
    attempt: u32,
    mode: ProviderRetryMode,
    identical_error_count: u32,
) -> ProviderRetryDecision {
    if response.finish_reason != "error" {
        return ProviderRetryDecision::stop(RetryStopReason::NotError);
    }
    if !is_transient_response(response) {
        return ProviderRetryDecision::stop(RetryStopReason::NonTransient);
    }
    match mode {
        ProviderRetryMode::Standard if attempt >= 4 => {
            ProviderRetryDecision::stop(RetryStopReason::AttemptsExhausted)
        }
        ProviderRetryMode::Persistent
            if identical_error_count >= PERSISTENT_IDENTICAL_ERROR_LIMIT =>
        {
            ProviderRetryDecision::stop(RetryStopReason::IdenticalTransientErrorLimit)
        }
        _ => ProviderRetryDecision::retry(delay_for_response(response, attempt, mode)),
    }
}

pub fn retry_decision_for_error(
    error: &ProviderError,
    attempt: u32,
    mode: ProviderRetryMode,
) -> ProviderRetryDecision {
    retry_decision_for_error_with_identical_count(error, attempt, mode, 0)
}

pub fn retry_decision_for_error_with_identical_count(
    error: &ProviderError,
    attempt: u32,
    mode: ProviderRetryMode,
    identical_error_count: u32,
) -> ProviderRetryDecision {
    if !is_transient_provider_error(error) {
        return ProviderRetryDecision::stop(RetryStopReason::NonTransient);
    }
    if mode == ProviderRetryMode::Standard && attempt >= 4 {
        return ProviderRetryDecision::stop(RetryStopReason::AttemptsExhausted);
    }
    if mode == ProviderRetryMode::Persistent
        && identical_error_count >= PERSISTENT_IDENTICAL_ERROR_LIMIT
    {
        return ProviderRetryDecision::stop(RetryStopReason::IdenticalTransientErrorLimit);
    }
    ProviderRetryDecision::retry(base_delay(attempt, mode))
}

pub fn is_transient_response(response: &LlmResponse) -> bool {
    if response.finish_reason != "error" {
        return false;
    }
    if let Some(should_retry) = response.error_should_retry {
        return should_retry;
    }
    if let Some(status) = response.error_status_code {
        if status == 429 {
            return is_retryable_429_response(response);
        }
        if matches!(status, 408 | 409) || status >= 500 {
            return true;
        }
        return false;
    }
    if response
        .error_kind
        .as_deref()
        .map(str::to_ascii_lowercase)
        .is_some_and(|kind| kind.contains("timeout") || kind.contains("connection"))
    {
        return true;
    }
    response
        .content
        .as_deref()
        .is_some_and(contains_transient_text)
}

pub fn is_transient_provider_error(error: &ProviderError) -> bool {
    match error {
        ProviderError::Api {
            status, retryable, ..
        } => {
            *retryable
                || status.is_some_and(|status| matches!(status, 408 | 409 | 429) || status >= 500)
        }
        ProviderError::ProviderNotFound { .. }
        | ProviderError::ModelNotFound { .. }
        | ProviderError::AuthRequired { .. }
        | ProviderError::UnsupportedCapability { .. } => false,
    }
}

pub fn retry_after_from_response(response: &LlmResponse) -> Option<f64> {
    response
        .error_retry_after_s
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| response.retry_after.filter(|seconds| *seconds > 0.0))
        .or_else(|| response.content.as_deref().and_then(extract_retry_after))
}

fn delay_for_response(response: &LlmResponse, attempt: u32, mode: ProviderRetryMode) -> f64 {
    let delay = retry_after_from_response(response).unwrap_or_else(|| base_delay(attempt, mode));
    if mode == ProviderRetryMode::Persistent {
        delay.min(PERSISTENT_MAX_DELAY_S)
    } else {
        delay
    }
}

fn base_delay(attempt: u32, _mode: ProviderRetryMode) -> f64 {
    let index = attempt.saturating_sub(1).min(2) as usize;
    STANDARD_DELAYS[index]
}

fn is_retryable_429_response(response: &LlmResponse) -> bool {
    let combined = [
        response.error_type.as_deref(),
        response.error_code.as_deref(),
        response.content.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::to_ascii_lowercase)
    .collect::<Vec<_>>()
    .join(" ");
    if contains_quota_exhausted_text(&combined) {
        return false;
    }
    true
}

fn contains_quota_exhausted_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "insufficient_quota",
        "quota_exceeded",
        "quota_exhausted",
        "billing_hard_limit_reached",
        "insufficient_balance",
        "payment_required",
        "credit_balance_too_low",
        "billing_not_active",
        "insufficient quota",
        "insufficient balance",
        "quota exceeded",
        "quota exhausted",
        "out of credits",
        "out of quota",
        "exceeded your current quota",
        "billing hard limit",
        "billing not active",
        "credit balance too low",
        "payment required",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn contains_transient_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "429",
        "rate limit",
        "500",
        "502",
        "503",
        "504",
        "overloaded",
        "timeout",
        "timed out",
        "connection",
        "server error",
        "temporarily unavailable",
        "速率限制",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn extract_retry_after(content: &str) -> Option<f64> {
    static PATTERNS: OnceLock<Result<[Regex; 4], regex::Error>> = OnceLock::new();
    let Ok(patterns) = PATTERNS.get_or_init(|| {
        Ok([
            Regex::new(r#"(?i)retry after\s+(\d+(?:\.\d+)?)\s*(ms|milliseconds|s|sec|secs|seconds|m|min|minutes)?"#)?,
            Regex::new(r#"(?i)wait\s+(\d+(?:\.\d+)?)\s*(ms|milliseconds|s|sec|secs|seconds|m|min|minutes)\s+before retry"#)?,
            Regex::new(r#"(?i)try again in\s+(\d+(?:\.\d+)?)\s*(ms|milliseconds|s|sec|secs|seconds|m|min|minutes)?"#)?,
            Regex::new(r#"(?i)retry[_-]?after["'\s:=]+(\d+(?:\.\d+)?)"#)?,
        ])
    }) else {
        return None;
    };
    patterns.iter().find_map(|pattern| {
        let captures = pattern.captures(content)?;
        let value = captures.get(1)?.as_str().parse::<f64>().ok()?;
        let unit = captures.get(2).map(|unit| unit.as_str());
        Some(to_retry_seconds(value, unit).max(0.1))
    })
}

fn to_retry_seconds(value: f64, unit: Option<&str>) -> f64 {
    match unit.map(str::to_ascii_lowercase).as_deref() {
        Some("ms" | "millisecond" | "milliseconds") => value / 1000.0,
        Some("m" | "min" | "minute" | "minutes") => value * 60.0,
        _ => value,
    }
}
