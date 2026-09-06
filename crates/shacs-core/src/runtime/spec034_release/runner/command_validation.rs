use super::super::artifacts::ArtifactSnapshot;
use super::super::model::{
    CommandEvidence, CommandStreamSummary, Spec034ReleaseArtifactError,
};
use super::super::tools::RetiredToolchain;
use super::super::tools::ResolvedToolchain;
use shacs_projection::{Spec031ReleaseCommandStatus, Spec031ReleaseGateKind};

pub(super) fn validate(
    snapshot: &ArtifactSnapshot,
    commands: &[CommandEvidence],
    source_digest: &str,
    toolchain: &RetiredToolchain,
) -> Result<(), Spec034ReleaseArtifactError> {
    validate_identities(
        snapshot,
        commands,
        source_digest,
        toolchain.cargo_identity(),
        toolchain.rustc_identity(),
    )
}

pub(super) fn validate_resolved(
    snapshot: &ArtifactSnapshot,
    commands: &[CommandEvidence],
    source_digest: &str,
    toolchain: &ResolvedToolchain,
) -> Result<(), Spec034ReleaseArtifactError> {
    validate_identities(
        snapshot,
        commands,
        source_digest,
        toolchain.cargo_identity(),
        toolchain.rustc_identity(),
    )
}

fn validate_identities(
    snapshot: &ArtifactSnapshot,
    commands: &[CommandEvidence],
    source_digest: &str,
    cargo: &super::super::model::PortableToolIdentity,
    rustc: &super::super::model::PortableToolIdentity,
) -> Result<(), Spec034ReleaseArtifactError> {
    for (evidence, spec) in commands.iter().zip(super::command_specs::COMMAND_SPECS.iter()) {
        validate_one(snapshot, evidence, source_digest, cargo, rustc, spec)?;
    }
    Ok(())
}

fn validate_one(
    snapshot: &ArtifactSnapshot,
    evidence: &CommandEvidence,
    source_digest: &str,
    cargo: &super::super::model::PortableToolIdentity,
    rustc: &super::super::model::PortableToolIdentity,
    spec: &super::command_specs::CommandSpec,
) -> Result<(), Spec034ReleaseArtifactError> {
    let command = &evidence.command;
    let id = format!("spec034-{}", spec.kind);
    let stdout_locator = format!("{id}.stdout");
    let stderr_locator = format!("{id}.stderr");
    let argv = spec.argv();
    let stdout: CommandStreamSummary = snapshot.json(&command.stdout_path)?;
    let stderr: CommandStreamSummary = snapshot.json(&command.stderr_path)?;
    let tests = command
        .tests
        .as_ref()
        .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
    let receipt = &evidence.portable_process_receipt;
    let valid = evidence.kind == spec.kind
        && evidence.source_digest == source_digest
        && &evidence.tool == cargo
        && &evidence.rustc == rustc
        && evidence.environment_policy == "spec034.controlled-toolchain.v1"
        && command.id == id
        && command.gate == Spec031ReleaseGateKind::FocusedCargoTest
        && command.package.as_deref() == Some(spec.package)
        && command.filter.is_none()
        && command.argv == argv
        && command.cwd == "."
        && command.status == Spec031ReleaseCommandStatus::Passed
        && command.exit_code == Some(0)
        && command.stdout_path == stdout_locator
        && command.stderr_path == stderr_locator
        && tests.tests_run == spec.tests_run
        && tests.tests_failed == 0
        && receipt.reaped
        && receipt.temp_paths_published
        && valid_summary(&stdout)
        && valid_summary(&stderr)
        && snapshot.digest(&command.stdout_path)? == evidence.stdout_digest
        && snapshot.digest(&command.stderr_path)? == evidence.stderr_digest;
    valid
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)
}

fn valid_summary(summary: &CommandStreamSummary) -> bool {
    summary.schema == "spec034.command_stream_summary.v1"
        && summary.byte_count <= 8 * 1024 * 1024
        && summary.digest.len() == 71
        && summary.digest.starts_with("sha256:")
        && summary.digest[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}
