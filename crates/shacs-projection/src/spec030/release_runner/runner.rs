use super::catalog::{audits, coverage, facts, surfaces};
use super::command_contract::CommandEvidenceMode;
use super::command_runner::collect;
use super::model::*;
use super::records::{add_environment_blockers, add_records, push_blocker};
use super::source_manifest::build_spec030_source_manifest;
use super::writer::write_artifacts;
use crate::release_evidence::EvidenceWriter;
use crate::Spec031ReleaseCommandStatus;

pub fn run_spec030_release_runner(
    config: &Spec030ReleaseRunnerConfig,
) -> Result<Spec030ReleaseRunArtifacts, Spec030ReleaseArtifactError> {
    run_with_command_evidence_mode(config, CommandEvidenceMode::for_runner(config.mode))
}

pub(super) fn run_with_command_evidence_mode(
    config: &Spec030ReleaseRunnerConfig,
    command_evidence_mode: CommandEvidenceMode,
) -> Result<Spec030ReleaseRunArtifacts, Spec030ReleaseArtifactError> {
    let writer = EvidenceWriter::open_new_run(&config.evidence_root)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    for directory in ["commands", "cleanup", "manual", "fixtures"] {
        writer
            .create_dir_all(directory)
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    }
    let source_manifest = build_spec030_source_manifest(&config.repo_root)
        .map_err(|_| Spec030ReleaseArtifactError::SourceMismatch)?;
    let mut blockers = Vec::new();
    let collected = collect(
        config,
        &source_manifest.source_digest,
        &writer,
        &mut blockers,
        command_evidence_mode,
    )?;
    let commands = collected.commands;
    add_command_blockers(&commands, &mut blockers);
    add_environment_blockers(config, &mut blockers)?;
    let passed = blockers.is_empty();
    let mut artifacts = Spec030ReleaseRunArtifacts {
        schema: SPEC030_RELEASE_RUNNER_SCHEMA.to_owned(),
        run_id: config.run_id.clone(),
        evidence_root: config.evidence_root.display().to_string(),
        repo_root: config.repo_root.display().to_string(),
        mode: config.mode,
        command_evidence_mode,
        source_manifest,
        verdict: if passed {
            Spec030ReleaseVerdict::Pass
        } else {
            Spec030ReleaseVerdict::Blocked
        },
        coverage: coverage(
            &commands,
            &collected.surface_assertions,
            command_evidence_mode,
        ),
        owner_audits: audits(&commands, &collected.surface_assertions),
        facts: facts(&collected.surface_assertions),
        surfaces: surfaces(),
        surface_owner: collected.surface_owner,
        surface_assertions: collected.surface_assertions,
        external_evidence: collected.external_evidence,
        commands,
        cleanup_records: Vec::new(),
        manual_records: Vec::new(),
        blockers,
    };
    add_records(config, &writer, &mut artifacts)?;
    write_artifacts(&writer, &artifacts)?;
    if artifacts.verdict == Spec030ReleaseVerdict::Pass {
        super::validate::validate_with_command_evidence_mode(&artifacts, command_evidence_mode)?;
    }
    Ok(artifacts)
}

fn add_command_blockers(
    commands: &[crate::Spec031ReleaseCommandRecord],
    blockers: &mut Vec<Spec030ReleaseBlocker>,
) {
    for command in commands {
        if command.status != Spec031ReleaseCommandStatus::Passed {
            push_blocker(blockers, "command_failed", &command.id);
        } else if (command.gate == crate::Spec031ReleaseGateKind::FocusedCargoTest
            || command.id == "cargo-test-workspace")
            && command
                .tests
                .as_ref()
                .map_or(true, |tests| tests.tests_run == 0)
        {
            push_blocker(blockers, "zero_tests", &command.id);
        }
    }
}
