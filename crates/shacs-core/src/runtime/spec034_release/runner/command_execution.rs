use super::super::model::*;
#[cfg(not(test))]
use super::super::source::MaterializedSource;
#[cfg(not(test))]
use super::super::tools::ResolvedToolchain;
#[cfg(not(test))]
use shacs_projection::{
    parse_cargo_test_counts, Spec031ReleaseCommandStatus, Spec031ReleaseGateKind,
};
#[cfg(not(test))]
use std::fs::File;
#[cfg(not(test))]
use std::io::{Read, Seek};
#[cfg(not(test))]
use std::path::Path;
#[cfg(not(test))]
use std::time::Instant;

#[path = "command_execution/process_group.rs"]
mod process_group;
#[path = "command_execution/descendants.rs"]
mod descendants;
#[cfg(not(test))]
use process_group::terminate_process_group;

#[cfg(not(test))]
struct RunningCommand {
    child: super::super::tools::spawn::ExecutionChild,
    descendants: descendants::DescendantTracker,
    cleaned: bool,
}

#[cfg(not(test))]
impl RunningCommand {
    fn new(child: super::super::tools::spawn::ExecutionChild) -> Result<Self, Spec034ReleaseArtifactError> {
        let descendants = descendants::DescendantTracker::new(child.id())?;
        Ok(Self { child, descendants, cleaned: false })
    }

    fn cleanup(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        if self.cleaned {
            return Ok(());
        }
        terminate_process_group(&mut self.child, &mut self.descendants)?;
        self.cleaned = true;
        Ok(())
    }
}

#[cfg(not(test))]
impl Drop for RunningCommand {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(not(test))]
const MAX_STREAM_BYTES: u64 = 8 * 1024 * 1024;

#[cfg(not(test))]
pub fn run(
    config: &Spec034ReleaseConfig,
    output: &Path,
    execution: &MaterializedSource,
    source_digest: &str,
    toolchain: &ResolvedToolchain,
) -> Result<Vec<CommandEvidence>, Spec034ReleaseArtifactError> {
    super::command_specs::COMMAND_SPECS.iter()
    .map(|spec| run_one(config, output, execution, source_digest, toolchain, spec))
    .collect()
}

#[cfg(not(test))]
fn run_one(
    config: &Spec034ReleaseConfig,
    output: &Path,
    execution: &MaterializedSource,
    source_digest: &str,
    toolchain: &ResolvedToolchain,
    spec: &super::command_specs::CommandSpec,
) -> Result<CommandEvidence, Spec034ReleaseArtifactError> {
    run_one_inner(
        config,
        output,
        execution,
        source_digest,
        toolchain,
        spec,
    )
}

#[cfg(not(test))]
fn run_one_inner(
    config: &Spec034ReleaseConfig,
    output: &Path,
    execution: &MaterializedSource,
    source_digest: &str,
    toolchain: &ResolvedToolchain,
    spec: &super::command_specs::CommandSpec,
) -> Result<CommandEvidence, Spec034ReleaseArtifactError> {
    let argv = spec.argv();
    let mut stdout_file = tempfile::tempfile().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut stderr_file = tempfile::tempfile().map_err(Spec034ReleaseArtifactError::Io)?;
    let manifest = execution.path().join("crates/Cargo.toml");
    let mut command = toolchain.command(&manifest)?;
    command
        .args([
            "test",
            "--manifest-path",
            manifest
                .to_str()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
            "--locked",
            "-p",
            spec.package,
            "--test",
            spec.target,
        ]);
    execution.verify()?;
    toolchain.verify()?;
    let child = toolchain.spawn_cargo(&command, &stdout_file, &stderr_file)?;
    let mut running = RunningCommand::new(child)?;
    let deadline = Instant::now()
        .checked_add(config.command_timeout)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let primary = loop {
        if let Err(error) = toolchain.verify_execution_ledger() {
            break Err(error);
        }
        if let Err(error) = running.descendants.observe_verified(toolchain) {
            break Err(error);
        }
        match running.child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(Spec034ReleaseArtifactError::Io(error)),
        }
        if Instant::now() >= deadline {
            break Err(Spec034ReleaseArtifactError::CommandFailed);
        }
        std::thread::yield_now();
    };
    let cleanup = running.cleanup();
    let status = Spec034ReleaseArtifactError::combine(primary, cleanup)?;
    toolchain.verify_execution_ledger()?;
    toolchain.verify()?;
    execution.verify()?;
    let stdout = read_bounded(&mut stdout_file)?;
    let stderr = read_bounded(&mut stderr_file)?;
    let stdout_text = std::str::from_utf8(&stdout)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
    let tests = parse_cargo_test_counts(stdout_text)
        .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
    let id = format!("spec034-{}", spec.kind);
    let stdout_path = format!("{id}.stdout");
    let stderr_path = format!("{id}.stderr");
    let stdout_digest = super::generation::write_summary(output, &stdout_path, &stdout)?;
    let stderr_digest = super::generation::write_summary(output, &stderr_path, &stderr)?;
    Ok(CommandEvidence {
        kind: spec.kind.to_owned(),
        source_digest: source_digest.to_owned(),
        tool: toolchain.cargo_identity().clone(),
        rustc: toolchain.rustc_identity().clone(),
        environment_policy: "spec034.controlled-toolchain.v1".to_owned(),
        command: PortableCommandRecord {
            id,
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: Some(spec.package.to_owned()),
            filter: None,
            argv,
            cwd: ".".to_owned(),
            status: if status.success() {
                Spec031ReleaseCommandStatus::Passed
            } else {
                Spec031ReleaseCommandStatus::Failed
            },
            exit_code: status.code(),
            stdout_path,
            stderr_path,
            tests: Some(tests),
        },
        portable_process_receipt: PortableProcessReceipt {
            reaped: true,
            temp_paths_published: true,
        },
        stdout_digest,
        stderr_digest,
    })
}

#[cfg(not(test))]
fn read_bounded(file: &mut File) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    file.rewind().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut bytes = Vec::new();
    file.take(MAX_STREAM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if bytes.len() as u64 > MAX_STREAM_BYTES {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(bytes)
}
