use super::support::{image_request, recorded_fixture};
use shacs_providers::{
    CodexClient, CodexHttpStreamResponse, CodexHttpTransport, CodexRequestParts, ProviderConfig,
    ProviderError, ProviderEvent, ProviderInvocation, ProviderMediaLifecycleStatus,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn cancellation_after_partial_emits_cancelled_without_candidate() -> Result<(), Box<dyn Error>> {
    // Given
    let cancellation = Arc::new(AtomicBool::new(false));
    let transport = CancellingTransport {
        cancellation: Arc::clone(&cancellation),
    };
    let client = CodexClient::new(ProviderConfig::default(), transport);
    let invocation = ProviderInvocation::new(None, cancellation);
    let mut events = Vec::new();

    // When
    let result = client.generate_image_with_invocation(
        image_request(),
        &mut |event| events.push(event),
        &invocation,
    );

    // Then
    assert!(result.is_err());
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::MediaLifecycle(observation)
            if observation.status() == ProviderMediaLifecycleStatus::Partial
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderEvent::MediaLifecycle(observation)
            if observation.status() == ProviderMediaLifecycleStatus::Cancelled
    )));
    let cancelled_id = events.iter().find_map(|event| match event {
        ProviderEvent::MediaLifecycle(observation)
            if observation.status() == ProviderMediaLifecycleStatus::Cancelled =>
        {
            Some(observation.candidate_id().as_str())
        }
        _ => None,
    });
    assert!(cancelled_id.is_some_and(|id| id.starts_with("item_sha256_")));
    assert_ne!(cancelled_id, Some("AKIAIOSFODNN7EXAMPLE"));
    Ok(())
}

#[test]
fn already_cancelled_and_repeated_interruption_never_start_transport() {
    // Given
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = CountingTransport {
        calls: Arc::clone(&calls),
    };
    let client = CodexClient::new(ProviderConfig::default(), transport);
    let cancellation = Arc::new(AtomicBool::new(true));
    let invocation = ProviderInvocation::new(None, cancellation);

    // When
    for _ in 0..2 {
        let result =
            client.generate_image_with_invocation(image_request(), &mut |_| {}, &invocation);
        assert!(result.is_err());
    }

    // Then
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn fresh_invocation_after_cancelled_call_resumes_without_stale_state() -> Result<(), Box<dyn Error>>
{
    // Given
    let calls = Arc::new(AtomicUsize::new(0));
    let transport = CountingTransport {
        calls: Arc::clone(&calls),
    };
    let client = CodexClient::new(ProviderConfig::default(), transport);
    let cancelled = ProviderInvocation::new(None, Arc::new(AtomicBool::new(true)));

    // When
    let first = client.generate_image_with_invocation(image_request(), &mut |_| {}, &cancelled);
    let resumed = client.generate_image_with_invocation(
        image_request(),
        &mut |_| {},
        &ProviderInvocation::default(),
    )?;

    // Then
    assert!(first.is_err());
    assert_eq!(resumed.images.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct CancellingTransport {
    cancellation: Arc<AtomicBool>,
}

impl CodexHttpTransport for CancellingTransport {
    fn post_json_stream(
        &self,
        _request: CodexRequestParts,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        unreachable!("frame transport is used")
    }

    fn post_json_stream_frames_bounded(
        &self,
        _request: CodexRequestParts,
        on_frame: &mut dyn FnMut(&str) -> Result<bool, ProviderError>,
        _timeout: Option<Duration>,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        let frames = recorded_fixture().split("\n\n").collect::<Vec<_>>();
        for (index, frame) in frames.iter().enumerate() {
            let frame = format!("{frame}\n\n");
            if on_frame(&frame)? {
                break;
            }
            if index == 1 {
                self.cancellation.store(true, Ordering::SeqCst);
            }
        }
        Ok(CodexHttpStreamResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: String::new(),
        })
    }
}

struct CountingTransport {
    calls: Arc<AtomicUsize>,
}

impl CodexHttpTransport for CountingTransport {
    fn post_json_stream(
        &self,
        _request: CodexRequestParts,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CodexHttpStreamResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: recorded_fixture().to_owned(),
        })
    }
}
