use serde_json::json;
use shacs_core::runtime::{
    AdapterSandboxRef, ConfigMigrationState, ConfigSnapshotRef, CredentialSnapshotRef,
    CurrentGateEvidence, CurrentSpec030Receipts, DataDisclosureWarning, ExecutionSnapshot,
    ExecutionSnapshotInput, LocalGateSource, LocalImprovementBlock, LocalImprovementProposal,
    LocalImprovementVerifier, ProfileSelectionSnapshot, ProviderInputSnapshot, ReplayContract,
    SandboxMode, TokenBudgetSnapshot, TrustedRuntimeFactRef, VerificationEvidence,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialStatus, ProcessAdapterKind, SandboxFallback,
};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct Gates {
    pub current: AtomicBool,
    pub complete: AtomicBool,
    pub calls: AtomicUsize,
}

impl Gates {
    pub fn new() -> Self {
        Self {
            current: AtomicBool::new(true),
            complete: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        }
    }
}

impl Default for Gates {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalGateSource for Gates {
    fn current_receipts(
        &self,
        proposal: &LocalImprovementProposal,
        target_digest: &str,
    ) -> Result<CurrentSpec030Receipts, LocalImprovementBlock> {
        let generation = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if !self.current.load(Ordering::SeqCst) {
            return Err(LocalImprovementBlock::StaleGateEvidence);
        }
        let evidence = |id: &str| {
            CurrentGateEvidence::new(
                &format!("{id}:{generation}"),
                &proposal.snapshot().provenance_digest,
                target_digest,
            )
        };
        let credential = self
            .complete
            .load(Ordering::SeqCst)
            .then(|| evidence("credential:1"));
        CurrentSpec030Receipts::try_new(
            evidence("hook:1"),
            evidence("confirmation:1"),
            evidence("process:1"),
            evidence("sandbox:1"),
            credential,
        )
    }
}

pub struct Verifier {
    pub passes: AtomicBool,
}

impl LocalImprovementVerifier for Verifier {
    fn verify(
        &self,
        _proposal: &LocalImprovementProposal,
        _current_target: &[u8],
    ) -> VerificationEvidence {
        VerificationEvidence::new(self.passes.load(Ordering::SeqCst), "verify:1")
    }
}

pub fn write_snapshot(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: "snapshot:local:1".to_owned(),
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
    })?;
    fs::write(path, serde_json::to_vec_pretty(&snapshot)?)?;
    Ok(())
}

pub fn proposal(
    id: &str,
    expected: &str,
    snapshot_path: &Path,
) -> Result<LocalImprovementProposal, LocalImprovementBlock> {
    LocalImprovementProposal::from_json_artifacts(
        id,
        "settings.json",
        expected,
        &json!({"enabled": true}).to_string(),
        &fs::read_to_string(snapshot_path).map_err(|_| LocalImprovementBlock::Io)?,
    )
}
