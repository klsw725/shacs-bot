use shacs_core::runtime::{
    AdapterSandboxRef, ConfigMigrationState, ConfigSnapshotRef, ContextInclusion,
    ContextSourceSnapshot, CredentialSnapshotRef, DataDisclosureWarning, ExecutionSnapshotInput,
    ProfileSelectionSnapshot, ProviderInputSnapshot, ReplayContract, ResourceIdentitySnapshot,
    SandboxMode, TokenBudgetSnapshot, TrustedRuntimeFactRef,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, DataSurface,
    ProcessAdapterKind, SandboxFallback,
};

pub fn input(id: &str, created_at_unix_ms: u64) -> ExecutionSnapshotInput {
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
        selected_tools: Vec::new(),
        selected_resources: vec![ResourceIdentitySnapshot {
            identity: "resource:skill:formatter".to_owned(),
            content_digest: Some("sha256:content-a".to_owned()),
            activation_ref: Some("activation:skill:formatter:v1".to_owned()),
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
