use serde_json::Value;
use shacs_api::{
    handle_api_request, ApiError, ApiHttpRequest, ChatCompletionAdapter, ChatCompletionInvocation,
    TRUSTED_RUNTIME_PATH,
};
use shacs_cli::spec030_cli::{render_trusted_runtime, Spec030CliFormat};
use shacs_core::runtime::trusted_runtime::{
    build_trusted_runtime_projection, LifecycleObservations, LocalSpec030ProjectionProvider,
    ProcessAdapterObservation, SandboxObservation, Spec030FactStore, TrustedRuntimeInput,
    TrustedRuntimeOwnerFacts, WorkspaceTrustObservation,
};
use shacs_projection::*;
use shacs_providers::LlmResponse;
use shacs_tui::trusted_runtime_view::{trusted_runtime_human, trusted_runtime_json};
use std::error::Error;
use std::process::Command;

#[derive(Clone)]
struct FixtureProvider(Spec030RuntimeProjection);

#[derive(Clone)]
struct SharedProvider(LocalSpec030ProjectionProvider);

impl Spec030ProjectionProvider for FixtureProvider {
    fn projection(&self) -> Spec030RuntimeProjection {
        self.0.clone()
    }
}

impl ChatCompletionAdapter for FixtureProvider {
    fn configured_model(&self) -> &str {
        "spec030-fixture"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        self.projection()
    }
}

impl Spec030ProjectionProvider for SharedProvider {
    fn projection(&self) -> Spec030RuntimeProjection {
        self.0.projection()
    }
}

impl ChatCompletionAdapter for SharedProvider {
    fn configured_model(&self) -> &str {
        "spec030-shared"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }

    fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        self.projection()
    }
}

#[test]
fn cli_api_and_tui_serialize_identical_spec030_owner_facts() -> Result<(), Box<dyn Error>> {
    let projection = fixture_projection()?;
    let provider = FixtureProvider(projection.clone());

    let cli: Value =
        serde_json::from_str(&render_trusted_runtime(&provider, Spec030CliFormat::Json)?)?;
    let api = handle_api_request(
        ApiHttpRequest::get(format!("{TRUSTED_RUNTIME_PATH}?schema_version=1")),
        &provider,
    );
    let tui: Value = serde_json::from_str(&trusted_runtime_json(&projection)?)?;

    assert_eq!(cli, api.body);
    assert_eq!(api.body, tui);
    let cli_human = render_trusted_runtime(&provider, Spec030CliFormat::Human)?;
    assert_eq!(cli_human, trusted_runtime_human(&projection));
    Ok(())
}

#[test]
fn cli_api_and_tui_read_the_same_updated_runtime_fact_snapshot() -> Result<(), Box<dyn Error>> {
    let store = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let provider = SharedProvider(LocalSpec030ProjectionProvider::new(store.clone()));
    store.update_credential(CredentialStatusProjection {
        availability: Spec030Availability::Degraded,
        status: CredentialStatus::Missing,
        source: None,
        fingerprint: CredentialFingerprintStatus::Unavailable,
        refresh_serialization: RefreshSerializationStatus::Inactive,
    })?;

    let cli: Value =
        serde_json::from_str(&render_trusted_runtime(&provider, Spec030CliFormat::Json)?)?;
    let api = handle_api_request(
        ApiHttpRequest::get(format!("{TRUSTED_RUNTIME_PATH}?schema_version=1")),
        &provider,
    );
    let tui: Value = serde_json::from_str(&trusted_runtime_json(&provider.projection())?)?;

    assert_eq!(cli, api.body);
    assert_eq!(api.body, tui);
    assert_eq!(cli["credential"]["status"], "missing");
    Ok(())
}

#[test]
fn cli_api_and_tui_share_live_configured_resource_and_trace_facts() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("prompt.md"), "prompt")?;
    let trace = root.path().join("trace.jsonl");
    std::fs::write(&trace, "one\ntwo\n")?;
    let config = root.path().join("config.json");
    std::fs::write(
        &config,
        serde_json::json!({"trustedRuntime": {
            "resources": [{"resourceRef":"prompt:live","kind":"prompt","path":"prompt.md"}],
            "trace": {"enabled":true,"destination":"configuredRemote","path":trace.to_string_lossy()}
        }})
        .to_string(),
    )?;
    let provider = SharedProvider(LocalSpec030ProjectionProvider::load(
        Some(config),
        Some(workspace),
    ));

    // When
    let cli: Value =
        serde_json::from_str(&render_trusted_runtime(&provider, Spec030CliFormat::Json)?)?;
    let api = handle_api_request(
        ApiHttpRequest::get(format!("{TRUSTED_RUNTIME_PATH}?schema_version=1")),
        &provider,
    );
    let tui: Value = serde_json::from_str(&trusted_runtime_json(&provider.projection())?)?;

    // Then
    assert_eq!(cli, api.body);
    assert_eq!(api.body, tui);
    assert_eq!(cli["resources"][0]["resourceRef"], "prompt:live");
    assert_eq!(
        cli["resources"][0]["contentSha256"].as_str().map(str::len),
        Some(64)
    );
    assert_eq!(cli["disclosure"]["trace"]["status"], "preview");
    assert_eq!(cli["disclosure"]["trace"]["preview"]["recordCount"], 2);
    assert_eq!(
        cli["disclosure"]["trace"]["preview"]["destination"],
        "configuredRemote"
    );
    assert!(cli["disclosure"]["trace"]["preview"]
        .get("exporter")
        .is_none());
    assert!(cli["disclosure"]["trace"]["preview"]
        .get("endpointSummary")
        .is_none());
    Ok(())
}

#[test]
fn tui_once_renders_trusted_runtime_when_session_store_is_missing() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;

    let output = Command::new(env!("CARGO_BIN_EXE_shacs-tui"))
        .args([
            "--workspace",
            workspace.path().to_string_lossy().as_ref(),
            "--once",
        ])
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = String::from_utf8(output.stdout)?;
    assert!(rendered.contains("Trusted runtime: availability=unavailable status=unavailable"));
    assert!(rendered.contains("Unavailable: reason=ownerUnavailable"));
    Ok(())
}

fn fixture_projection() -> Result<Spec030RuntimeProjection, Box<dyn Error>> {
    let facts = TrustedRuntimeOwnerFacts {
        workspace_trust: WorkspaceTrustObservation::Untrusted,
        lifecycle: LifecycleObservations {
            daemon_worker: LifecycleBoundaryStatus::Active,
            kernel: LifecycleBoundaryStatus::Unavailable,
        },
        hooks: HookRuntimeProjection {
            availability: Spec030Availability::Available,
            status: HookRuntimeStatus::Active,
            registered_handlers: 1,
            diagnostics: Vec::new(),
            recent_denials: Vec::new(),
        },
        process_adapters: vec![ProcessAdapterObservation::supported(
            ProcessAdapterKind::Bash,
            ProcessAdapterCapabilities {
                timeout: true,
                abort: true,
                cwd: true,
                env: true,
                bounded_output: true,
                descendant_cleanup: true,
                startup_readiness: false,
                generation_fencing: false,
            },
            Vec::new(),
            shacs_projection::ProcessControlReason::ControlledChildObservedNoRollback,
        )],
        credential: CredentialStatusProjection {
            availability: Spec030Availability::Available,
            status: CredentialStatus::Resolved,
            source: Some(CredentialSource::Environment),
            fingerprint: CredentialFingerprintStatus::Current,
            refresh_serialization: RefreshSerializationStatus::Active,
        },
        sandbox: SandboxObservation::Active {
            applied_adapters: vec![ProcessAdapterKind::Bash],
            filesystem_policy: SandboxFilesystemPolicy::Applied,
            network_policy: SandboxNetworkPolicy::Applied,
        },
        resources: vec![ResourceCandidateProjection {
            resource_ref: "extension:fixture".to_owned(),
            kind: ResourceKind::Extension,
            source: ResourceSource::Explicit,
            precedence: ResourcePrecedence::Explicit,
            canonical_path: "/fixture/extension".to_owned(),
            content_sha256: Some("0".repeat(64)),
            collision: ResourceCollisionStatus::Winner,
            load_status: ResourceLoadStatus::Loaded,
            activation: ResourceActivation::Explicit,
            trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            diagnostics: Vec::new(),
        }],
        disclosure: DataDisclosureProjection {
            raw_content_possible: true,
            surfaces: vec![DataSurface::Session, DataSurface::Trace],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Preview,
                preview: Some(TracePreviewProjection {
                    record_count: 2,
                    approximate_bytes: 128,
                    destination: TraceDestination::LocalOnly,
                    exporter: None,
                    endpoint_summary: None,
                }),
            },
        },
    };
    Ok(build_trusted_runtime_projection(
        TrustedRuntimeInput::Available(Box::new(facts)),
    )?)
}
