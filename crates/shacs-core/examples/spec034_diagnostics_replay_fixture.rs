use serde::Serialize;
use shacs_core::generated_media::GeneratedArtifactRecord;
use shacs_core::runtime::{
    project_media_evidence_diagnostics, project_video_analyzer, replay_recorded_media_evidence,
    ConfigMigrationState, ConfigSnapshotRef, CredentialSnapshotRef, DataDisclosureWarning,
    ExecutionSnapshot, ExecutionSnapshotInput, MediaEvidenceDiagnosticsInput,
    MediaEvidenceReplayDependencies, ProfileSelectionSnapshot, ProviderInputSnapshot,
    ReplayContract, ResourceIdentitySnapshot, TokenBudgetSnapshot, TrustedRuntimeFactRef,
    VideoAnalysisPolicy, VideoAnalyzerCapability, VideoAnalyzerOutcomeInput,
    VideoAnalyzerOwnerFactsInput, VideoAnalyzerProjectionInput, VideoContextAnalysis,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, CredentialStatusProjection,
    DataDisclosureProjection, DataSurface, DefaultContainment, ExecutionAuthority,
    OptionalSandboxScope, RefreshSerializationStatus, ResourceActivation,
    ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind, ResourceLoadStatus,
    ResourcePrecedence, ResourceSource, ResourceTrust, SandboxFallback, SandboxFilesystemPolicy,
    SandboxNetworkPolicy, SandboxStatus, SandboxStatusProjection, Spec030Availability,
    Spec031ExternalOwnerRef, Spec031Freshness, TraceDisclosureProjection, TraceStatus,
    TrustedCodeDisclosure, TrustedProfileStatus, TrustedRuntimeProfile,
    TrustedRuntimeProfileProjection, WorkspaceTrust,
};
use std::cell::Cell;
use std::error::Error;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct FixtureOutput {
    artifact_status: &'static str,
    analyzer_status: String,
    raw_content_possible: bool,
    snapshot_ref: String,
    network_calls: u64,
    credential_calls: u64,
    analyzer_calls: u64,
    resource_calls: u64,
}

#[derive(Default)]
struct DependencySpies {
    network: Cell<u64>,
    credential: Cell<u64>,
    analyzer: Cell<u64>,
    resource: Cell<u64>,
}

impl MediaEvidenceReplayDependencies for DependencySpies {
    fn request_network(&self) {
        self.network.set(self.network.get() + 1);
    }

    fn resolve_credential(&self) {
        self.credential.set(self.credential.get() + 1);
    }

    fn invoke_analyzer(&self) {
        self.analyzer.set(self.analyzer.get() + 1);
    }

    fn resolve_resource(&self) {
        self.resource.set(self.resource.get() + 1);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let artifact: GeneratedArtifactRecord = serde_json::from_value(serde_json::json!({
        "schema": "shacs.generated-artifact.v1",
        "artifactId": "artifact-034-driver",
        "candidateId": "candidate-034-driver",
        "kind": "image",
        "mediaRootRelativePath": "artifacts/artifact-034-driver/image.png",
        "mimeType": "image/png",
        "byteLen": 12,
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "provenance": {
            "kind": "generated",
            "providerId": "provider-safe",
            "modelId": "model-safe",
            "operation": "generate",
            "sourceArtifactIds": []
        },
        "generationOptionsSummary": {},
        "createdAt": "2026-08-15T00:00:00Z",
        "retention": { "policy": "user_managed" },
        "disclosure": "raw_content_possible_elsewhere"
    }))?;
    let disclosure = DataDisclosureProjection {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Session, DataSurface::Log],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Disabled,
            preview: None,
        },
    };
    let analyzer_ref = Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/driver")?;
    let resource = analyzer_resource(analyzer_ref.as_str());
    let profile = analyzer_profile();
    let sandbox = analyzer_sandbox();
    let credential = analyzer_credential();
    let snapshot = execution_snapshot(analyzer_ref.as_str())?;
    let analysis = VideoContextAnalysis {
        metadata: None,
        subtitles: Some("recorded subtitle".to_owned()),
        scene_summary: None,
        keyframe_summary: None,
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    };
    let analyzer = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: VideoAnalyzerOwnerFactsInput {
            analyzer_ref: Some(&analyzer_ref),
            analyzer_resource: Some(&resource),
            profile: Some(&profile),
            sandbox: Some(&sandbox),
            credential: Some(&credential),
            disclosure: Some(&disclosure),
            snapshot: Some(&snapshot),
            freshness: Spec031Freshness::Current,
        },
    })?;
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: &[artifact],
        analyzer: &analyzer,
        disclosure: &disclosure,
    })?;
    let dependencies = DependencySpies::default();
    let receipt =
        replay_recorded_media_evidence(&serde_json::to_string(&diagnostics)?, &dependencies)?;
    let output = FixtureOutput {
        artifact_status: receipt.artifact_status.as_str(),
        analyzer_status: receipt.analyzer_status.as_str().to_owned(),
        raw_content_possible: receipt.disclosure.raw_content_possible,
        snapshot_ref: receipt.snapshot.snapshot_id,
        network_calls: dependencies.network.get(),
        credential_calls: dependencies.credential.get(),
        analyzer_calls: dependencies.analyzer.get(),
        resource_calls: dependencies.resource.get(),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn analyzer_resource(analyzer_ref: &str) -> ResourceCandidateProjection {
    ResourceCandidateProjection {
        resource_ref: analyzer_ref.to_owned(),
        kind: ResourceKind::Package,
        source: ResourceSource::Package,
        precedence: ResourcePrecedence::Package,
        canonical_path: "/private/not-projected/analyzer".to_owned(),
        content_sha256: Some("sha256:resource".to_owned()),
        collision: ResourceCollisionStatus::None,
        load_status: ResourceLoadStatus::Loaded,
        activation: ResourceActivation::Explicit,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        diagnostics: Vec::new(),
    }
}

fn analyzer_profile() -> TrustedRuntimeProfileProjection {
    TrustedRuntimeProfileProjection {
        availability: Spec030Availability::Available,
        status: TrustedProfileStatus::Active,
        profile: TrustedRuntimeProfile::TrustedLocalAgent,
        execution_authority: ExecutionAuthority::CurrentOsUser,
        workspace_trust: WorkspaceTrust::UserAsserted,
        workspace_trust_remediation: None,
        resource_trust: ResourceTrust::ExplicitOrTrustedWorkspace,
        default_containment: DefaultContainment::None,
        optional_sandbox: OptionalSandboxScope::AdapterScoped,
    }
}

fn analyzer_sandbox() -> SandboxStatusProjection {
    SandboxStatusProjection {
        availability: Spec030Availability::Unknown,
        status: SandboxStatus::Unknown,
        fallback: SandboxFallback::Unknown,
        applied_adapters: Vec::new(),
        filesystem_policy: SandboxFilesystemPolicy::Unknown,
        network_policy: SandboxNetworkPolicy::Unknown,
    }
}

fn analyzer_credential() -> CredentialStatusProjection {
    CredentialStatusProjection {
        availability: Spec030Availability::Degraded,
        status: CredentialStatus::Missing,
        source: Some(CredentialSource::Environment),
        fingerprint: CredentialFingerprintStatus::Unavailable,
        refresh_serialization: RefreshSerializationStatus::Inactive,
    }
}

fn execution_snapshot(analyzer_ref: &str) -> Result<ExecutionSnapshot, Box<dyn Error>> {
    Ok(ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: "snapshot:034:driver".to_owned(),
        created_at_unix_ms: 34,
        config: ConfigSnapshotRef {
            source_ref: "config:driver".to_owned(),
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
            profile_ref: "trusted-runtime:driver".to_owned(),
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
            surfaces: vec![DataSurface::Session, DataSurface::Log],
        },
        replay: ReplayContract::diagnostic_only(),
    })?)
}
