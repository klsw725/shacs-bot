use shacs_core::runtime::{
    ConfigMigrationState, ConfigSnapshotRef, ContextInclusion, ContextSourceSnapshot,
    CredentialSnapshotRef, DataDisclosureWarning, ExecutionSnapshot, ExecutionSnapshotInput,
    ProfileSelectionSnapshot, ProviderInputSnapshot, RecordedBoundaryRequirement,
    RecordedSourceArtifactInput, RecordedTrajectoryInput, RecordedTrajectoryOrigin,
    RecordedTrajectoryRecord, RecordedTrajectoryStore, ReplayContract, TokenBudgetSnapshot,
    TrustedRuntimeFactRef,
};
use shacs_eval::evaluator::{
    ConfidenceBand, ProjectionStatus, ReplayDatasetItem, TaskOutcomeClass, VerdictKind,
};
use shacs_projection::{CredentialFingerprintStatus, CredentialStatus};
use std::error::Error;

pub fn write_trajectory(
    store: &RecordedTrajectoryStore,
    input: RecordedTrajectoryInput,
) -> Result<RecordedTrajectoryRecord, Box<dyn Error>> {
    Ok(store.write(input)?)
}

pub fn recorded_trajectory() -> RecordedTrajectoryInput {
    RecordedTrajectoryInput {
        trajectory_id: "trajectory-004".to_owned(),
        snapshot: snapshot().expect("valid fixture snapshot"),
        sources: vec![RecordedSourceArtifactInput {
            source_id: "context-system".to_owned(),
            bytes: b"recorded source".to_vec(),
        }],
        owner_outcome: ReplayDatasetItem {
            dataset_id: "dataset-004".to_owned(),
            case_id: "case-004".to_owned(),
            trajectory_refs: Vec::new(),
            expected_verdict: VerdictKind::Pass,
            expected_outcome: TaskOutcomeClass::Notify,
            expected_projection_status: ProjectionStatus::Success,
            expected_confidence_band: ConfidenceBand::High,
            allowed_judge_roles: Vec::new(),
            redaction_profile: "shacs-redaction-v1".to_owned(),
            tool_outcome_policies: Vec::new(),
            actual_verdict: Some(VerdictKind::Pass),
            actual_outcome: Some(TaskOutcomeClass::Notify),
            actual_projection_status: Some(ProjectionStatus::Success),
            actual_confidence_band: Some(ConfidenceBand::High),
            auxiliary_judge_routes: Vec::new(),
            diagnostics_refs: Vec::new(),
            coverage_refs: Vec::new(),
        },
        boundary_requirement: RecordedBoundaryRequirement::RecordedOnly,
        origin: RecordedTrajectoryOrigin::Fixture,
    }
}

fn snapshot() -> Result<ExecutionSnapshot, Box<dyn Error>> {
    Ok(ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: "execution:004".to_owned(),
        created_at_unix_ms: 33_004,
        config: ConfigSnapshotRef {
            source_ref: "config:workspace".to_owned(),
            schema_version: 1,
            migration_state: ConfigMigrationState::Current,
        },
        profiles: ProfileSelectionSnapshot {
            provider: None,
            trusted_runtime: Some("runtime:local".to_owned()),
            context: None,
        },
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: 1,
            profile_ref: "trusted:local".to_owned(),
            projection_digest: "sha256:trusted".to_owned(),
        },
        sandbox: Vec::new(),
        credential: CredentialSnapshotRef {
            source_kind: None,
            status: CredentialStatus::Resolved,
            fingerprint_status: CredentialFingerprintStatus::Current,
        },
        context_sources: vec![ContextSourceSnapshot {
            source_ref: "context-system".to_owned(),
            content_digest: digest(b"recorded source"),
            inclusion: ContextInclusion::Included,
            original_bytes: 15,
            included_bytes: 15,
            precedence: shacs_core::runtime::ContextArtifactPriority::ExplicitInline,
            decision: shacs_core::runtime::ContextBudgetDecision::Included,
            estimated_tokens: 4,
            included_tokens: 4,
            reason: None,
        }],
        selected_tools: Vec::new(),
        selected_resources: Vec::new(),
        provider: ProviderInputSnapshot {
            provider: "openai".to_owned(),
            model: "gpt-test".to_owned(),
            shaping_version: "v1".to_owned(),
            messages_digest: "sha256:messages".to_owned(),
            tools_digest: "sha256:tools".to_owned(),
        },
        token_budget: TokenBudgetSnapshot {
            tokenizer: "estimated:chars".to_owned(),
            estimator_uncertainty_percent: 25,
            budget_tokens: 4096,
            reserved_tokens: 0,
            used_context_tokens: 0,
            estimated_input_tokens: 0,
        },
        disclosure: DataDisclosureWarning {
            raw_content_possible: false,
            surfaces: Vec::new(),
        },
        replay: ReplayContract::diagnostic_only(),
    })?)
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
