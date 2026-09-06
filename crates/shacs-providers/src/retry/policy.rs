use super::{
    ProviderRetryDecision, ProviderRetryMode, RetryStopReason, PERSISTENT_IDENTICAL_ERROR_LIMIT,
    PERSISTENT_MAX_DELAY_S, STANDARD_DELAYS,
};
use crate::error::ProviderError;
use crate::types::LlmResponse;
use regex::Regex;
use std::sync::OnceLock;

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

pub(super) fn base_delay(attempt: u32, _mode: ProviderRetryMode) -> f64 {
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
    !contains_quota_exhausted_text(&combined)
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
