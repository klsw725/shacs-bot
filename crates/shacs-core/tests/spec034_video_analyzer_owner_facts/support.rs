use shacs_core::runtime::{
    ConfigMigrationState, ConfigSnapshotRef, CredentialSnapshotRef, DataDisclosureWarning,
    ExecutionSnapshot, ExecutionSnapshotInput, ProfileSelectionSnapshot, ProviderInputSnapshot,
    ReplayContract, ResourceIdentitySnapshot, TokenBudgetSnapshot, TrustedRuntimeFactRef,
    VideoAnalyzerOwnerFactsInput,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, CredentialStatusProjection,
    DataDisclosureProjection, DataSurface, DefaultContainment, ExecutionAuthority,
    OptionalSandboxScope, RefreshSerializationStatus, ResourceActivation,
    ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind, ResourceLoadStatus,
    ResourcePrecedence, ResourceSource, ResourceTrust, SandboxFallback, SandboxFilesystemPolicy,
    SandboxNetworkPolicy, SandboxStatus, SandboxStatusProjection, Spec030Availability,
    Spec031ExternalOwnerRef, Spec031Freshness, TraceDisclosureProjection, TracePreviewProjection,
    TraceStatus, TrustedCodeDisclosure, TrustedProfileStatus, TrustedRuntimeProfile,
    TrustedRuntimeProfileProjection, WorkspaceTrust,
};
use std::error::Error;

pub struct OwnerFixture {
    analyzer_ref: Spec031ExternalOwnerRef,
    resource: ResourceCandidateProjection,
    profile: TrustedRuntimeProfileProjection,
    sandbox: SandboxStatusProjection,
    credential: CredentialStatusProjection,
    disclosure: DataDisclosureProjection,
    snapshot: ExecutionSnapshot,
}

impl OwnerFixture {
    pub fn new(
        snapshot_id: &str,
        trace_preview: Option<TracePreviewProjection>,
    ) -> Result<Self, Box<dyn Error>> {
        let (surfaces, status) = if trace_preview.is_some() {
            (vec![DataSurface::Trace], TraceStatus::Enabled)
        } else {
            (
                vec![DataSurface::Session, DataSurface::Log],
                TraceStatus::Disabled,
            )
        };
        let disclosure = DataDisclosureProjection {
            raw_content_possible: true,
            surfaces,
            trace: TraceDisclosureProjection {
                status,
                preview: trace_preview,
            },
        };
        let analyzer_ref = Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/fixture")?;
        let resource = ResourceCandidateProjection {
            resource_ref: analyzer_ref.as_str().to_owned(),
            kind: ResourceKind::Package,
            source: ResourceSource::Package,
            precedence: ResourcePrecedence::Package,
            canonical_path: "/Users/private/bin/analyzer".to_owned(),
            content_sha256: Some("sha256:resource".to_owned()),
            collision: ResourceCollisionStatus::None,
            load_status: ResourceLoadStatus::Loaded,
            activation: ResourceActivation::Explicit,
            trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            diagnostics: Vec::new(),
        };
        let profile = TrustedRuntimeProfileProjection {
            availability: Spec030Availability::Available,
            status: TrustedProfileStatus::Active,
            profile: TrustedRuntimeProfile::TrustedLocalAgent,
            execution_authority: ExecutionAuthority::CurrentOsUser,
            workspace_trust: WorkspaceTrust::UserAsserted,
            workspace_trust_remediation: None,
            resource_trust: ResourceTrust::ExplicitOrTrustedWorkspace,
            default_containment: DefaultContainment::None,
            optional_sandbox: OptionalSandboxScope::AdapterScoped,
        };
        let sandbox = SandboxStatusProjection {
            availability: Spec030Availability::Unknown,
            status: SandboxStatus::Unknown,
            fallback: SandboxFallback::Unknown,
            applied_adapters: Vec::new(),
            filesystem_policy: SandboxFilesystemPolicy::Unknown,
            network_policy: SandboxNetworkPolicy::Unknown,
        };
        let credential = CredentialStatusProjection {
            availability: Spec030Availability::Degraded,
            status: CredentialStatus::Missing,
            source: Some(CredentialSource::Environment),
            fingerprint: CredentialFingerprintStatus::Unavailable,
            refresh_serialization: RefreshSerializationStatus::Inactive,
        };
        let snapshot = execution_snapshot(snapshot_id, analyzer_ref.as_str())?;
        Ok(Self {
            analyzer_ref,
            resource,
            profile,
            sandbox,
            credential,
            disclosure,
            snapshot,
        })
    }

    pub fn input(&self, freshness: Spec031Freshness) -> VideoAnalyzerOwnerFactsInput<'_> {
        VideoAnalyzerOwnerFactsInput {
            analyzer_ref: Some(&self.analyzer_ref),
            analyzer_resource: Some(&self.resource),
            profile: Some(&self.profile),
            sandbox: Some(&self.sandbox),
            credential: Some(&self.credential),
            disclosure: Some(&self.disclosure),
            snapshot: Some(&self.snapshot),
            freshness,
        }
    }
}

fn execution_snapshot(
    snapshot_id: &str,
    analyzer_ref: &str,
) -> Result<ExecutionSnapshot, Box<dyn Error>> {
    Ok(ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: snapshot_id.to_owned(),
        created_at_unix_ms: 1,
        config: ConfigSnapshotRef {
            source_ref: "config:fixture".to_owned(),
            schema_version: 1,
            migration_state: ConfigMigrationState::Current,
        },
        profiles: ProfileSelectionSnapshot {
            provider: None,
            trusted_runtime: Some("trusted-local-agent".to_owned()),
            context: None,
        },
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: 1,
            profile_ref: "trusted-runtime:fixture".to_owned(),
            projection_digest: "sha256:trusted-runtime".to_owned(),
        },
        sandbox: Vec::new(),
        credential: CredentialSnapshotRef {
            source_kind: Some(CredentialSource::Environment),
            status: CredentialStatus::Missing,
            fingerprint_status: CredentialFingerprintStatus::Unavailable,
        },
        context_sources: Vec::new(),
        selected_tools: Vec::new(),
        selected_resources: vec![ResourceIdentitySnapshot {
            identity: analyzer_ref.to_owned(),
            content_digest: Some("sha256:resource".to_owned()),
            activation_ref: None,
        }],
        provider: ProviderInputSnapshot {
            provider: "fixture".to_owned(),
            model: "fixture".to_owned(),
            shaping_version: "v1".to_owned(),
            messages_digest: "sha256:messages".to_owned(),
            tools_digest: "sha256:tools".to_owned(),
        },
        token_budget: TokenBudgetSnapshot {
            tokenizer: "fixture".to_owned(),
            estimator_uncertainty_percent: 0,
            budget_tokens: 1,
            reserved_tokens: 0,
            used_context_tokens: 0,
            estimated_input_tokens: 0,
        },
        disclosure: DataDisclosureWarning {
            raw_content_possible: true,
            surfaces: vec![DataSurface::Session],
        },
        replay: ReplayContract::diagnostic_only(),
    })?)
}
