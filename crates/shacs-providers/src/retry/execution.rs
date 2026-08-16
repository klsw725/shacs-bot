use super::fallback::request_with_stripped_images;
use super::policy::{base_delay, retry_decision_for_error_with_identical_count};
use super::{
    retry_decision_for_response, ProviderRetryMode, ProviderRetryWaiter, RetryStopReason,
    ThreadRetryWaiter,
};
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderRequest};
use crate::types::LlmResponse;
use std::collections::BTreeSet;

pub fn chat_with_retry(
    client: &dyn ProviderClient,
    request: ProviderRequest,
    mode: ProviderRetryMode,
) -> Result<LlmResponse, ProviderError> {
    let mut waiter = ThreadRetryWaiter::default();
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
            Ok(response)
                if response.finish_reason != "error" || !response.media_candidates.is_empty() =>
            {
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

pub fn chat_stream_with_retry(
    client: &dyn ProviderClient,
    request: ProviderRequest,
    mode: ProviderRetryMode,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    let mut waiter = ThreadRetryWaiter::default();
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
    let mut media_observations = BTreeSet::new();
    loop {
        attempt += 1;
        let outcome = client.chat_stream(request.clone(), &mut |event| match &event {
            ProviderEvent::MediaLifecycle(observation)
                if !media_observations.insert(observation.clone()) => {}
            _ => on_event(event),
        });
        match outcome {
            Ok(response)
                if response.finish_reason != "error" || !response.media_candidates.is_empty() =>
            {
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
