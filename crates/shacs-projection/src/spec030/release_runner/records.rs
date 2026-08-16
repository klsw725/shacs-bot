use super::cleanup::cleanup_receipt;
use super::model::*;
use super::writer::write_json;
use super::{
    parse_spec030_manual_qa, Spec030ManualCommand, Spec030ManualCommandStatus,
    Spec030ManualQaRecord, SPEC030_MANUAL_QA_SCHEMA,
};
use crate::release_evidence::EvidenceWriter;
use std::path::Path;
use std::process::Command;

pub(super) fn add_environment_blockers(
    config: &Spec030ReleaseRunnerConfig,
    blockers: &mut Vec<Spec030ReleaseBlocker>,
) -> Result<(), Spec030ReleaseArtifactError> {
    if config.mode == Spec030ReleaseRunnerMode::CurrentWorktree {
        let output = Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(&config.repo_root)
            .output()
            .map_err(|_| Spec030ReleaseArtifactError::Io)?;
        if !output.status.success() || !output.stdout.is_empty() {
            push_blocker(
                blockers,
                "dirty_worktree",
                "checkout has uncommitted changes",
            );
        }
        if config.manual_records.is_empty() {
            push_blocker(
                blockers,
                "missing_manual_records",
                "surface and disclosure QA records are absent",
            );
        }
    }
    Ok(())
}

pub(super) fn add_records(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    let extra_removed = match config.mode {
        Spec030ReleaseRunnerMode::SuccessFixture => {
            remove_fixture_target(&config.evidence_root)?;
            1
        }
        Spec030ReleaseRunnerMode::CurrentWorktree => 0,
    };
    let cleanup = "cleanup/run.json";
    write_json(
        writer,
        cleanup,
        &cleanup_receipt(&artifacts.commands, &config.evidence_root, extra_removed)?,
    )?;
    artifacts.cleanup_records.push(cleanup.to_owned());
    match config.mode {
        Spec030ReleaseRunnerMode::SuccessFixture => add_fixture_manual(writer, artifacts),
        Spec030ReleaseRunnerMode::CurrentWorktree => add_manual_records(config, writer, artifacts),
    }
}

fn remove_fixture_target(evidence_root: &Path) -> Result<(), Spec030ReleaseArtifactError> {
    match std::fs::remove_dir_all(evidence_root.join("fixtures/success/target")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Spec030ReleaseArtifactError::Io),
    }
}

pub(super) fn push_blocker(blockers: &mut Vec<Spec030ReleaseBlocker>, code: &str, detail: &str) {
    blockers.push(Spec030ReleaseBlocker {
        code: code.to_owned(),
        detail: detail.to_owned(),
    });
}

fn add_fixture_manual(
    writer: &EvidenceWriter,
    artifacts: &mut Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    let manual = "manual/success-fixture.json";
    write_json(
        writer,
        manual,
        &Spec030ManualQaRecord {
            schema: SPEC030_MANUAL_QA_SCHEMA.to_owned(),
            source_digest: artifacts.source_manifest.source_digest.clone(),
            observed_commands: required_manual_commands(),
            non_guarantees: required_non_guarantees(),
        },
    )?;
    artifacts.manual_records.push(manual.to_owned());
    Ok(())
}

fn add_manual_records(
    config: &Spec030ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    for (index, path) in config.manual_records.iter().enumerate() {
        let record = parse_spec030_manual_qa(path, &artifacts.source_manifest.source_digest)?;
        let artifact = format!("manual/record-{index}.json");
        write_json(writer, &artifact, &record)?;
        artifacts.manual_records.push(artifact);
    }
    Ok(())
}

fn required_manual_commands() -> Vec<Spec030ManualCommand> {
    [
        "cli-json",
        "cli-human",
        "tui-no-session",
        "api-schema-1",
        "api-schema-2",
    ]
    .into_iter()
    .map(|id| Spec030ManualCommand {
        id: id.to_owned(),
        status: Spec030ManualCommandStatus::Passed,
    })
    .collect()
}

fn required_non_guarantees() -> Vec<String> {
    [
        "current_os_user_authority",
        "not_kernel_isolation",
        "optional_adapter_scoped_sandbox",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::remove_fixture_target;

    #[test]
    fn fixture_cleanup_succeeds_when_external_cargo_target_leaves_local_target_absent() {
        // Given
        let evidence_root = tempfile::tempdir().expect("temporary evidence root");

        // When
        let result = remove_fixture_target(evidence_root.path());

        // Then
        assert_eq!(result, Ok(()));
    }
}
