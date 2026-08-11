use super::bwrap_provenance::validate_spec030_bwrap_record;
use super::catalog::{audits, coverage, facts};
use super::command_contract::{
    lifecycle_record_matches, validate_exact_ids, CommandEvidenceMode, LifecycleCwdRoots,
};
use super::disk_validate::validate_disk;
use super::model::*;
use super::target_catalog::spec030_integration_targets;
use super::{parse_spec030_manual_qa, validate_spec030_cleanup_receipt};
use crate::Spec031ReleaseCommandStatus;
use std::path::Path;

pub fn validate_spec030_release_artifacts(
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    validate_with_command_evidence_mode(artifacts, artifacts.command_evidence_mode)
}

pub(super) fn validate_with_command_evidence_mode(
    artifacts: &Spec030ReleaseRunArtifacts,
    command_evidence_mode: CommandEvidenceMode,
) -> Result<(), Spec030ReleaseArtifactError> {
    if artifacts.schema != SPEC030_RELEASE_RUNNER_SCHEMA {
        return Err(Spec030ReleaseArtifactError::UnsupportedSchema);
    }
    validate_commands(artifacts, command_evidence_mode)?;
    validate_coverage(artifacts, command_evidence_mode)?;
    validate_audits(artifacts)?;
    validate_claims(artifacts)?;
    super::surface_owner_evidence::validate(
        &artifacts.surface_owner,
        artifacts.mode,
        Path::new(&artifacts.evidence_root),
        Path::new(&artifacts.repo_root),
    )?;
    validate_external_evidence(artifacts)?;
    if artifacts.cleanup_records.is_empty() {
        return Err(Spec030ReleaseArtifactError::MissingCleanupRecord);
    }
    if artifacts.manual_records.is_empty() {
        return Err(Spec030ReleaseArtifactError::MissingManualRecord);
    }
    validate_records(artifacts)?;
    for required in [
        "manifest.json",
        "coverage-matrix.json",
        "owner-audits.json",
        "facts.json",
        "surfaces.json",
        "surface-owner.json",
        "surface-assertions.json",
        "external-evidence.json",
        "results.json",
        "failure-triage.json",
        "summary.md",
        "source-manifest.json",
        "artifact-manifest.json",
    ] {
        if !Path::new(&artifacts.evidence_root).join(required).is_file() {
            return Err(Spec030ReleaseArtifactError::InvalidArtifactPath);
        }
    }
    scan_tree(Path::new(&artifacts.evidence_root))?;
    validate_disk(artifacts)
}

fn validate_records(
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    let root = Path::new(&artifacts.evidence_root);
    let extra_removed = u64::from(artifacts.mode == Spec030ReleaseRunnerMode::SuccessFixture);
    let expected_cleanup =
        super::cleanup::cleanup_receipt(&artifacts.commands, root, extra_removed)?;
    for relative in &artifacts.cleanup_records {
        let receipt = validate_spec030_cleanup_receipt(&root.join(relative))?;
        if receipt != expected_cleanup {
            return Err(Spec030ReleaseArtifactError::InvalidCleanupRecord);
        }
    }
    for relative in &artifacts.manual_records {
        parse_spec030_manual_qa(
            &root.join(relative),
            &artifacts.source_manifest.source_digest,
        )?;
    }
    let assertions = &artifacts.surface_assertions;
    if assertions.schema_version != 1
        || !assertions.cli_api_json_parity
        || !assertions.cli_human_tui_runtime_parity
        || !assertions.tui_no_session
        || !assertions.tui_runtime_owner_facts
        || assertions.api_schema1_status != 200
        || assertions.api_schema2_status != 400
    {
        return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    }
    Ok(())
}

fn validate_external_evidence(
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    match artifacts.mode {
        Spec030ReleaseRunnerMode::SuccessFixture => {
            if !artifacts.external_evidence.is_empty() {
                return Err(Spec030ReleaseArtifactError::InvalidCoverageEvidence);
            }
        }
        Spec030ReleaseRunnerMode::CurrentWorktree => {
            let [evidence] = artifacts.external_evidence.as_slice() else {
                return Err(Spec030ReleaseArtifactError::InvalidCoverageEvidence);
            };
            if evidence.kind != "linux_bwrap_active_lane"
                || evidence.artifact != "external/bwrap-linux-record.json"
            {
                return Err(Spec030ReleaseArtifactError::InvalidCoverageEvidence);
            }
            let record = validate_spec030_bwrap_record(
                &Path::new(&artifacts.evidence_root).join(&evidence.artifact),
            )
            .map_err(|_| Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
            let artifact_hash = record
                .artifact_hash()
                .map_err(|_| Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
            if evidence.artifact_hash != artifact_hash
                || record.source_digest != artifacts.source_manifest.source_digest
            {
                return Err(Spec030ReleaseArtifactError::InvalidCoverageEvidence);
            }
            let command = artifacts
                .commands
                .iter()
                .find(|command| command.id == "spec030-bwrap-active")
                .ok_or(Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
            let receipt = command
                .process_receipt
                .as_ref()
                .ok_or(Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
            let stdout =
                std::fs::read(Path::new(&artifacts.evidence_root).join(&command.stdout_path))
                    .map_err(|_| Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
            let stderr =
                std::fs::read(Path::new(&artifacts.evidence_root).join(&command.stderr_path))
                    .map_err(|_| Spec030ReleaseArtifactError::InvalidCoverageEvidence)?;
            if record.producer.command_id != command.id
                || record.producer.pid != receipt.pid
                || record.producer.argv != command.argv
                || record.producer.exit_code != command.exit_code.unwrap_or(-1)
                || record.producer.stdout_sha256 != super::source_manifest::sha256_bytes(&stdout)
                || record.producer.stderr_sha256 != super::source_manifest::sha256_bytes(&stderr)
                || record.producer.stdout_temp_path != receipt.stdout_temp_path
                || record.producer.stderr_temp_path != receipt.stderr_temp_path
            {
                return Err(Spec030ReleaseArtifactError::InvalidCoverageEvidence);
            }
        }
    }
    Ok(())
}

fn validate_coverage(
    artifacts: &Spec030ReleaseRunArtifacts,
    evidence_mode: CommandEvidenceMode,
) -> Result<(), Spec030ReleaseArtifactError> {
    if artifacts.coverage
        != coverage(
            &artifacts.commands,
            &artifacts.surface_assertions,
            evidence_mode,
        )
        || artifacts.coverage.iter().any(|row| !row.passed)
    {
        return Err(Spec030ReleaseArtifactError::MissingCoverageRow);
    }
    Ok(())
}

fn validate_audits(
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    if artifacts.owner_audits != audits(&artifacts.commands, &artifacts.surface_assertions)
        || artifacts.owner_audits.iter().any(|audit| !audit.passed)
    {
        return Err(Spec030ReleaseArtifactError::InvalidOwnerAudit);
    }
    Ok(())
}

fn validate_commands(
    artifacts: &Spec030ReleaseRunArtifacts,
    evidence_mode: CommandEvidenceMode,
) -> Result<(), Spec030ReleaseArtifactError> {
    if !validate_exact_ids(
        evidence_mode,
        artifacts.commands.iter().map(|command| command.id.as_str()),
    ) {
        return Err(Spec030ReleaseArtifactError::CommandFailed);
    }
    if evidence_mode == CommandEvidenceMode::LinuxCurrentWorktree {
        let lifecycle = artifacts
            .commands
            .iter()
            .find(|command| command.id == super::command_contract::OWNER_LIFECYCLE_ID)
            .ok_or(Spec030ReleaseArtifactError::CommandFailed)?;
        if !lifecycle_record_matches(
            lifecycle,
            LifecycleCwdRoots {
                runner_mode: artifacts.mode,
                evidence_root: Path::new(&artifacts.evidence_root),
                repo_root: Path::new(&artifacts.repo_root),
            },
        ) {
            return Err(Spec030ReleaseArtifactError::CommandFailed);
        }
    }
    if artifacts.mode == Spec030ReleaseRunnerMode::CurrentWorktree {
        let api = artifacts
            .commands
            .iter()
            .find(|command| command.id == "surface-api-schema")
            .ok_or(Spec030ReleaseArtifactError::InvalidSurfaceEvidence)?;
        let address = format!("127.0.0.1:{}", artifacts.surface_owner.bound_port);
        if api.argv.iter().any(|argument| argument == "--projection")
            || !api
                .argv
                .windows(2)
                .any(|pair| pair[0] == "--address" && pair[1] == address)
        {
            return Err(Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
        }
    }
    for target in spec030_integration_targets() {
        let command = artifacts
            .commands
            .iter()
            .find(|command| command.id == target.command_id)
            .ok_or(Spec030ReleaseArtifactError::CommandFailed)?;
        let exact_target = command
            .argv
            .windows(2)
            .any(|pair| pair == ["--test", target.target]);
        if command.package.as_deref() != Some(target.package)
            || command.filter.as_deref() != Some(target.target)
            || !exact_target
        {
            return Err(Spec030ReleaseArtifactError::CommandFailed);
        }
    }
    for command in &artifacts.commands {
        if command.status != Spec031ReleaseCommandStatus::Passed {
            return Err(Spec030ReleaseArtifactError::CommandFailed);
        }
        let receipt = command
            .process_receipt
            .as_ref()
            .ok_or(Spec030ReleaseArtifactError::InvalidCleanupRecord)?;
        if receipt.pid == 0
            || !receipt.reaped
            || !receipt.temp_paths_published
            || super::cleanup::process_is_live(receipt.pid)
        {
            return Err(Spec030ReleaseArtifactError::InvalidCleanupRecord);
        }
        let root = Path::new(&artifacts.evidence_root);
        for (path, suffix) in [
            (&receipt.stdout_temp_path, "stdout"),
            (&receipt.stderr_temp_path, "stderr"),
        ] {
            if super::cleanup::command_temp_path(root, &command.id, path, suffix)?.exists() {
                return Err(Spec030ReleaseArtifactError::InvalidCleanupRecord);
            }
        }
        if command.gate == crate::Spec031ReleaseGateKind::FocusedCargoTest
            || command.id == "cargo-test-workspace"
        {
            let tests = command
                .tests
                .as_ref()
                .ok_or(Spec030ReleaseArtifactError::ZeroTestsRun)?;
            if tests.tests_run == 0 {
                return Err(Spec030ReleaseArtifactError::ZeroTestsRun);
            }
            if tests.tests_failed != 0 {
                return Err(Spec030ReleaseArtifactError::CommandFailed);
            }
        }
    }
    Ok(())
}

fn validate_claims(
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    if artifacts.facts != facts(&artifacts.surface_assertions)
        || artifacts.facts.iter().any(|fact| fact.evidence.is_empty())
    {
        return Err(Spec030ReleaseArtifactError::FalseSupportedClaim);
    }
    Ok(())
}

fn scan_tree(root: &Path) -> Result<(), Spec030ReleaseArtifactError> {
    let metadata = std::fs::symlink_metadata(root)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidArtifactPath)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Spec030ReleaseArtifactError::InvalidArtifactPath);
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).map_err(|_| Spec030ReleaseArtifactError::Io)? {
            let entry = entry.map_err(|_| Spec030ReleaseArtifactError::Io)?;
            let metadata = entry
                .file_type()
                .map_err(|_| Spec030ReleaseArtifactError::Io)?;
            if metadata.is_symlink() {
                return Err(Spec030ReleaseArtifactError::InvalidArtifactPath);
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                let bytes =
                    std::fs::read(entry.path()).map_err(|_| Spec030ReleaseArtifactError::Io)?;
                if raw_material(&bytes) {
                    return Err(Spec030ReleaseArtifactError::RawCredentialMaterial);
                }
            }
        }
    }
    Ok(())
}

fn raw_material(bytes: &[u8]) -> bool {
    [
        b"SPEC030_RAW_CREDENTIAL_CANARY".as_slice(),
        b"Authorization: Bearer ".as_slice(),
        b"\"refresh_token\":\"".as_slice(),
        b"\"access_token\":\"".as_slice(),
    ]
    .iter()
    .any(|canary| bytes.windows(canary.len()).any(|window| window == *canary))
}
