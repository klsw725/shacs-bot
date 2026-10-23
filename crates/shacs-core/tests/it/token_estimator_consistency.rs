use serde_json::Value;
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    select_token_estimator, AgentLoop, AgentLoopConfig, ContextBuilder, InboundMessage, MessageBus,
    SessionManager,
};
use shacs_core::tools::ToolRegistry;
use shacs_projection::Spec030ProjectionProvider;
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use std::sync::{Arc, Mutex};

struct CapturingClient {
    requests: Arc<Mutex<Vec<ProviderRequest>>>,
}

impl ProviderClient for CapturingClient {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request);
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
fn live_agent_loop_snapshot_uses_handoff_estimator_and_evidence() {
    let workspace = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = CapturingClient {
        requests: Arc::clone(&requests),
    };
    let tools = ToolRegistry::new();
    let provider = LocalSpec030ProjectionProvider::new(Spec030FactStore::new(
        WorkspaceTrustObservation::Trusted,
    ));
    let mut config = AgentLoopConfig::new(workspace.path(), "claude-test");
    config.provider_id = "anthropic".to_owned();
    config.context_block_limit = Some(4096);
    config.execution_snapshot_source = config
        .execution_snapshot_source
        .clone()
        .with_spec030_provider(Arc::new(move || provider.projection()));
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())
            .unwrap_or_else(|error| panic!("session manager failed: {error}")),
        ContextBuilder::new(workspace.path()),
        &tools,
        &client,
        config,
    );

    runtime
        .process_message(InboundMessage::new("cli", "direct", "user", "abcdefghijkl"))
        .unwrap_or_else(|error| panic!("agent loop failed: {error}"));

    let snapshots = runtime.execution_snapshots();
    let snapshot = snapshots
        .first()
        .unwrap_or_else(|| panic!("execution snapshot missing"));
    let estimator = select_token_estimator("anthropic", "claude-test");
    let requests = requests
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let request = requests
        .first()
        .unwrap_or_else(|| panic!("provider request missing"));
    let estimated_input_tokens = request
        .messages
        .iter()
        .map(Value::to_string)
        .map(|message| estimator.estimate(&message) as u64)
        .sum::<u64>();

    assert_eq!(snapshot.token_budget.tokenizer, estimator.name);
    assert_eq!(snapshot.token_budget.estimator_uncertainty_percent, 20);
    assert_eq!(snapshot.token_budget.budget_tokens, 4096);
    assert!(snapshot.token_budget.reserved_tokens > 0);
    assert_eq!(snapshot.token_budget.used_context_tokens, 0);
    assert_eq!(
        snapshot.token_budget.estimated_input_tokens,
        estimated_input_tokens
    );
}
