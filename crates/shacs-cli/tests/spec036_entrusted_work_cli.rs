use shacs_cli::{parse_cli_args, run_command};
use shacs_core::runtime::{EntrustedWorkSnapshot, PersistentGoalStatus};
use shacs_session::{Session, SessionManager};
use std::path::Path;
use std::process::Command;

fn run_work(workspace: &Path, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["work".to_owned()];
    args.extend(arguments.iter().map(|value| (*value).to_owned()));
    args.extend([
        "--workspace".to_owned(),
        workspace.display().to_string(),
        "--session".to_owned(),
        "cli:direct".to_owned(),
    ]);
    Ok(run_command(parse_cli_args(args)?)?)
}

#[test]
fn work_commands_persist_lifecycle_and_reject_stale_revision(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;

    // When
    let created: EntrustedWorkSnapshot = serde_json::from_str(&run_work(
        &workspace,
        &[
            "create",
            "watch the release",
            "--consent",
            "explicit-request",
            "--evidence-ref",
            "cli:user-command",
            "--instructions",
            "check the release page",
            "--budget",
            "daily",
        ],
    )?)?;
    let work_id = created.work.work_id.clone();
    let revised: EntrustedWorkSnapshot = serde_json::from_str(&run_work(
        &workspace,
        &[
            "revise",
            &work_id,
            "--expected-revision",
            "1",
            "--instructions",
            "check release and changelog",
        ],
    )?)?;
    let stale = run_work(
        &workspace,
        &[
            "revise",
            &work_id,
            "--expected-revision",
            "1",
            "--instructions",
            "overwrite latest conditions",
        ],
    );
    run_work(&workspace, &["pause", &work_id])?;
    run_work(&workspace, &["resume", &work_id])?;
    run_work(&workspace, &["done", &work_id])?;

    // Then
    assert_eq!(revised.work.current_revision, 2);
    assert!(stale.is_err());
    let restarted: EntrustedWorkSnapshot =
        serde_json::from_str(&run_work(&workspace, &["read", &work_id])?)?;
    assert_eq!(restarted.work.work_id, work_id);
    assert_eq!(restarted.work.current_revision, 2);
    assert_eq!(restarted.goal_status, PersistentGoalStatus::Done);
    assert_eq!(
        restarted
            .work
            .current_conditions()
            .ok_or("current conditions")?
            .instructions,
        "check release and changelog"
    );
    let second: EntrustedWorkSnapshot = serde_json::from_str(&run_work(
        &workspace,
        &[
            "create",
            "clearable work",
            "--consent",
            "confirmed-proposal",
            "--evidence-ref",
            "cli:confirmed-proposal",
            "--instructions",
            "clear this work",
        ],
    )?)?;
    run_work(&workspace, &["clear", &second.work.work_id])?;
    let cleared: EntrustedWorkSnapshot =
        serde_json::from_str(&run_work(&workspace, &["read", &second.work.work_id])?)?;
    assert_eq!(cleared.goal_status, PersistentGoalStatus::Cleared);
    Ok(())
}

#[test]
fn built_cli_help_and_bad_input_use_isolated_paths() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;

    // When
    let help = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--help")
        .output()?;
    let invalid = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "work",
            "create",
            "watch",
            "--workspace",
            &workspace.display().to_string(),
            "--session",
            "cli:direct",
        ])
        .output()?;

    // Then
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)?.contains("  work      "));
    assert!(!invalid.status.success());
    assert!(String::from_utf8(invalid.stderr)?.contains("requires --consent"));
    Ok(())
}
