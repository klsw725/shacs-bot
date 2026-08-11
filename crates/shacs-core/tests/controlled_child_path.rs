#![cfg(unix)]

use shacs_core::controlled_child::{
    run_generic_argv, ControlledChildAbort, ControlledChildCommand, ControlledChildError,
    ControlledChildOutcome,
};
use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::time::Duration;

#[test]
fn controlled_child_resolves_program_from_typed_path_without_inherited_environment(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let bin = root.path().join("bin");
    fs::create_dir(&bin)?;
    let program = bin.join("typed-command");
    symlink("/bin/sh", &program)?;
    let mut command = ControlledChildCommand::new(
        ["typed-command", "-c", "exit 9"],
        root.path(),
        Duration::from_secs(3),
    );
    command.inherit_env = false;
    command.env.insert("PATH".into(), bin.into());

    let receipt = run_generic_argv(&command, &ControlledChildAbort::new())?;

    assert_eq!(
        receipt.outcome,
        ControlledChildOutcome::Failed { code: Some(9) }
    );
    Ok(())
}

#[test]
fn controlled_child_reports_spawn_when_typed_path_is_missing() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut command =
        ControlledChildCommand::new(["missing-command"], root.path(), Duration::from_secs(3));
    command.inherit_env = false;

    let result = run_generic_argv(&command, &ControlledChildAbort::new());

    assert!(matches!(result, Err(ControlledChildError::Spawn(_))));
    Ok(())
}
