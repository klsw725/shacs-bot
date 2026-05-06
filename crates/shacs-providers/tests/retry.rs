use serde_json::Value;
use shacs_providers::{
    chat_stream_with_retry_using_waiter, chat_with_retry_using_waiter, is_transient_provider_error,
    is_transient_response, retry_after_from_response, retry_decision_for_error,
    retry_decision_for_error_with_identical_count, retry_decision_for_response, GenerationSettings,
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ProviderRetryMode,
    ProviderRetryWaiter, RetryStopReason,
};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::Mutex;

#[test]
fn retry_policy_skips_non_error_responses() -> Result<(), Box<dyn Error>> {
    let response = LlmResponse {
        content: Some("ok".to_owned()),
        ..LlmResponse::default()
    };
    let decision = retry_decision_for_response(&response, 1, ProviderRetryMode::Standard, 0);
    if decision.should_retry || decision.stop_reason != Some(RetryStopReason::NotError) {
        return Err(format!("non-error response should not retry: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn retry_policy_handles_standard_and_persistent_attempts() -> Result<(), Box<dyn Error>> {
    let response = error_response("Error: 429 rate limit exceeded");
    let first = retry_decision_for_response(&response, 1, ProviderRetryMode::Standard, 0);
    let exhausted = retry_decision_for_response(&response, 4, ProviderRetryMode::Standard, 0);
    let persistent = retry_decision_for_response(&response, 4, ProviderRetryMode::Persistent, 0);
    let identical_limit =
        retry_decision_for_response(&response, 4, ProviderRetryMode::Persistent, 10);
    if first.delay_s != Some(1.0)
        || exhausted.stop_reason != Some(RetryStopReason::AttemptsExhausted)
        || persistent.delay_s != Some(4.0)
        || identical_limit.stop_reason != Some(RetryStopReason::IdenticalTransientErrorLimit)
    {
        return Err(format!(
            "attempt policy drifted: first={first:?} exhausted={exhausted:?} persistent={persistent:?} identical={identical_limit:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn retry_policy_respects_explicit_should_retry_tristate() -> Result<(), Box<dyn Error>> {
    let explicit_false = LlmResponse {
        error_should_retry: Some(false),
        error_status_code: Some(429),
        ..error_response("Error: 429 rate limit")
    };
    let explicit_true = LlmResponse {
        error_should_retry: Some(true),
        error_status_code: Some(401),
        ..error_response("Error: unauthorized")
    };
    let unknown = LlmResponse {
        error_should_retry: None,
        error_status_code: Some(429),
        ..error_response("Error: rate limit")
    };
    if is_transient_response(&explicit_false)
        || !is_transient_response(&explicit_true)
        || !is_transient_response(&unknown)
    {
        return Err("explicit should_retry tri-state drifted".into());
    }
    Ok(())
}

#[test]
fn retry_policy_uses_status_code_and_429_quota_metadata() -> Result<(), Box<dyn Error>> {
    let retryable_408 = LlmResponse {
        error_status_code: Some(408),
        ..error_response("timeout")
    };
    let plain_400 = LlmResponse {
        error_status_code: Some(400),
        ..error_response("bad request")
    };
    let quota = LlmResponse {
        error_status_code: Some(429),
        error_code: Some("insufficient_quota".to_owned()),
        ..error_response("insufficient quota")
    };
    let rate_limit = LlmResponse {
        error_status_code: Some(429),
        error_code: Some("rate_limit_exceeded".to_owned()),
        ..error_response("rate limit")
    };
    if !is_transient_response(&retryable_408)
        || is_transient_response(&plain_400)
        || is_transient_response(&quota)
        || !is_transient_response(&rate_limit)
    {
        return Err("status/quota retry policy drifted".into());
    }
    Ok(())
}

#[test]
fn retry_policy_recognizes_nanobot_quota_and_billing_429_markers() -> Result<(), Box<dyn Error>> {
    for marker in [
        "credit_balance_too_low",
        "billing_not_active",
        "out of credits",
        "out of quota",
        "quota exhausted",
        "exceeded your current quota",
        "billing hard limit",
        "billing not active",
        "insufficient balance",
        "credit balance too low",
    ] {
        let response = LlmResponse {
            error_status_code: Some(429),
            error_code: Some(marker.to_owned()),
            ..error_response(marker)
        };
        if is_transient_response(&response) {
            return Err(format!("quota/billing marker should not retry: {marker}").into());
        }
    }
    Ok(())
}

#[test]
fn retry_policy_stops_persistent_provider_errors_after_identical_limit(
) -> Result<(), Box<dyn Error>> {
    let error = ProviderError::Api {
        status: None,
        message: "connection failed".to_owned(),
        retryable: true,
        headers: BTreeMap::new(),
        body: None,
    };
    let decision = retry_decision_for_error_with_identical_count(
        &error,
        99,
        ProviderRetryMode::Persistent,
        10,
    );
    if decision.should_retry
        || decision.stop_reason != Some(RetryStopReason::IdenticalTransientErrorLimit)
    {
        return Err(format!("persistent ProviderError limit drifted: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn retry_policy_extracts_retry_after_with_expected_precedence() -> Result<(), Box<dyn Error>> {
    let response = LlmResponse {
        error_retry_after_s: Some(3.0),
        retry_after: Some(2.0),
        ..error_response("retry after 1 seconds")
    };
    let header_fallback = LlmResponse {
        retry_after: Some(2.0),
        ..error_response("retry after 1 seconds")
    };
    let retry_after_text = error_response("retry after 1 seconds");
    let text_fallback = error_response("please wait 250 ms before retry");
    let try_again = error_response("please try again in 0.01 seconds");
    let retry_after_key = error_response("retry_after: 2");
    let capped = LlmResponse {
        error_retry_after_s: Some(120.0),
        ..error_response("rate limit")
    };
    let capped_decision = retry_decision_for_response(&capped, 1, ProviderRetryMode::Persistent, 0);
    if retry_after_from_response(&response) != Some(3.0)
        || retry_after_from_response(&header_fallback) != Some(2.0)
        || retry_after_from_response(&retry_after_text) != Some(1.0)
        || retry_after_from_response(&text_fallback) != Some(0.25)
        || retry_after_from_response(&try_again) != Some(0.1)
        || retry_after_from_response(&retry_after_key) != Some(2.0)
        || capped_decision.delay_s != Some(60.0)
    {
        return Err(format!(
            "retry_after precedence drifted: response={:?} header={:?} text={:?} capped={capped_decision:?}",
            retry_after_from_response(&response),
            retry_after_from_response(&header_fallback),
            retry_after_from_response(&text_fallback)
        )
        .into());
    }
    Ok(())
}

#[test]
fn retry_policy_handles_provider_error_inputs() -> Result<(), Box<dyn Error>> {
    let retryable_api = ProviderError::Api {
        status: None,
        message: "connection failed".to_owned(),
        retryable: true,
        headers: BTreeMap::new(),
        body: None,
    };
    let server_error = ProviderError::Api {
        status: Some(500),
        message: "server error".to_owned(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    };
    let not_found = ProviderError::ProviderNotFound {
        provider_id: "missing".to_owned(),
        suggestions: Vec::new(),
    };
    if !is_transient_provider_error(&retryable_api)
        || !retry_decision_for_error(&server_error, 1, ProviderRetryMode::Standard).should_retry
        || retry_decision_for_error(&not_found, 1, ProviderRetryMode::Standard).should_retry
    {
        return Err("provider error retry policy drifted".into());
    }
    Ok(())
}

#[test]
fn retry_mode_from_config_defaults_unknown_values_to_standard() -> Result<(), Box<dyn Error>> {
    if ProviderRetryMode::from_config("persistent") != ProviderRetryMode::Persistent
        || ProviderRetryMode::from_config("PERSISTENT") != ProviderRetryMode::Persistent
        || ProviderRetryMode::from_config("standard") != ProviderRetryMode::Standard
        || ProviderRetryMode::from_config("unknown") != ProviderRetryMode::Standard
    {
        return Err("retry mode config parsing drifted".into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_retries_transient_response_then_returns_success() -> Result<(), Box<dyn Error>>
{
    let client = SequenceClient::new(vec![
        Ok(error_response("Error: 429 rate limit")),
        Ok(ok_response("done")),
    ]);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;
    if response.content.as_deref() != Some("done")
        || client.calls()? != 2
        || waiter.delays != vec![1.0]
    {
        return Err(format!(
            "chat retry runner did not retry then succeed: response={response:?} calls={} delays={:?}",
            client.calls()?, waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_returns_last_response_after_standard_attempts_exhausted(
) -> Result<(), Box<dyn Error>> {
    let client = SequenceClient::new(vec![
        Ok(error_response("Error: 503 overloaded")),
        Ok(error_response("Error: 503 overloaded")),
        Ok(error_response("Error: 503 overloaded")),
        Ok(error_response("Error: 503 overloaded")),
    ]);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;
    if response.finish_reason != "error"
        || client.calls()? != 4
        || waiter.delays != vec![1.0, 2.0, 4.0]
    {
        return Err(format!(
            "standard retry exhaustion drifted: response={response:?} calls={} delays={:?}",
            client.calls()?,
            waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_stops_persistent_identical_provider_errors() -> Result<(), Box<dyn Error>> {
    let outcomes = (0..10)
        .map(|_| Err(provider_api_error(Some(500), true)))
        .collect::<Vec<_>>();
    let client = SequenceClient::new(outcomes);
    let mut waiter = CaptureWaiter::default();
    let error = match chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Persistent,
        &mut waiter,
    ) {
        Ok(response) => {
            return Err(format!("persistent provider error should fail: {response:?}").into())
        }
        Err(error) => error,
    };
    if !error.to_string().contains("server error")
        || client.calls()? != 10
        || waiter.delays.len() != 9
    {
        return Err(format!(
            "persistent ProviderError stop drifted: error={error} calls={} delays={:?}",
            client.calls()?,
            waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_does_not_retry_non_transient_response() -> Result<(), Box<dyn Error>> {
    let client = SequenceClient::new(vec![Ok(LlmResponse {
        error_status_code: Some(401),
        ..error_response("Error: unauthorized")
    })]);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;
    if response.error_status_code != Some(401) || client.calls()? != 1 || !waiter.delays.is_empty()
    {
        return Err(format!(
            "non-transient response should not retry: response={response:?} calls={} delays={:?}",
            client.calls()?,
            waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_uses_response_retry_after_delay() -> Result<(), Box<dyn Error>> {
    let client = SequenceClient::new(vec![
        Ok(LlmResponse {
            error_retry_after_s: Some(0.2),
            ..error_response("Error: 429 rate limit")
        }),
        Ok(ok_response("done")),
    ]);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;
    if response.content.as_deref() != Some("done") || waiter.delays != vec![0.2] {
        return Err(format!(
            "response retry-after should drive delay: response={response:?} delays={:?}",
            waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_retries_transient_provider_error_then_succeeds() -> Result<(), Box<dyn Error>>
{
    let client = SequenceClient::new(vec![
        Err(provider_api_error(None, true)),
        Ok(ok_response("done")),
    ]);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;
    if response.content.as_deref() != Some("done")
        || client.calls()? != 2
        || waiter.delays != vec![1.0]
    {
        return Err(format!(
            "transient ProviderError should retry then succeed: response={response:?} calls={} delays={:?}",
            client.calls()?, waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_returns_provider_error_after_standard_exhaustion() -> Result<(), Box<dyn Error>>
{
    let client = SequenceClient::new(vec![
        Err(provider_api_error(Some(500), false)),
        Err(provider_api_error(Some(500), false)),
        Err(provider_api_error(Some(500), false)),
        Err(provider_api_error(Some(500), false)),
    ]);
    let mut waiter = CaptureWaiter::default();
    let error = match chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    ) {
        Ok(response) => {
            return Err(format!("exhausted ProviderError should fail: {response:?}").into())
        }
        Err(error) => error,
    };
    if !error.to_string().contains("server error")
        || client.calls()? != 4
        || waiter.delays != vec![1.0, 2.0, 4.0]
    {
        return Err(format!(
            "standard ProviderError exhaustion drifted: error={error} calls={} delays={:?}",
            client.calls()?,
            waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_stops_persistent_identical_response_errors() -> Result<(), Box<dyn Error>> {
    let outcomes = (0..10)
        .map(|_| Ok(error_response("Error: 503 overloaded")))
        .collect::<Vec<_>>();
    let client = SequenceClient::new(outcomes);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Persistent,
        &mut waiter,
    )?;
    if response.finish_reason != "error"
        || client.calls()? != 10
        || waiter.delays != vec![1.0, 2.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0]
    {
        return Err(format!(
            "persistent response identical limit drifted: response={response:?} calls={} delays={:?}",
            client.calls()?, waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_stream_retry_runner_retries_transient_response_then_flushes_success_events(
) -> Result<(), Box<dyn Error>> {
    let client = StreamSequenceClient::new(vec![
        stream_outcome(
            Ok(error_response("Error: 429 rate limit")),
            vec![text_delta("failed attempt")],
        ),
        stream_outcome(
            Ok(ok_response("done")),
            vec![text_delta("done"), finish_event("stop")],
        ),
    ]);
    let mut waiter = CaptureWaiter::default();
    let mut events = Vec::new();
    let response = chat_stream_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut |event| events.push(event),
        &mut waiter,
    )?;
    if response.content.as_deref() != Some("done")
        || client.stream_calls()? != 2
        || waiter.delays != vec![1.0]
        || events
            != vec![
                text_delta("failed attempt"),
                text_delta("done"),
                finish_event("stop"),
            ]
    {
        return Err(format!(
            "stream retry should forward attempt events in real time: response={response:?} calls={} delays={:?} events={events:?}",
            client.stream_calls()?, waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_stream_retry_runner_returns_last_error_without_flushing_failed_attempts(
) -> Result<(), Box<dyn Error>> {
    let client = StreamSequenceClient::new(vec![
        stream_outcome(
            Ok(error_response("Error: 503 overloaded")),
            vec![text_delta("failed 1")],
        ),
        stream_outcome(
            Ok(error_response("Error: 503 overloaded")),
            vec![text_delta("failed 2")],
        ),
        stream_outcome(
            Ok(error_response("Error: 503 overloaded")),
            vec![text_delta("failed 3")],
        ),
        stream_outcome(
            Ok(error_response("Error: 503 overloaded")),
            vec![text_delta("failed 4")],
        ),
    ]);
    let mut waiter = CaptureWaiter::default();
    let mut events = Vec::new();
    let response = chat_stream_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut |event| events.push(event),
        &mut waiter,
    )?;
    if response.finish_reason != "error"
        || client.stream_calls()? != 4
        || waiter.delays != vec![1.0, 2.0, 4.0]
        || events
            != vec![
                text_delta("failed 1"),
                text_delta("failed 2"),
                text_delta("failed 3"),
                text_delta("failed 4"),
            ]
    {
        return Err(format!(
            "stream exhaustion should keep real-time attempt events: response={response:?} calls={} delays={:?} events={events:?}",
            client.stream_calls()?, waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_stream_retry_runner_retries_provider_error_then_flushes_success_events(
) -> Result<(), Box<dyn Error>> {
    let client = StreamSequenceClient::new(vec![
        stream_outcome(
            Err(provider_api_error(Some(500), false)),
            vec![text_delta("partial")],
        ),
        stream_outcome(Ok(ok_response("done")), vec![text_delta("done")]),
    ]);
    let mut waiter = CaptureWaiter::default();
    let mut events = Vec::new();
    let response = chat_stream_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut |event| events.push(event),
        &mut waiter,
    )?;
    if response.content.as_deref() != Some("done")
        || client.stream_calls()? != 2
        || waiter.delays != vec![1.0]
        || events != vec![text_delta("partial"), text_delta("done")]
    {
        return Err(format!(
            "stream ProviderError retry should forward attempt events in real time: response={response:?} calls={} delays={:?} events={events:?}",
            client.stream_calls()?, waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_retry_runner_strips_images_once_after_non_transient_response() -> Result<(), Box<dyn Error>>
{
    let client = CapturingSequenceClient::new(vec![
        Ok(LlmResponse {
            error_status_code: Some(400),
            ..error_response("Error: image input unsupported")
        }),
        Ok(ok_response("done")),
    ]);
    let mut waiter = CaptureWaiter::default();
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request_with_image(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;
    let requests = client.requests()?;
    if response.content.as_deref() != Some("done")
        || requests.len() != 2
        || requests[0][0]["content"][1]["type"] != "image_url"
        || requests[1][0]["content"][1]
            != serde_json::json!({"type": "text", "text": "[image: /tmp/cat.png]"})
        || !waiter.delays.is_empty()
    {
        return Err(format!(
            "image fallback should strip once without retry delay: response={response:?} requests={requests:?} delays={:?}",
            waiter.delays
        )
        .into());
    }
    Ok(())
}

#[test]
fn chat_stream_retry_runner_strips_images_and_forwards_realtime_events(
) -> Result<(), Box<dyn Error>> {
    let client = CapturingStreamClient::new(vec![
        stream_outcome(
            Ok(LlmResponse {
                error_status_code: Some(400),
                ..error_response("Error: image input unsupported")
            }),
            vec![text_delta("failed")],
        ),
        stream_outcome(Ok(ok_response("done")), vec![text_delta("done")]),
    ]);
    let mut waiter = CaptureWaiter::default();
    let mut events = Vec::new();
    let response = chat_stream_with_retry_using_waiter(
        &client,
        provider_request_with_image(),
        ProviderRetryMode::Standard,
        &mut |event| events.push(event),
        &mut waiter,
    )?;
    let requests = client.requests()?;
    if response.content.as_deref() != Some("done")
        || requests.len() != 2
        || requests[1][0]["content"][1]["text"] != "[image: /tmp/cat.png]"
        || events != vec![text_delta("failed"), text_delta("done")]
        || !waiter.delays.is_empty()
    {
        return Err(format!(
            "stream image fallback should strip and forward realtime events: response={response:?} requests={requests:?} events={events:?} delays={:?}",
            waiter.delays
        )
        .into());
    }
    Ok(())
}

fn error_response(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        finish_reason: "error".to_owned(),
        ..LlmResponse::default()
    }
}

fn ok_response(content: &str) -> LlmResponse {
    LlmResponse {
        content: Some(content.to_owned()),
        ..LlmResponse::default()
    }
}

fn provider_request() -> ProviderRequest {
    ProviderRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        model: "gpt-4.1".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    }
}

fn provider_request_with_image() -> ProviderRequest {
    ProviderRequest {
        messages: vec![serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,aaa"}, "_meta": {"path": "/tmp/cat.png"}}
            ]
        })],
        tools: Vec::new(),
        model: "gpt-4.1".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    }
}

fn provider_api_error(status: Option<u16>, retryable: bool) -> ProviderError {
    ProviderError::Api {
        status,
        message: "server error".to_owned(),
        retryable,
        headers: BTreeMap::new(),
        body: None,
    }
}

fn text_delta(text: &str) -> ProviderEvent {
    ProviderEvent::TextDelta {
        text: text.to_owned(),
    }
}

fn finish_event(reason: &str) -> ProviderEvent {
    ProviderEvent::Finish {
        usage: serde_json::json!({}),
        reason: reason.to_owned(),
    }
}

fn stream_outcome(
    result: Result<LlmResponse, ProviderError>,
    events: Vec<ProviderEvent>,
) -> StreamOutcome {
    StreamOutcome { result, events }
}

struct StreamOutcome {
    result: Result<LlmResponse, ProviderError>,
    events: Vec<ProviderEvent>,
}

struct SequenceClient {
    outcomes: Mutex<VecDeque<Result<LlmResponse, ProviderError>>>,
    calls: Mutex<u32>,
}

struct CapturingSequenceClient {
    outcomes: Mutex<VecDeque<Result<LlmResponse, ProviderError>>>,
    requests: Mutex<Vec<Vec<Value>>>,
}

struct StreamSequenceClient {
    outcomes: Mutex<VecDeque<StreamOutcome>>,
    stream_calls: Mutex<u32>,
}

struct CapturingStreamClient {
    outcomes: Mutex<VecDeque<StreamOutcome>>,
    requests: Mutex<Vec<Vec<Value>>>,
}

impl CapturingSequenceClient {
    fn new(outcomes: Vec<Result<LlmResponse, ProviderError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Result<Vec<Vec<Value>>, String> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .map_err(|error| error.to_string())
    }
}

impl ProviderClient for CapturingSequenceClient {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?
            .push(request.messages);
        self.outcomes
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?
            .pop_front()
            .unwrap_or_else(|| Ok(ok_response("default")))
    }

    fn chat_stream(
        &self,
        _request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        Err(provider_lock_error("stream not implemented"))
    }
}

impl CapturingStreamClient {
    fn new(outcomes: Vec<StreamOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Result<Vec<Vec<Value>>, String> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .map_err(|error| error.to_string())
    }
}

impl ProviderClient for CapturingStreamClient {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Err(provider_lock_error("chat not implemented"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?
            .push(request.messages);
        let outcome = self
            .outcomes
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?
            .pop_front()
            .unwrap_or_else(|| stream_outcome(Ok(ok_response("default")), Vec::new()));
        for event in outcome.events {
            on_event(event);
        }
        outcome.result
    }
}

impl StreamSequenceClient {
    fn new(outcomes: Vec<StreamOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            stream_calls: Mutex::new(0),
        }
    }

    fn stream_calls(&self) -> Result<u32, String> {
        self.stream_calls
            .lock()
            .map(|calls| *calls)
            .map_err(|error| error.to_string())
    }
}

impl ProviderClient for StreamSequenceClient {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Err(provider_lock_error("chat not implemented"))
    }

    fn chat_stream(
        &self,
        _request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        let mut calls = self
            .stream_calls
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?;
        *calls += 1;
        let outcome = self
            .outcomes
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?
            .pop_front()
            .unwrap_or_else(|| stream_outcome(Ok(ok_response("default")), Vec::new()));
        for event in outcome.events {
            on_event(event);
        }
        outcome.result
    }
}

impl SequenceClient {
    fn new(outcomes: Vec<Result<LlmResponse, ProviderError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> Result<u32, String> {
        self.calls
            .lock()
            .map(|calls| *calls)
            .map_err(|error| error.to_string())
    }
}

impl ProviderClient for SequenceClient {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let mut calls = self
            .calls
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?;
        *calls += 1;
        self.outcomes
            .lock()
            .map_err(|error| provider_lock_error(error.to_string()))?
            .pop_front()
            .unwrap_or_else(|| Ok(ok_response("default")))
    }

    fn chat_stream(
        &self,
        _request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        Err(provider_lock_error("stream not implemented"))
    }
}

fn provider_lock_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}

#[derive(Default)]
struct CaptureWaiter {
    delays: Vec<f64>,
    messages: Vec<String>,
}

impl ProviderRetryWaiter for CaptureWaiter {
    fn wait(&mut self, delay_s: f64, message: &str) {
        self.delays.push(delay_s);
        self.messages.push(message.to_owned());
    }
}
