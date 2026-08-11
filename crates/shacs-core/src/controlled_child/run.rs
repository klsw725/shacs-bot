use super::output::{join_reader, spawn_reader};
use super::process_group;
use super::{
    descendant_cleanup_capability, empty_receipt, ControlledChildAbort, ControlledChildAdapter,
    ControlledChildCommand, ControlledChildError, ControlledChildOutcome, ControlledChildReceipt,
};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const WAIT_STEP: Duration = Duration::from_millis(10);

pub fn run_bash(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    run(command, abort, ControlledChildAdapter::Bash)
}

pub fn run_generic_argv(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    run(command, abort, ControlledChildAdapter::GenericArgv)
}

pub fn run_configured_credential_command(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    run(command, abort, ControlledChildAdapter::CredentialCommand)
}

pub fn run_configured_package_command(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    run(command, abort, ControlledChildAdapter::PackageCommand)
}

pub fn run_configured_load_check(
    command: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    run(command, abort, ControlledChildAdapter::LoadCheck)
}

fn run(
    spec: &ControlledChildCommand,
    abort: &ControlledChildAbort,
    adapter: ControlledChildAdapter,
) -> Result<ControlledChildReceipt, ControlledChildError> {
    if !spec.cwd.is_dir() {
        return Ok(empty_receipt(adapter, ControlledChildOutcome::InvalidCwd));
    }
    let (program, args) = spec
        .argv
        .split_first()
        .ok_or(ControlledChildError::EmptyArgv)?;
    let program = resolve_program(spec, program)?;
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !spec.inherit_env {
        command.env_clear();
        command.envs(&spec.env);
    }
    process_group::configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| ControlledChildError::Spawn(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or(ControlledChildError::MissingPipe)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(ControlledChildError::MissingPipe)?;
    let stdout_reader = spawn_reader(stdout, spec.output_limit);
    let stderr_reader = spawn_reader(stderr, spec.output_limit);

    let terminal = wait_for_terminal(&mut child, spec, abort)?;
    let (outcome, cleanup_attempted) = match terminal {
        Terminal::Exited(status) => {
            let attempted = cleanup_after_exit(&mut child, spec.termination_grace);
            (exit_outcome(status), attempted)
        }
        Terminal::TimedOut => (
            ControlledChildOutcome::TimedOut,
            process_group::cleanup(&mut child, spec.termination_grace),
        ),
        Terminal::Aborted => (
            ControlledChildOutcome::Aborted,
            process_group::cleanup(&mut child, spec.termination_grace),
        ),
    };
    Ok(ControlledChildReceipt {
        adapter,
        outcome,
        stdout: join_reader(stdout_reader)?,
        stderr: join_reader(stderr_reader)?,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        descendant_cleanup: descendant_cleanup_capability(),
        cleanup_attempted,
        abort_capable: abort.is_propagated(),
    })
}

fn resolve_program(
    spec: &ControlledChildCommand,
    program: &OsStr,
) -> Result<PathBuf, ControlledChildError> {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return Ok(program_path.to_path_buf());
    }
    let path = spec
        .env
        .get(OsStr::new("PATH"))
        .cloned()
        .or_else(|| spec.inherit_env.then(|| std::env::var_os("PATH")).flatten())
        .ok_or_else(|| missing_program(program))?;
    std::env::split_paths(&path)
        .map(|directory| resolve_path_directory(&spec.cwd, directory).join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| missing_program(program))
}

fn resolve_path_directory(cwd: &Path, directory: PathBuf) -> PathBuf {
    if directory.as_os_str().is_empty() {
        cwd.to_path_buf()
    } else if directory.is_absolute() {
        directory
    } else {
        cwd.join(directory)
    }
}

fn missing_program(program: &OsStr) -> ControlledChildError {
    ControlledChildError::Spawn(format!(
        "program not found in controlled child PATH: {}",
        OsString::from(program).to_string_lossy()
    ))
}

fn wait_for_terminal(
    child: &mut std::process::Child,
    spec: &ControlledChildCommand,
    abort: &ControlledChildAbort,
) -> Result<Terminal, ControlledChildError> {
    let deadline = Instant::now() + spec.timeout;
    loop {
        if abort.is_aborted() {
            return Ok(Terminal::Aborted);
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| ControlledChildError::Wait(error.to_string()))?
        {
            return Ok(Terminal::Exited(status));
        }
        if Instant::now() >= deadline {
            return Ok(Terminal::TimedOut);
        }
        thread::sleep(WAIT_STEP);
    }
}

fn cleanup_after_exit(child: &mut std::process::Child, grace: Duration) -> bool {
    if process_group::cleanup_capability_supported() {
        process_group::cleanup(child, grace)
    } else {
        false
    }
}

fn exit_outcome(status: ExitStatus) -> ControlledChildOutcome {
    if status.success() {
        ControlledChildOutcome::Succeeded {
            code: status.code(),
        }
    } else {
        ControlledChildOutcome::Failed {
            code: status.code(),
        }
    }
}

enum Terminal {
    Exited(ExitStatus),
    TimedOut,
    Aborted,
}
