use serde_json::json;
use sha2::Digest;
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    trusted_runtime_fact_refs, AdapterSandboxRef, ConfigMigrationState, ConfigSnapshotRef,
    ContextInclusion, ContextSourceSnapshot, CredentialSnapshotRef, DataDisclosureWarning,
    ExecutionSnapshot, ExecutionSnapshotInput, ProfileSelectionSnapshot, ProviderInputSnapshot,
    ReplayContract, ResourceIdentitySnapshot, SandboxMode, SelectedIdentitySnapshot,
    TokenBudgetSnapshot, TrustedRuntimeFactRef,
};
use shacs_core::runtime::{
    ActiveLoopTask, AgentLoop, AgentLoopConfig, AgentRunSpec, AgentRunner, CancellationToken,
    ContextBuilder, InboundMessage, MessageBus, SessionManager,
};
use shacs_core::tools::ToolRegistry;
use shacs_projection::Spec030ProjectionProvider;
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, DataSurface,
    ProcessAdapterKind, SandboxFallback,
};
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use std::error::Error;
use std::sync::{Arc, Mutex};

fn input(id: &str, created_at_unix_ms: u64) -> ExecutionSnapshotInput {
    ExecutionSnapshotInput {
        snapshot_id: id.to_owned(),
        created_at_unix_ms,
        config: ConfigSnapshotRef {
            source_ref: "config:workspace".to_owned(),
            schema_version: 1,
            migration_state: ConfigMigrationState::Current,
        },
        profiles: ProfileSelectionSnapshot {
            provider: Some("provider:primary".to_owned()),
            trusted_runtime: Some("runtime:local".to_owned()),
            context: Some("context:default".to_owned()),
        },
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: 1,
            profile_ref: "trusted:local-agent".to_owned(),
            projection_digest: "sha256:trusted".to_owned(),
        },
        sandbox: vec![AdapterSandboxRef {
            adapter: ProcessAdapterKind::GenericExec,
            mode: SandboxMode::Active,
            fallback: SandboxFallback::NotApplicable,
        }],
        credential: CredentialSnapshotRef {
            source_kind: Some(CredentialSource::Environment),
            status: CredentialStatus::Resolved,
            fingerprint_status: CredentialFingerprintStatus::Current,
        },
        context_sources: vec![ContextSourceSnapshot {
            source_ref: "context:system".to_owned(),
            content_digest: "sha256:context".to_owned(),
            inclusion: ContextInclusion::Included,
            original_bytes: 20,
            included_bytes: 20,
            precedence: shacs_core::runtime::ContextArtifactPriority::ExplicitInline,
            decision: shacs_core::runtime::ContextBudgetDecision::Included,
            estimated_tokens: 5,
            included_tokens: 5,
            reason: None,
        }],
        selected_tools: vec![SelectedIdentitySnapshot {
            identity: "tool:read_file".to_owned(),
            activation_ref: None,
        }],
        selected_resources: vec![ResourceIdentitySnapshot {
            identity: "resource:skill:test".to_owned(),
            content_digest: Some("sha256:resource".to_owned()),
            activation_ref: Some("activation:test:v1".to_owned()),
        }],
        provider: ProviderInputSnapshot {
            provider: "openai".to_owned(),
            model: "gpt-test".to_owned(),
            shaping_version: "openai-compatible.v1".to_owned(),
            messages_digest: "sha256:messages".to_owned(),
            tools_digest: "sha256:tools".to_owned(),
        },
        token_budget: TokenBudgetSnapshot {
            tokenizer: "estimated:chars".to_owned(),
            estimator_uncertainty_percent: 25,
            budget_tokens: 4096,
            reserved_tokens: 256,
            used_context_tokens: 128,
            estimated_input_tokens: 512,
        },
        disclosure: DataDisclosureWarning {
            raw_content_possible: true,
            surfaces: vec![DataSurface::Session, DataSurface::Trace],
        },
        replay: ReplayContract::diagnostic_only(),
    }
}

#[test]
fn snapshot_round_trips_and_validates_provenance() -> Result<(), Box<dyn Error>> {
    // Given
    let snapshot = ExecutionSnapshot::create(input("execution:1", 31_002))?;

    // When
    let encoded = serde_json::to_string(&snapshot)?;
    let decoded = ExecutionSnapshot::parse_json(&encoded)?;

    // Then
    assert_eq!(decoded, snapshot);
    assert_eq!(decoded.replay, ReplayContract::diagnostic_only());
    assert!(decoded.validate_provenance().is_ok());
    Ok(())
}

#[test]
fn snapshot_rejects_tampered_provenance_and_authorization_fields() -> Result<(), Box<dyn Error>> {
    // Given
    let snapshot = ExecutionSnapshot::create(input("execution:2", 31_003))?;
    let mut value = serde_json::to_value(snapshot)?;

    // When
    value["provider"]["model"] = json!("mutated-model");
    let tampered = serde_json::from_value::<ExecutionSnapshot>(value.clone())?;
    value["permission"] = json!({"allowed": true});

    // Then
    assert!(tampered.validate_provenance().is_err());
    assert!(serde_json::from_value::<ExecutionSnapshot>(value).is_err());
    Ok(())
}

#[test]
fn spec030_adapter_returns_fact_refs_without_authorization_state() -> Result<(), Box<dyn Error>> {
    // Given
    let provider = LocalSpec030ProjectionProvider::new(Spec030FactStore::new(
        WorkspaceTrustObservation::Trusted,
    ));
    let projection = provider.projection();

    // When
    let refs = trusted_runtime_fact_refs(&projection)?;
    let value = serde_json::to_value((
        &refs.trusted_runtime,
        &refs.sandbox,
        &refs.credential,
        &refs.resources,
        &refs.disclosure,
    ))?;

    // Then
    assert_eq!(
        refs.trusted_runtime.schema_version,
        projection.schema_version()
    );
    assert_eq!(refs.sandbox.len(), projection.process_adapters().len());
    assert_eq!(refs.resources.len(), projection.resources().len());
    let encoded = value.to_string();
    for excluded in [
        "permission",
        "approval",
        "capability",
        "authorization",
        "policy",
    ] {
        assert!(!encoded.contains(excluded));
    }
    Ok(())
}

struct CapturingClient {
    requests: Arc<Mutex<Vec<ProviderRequest>>>,
}

impl ProviderClient for CapturingClient {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .map_err(|_| ProviderError::Api {
                status: None,
                message: "capture lock poisoned".to_owned(),
                retryable: false,
                headers: Default::default(),
                body: None,
            })?
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
fn runner_freezes_fresh_snapshot_before_provider_handoff() -> Result<(), Box<dyn Error>> {
    // Given
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = CapturingClient { requests };
    let tools = ToolRegistry::new();
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let sequence = Arc::new(Mutex::new(0_u64));
    let mut spec = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "hello"})],
        &tools,
        &client,
        "gpt-test",
    );
    spec.execution_snapshot_resolver = {
        let sequence = Arc::clone(&sequence);
        Arc::new(move |_request| {
            let mut value = sequence
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *value += 1;
            Ok(input(&format!("execution:{}", *value), 31_000 + *value))
        })
    };
    spec.execution_snapshot_callback = {
        let snapshots = Arc::clone(&snapshots);
        Arc::new(move |snapshot| {
            snapshots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(snapshot.clone());
        })
    };

    // When
    AgentRunner::new().run(spec.clone())?;
    AgentRunner::new().run(spec)?;

    // Then
    let captured = snapshots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(captured.len(), 2);
    assert_ne!(captured[0].snapshot_id, captured[1].snapshot_id);
    assert_eq!(captured[0].provider.model, "gpt-test");
    assert!(captured
        .iter()
        .all(|snapshot| !snapshot.replay.live_execution_authorized));
    Ok(())
}

#[test]
fn adapter_handoff_cannot_reread_or_mutate_frozen_sources() -> Result<(), Box<dyn Error>> {
    // Given
    let request = ProviderRequest {
        messages: vec![json!({"role": "user", "content": "frozen"})],
        tools: Vec::new(),
        model: "gpt-before".to_owned(),
        settings: Default::default(),
        tool_choice: None,
    };
    let mut source = input("execution:immutable", 31_010);
    source.provider.model = "stale-source-model".to_owned();
    let handoff = shacs_core::runtime::ProviderExecutionHandoff::freeze(source, request)?;

    // When
    let snapshot = handoff.snapshot().clone();
    let adapter_request = handoff.into_request();

    // Then
    assert_eq!(snapshot.provider.model, "gpt-before");
    assert_eq!(adapter_request.model, "gpt-before");
    assert_eq!(snapshot.selected_tools, Vec::new());
    assert!(snapshot.validate_provenance().is_ok());
    Ok(())
}

#[test]
fn live_agent_loop_emits_fresh_snapshots_without_runner_hook_setup() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let sessions = SessionManager::new(workspace.path())?;
    let context = ContextBuilder::new(workspace.path());
    let tools = ToolRegistry::new();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = CapturingClient {
        requests: Arc::clone(&requests),
    };
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let provider = LocalSpec030ProjectionProvider::new(facts.clone());
    let mut config = AgentLoopConfig::new(workspace.path(), "gpt-live");
    config.execution_snapshot_source = config
        .execution_snapshot_source
        .clone()
        .with_spec030_provider(Arc::new(move || provider.projection()));
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        sessions,
        context,
        &tools,
        &client,
        config,
    );

    // When
    runtime.process_message(InboundMessage::new("cli", "direct", "user", "first"))?;
    facts.update_sandbox(shacs_core::runtime::trusted_runtime::SandboxObservation::Disabled)?;
    runtime.process_message(InboundMessage::new("cli", "direct", "user", "second"))?;

    // Then
    let snapshots = runtime.execution_snapshots();
    assert_eq!(snapshots.len(), 2);
    assert_ne!(snapshots[0].snapshot_id, snapshots[1].snapshot_id);
    assert_ne!(
        snapshots[0].trusted_runtime.projection_digest,
        snapshots[1].trusted_runtime.projection_digest
    );
    assert_eq!(
        snapshots[0].provider.messages_digest,
        request_messages_digest(&requests, 0)?
    );
    assert_eq!(
        snapshots[1].provider.messages_digest,
        request_messages_digest(&requests, 1)?
    );
    assert!(snapshots
        .iter()
        .all(|snapshot| snapshot.config.migration_state == ConfigMigrationState::Unavailable));
    let persisted = runtime
        .session_manager_mut()
        .get_or_create("cli:user")
        .metadata
        .get("spec031_execution_snapshot")
        .cloned()
        .ok_or("persisted snapshot missing")?;
    assert_eq!(
        ExecutionSnapshot::parse_json(&persisted.to_string())?,
        snapshots[1]
    );
    Ok(())
}

#[test]
fn no_provider_runs_never_persist_a_prior_snapshot() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let sessions = SessionManager::new(workspace.path())?;
    let tools = ToolRegistry::new();
    let client = CapturingClient {
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        sessions,
        ContextBuilder::new(workspace.path()),
        &tools,
        &client,
        AgentLoopConfig::new(workspace.path(), "gpt-live"),
    );
    runtime.process_message(InboundMessage::new("cli", "sender", "same", "provider"))?;
    let prior = persisted_snapshot(&mut runtime, "cli:same")?.ok_or("prior missing")?;

    // When: zero-iteration in the same and another session
    runtime.config_mut().max_iterations = 0;
    runtime.process_message(InboundMessage::new("cli", "sender", "same", "zero"))?;
    runtime.process_message(InboundMessage::new("cli", "sender", "other", "zero"))?;

    // Then
    assert_eq!(
        persisted_snapshot(&mut runtime, "cli:same")?,
        Some(prior.clone())
    );
    assert_eq!(persisted_snapshot(&mut runtime, "cli:other")?, None);

    // When: resolver failure in the same and another session
    runtime.config_mut().max_iterations = 200;
    runtime
        .config_mut()
        .execution_snapshot_source
        .provider_id
        .clear();
    runtime.process_message(InboundMessage::new("cli", "sender", "same", "error"))?;
    runtime.process_message(InboundMessage::new("cli", "sender", "failed", "error"))?;

    // Then
    assert_eq!(persisted_snapshot(&mut runtime, "cli:same")?, Some(prior));
    assert_eq!(persisted_snapshot(&mut runtime, "cli:failed")?, None);

    // When: cancellation before provider dispatch in another session
    runtime.config_mut().execution_snapshot_source.provider_id = "provider:test".to_owned();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    runtime.loop_task_registry().register(ActiveLoopTask::new(
        "cli:cancelled",
        "cancelled-task",
        cancellation,
    ));
    runtime.process_message(InboundMessage::new(
        "cli",
        "sender",
        "cancelled",
        "cancelled",
    ))?;

    // Then
    assert_eq!(persisted_snapshot(&mut runtime, "cli:cancelled")?, None);
    Ok(())
}

fn persisted_snapshot(
    runtime: &mut AgentLoop<'_>,
    session_key: &str,
) -> Result<Option<ExecutionSnapshot>, Box<dyn Error>> {
    runtime
        .session_manager_mut()
        .get_or_create(session_key)
        .metadata
        .get("spec031_execution_snapshot")
        .map(|value| ExecutionSnapshot::parse_json(&value.to_string()))
        .transpose()
        .map_err(Into::into)
}

fn request_messages_digest(
    requests: &Arc<Mutex<Vec<ProviderRequest>>>,
    index: usize,
) -> Result<String, Box<dyn Error>> {
    let requests = requests.lock().map_err(|_| "request lock poisoned")?;
    let request = requests.get(index).ok_or("request missing")?;
    Ok(format!(
        "sha256:{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&request.messages)?)
    ))
}
