use super::support::{image_request, partial_frame};
use serde_json::json;
use shacs_providers::{
    chat_stream_with_retry_using_waiter, chat_with_retry_using_waiter, CodexClient,
    GenerationSettings, ImageGenerationClient, ProviderClient, ProviderConfig, ProviderError,
    ProviderEvent, ProviderMediaBytes, ProviderMediaCandidate, ProviderMediaCandidateId,
    ProviderMediaLifecycleObservation, ProviderMediaOrigin, ProviderRequest, ProviderRetryMode,
    ProviderRetryWaiter, UreqCodexHttpTransport,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn retry_deduplicates_media_lifecycle_observations() -> Result<(), Box<dyn Error>> {
    // Given
    let client = RetryMediaClient::default();
    let mut waiter = NoWait;
    let mut events = Vec::new();

    // When
    let response = chat_stream_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut |event| events.push(event),
        &mut waiter,
    )?;

    // Then
    assert_eq!(response.content.as_deref(), Some("done"));
    assert_eq!(client.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ProviderEvent::MediaLifecycle(_)))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn final_media_candidate_prevents_provider_retry_even_with_error_finish(
) -> Result<(), Box<dyn Error>> {
    // Given
    let client = ArtifactOnlyClient::default();
    let mut waiter = NoWait;

    // When
    let response = chat_with_retry_using_waiter(
        &client,
        provider_request(),
        ProviderRetryMode::Standard,
        &mut waiter,
    )?;

    // Then
    assert_eq!(client.calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.media_candidates.len(), 1);
    Ok(())
}

#[test]
fn hung_stream_is_bounded_by_idle_timeout() -> Result<(), Box<dyn Error>> {
    // Given
    let (base_url, handle) = serve_hung_stream()?;
    let client = CodexClient::new(
        ProviderConfig::default(),
        UreqCodexHttpTransport::with_timeout(base_url, Duration::from_millis(50)),
    );

    // When
    let result = client.generate_image(image_request());

    // Then
    assert!(result.is_err());
    handle
        .join()
        .map_err(|_| "hung fixture thread panicked")??;
    Ok(())
}

#[derive(Default)]
struct RetryMediaClient {
    calls: AtomicUsize,
}

impl ProviderClient for RetryMediaClient {
    fn chat(
        &self,
        request: ProviderRequest,
    ) -> Result<shacs_providers::LlmResponse, ProviderError> {
        self.chat_stream(request, &mut |_| {})
    }

    fn chat_stream(
        &self,
        _request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<shacs_providers::LlmResponse, ProviderError> {
        let candidate_id =
            shacs_providers::ProviderMediaCandidateId::new("ig_retry").map_err(|error| {
                ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                }
            })?;
        on_event(ProviderEvent::MediaLifecycle(
            ProviderMediaLifecycleObservation::started(candidate_id.clone()),
        ));
        on_event(ProviderEvent::MediaLifecycle(
            ProviderMediaLifecycleObservation::partial(candidate_id, 1),
        ));
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Ok(shacs_providers::LlmResponse {
                finish_reason: "error".to_owned(),
                content: Some("temporary".to_owned()),
                error_should_retry: Some(true),
                ..shacs_providers::LlmResponse::default()
            });
        }
        Ok(shacs_providers::LlmResponse {
            content: Some("done".to_owned()),
            ..shacs_providers::LlmResponse::default()
        })
    }
}

struct NoWait;

impl ProviderRetryWaiter for NoWait {
    fn wait(&mut self, _delay_s: f64, _message: &str) {}
}

#[derive(Default)]
struct ArtifactOnlyClient {
    calls: AtomicUsize,
}

impl ProviderClient for ArtifactOnlyClient {
    fn chat(
        &self,
        _request: ProviderRequest,
    ) -> Result<shacs_providers::LlmResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(shacs_providers::LlmResponse {
            finish_reason: "error".to_owned(),
            error_should_retry: Some(true),
            media_candidates: vec![ProviderMediaCandidate::bytes(
                ProviderMediaCandidateId::new("ig_artifact_only").map_err(|error| {
                    ProviderError::Api {
                        status: None,
                        message: error.to_string(),
                        retryable: false,
                        headers: BTreeMap::new(),
                        body: None,
                    }
                })?,
                ProviderMediaOrigin::new("openai_codex", "gpt-5.6"),
                ProviderMediaBytes::new("image/png", b"artifact-only".to_vec()),
            )],
            ..shacs_providers::LlmResponse::default()
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<shacs_providers::LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn provider_request() -> ProviderRequest {
    ProviderRequest {
        messages: vec![json!({"role": "user", "content": "draw"})],
        tools: Vec::new(),
        model: "gpt-5.6".to_owned(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    }
}

type HungServerHandle = thread::JoinHandle<Result<(), String>>;

fn serve_hung_stream() -> Result<(String, HungServerHandle), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        read_request(&mut stream)?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .map_err(|error| error.to_string())?;
        stream
            .write_all(partial_frame("ig_hung", 1, 0).as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(150));
        Ok(())
    });
    Ok((format!("http://{address}"), handle))
}

fn read_request(stream: &mut TcpStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 512];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
    }
}
