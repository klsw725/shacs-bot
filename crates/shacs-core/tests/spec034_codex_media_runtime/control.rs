use super::*;
use shacs_core::runtime::CancellationToken;
use shacs_providers::{
    CodexClient, CodexHttpStreamResponse, CodexHttpTransport, CodexRequestParts,
    DefaultModelImageGenerationClient, ProviderConfig,
};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[test]
fn production_image_tool_propagates_control_and_cancels_before_persistence(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let stage = Arc::new(AtomicUsize::new(0));
    let cancellation = CancellationToken::new();
    let observed_timeout = Arc::new(Mutex::new(None));
    let stopped_after_partial = Arc::new(AtomicBool::new(false));
    let client = CodexClient::new(
        ProviderConfig::default(),
        RuntimeCancellingTransport {
            stage: Arc::clone(&stage),
            cancellation: cancellation.clone(),
            observed_timeout: Arc::clone(&observed_timeout),
            stopped_after_partial: Arc::clone(&stopped_after_partial),
        },
    );
    let mut registry = ToolRegistry::new();
    registry.register(
        ImageGenerateTool::new(
            Box::new(DefaultModelImageGenerationClient::new(
                "gpt-5.6",
                Box::new(client),
            )),
            root.path().join("legacy"),
            tool_config(),
        )
        .with_artifact_store(ArtifactStore::open(root.path().join("media"))?),
    );
    let provider = FixtureProvider::new(vec![tool_call_response()]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "draw"})],
        &registry,
        &provider,
        "test",
    );
    spec.tool_context = bypass_context();
    spec.cancellation_token = Some(cancellation);
    spec.deadline = Some(Instant::now() + Duration::from_secs(60));
    spec.agent_hook = Some(Arc::new(AdmissionHook {
        stage: Arc::clone(&stage),
        block: false,
    }));
    let snapshot_stage = Arc::clone(&stage);
    spec.execution_snapshot_callback = Arc::new(move |_| {
        snapshot_stage
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
    });

    // When
    let result = AgentRunner::new().run(spec)?;

    // Then
    assert!(stopped_after_partial.load(Ordering::SeqCst));
    let timeout = observed_timeout
        .lock()
        .map_err(|error| error.to_string())?
        .ok_or("Codex transport did not receive the production deadline")?;
    assert!(!timeout.is_zero());
    assert!(timeout <= Duration::from_secs(60));
    assert_eq!(result.stop_reason, "cancelled");
    assert!(result.generated_artifacts.is_empty());
    assert_eq!(
        std::fs::read_dir(root.path().join("media/artifacts"))?.count(),
        0
    );
    Ok(())
}

#[test]
fn cancelled_partial_then_fresh_final_publishes_exactly_one_artifact() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let media_root = root.path().join("media");
    let cancelled_stage = Arc::new(AtomicUsize::new(0));
    let cancellation = CancellationToken::new();
    let mut cancelled_registry = ToolRegistry::new();
    cancelled_registry.register(
        ImageGenerateTool::new(
            Box::new(DefaultModelImageGenerationClient::new(
                "gpt-5.6",
                Box::new(CodexClient::new(
                    ProviderConfig::default(),
                    RuntimeCancellingTransport {
                        stage: Arc::clone(&cancelled_stage),
                        cancellation: cancellation.clone(),
                        observed_timeout: Arc::new(Mutex::new(None)),
                        stopped_after_partial: Arc::new(AtomicBool::new(false)),
                    },
                )),
            )),
            root.path().join("cancelled-legacy"),
            tool_config(),
        )
        .with_artifact_store(ArtifactStore::open(&media_root)?),
    );
    let cancelled_provider = FixtureProvider::new(vec![tool_call_response()]);
    let mut cancelled_spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "draw cancelled"})],
        &cancelled_registry,
        &cancelled_provider,
        "test",
    );
    cancelled_spec.tool_context = bypass_context();
    cancelled_spec.cancellation_token = Some(cancellation);
    cancelled_spec.deadline = Some(Instant::now() + Duration::from_secs(60));
    cancelled_spec.agent_hook = Some(Arc::new(AdmissionHook {
        stage: Arc::clone(&cancelled_stage),
        block: false,
    }));
    cancelled_spec.execution_snapshot_callback = Arc::new(move |_| {
        cancelled_stage
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
    });

    // When
    let cancelled = AgentRunner::new().run(cancelled_spec)?;

    // Then
    assert_eq!(cancelled.stop_reason, "cancelled");
    assert!(cancelled.generated_artifacts.is_empty());
    assert_eq!(std::fs::read_dir(media_root.join("artifacts"))?.count(), 0);

    // Given
    let fresh_stage = Arc::new(AtomicUsize::new(0));
    let fresh_calls = Arc::new(AtomicUsize::new(0));
    let mut fresh_registry = ToolRegistry::new();
    fresh_registry.register(
        ImageGenerateTool::new(
            Box::new(FixtureImageClient {
                calls: Arc::clone(&fresh_calls),
                stage: Arc::clone(&fresh_stage),
            }),
            root.path().join("fresh-legacy"),
            tool_config(),
        )
        .with_artifact_store(ArtifactStore::open(&media_root)?),
    );
    let fresh_provider = FixtureProvider::new(vec![tool_call_response(), LlmResponse::default()]);
    let mut fresh_spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "draw fresh"})],
        &fresh_registry,
        &fresh_provider,
        "test",
    );
    fresh_spec.tool_context = bypass_context();
    fresh_spec.agent_hook = Some(Arc::new(AdmissionHook {
        stage: Arc::clone(&fresh_stage),
        block: false,
    }));
    fresh_spec.execution_snapshot_callback = Arc::new(move |_| {
        fresh_stage
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .ok();
    });

    // When
    let fresh = AgentRunner::new().run(fresh_spec)?;

    // Then
    assert_eq!(fresh_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fresh.generated_artifacts.len(), 1);
    let store = ArtifactStore::open(&media_root)?;
    let committed = store.read(&fresh.generated_artifacts[0].artifact_id)?;
    assert_eq!(committed.artifact_ref(), fresh.generated_artifacts[0]);
    assert!(!committed.media_root_relative_path.as_path().is_absolute());
    assert_eq!(store.read_payload(&committed)?, b"raw-image-secret");
    let entries =
        std::fs::read_dir(media_root.join("artifacts"))?.collect::<Result<Vec<_>, _>>()?;
    assert_eq!(entries.len(), 1);
    assert!(!entries[0]
        .file_name()
        .to_string_lossy()
        .starts_with(".stage-"));
    Ok(())
}

struct RuntimeCancellingTransport {
    stage: Arc<AtomicUsize>,
    cancellation: CancellationToken,
    observed_timeout: Arc<Mutex<Option<Duration>>>,
    stopped_after_partial: Arc<AtomicBool>,
}

impl CodexHttpTransport for RuntimeCancellingTransport {
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
        timeout: Option<Duration>,
    ) -> Result<CodexHttpStreamResponse, ProviderError> {
        if self.stage.load(Ordering::SeqCst) != 2 {
            return Err(provider_error(
                "native request preceded snapshot/hook gates",
            ));
        }
        *self
            .observed_timeout
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = timeout;
        let frames = [
            concat!(
                "event: response.output_item.added\n",
                "data: {\"type\":\"response.output_item.added\",\"sequence_number\":0,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_runtime\"}}\n\n"
            ),
            concat!(
                "event: response.image_generation_call.partial_image\n",
                "data: {\"type\":\"response.image_generation_call.partial_image\",\"item_id\":\"ig_runtime\",\"sequence_number\":1,\"partial_image_index\":0,\"partial_image_b64\":\"cGFydGlhbA==\"}\n\n"
            ),
            concat!(
                "event: response.output_item.done\n",
                "data: {\"type\":\"response.output_item.done\",\"sequence_number\":2,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_runtime\",\"status\":\"completed\",\"result\":\"ZmluYWwtaW1hZ2U=\"}}\n\n"
            ),
        ];
        assert!(!on_frame(frames[0])?);
        assert!(!on_frame(frames[1])?);
        self.cancellation.cancel();
        self.stopped_after_partial
            .store(on_frame(frames[2])?, Ordering::SeqCst);
        Ok(CodexHttpStreamResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: String::new(),
        })
    }
}
