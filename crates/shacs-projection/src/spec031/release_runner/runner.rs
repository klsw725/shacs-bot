use super::audit::add_external_audits;
use super::command::execute_spec031_release_command_with;
use super::coverage::Spec031CoverageStatus;
use super::coverage_ids::required_command_ids;
use super::coverage_matrix::coverage_entries;
use super::current_commands::required_worktree_commands;
use super::external_audit_facts::external_owner_facts;
use super::fixture::prepare_success_fixture_project;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandSpec, Spec031ReleaseGateKind,
    Spec031ReleaseRunArtifacts, Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode,
    SPEC031_RELEASE_RUNNER_SCHEMA,
};
use super::runner_outputs::{
    push_blocked_external_triage, push_cleanup, write_evidence_index, CleanupReceiptSpec,
};
use super::validate::validate_spec031_release_artifacts_with_repo_root;
use super::writer::{write_json, write_spec031_release_artifacts_with};
use super::REQUIRED_ARTIFACTS;
use crate::release_evidence::EvidenceWriter;
use std::path::Path;
use std::process::Command;

pub fn run_spec031_release_runner(
    config: &Spec031ReleaseRunnerConfig,
) -> Result<Spec031ReleaseRunArtifacts, Spec031ReleaseArtifactError> {
    let writer = EvidenceWriter::open_new_run(&config.evidence_root)
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("commands")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("cleanup")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("fixtures")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .create_dir_all("triage")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    let mut artifacts = empty_artifacts(config);
    match config.mode {
        Spec031ReleaseRunnerMode::SuccessFixture => {
            add_success_fixture(config, &writer, &mut artifacts)?;
        }
        Spec031ReleaseRunnerMode::CurrentWorktree => {
            add_current_worktree_triage(config, &writer, &mut artifacts)?;
        }
    }
    write_spec031_release_artifacts_with(&writer, &artifacts)?;
    validate_spec031_release_artifacts_with_repo_root(&artifacts, &config.repo_root)?;
    Ok(artifacts)
}

fn empty_artifacts(config: &Spec031ReleaseRunnerConfig) -> Spec031ReleaseRunArtifacts {
    Spec031ReleaseRunArtifacts {
        schema: SPEC031_RELEASE_RUNNER_SCHEMA.to_owned(),
        run_id: config.run_id.clone(),
        evidence_root: config.evidence_root.display().to_string(),
        fixture_registry: Vec::new(),
        command_registry: Vec::new(),
        cleanup_registry: Vec::new(),
        manifest_files: REQUIRED_ARTIFACTS
            .iter()
            .map(|file| (*file).to_owned())
            .collect(),
        coverage_matrix: Vec::new(),
        external_audits: Vec::new(),
        failure_triage: Vec::new(),
        reproducibility_observations: Vec::new(),
    }
}

fn add_success_fixture(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    artifacts
        .fixture_registry
        .push("fixtures/success-fixture/Cargo.toml".to_owned());
    let fixture_root = config.evidence_root.join("fixtures/success-fixture");
    prepare_success_fixture_project(writer)?;
    for &(_, command_id) in required_command_ids() {
        let command = Spec031ReleaseCommandSpec {
            id: command_id.to_owned(),
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: Some("shacs-projection".to_owned()),
            filter: Some("spec031_release_runner_success_fixture".to_owned()),
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            cwd: fixture_root.clone(),
            timeout: config.command_timeout,
        };
        let record = execute_spec031_release_command_with(writer, &command)?;
        artifacts.command_registry.push(record);
    }
    for command_id in external_owner_facts()
        .iter()
        .flat_map(|descriptor| descriptor.command_result_ids)
    {
        let command = Spec031ReleaseCommandSpec {
            id: (*command_id).to_owned(),
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: Some("shacs-projection".to_owned()),
            filter: Some("spec031_release_runner_success_fixture".to_owned()),
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            cwd: fixture_root.clone(),
            timeout: config.command_timeout,
        };
        let record = execute_spec031_release_command_with(writer, &command)?;
        artifacts.command_registry.push(record);
    }
    push_cleanup(
        config,
        writer,
        artifacts,
        CleanupReceiptSpec {
            file_name: "success-fixture-receipt.json",
            status: "cleaned",
            resource_id: "fixtures/success-fixture",
            check_artifact: "commands/spec031-test-release-runner.stdout",
        },
    )?;
    add_external_audits(config, writer, artifacts, true)?;
    write_evidence_index(config, writer, artifacts)?;
    artifacts.coverage_matrix = coverage_entries(
        &config.evidence_root,
        "results.json",
        Spec031CoverageStatus::Pass,
        &artifacts.command_registry,
        &artifacts.external_audits,
    )?;
    Ok(())
}

fn add_current_worktree_triage(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    artifacts
        .fixture_registry
        .push("fixtures/current-worktree.json".to_owned());
    write_json(
        writer,
        "fixtures/current-worktree.json",
        &serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": config.run_id.as_str(),
            "resource_id": "current-worktree"
        }),
    )?;
    if config.repo_root.join("crates/Cargo.toml").is_file() {
        for command in required_worktree_commands(config) {
            let record = execute_spec031_release_command_with(writer, &command)?;
            artifacts.command_registry.push(record);
        }
    }
    if worktree_dirty(&config.repo_root)? {
        super::runner_outputs::push_reproducibility_observation(config, writer, artifacts)?;
    }
    add_external_audits(config, writer, artifacts, false)?;
    if artifacts
        .external_audits
        .iter()
        .any(|audit| audit.status == super::coverage::Spec031ExternalAuditStatus::Blocked)
    {
        push_blocked_external_triage(config, writer, artifacts)?;
    }
    push_cleanup(
        config,
        writer,
        artifacts,
        CleanupReceiptSpec {
            file_name: "current-worktree-receipt.json",
            status: "verified",
            resource_id: "current-worktree",
            check_artifact: "commands/spec031-test-surface-smoke.stdout",
        },
    )?;
    write_evidence_index(config, writer, artifacts)?;
    artifacts.coverage_matrix = coverage_entries(
        &config.evidence_root,
        "failure-triage.json",
        Spec031CoverageStatus::Blocked,
        &artifacts.command_registry,
        &artifacts.external_audits,
    )?;
    Ok(())
}

fn worktree_dirty(repo_root: &Path) -> Result<bool, Spec031ReleaseArtifactError> {
    let output = Command::new("git")
        .arg("status")
        .arg("--porcelain=v1")
        .current_dir(repo_root)
        .output()
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    Ok(!output.stdout.is_empty())
}
