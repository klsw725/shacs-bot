use super::*;

pub(in crate::runtime::spec033_release::release_runner) fn blocker_coverage(
    edges: &[Spec033EdgeCommandEvidence],
) -> Result<Vec<Spec033BlockerCoverageRow>, Spec033ReleaseArtifactError> {
    required()
        .iter()
        .map(|entry| {
            let edge = edges
                .iter()
                .find(|edge| edge.blocker == entry.blocker)
                .ok_or(Spec033ReleaseArtifactError::MissingGuarantee)?;
            Ok(Spec033BlockerCoverageRow {
                blocker: entry.blocker.to_owned(),
                code_path: entry.path.to_owned(),
                test_command: edge.command.argv.join(" "),
                artifact: edge.artifact.clone(),
                artifact_digest: edge.artifact_digest.clone(),
            })
        })
        .collect()
}

pub(in crate::runtime::spec033_release::release_runner) fn validate_blocker_coverage(
    rows: &[Spec033BlockerCoverageRow],
) -> Result<(), Spec033ReleaseArtifactError> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let expected = required();
    if rows.len() != expected.len() {
        return Err(Spec033ReleaseArtifactError::MissingGuarantee);
    }
    for entry in expected {
        let row = rows
            .iter()
            .find(|row| row.blocker == entry.blocker)
            .ok_or(Spec033ReleaseArtifactError::MissingGuarantee)?;
        if row.code_path != entry.path
            || !repo.join(entry.path).is_file()
            || row.test_command != entry.command().join(" ")
            || row.artifact.is_empty()
            || !super::valid_digest(&row.artifact_digest)
        {
            return Err(Spec033ReleaseArtifactError::MissingGuarantee);
        }
    }
    Ok(())
}

pub(in crate::runtime::spec033_release::release_runner) struct BlockerSpec {
    pub blocker: &'static str,
    pub path: &'static str,
    pub package: &'static str,
    pub target: &'static str,
    pub test_id: &'static str,
}

impl BlockerSpec {
    pub fn command(&self) -> Vec<String> {
        [
            "cargo",
            "test",
            "--manifest-path",
            "crates/Cargo.toml",
            "-p",
            self.package,
            "--test",
            self.target,
            self.test_id,
            "--",
            "--exact",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
}

pub(in crate::runtime::spec033_release::release_runner) const fn required() -> [BlockerSpec; 17] {
    [
        edge(
            "HookVeto",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_release_edges",
            "trusted_hook_veto_blocks_dispatch",
        ),
        edge(
            "HeadlessConfirmationDenied",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_production_local_gate",
            "production_gate_denies_headless_confirmation_without_durable_approval",
        ),
        edge(
            "MissingHookEvidence",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_automation_dispatch",
            "missing_hook_evidence_blocks_sensitive_work_without_reporting_queued",
        ),
        edge(
            "ProcessTimeout",
            "crates/shacs-core/src/controlled_child/run.rs",
            "shacs-core",
            "spec030_process_controlled_child",
            "spec030_process_timeout_returns_promptly",
        ),
        edge(
            "AbortCleanupIncomplete",
            "crates/shacs-core/src/runtime/app_supervisor.rs",
            "shacs-core",
            "spec032_app_supervisor",
            "uncertain_cleanup_remains_recovery_needed",
        ),
        edge(
            "SnapshotMissing",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_automation_rejection",
            "missing_execution_snapshot_blocks_dispatch",
        ),
        edge(
            "SandboxUnsupported",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_automation_rejection",
            "unsupported_required_sandbox_blocks_dispatch",
        ),
        edge(
            "SandboxFailed",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_release_edges",
            "sandbox_failure_blocks_dispatch",
        ),
        edge(
            "Credential",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_release_edges",
            "credential_failure_blocks_dispatch",
        ),
        edge(
            "SnapshotMismatch",
            "crates/shacs-core/src/runtime/automation_gates.rs",
            "shacs-core",
            "spec033_automation_dispatch",
            "relevant_current_fact_mutation_blocks_dispatch",
        ),
        edge(
            "SourceMutation",
            "crates/shacs-core/src/runtime/snapshot_replay.rs",
            "shacs-core",
            "spec033_release_edges",
            "recorded_source_mutation_blocks_replay",
        ),
        edge(
            "MissingRedactionEvidence",
            "crates/shacs-projection/src/spec033/artifacts.rs",
            "shacs-projection",
            "spec033_review_artifacts",
            "artifact_transform_rejects_missing_redaction_evidence",
        ),
        edge(
            "Duplicate",
            "crates/shacs-core/src/runtime/automation_lifecycle.rs",
            "shacs-core",
            "spec033_release_edges",
            "duplicate_automation_blocks_dispatch",
        ),
        edge(
            "Superseded",
            "crates/shacs-session/src/durable_work.rs",
            "shacs-session",
            "durable_work",
            "completed_dedupe_lineage_is_superseded_without_reexecution",
        ),
        edge(
            "Recursion",
            "crates/shacs-core/src/runtime/automation_lifecycle.rs",
            "shacs-core",
            "spec033_release_edges",
            "recursive_automation_blocks_dispatch",
        ),
        edge(
            "Delivery",
            "crates/shacs-core/src/runtime/automation_lifecycle.rs",
            "shacs-core",
            "spec033_automation_results",
            "job_and_delivery_results_are_independent_records",
        ),
        edge(
            "ReplayMismatch",
            "crates/shacs-core/src/runtime/replay.rs",
            "shacs-core",
            "runtime_loop",
            "replay_runner_separates_verdict_and_confidence_mismatch_severity",
        ),
    ]
}

const fn edge(
    blocker: &'static str,
    path: &'static str,
    package: &'static str,
    target: &'static str,
    test_id: &'static str,
) -> BlockerSpec {
    BlockerSpec {
        blocker,
        path,
        package,
        target,
        test_id,
    }
}
