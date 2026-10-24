use serde_json::{json, Value};
use shacs_core::generated_media::ArtifactStore;
use shacs_core::generated_media::{
    GeneratedArtifactRef, GenerationOperation, ProjectionDisclosure, RetentionPolicy,
};
use shacs_core::runtime::{AgentRunResult, AgentRunSpec, AgentRunner};
use shacs_core::tools::{ImageGenerateTool, ToolRegistry};
use shacs_providers::{LlmResponse, ProviderError};
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const RAW_PROVIDER_ITEM_ID: &str = "AKIAIOSFODNN7EXAMPLE";

#[path = "spec034_codex_media_runtime/control.rs"]
mod control;
#[path = "spec034_codex_media_runtime/support.rs"]
mod support;

use support::*;

#[test]
fn public_agent_run_result_retains_vec_fields_and_send_contract() {
    // Given
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn messages(result: &AgentRunResult) -> &Vec<Value> {
        &result.messages
    }
    fn generated_artifacts(result: &AgentRunResult) -> &Vec<GeneratedArtifactRef> {
        &result.generated_artifacts
    }

    // When
    assert_send::<AgentRunResult>();
    assert_sync::<AgentRunResult>();

    // Then
    let _ = (messages, generated_artifacts);
}

#[test]
fn approved_tool_persists_before_artifact_only_success_without_blank_retry(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let stage = Arc::new(AtomicUsize::new(0));
    let image_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(
        ImageGenerateTool::new(
            Box::new(FixtureImageClient {
                calls: Arc::clone(&image_calls),
                stage: Arc::clone(&stage),
            }),
            root.path().join("legacy"),
            tool_config(),
        )
        .with_artifact_store(ArtifactStore::open(root.path().join("media"))?),
    );
    let provider = FixtureProvider::new(vec![
        tool_call_response(),
        LlmResponse::default(),
        LlmResponse {
            content: Some("unexpected retry sentinel".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let checkpoints = Arc::new(Mutex::new(Vec::new()));
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "draw"})],
        &registry,
        &provider,
        "test",
    );
    spec.tool_context = bypass_context();
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
    let checkpoint_sink = Arc::clone(&checkpoints);
    spec.checkpoint_callback = Some(Arc::new(move |checkpoint| {
        checkpoint_sink
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(checkpoint.clone());
    }));

    // When
    let result = AgentRunner::new().run(spec)?;

    // Then
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        2,
        "artifact-only response retried: {result:?}"
    );
    assert_eq!(image_calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.stop_reason, "completed");
    assert!(result.final_content.is_none());
    assert_eq!(result.generated_artifacts.len(), 1);
    let artifact = ArtifactStore::open(root.path().join("media"))?
        .read(&result.generated_artifacts[0].artifact_id)?;
    assert_eq!(artifact.artifact_ref(), result.generated_artifacts[0]);
    assert!(!artifact.media_root_relative_path.as_path().is_absolute());
    assert_eq!(artifact.provenance.operation, GenerationOperation::Generate);
    assert!(artifact.provenance.source_artifact_ids.is_empty());
    assert_eq!(artifact.retention, RetentionPolicy::UserManaged);
    assert_eq!(
        artifact.disclosure,
        ProjectionDisclosure::RawContentPossibleElsewhere
    );
    let options = serde_json::to_value(&artifact.generation_options_summary)?;
    assert_eq!(
        options.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.6")
    );
    assert_eq!(
        options.get("format").and_then(serde_json::Value::as_str),
        Some("png")
    );
    assert_eq!(
        options.get("count").and_then(serde_json::Value::as_str),
        Some("1")
    );
    let rendered = format!(
        "{} {} {}",
        serde_json::to_string(&result)?,
        serde_json::to_string(&*checkpoints.lock().map_err(|error| error.to_string())?)?,
        serde_json::to_string(&artifact)?
    );
    for forbidden in [
        "raw-image-secret",
        "cmF3LWltYWdlLXNlY3JldA==",
        RAW_PROVIDER_ITEM_ID,
        "/Users/",
    ] {
        assert!(!rendered.contains(forbidden), "runtime leak: {rendered}");
    }
    Ok(())
}

#[test]
fn hook_block_prevents_native_adapter_request_and_artifact_publication(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let stage = Arc::new(AtomicUsize::new(0));
    let image_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(
        ImageGenerateTool::new(
            Box::new(FixtureImageClient {
                calls: Arc::clone(&image_calls),
                stage: Arc::clone(&stage),
            }),
            root.path().join("legacy"),
            tool_config(),
        )
        .with_artifact_store(ArtifactStore::open(root.path().join("media"))?),
    );
    let provider = FixtureProvider::new(vec![
        tool_call_response(),
        LlmResponse {
            content: Some("blocked safely".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "draw"})],
        &registry,
        &provider,
        "test",
    );
    spec.tool_context = bypass_context();
    spec.agent_hook = Some(Arc::new(AdmissionHook { stage, block: true }));

    // When
    let result = AgentRunner::new().run(spec)?;

    // Then
    assert_eq!(image_calls.load(Ordering::SeqCst), 0);
    assert!(result.generated_artifacts.is_empty());
    assert_eq!(result.final_content.as_deref(), Some("blocked safely"));
    Ok(())
}
