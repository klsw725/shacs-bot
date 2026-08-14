use shacs_core::runtime::{
    AdapterSandboxRef, ConfigMigrationState, ConfigSnapshotRef, CredentialSnapshotRef,
    DataDisclosureWarning, ExecutionSnapshot, ExecutionSnapshotInput, LocalGateSource,
    LocalImprovementBlock, LocalImprovementProposal, ProductionLocalGateSource,
    ProfileSelectionSnapshot, ProviderInputSnapshot, ReplayContract, SandboxMode,
    TokenBudgetSnapshot, TrustedRuntimeFactRef,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, CredentialStatusProjection,
    DataDisclosureProjection, ExecutionAuthority, HookRuntimeProjection, HookRuntimeStatus,
    OptionalSandboxScope, ProcessAdapterCapabilities, ProcessAdapterKind, ProcessAdapterProjection,
    ProcessAdapterSupport, ProcessControlReason, ProcessControlScope, RefreshSerializationStatus,
    ResourceTrust, SandboxFallback, SandboxFilesystemPolicy, SandboxNetworkPolicy, SandboxStatus,
    SandboxStatusProjection, Spec030Availability, Spec030ProjectionProvider,
    Spec030RuntimeProjection, Spec030RuntimeProjectionInput, Spec030RuntimeStatus,
    TraceDisclosureProjection, TraceStatus, TrustedProfileStatus, TrustedRuntimeProfile,
    TrustedRuntimeProfileProjection, WorkspaceTrust,
};
use std::sync::Arc;

#[derive(Debug)]
struct Projection(Spec030RuntimeProjection);

impl Spec030ProjectionProvider for Projection {
    fn projection(&self) -> Spec030RuntimeProjection {
        self.0.clone()
    }
}

#[test]
fn production_gate_denies_headless_confirmation_without_durable_approval(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let snapshot = root.path().join("snapshot.json");
    let snapshot_value = snapshot_fixture()?;
    std::fs::write(&snapshot, serde_json::to_vec(&snapshot_value)?)?;
    let proposal = LocalImprovementProposal::from_json_artifacts(
        "proposal:headless",
        "settings.json",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        r#"{"enabled":true}"#,
        &std::fs::read_to_string(&snapshot)?,
    )?
    .requiring_confirmation();
    let gates = ProductionLocalGateSource::headless(Arc::new(Projection(active_projection()?)));

    // When
    let result = gates.current_receipts(&proposal, proposal.expected_target_digest());

    // Then
    assert_eq!(
        result,
        Err(LocalImprovementBlock::HeadlessConfirmationDenied)
    );
    Ok(())
}

fn snapshot_fixture() -> Result<ExecutionSnapshot, Box<dyn std::error::Error>> {
    Ok(ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: "snapshot:gate:1".to_owned(),
        created_at_unix_ms: 33_003,
        config: ConfigSnapshotRef {
            source_ref: "config:local".to_owned(),
            schema_version: 1,
            migration_state: ConfigMigrationState::Current,
        },
        profiles: ProfileSelectionSnapshot {
            provider: None,
            trusted_runtime: Some("trusted:local".to_owned()),
            context: None,
        },
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: 1,
            profile_ref: "trusted:local".to_owned(),
            projection_digest: "sha256:trusted".to_owned(),
        },
        sandbox: vec![AdapterSandboxRef {
            adapter: ProcessAdapterKind::GenericExec,
            mode: SandboxMode::Active,
            fallback: SandboxFallback::NotApplicable,
        }],
        credential: CredentialSnapshotRef {
            source_kind: None,
            status: CredentialStatus::Resolved,
            fingerprint_status: CredentialFingerprintStatus::Current,
        },
        context_sources: Vec::new(),
        selected_tools: Vec::new(),
        selected_resources: Vec::new(),
        provider: ProviderInputSnapshot {
            provider: "provider:local".to_owned(),
            model: "model:local".to_owned(),
            shaping_version: "v1".to_owned(),
            messages_digest: "sha256:messages".to_owned(),
            tools_digest: "sha256:tools".to_owned(),
        },
        token_budget: TokenBudgetSnapshot {
            tokenizer: "estimate".to_owned(),
            estimator_uncertainty_percent: 0,
            budget_tokens: 100,
            reserved_tokens: 10,
            used_context_tokens: 10,
            estimated_input_tokens: 10,
        },
        disclosure: DataDisclosureWarning {
            raw_content_possible: false,
            surfaces: Vec::new(),
        },
        replay: ReplayContract::diagnostic_only(),
    })?)
}

fn active_projection() -> Result<Spec030RuntimeProjection, Box<dyn std::error::Error>> {
    Ok(Spec030RuntimeProjection::try_new(
        Spec030RuntimeProjectionInput {
            availability: Spec030Availability::Available,
            status: Spec030RuntimeStatus::Active,
            unavailable_reason: None,
            profile: TrustedRuntimeProfileProjection {
                availability: Spec030Availability::Available,
                status: TrustedProfileStatus::Active,
                profile: TrustedRuntimeProfile::TrustedLocalAgent,
                execution_authority: ExecutionAuthority::CurrentOsUser,
                workspace_trust: WorkspaceTrust::UserAsserted,
                workspace_trust_remediation: None,
                resource_trust: ResourceTrust::ExplicitOrTrustedWorkspace,
                default_containment: shacs_projection::DefaultContainment::None,
                optional_sandbox: OptionalSandboxScope::AdapterScoped,
            },
            lifecycle_boundaries: Vec::new(),
            hooks: HookRuntimeProjection {
                availability: Spec030Availability::Available,
                status: HookRuntimeStatus::Active,
                registered_handlers: 1,
                diagnostics: Vec::new(),
                recent_denials: Vec::new(),
            },
            process_adapters: vec![ProcessAdapterProjection {
                adapter: ProcessAdapterKind::GenericExec,
                availability: Spec030Availability::Available,
                support: ProcessAdapterSupport::Supported,
                control_scope: ProcessControlScope::ControlledChild,
                reason: ProcessControlReason::ControlledChildObservedNoRollback,
                capabilities: ProcessAdapterCapabilities {
                    timeout: true,
                    abort: true,
                    cwd: true,
                    env: true,
                    bounded_output: true,
                    descendant_cleanup: true,
                    startup_readiness: false,
                    generation_fencing: false,
                },
                recent_outcomes: Vec::new(),
            }],
            credential: CredentialStatusProjection {
                availability: Spec030Availability::Available,
                status: CredentialStatus::Resolved,
                source: Some(CredentialSource::Environment),
                fingerprint: CredentialFingerprintStatus::Current,
                refresh_serialization: RefreshSerializationStatus::Active,
            },
            sandbox: SandboxStatusProjection {
                availability: Spec030Availability::Available,
                status: SandboxStatus::Active,
                fallback: SandboxFallback::NotApplicable,
                applied_adapters: vec![ProcessAdapterKind::GenericExec],
                filesystem_policy: SandboxFilesystemPolicy::Applied,
                network_policy: SandboxNetworkPolicy::Applied,
            },
            resources: Vec::new(),
            disclosure: DataDisclosureProjection {
                raw_content_possible: false,
                surfaces: Vec::new(),
                trace: TraceDisclosureProjection {
                    status: TraceStatus::Disabled,
                    preview: None,
                },
            },
        },
    )?)
}
