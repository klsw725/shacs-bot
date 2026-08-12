use serde_json::json;
use shacs_app::app::AppLifecycleState;
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    ActivationCurrentIdentity, ActivationSnapshotCandidate, AgentLoop, AgentLoopConfig,
    ContextBuilder, InboundMessage, LiveExecutionSnapshotSource, MessageBus, SessionManager,
    WorkspaceTrustRef,
};
use shacs_core::tools::ToolRegistry;
use shacs_projection::Spec030ProjectionProvider;
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use std::error::Error;
use std::sync::{Arc, Mutex};

#[path = "spec031_activation_execution/fixture.rs"]
mod fixture;

struct Client;

impl ProviderClient for Client {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            content: Some("ok".to_owned()),
            ..LlmResponse::default()
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

#[test]
fn agent_loop_snapshot_attaches_only_current_admitted_activation_ref() -> Result<(), Box<dyn Error>>
{
    // Given
    let workspace = tempfile::tempdir()?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    facts.update_resources(vec![fixture::eligible_resource(&"a".repeat(64))])?;
    let provider = LocalSpec030ProjectionProvider::new(facts);
    let activation = fixture::activation();
    let current = Arc::new(Mutex::new(ActivationCurrentIdentity::new(
        WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        "source:project:.shacs/skills/formatter",
        "sha256:deps-a",
        AppLifecycleState::Enabled,
    )));
    let source = LiveExecutionSnapshotSource::default()
        .with_spec030_provider(Arc::new(move || provider.projection()))
        .with_activation_provider({
            let current = Arc::clone(&current);
            Arc::new(move |_resource| {
                Some(ActivationSnapshotCandidate::new(
                    activation.clone(),
                    current
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone(),
                ))
            })
        });
    let mut config = AgentLoopConfig::new(workspace.path(), "gpt-live");
    config.execution_snapshot_source = source;
    let sessions = SessionManager::new(workspace.path())?;
    let tools = ToolRegistry::new();
    let client = Client;
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        sessions,
        ContextBuilder::new(workspace.path()),
        &tools,
        &client,
        config,
    );

    // When
    runtime.process_message(InboundMessage::new("cli", "direct", "user", "first"))?;
    *current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = ActivationCurrentIdentity::new(
        WorkspaceTrustRef::new("workspace:sha256:owner-b"),
        "source:project:.shacs/skills/formatter",
        "sha256:deps-a",
        AppLifecycleState::Enabled,
    );
    runtime.process_message(InboundMessage::new("cli", "direct", "user", "second"))?;

    // Then
    let snapshots = runtime.execution_snapshots();
    assert_eq!(
        snapshots[0].selected_resources[0].activation_ref.as_deref(),
        Some("activation:skill:formatter:v1")
    );
    assert_eq!(snapshots[1].selected_resources[0].activation_ref, None);
    assert!(!serde_json::to_string(&snapshots)?.contains("authorization"));
    assert_eq!(
        json!(snapshots[0].replay.live_execution_authorized),
        json!(false)
    );
    Ok(())
}
