use shacs_core::controlled_child::{descendant_cleanup_capability, DescendantCleanupCapability};
#[cfg(unix)]
use shacs_core::controlled_child::{
    run_bash, run_configured_credential_command, run_configured_load_check,
    run_configured_package_command, run_generic_argv, ControlledChildAbort, ControlledChildAdapter,
    ControlledChildCommand, ControlledChildError, ControlledChildOutcome, ControlledChildReceipt,
};
#[cfg(unix)]
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
#[cfg(unix)]
use shacs_projection::{ProcessAdapterKind, ProcessTerminalOutcome, Spec030ProjectionProvider};
#[cfg(unix)]
use std::error::Error;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
#[path = "spec030_process_controlled_child/support.rs"]
mod support;
#[cfg(unix)]
use support::{wait_for_path, wait_for_process_exit};

#[cfg(unix)]
fn shell_command(cwd: &Path, script: &str) -> ControlledChildCommand {
    let mut command =
        ControlledChildCommand::new(["/bin/sh", "-c", script], cwd, Duration::from_secs(5));
    command.output_limit = 4_096;
    command.termination_grace = Duration::from_millis(100);
    command
}

#[test]
#[cfg(unix)]
fn spec030_process_generic_argv_applies_cwd_env_and_reports_success() -> Result<(), Box<dyn Error>>
{
    let temp = tempfile::tempdir()?;
    let mut command = shell_command(temp.path(), "printf '%s|%s' \"$SHACS_TEST\" \"$PWD\"");
    command.env.insert("SHACS_TEST".into(), "configured".into());

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;

    assert_eq!(receipt.adapter, ControlledChildAdapter::GenericArgv);
    assert_eq!(
        receipt.outcome,
        ControlledChildOutcome::Succeeded { code: Some(0) }
    );
    let stdout = String::from_utf8(receipt.stdout.captured)?;
    assert_eq!(
        stdout,
        format!(
            "configured|{}",
            std::fs::canonicalize(temp.path())?.display()
        )
    );
    assert_eq!(
        receipt.descendant_cleanup,
        DescendantCleanupCapability::Supported
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn spec030_process_drains_large_bounded_stdout_and_stderr_without_deadlock(
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let command = shell_command(
        temp.path(),
        "i=0; while [ $i -lt 20000 ]; do printf x; printf y >&2; i=$((i+1)); done",
    );

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;

    assert_eq!(
        receipt.outcome,
        ControlledChildOutcome::Succeeded { code: Some(0) }
    );
    assert_eq!(receipt.stdout.captured.len(), command.output_limit);
    assert_eq!(receipt.stderr.captured.len(), command.output_limit);
    assert_eq!(receipt.stdout.total_bytes, 20_000);
    assert_eq!(receipt.stderr.total_bytes, 20_000);
    assert!(receipt.stdout.truncated && receipt.stderr.truncated);
    Ok(())
}

#[test]
#[cfg(unix)]
fn spec030_process_reports_non_zero_and_empty_output() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let command = shell_command(temp.path(), "exit 7");

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;

    assert_eq!(
        receipt.outcome,
        ControlledChildOutcome::Failed { code: Some(7) }
    );
    assert_eq!(receipt.stdout.total_bytes, 0);
    assert_eq!(receipt.stderr.total_bytes, 0);
    assert!(!receipt.stdout.truncated && !receipt.stderr.truncated);
    Ok(())
}

#[test]
#[cfg(unix)]
fn spec030_process_invalid_cwd_returns_typed_outcome_without_spawn() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let marker = temp.path().join("must-not-exist");
    let command = shell_command(&temp.path().join("missing"), "touch ../must-not-exist");

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;

    assert_eq!(receipt.outcome, ControlledChildOutcome::InvalidCwd);
    assert!(!marker.exists());
    Ok(())
}

#[test]
#[cfg(unix)]
fn spec030_process_timeout_returns_promptly() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let mut command = shell_command(temp.path(), "sleep 30");
    command.timeout = Duration::from_millis(100);
    let started = Instant::now();
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;
    facts.record_controlled_child_receipt(&receipt)?;

    assert_eq!(receipt.outcome, ControlledChildOutcome::TimedOut);
    assert!(receipt.cleanup_attempted);
    assert!(started.elapsed() < Duration::from_secs(3));
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let adapter = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::GenericExec)
        .ok_or("generic exec projection missing")?;
    assert_eq!(
        adapter.recent_outcomes[0].outcome,
        ProcessTerminalOutcome::TimedOut
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn spec030_process_explicit_abort_stops_running_command() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let marker = temp.path().join("started");
    let command = shell_command(temp.path(), "touch started; sleep 30");
    let abort = ControlledChildAbort::new();
    let runner_abort = abort.clone();
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runner = thread::spawn(move || run_generic_argv(&command, &runner_abort));
    wait_for_path(&marker)?;

    abort.abort();
    let receipt = runner
        .join()
        .map_err(|_| "controlled child thread panicked")??;
    facts.record_controlled_child_receipt(&receipt)?;

    assert_eq!(receipt.outcome, ControlledChildOutcome::Aborted);
    assert!(receipt.cleanup_attempted);
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();
    let adapter = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::GenericExec)
        .ok_or("generic exec projection missing")?;
    assert_eq!(
        adapter.recent_outcomes[0].outcome,
        ProcessTerminalOutcome::Aborted
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn spec030_process_timeout_cleans_term_ignoring_descendant() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let pid_path = temp.path().join("descendant.pid");
    let script =
        "trap '' TERM; /bin/sh -c 'trap \"\" TERM; echo $$ > descendant.pid; exec sleep 30' & wait";
    let mut command = shell_command(temp.path(), script);
    command.timeout = Duration::from_millis(200);

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;
    let pid = std::fs::read_to_string(&pid_path)?.trim().parse::<i32>()?;

    assert_eq!(receipt.outcome, ControlledChildOutcome::TimedOut);
    wait_for_process_exit(pid)?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn spec030_process_configured_entry_seams_preserve_adapter_identity() -> Result<(), Box<dyn Error>>
{
    type Runner = fn(
        &ControlledChildCommand,
        &ControlledChildAbort,
    ) -> Result<ControlledChildReceipt, ControlledChildError>;
    let temp = tempfile::tempdir()?;
    for (run, expected) in [
        (run_bash as Runner, ControlledChildAdapter::Bash),
        (
            run_configured_credential_command as Runner,
            ControlledChildAdapter::CredentialCommand,
        ),
        (
            run_configured_package_command as Runner,
            ControlledChildAdapter::PackageCommand,
        ),
        (
            run_configured_load_check as Runner,
            ControlledChildAdapter::LoadCheck,
        ),
    ] {
        let receipt = run(
            &shell_command(temp.path(), ":"),
            &ControlledChildAbort::new(),
        )?;
        assert_eq!(receipt.adapter, expected);
        assert_eq!(
            receipt.outcome,
            ControlledChildOutcome::Succeeded { code: Some(0) }
        );
    }
    Ok(())
}

#[test]
fn spec030_process_cleanup_capability_matches_platform_support() {
    let expected = if cfg!(unix) {
        DescendantCleanupCapability::Supported
    } else {
        DescendantCleanupCapability::Unsupported
    };
    assert_eq!(descendant_cleanup_capability(), expected);
}
