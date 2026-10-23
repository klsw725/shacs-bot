use shacs_config::{config_context, ConfigContext};
use shacs_core::app::{AppId, AppLifecycleState, AppRegistry, AppRegistryEntry, AppRegistryStore};
use shacs_core::app_lifecycle::{AppLifecycleAction, AppProcessState, AppSupervisorJournal};
use shacs_projection::{
    Spec035TaskActionStatus, Spec035TaskCount, Spec035TaskOwnerKind, Spec035TaskState,
    Spec035TasksProjection,
};
use std::error::Error;
use std::process::Command;

fn tasks_command(context: &ConfigContext) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shacs-bot"));
    command
        .args(["tasks", "--json", "--config"])
        .arg(&context.config_path)
        .arg("--workspace")
        .arg(&context.workspace)
        .arg("--data-dir")
        .arg(&context.data_dir)
        .args(["--session", "cli:direct"]);
    command
}

fn assert_tasks(
    context: &ConfigContext,
    state: Spec035TaskState,
    action_status: Spec035TaskActionStatus,
) -> Result<(), Box<dyn Error>> {
    let output = tasks_command(context).output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = std::str::from_utf8(&output.stdout)?;
    let projection = Spec035TasksProjection::parse_json(json)?;
    assert_eq!(projection.coverage().app, Spec035TaskCount::available(1));
    let app = projection.rows().first().ok_or("app row missing")?;
    assert_eq!(app.owner().kind(), Spec035TaskOwnerKind::App);
    assert_eq!(app.owner().locator().as_str(), "fixture-app");
    assert_eq!(app.state(), state);
    assert_eq!(
        app.action().map(|action| action.status),
        Some(action_status)
    );
    println!("{json}");
    Ok(())
}

#[test]
fn compiled_apps_owner_and_tasks_stop_share_canonical_journal() -> Result<(), Box<dyn Error>> {
    for use_tasks_action in [false, true] {
        let root = tempfile::Builder::new()
            .prefix("tac-")
            .tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let context = config_context(
            Some(root.path().join("data/config.json")),
            Some(root.path().join("workspace")),
        );
        let app_id = AppId::parse("fixture-app")?;
        let entry = AppRegistryEntry {
            app_id: app_id.clone(),
            version: "1".to_owned(),
            digest: "sha256:fixture".to_owned(),
            bundle_path: context.data_dir.join("apps/fixture-app.shacsapp"),
            lifecycle_state: AppLifecycleState::Enabled,
            permission_requests: Vec::new(),
            secret_requests: Vec::new(),
            resource_summaries: Vec::new(),
            grant_reference: None,
            unavailable_reasons: Vec::new(),
            process_snapshots: Vec::new(),
            installed_at_unix_ms: 1,
        };
        AppRegistryStore::new(&context.data_dir).save(&AppRegistry {
            entries: [(app_id.clone(), entry)].into(),
        })?;
        std::fs::write(&context.config_path, b"{}")?;
        std::fs::create_dir_all(&context.workspace)?;
        let journal = AppSupervisorJournal::new(context.runtime_subdir("apps"));
        let started = journal.request(&app_id, AppLifecycleAction::Start)?;
        journal.complete(&started, AppProcessState::Running, "", "", Vec::new())?;
        assert_tasks(
            &context,
            Spec035TaskState::Running,
            Spec035TaskActionStatus::Available,
        )?;

        let output = if use_tasks_action {
            tasks_command(&context)
                .args([
                    "--owner",
                    "app",
                    "--action",
                    "stop",
                    "--locator",
                    app_id.as_str(),
                    "--transport-hello",
                    r#"{"client_id":"client:app-path","schema_versions":[1],"mutation_capabilities":["task_stop"]}"#,
                ])
                .output()?
        } else {
            Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
                .args(["apps", "stop", app_id.as_str(), "--config"])
                .arg(&context.config_path)
                .arg("--workspace")
                .arg(&context.workspace)
                .output()?
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        println!(
            "owner use_tasks_action={use_tasks_action}: {}",
            String::from_utf8(output.stdout)?
        );
        assert_tasks(
            &context,
            Spec035TaskState::Requested,
            Spec035TaskActionStatus::Requested,
        )?;

        let stopped = journal
            .pending_request(&app_id, AppLifecycleAction::Stop)?
            .ok_or("canonical owner stop request missing")?;
        journal.complete(&stopped, AppProcessState::Stopped, "", "", Vec::new())?;
        assert_tasks(
            &context,
            Spec035TaskState::Done,
            Spec035TaskActionStatus::Completed,
        )?;
        assert!(!context.data_dir.join("runtime/apps").exists());
        assert_eq!(std::fs::read(&context.config_path)?, b"{}");
        root.close()?;
    }
    Ok(())
}
