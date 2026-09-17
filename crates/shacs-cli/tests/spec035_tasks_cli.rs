use shacs_cli::parse_cli_args;
use shacs_projection::{Spec035TaskDetail, Spec035TaskOwnerKind, Spec035TasksProjection};
use shacs_session::{Session, SessionManager};
use std::fs;
use std::process::Command;
use std::sync::Arc;
use tower::ServiceExt;

struct WorkspaceAdapter {
    workspace: std::path::PathBuf,
    data_dir: std::path::PathBuf,
}

impl shacs_api::ChatCompletionAdapter for WorkspaceAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<shacs_providers::LlmResponse, shacs_api::ApiError> {
        unreachable!("tasks routes do not complete chat")
    }

    fn session_workspace(&self) -> Option<std::path::PathBuf> {
        Some(self.workspace.clone())
    }

    fn runtime_data_dir(&self) -> Option<std::path::PathBuf> {
        Some(self.data_dir.clone())
    }
}

#[test]
fn compiled_cli_help_lists_tasks_command() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--help")
        .output()?;

    // Then
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)?
        .lines()
        .any(|line| line.trim_start().starts_with("tasks ")));
    Ok(())
}

#[tokio::test]
async fn compiled_cli_and_api_return_identical_canonical_tasks_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    shacs_core::runtime::apply_goal_surface_action(
        &workspace,
        "cli:direct",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "ship task five".to_owned(),
            turn_budget: 17,
        },
        "1",
    )?;
    shacs_core::runtime::apply_goal_surface_action(
        &workspace,
        "cli:direct",
        shacs_core::runtime::GoalSurfaceAction::Blocked {
            reason: "owner evidence pending".to_owned(),
        },
        "2",
    )?;

    // When
    let cli = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "tasks",
            "--json",
            "--workspace",
            &workspace.display().to_string(),
            "--data-dir",
            &data_dir.display().to_string(),
        ])
        .output()?;
    let request = axum::http::Request::builder()
        .uri("/v1/tasks?session_id=cli%3Adirect")
        .body(axum::body::Body::empty())?;
    let response = shacs_api::api_router(Arc::new(WorkspaceAdapter {
        workspace,
        data_dir,
    }))
    .oneshot(request)
    .await?;
    let api = axum::body::to_bytes(response.into_body(), 1 << 20).await?;

    // Then
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(cli.stdout.strip_suffix(b"\n").unwrap_or(&cli.stdout), api);
    let projection = Spec035TasksProjection::parse_json(std::str::from_utf8(&api)?)?;
    let goal = projection
        .rows()
        .iter()
        .find(|row| row.owner().kind() == Spec035TaskOwnerKind::Goal)
        .ok_or("goal row")?;
    let Spec035TaskDetail::Goal { fact } = goal.detail() else {
        return Err("goal detail".into());
    };
    assert_eq!(fact.stop_reason.as_deref(), Some("blocked_by_user"));
    assert_eq!(fact.budget.turn_budget, 17);
    Ok(())
}

#[test]
fn malformed_tasks_action_is_rejected_by_cli_parser_before_execution(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given / When
    let result = parse_cli_args([
        "tasks",
        "--json",
        "--workspace",
        "/must-not-be-read",
        "--data-dir",
        "/must-not-be-read",
        "--session",
        "cli:direct",
        "--owner",
        "app",
        "--action",
        "pause",
        "--locator",
        "/private/app",
    ]);

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn compiled_cli_rejects_unsupported_capability_before_owner_access(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    let config_path = root.path().join("config.json");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    shacs_core::runtime::apply_goal_surface_action(
        &workspace,
        "cli:direct",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "capability gate".to_owned(),
            turn_budget: 3,
        },
        "1",
    )?;
    let projection =
        shacs_core::runtime::build_spec035_tasks_projection(&workspace, &data_dir, "cli:direct")?;
    let locator = projection
        .rows()
        .first()
        .ok_or("missing goal row")?
        .owner()
        .locator();
    let before = directory_bytes(root.path())?;

    // When
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "tasks",
            "--json",
            "--config",
            &config_path.display().to_string(),
            "--workspace",
            &workspace.display().to_string(),
            "--data-dir",
            &data_dir.display().to_string(),
            "--session",
            "cli:direct",
            "--owner",
            "goal",
            "--action",
            "pause",
            "--locator",
            locator.as_str(),
            "--transport-hello",
            r#"{"client_id":"client:cli","schema_versions":[1],"mutation_capabilities":[]}"#,
        ])
        .output()?;
    let after = directory_bytes(root.path())?;

    // Then
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr)?,
        "unsupported: capability_unavailable\n"
    );
    assert_eq!(before, after);
    Ok(())
}

fn directory_bytes(root: &std::path::Path) -> Result<Vec<(String, Vec<u8>)>, std::io::Error> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .map_or_else(|_| path.clone(), std::path::Path::to_path_buf);
                files.push((relative.display().to_string(), fs::read(path)?));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}
