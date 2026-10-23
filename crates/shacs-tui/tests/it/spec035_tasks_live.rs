use shacs_config::config_context;
use shacs_core::runtime::{
    apply_goal_surface_action, build_spec035_tasks_projection, GoalSurfaceAction,
    Spec035TasksSemanticAction, SurfaceActionOutcomeKind,
};
use shacs_projection::{
    Spec035TransportClientHello, Spec035TransportClientHelloInput, Spec035TransportClientId,
    Spec035TransportSchemaVersion,
};
use shacs_session::{Session, SessionManager};
use shacs_tui::action_runner::{run_tasks_action, tui_transport_hello, TasksActionRequest};
use shacs_tui::live_source::{RuntimeProjectionSource, SessionRuntimeSource};
use shacs_tui::state::TuiState;
use shacs_tui::update::apply_task_action_result;
use shacs_tui::view::render_lines;
use std::fs;

#[test]
fn live_tasks_action_validates_before_owner_side_effects() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config_path = root.path().join("data/config.json");
    let context = config_context(Some(config_path.clone()), Some(workspace.clone()));
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    apply_goal_surface_action(
        &workspace,
        "cli:direct",
        GoalSurfaceAction::Set {
            text: "ship TUI tasks".to_owned(),
            turn_budget: 9,
        },
        "1",
    )?;
    let projection = build_spec035_tasks_projection(&workspace, &context.data_dir, "cli:direct")?;
    let goal = projection.rows().first().ok_or("missing goal row")?;
    let transport_hello = tui_transport_hello()?;

    // When
    let supported = run_tasks_action(TasksActionRequest {
        config_path: Some(&config_path),
        workspace: &workspace,
        session_id: "cli:direct",
        action: Spec035TasksSemanticAction::GoalPause {
            locator: goal.owner().locator().as_str().to_owned(),
        },
        transport_hello: Some(&transport_hello),
    });
    let before_invalid = directory_bytes(root.path())?;
    let invalid = run_tasks_action(TasksActionRequest {
        config_path: Some(&config_path),
        workspace: &workspace,
        session_id: "cli:direct",
        action: Spec035TasksSemanticAction::GoalPause {
            locator: goal.owner().locator().as_str().to_owned(),
        },
        transport_hello: Some(&transport_hello),
    });
    let after_invalid = directory_bytes(root.path())?;

    // Then
    let shacs_tui::action_runner::TasksActionResult::Owner(supported) = supported else {
        return Err("supported action was rejected".into());
    };
    let shacs_tui::action_runner::TasksActionResult::Owner(invalid) = invalid else {
        return Err("invalid owner action was rejected by transport".into());
    };
    assert_eq!(supported.kind, SurfaceActionOutcomeKind::Completed);
    assert_eq!(invalid.kind, SurfaceActionOutcomeKind::Unavailable);
    assert_eq!(invalid.detail, "tasks action is not advertised");
    assert_eq!(before_invalid, after_invalid);
    Ok(())
}

#[test]
fn production_tui_action_result_view_rejects_unsupported_capability(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config_path = root.path().join("config.json");
    let context = config_context(Some(config_path.clone()), Some(workspace.clone()));
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    apply_goal_surface_action(
        &workspace,
        "cli:direct",
        GoalSurfaceAction::Set {
            text: "TUI capability gate".to_owned(),
            turn_budget: 3,
        },
        "1",
    )?;
    let projection = build_spec035_tasks_projection(&workspace, &context.data_dir, "cli:direct")?;
    let locator = projection
        .rows()
        .first()
        .ok_or("missing goal row")?
        .owner()
        .locator();
    let source = SessionRuntimeSource::with_config(Some(config_path.clone()), &workspace);
    let mut state = TuiState::from_snapshot(source.load()?, None);
    let unsupported_hello =
        Spec035TransportClientHello::try_new(Spec035TransportClientHelloInput {
            client_id: Spec035TransportClientId::try_new("client:tui-test")?,
            schema_versions: vec![Spec035TransportSchemaVersion::try_new(1)?],
            mutation_capabilities: Vec::new(),
            resume: None,
        })?;
    let before = directory_bytes(root.path())?;

    // When
    let outcome = run_tasks_action(TasksActionRequest {
        config_path: Some(&config_path),
        workspace: &workspace,
        session_id: "cli:direct",
        action: Spec035TasksSemanticAction::GoalPause {
            locator: locator.as_str().to_owned(),
        },
        transport_hello: Some(&unsupported_hello),
    });
    apply_task_action_result(&mut state, outcome);
    let rendered = render_lines(&state);
    let after = directory_bytes(root.path())?;

    // Then
    assert!(rendered
        .iter()
        .any(|line| line == "unsupported: capability_unavailable"));
    assert_eq!(before, after);
    Ok(())
}

fn directory_bytes(root: &std::path::Path) -> Result<Vec<(String, Vec<u8>)>, std::io::Error> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_or_else(|_| entry.path(), std::path::Path::to_path_buf);
                files.push((relative.display().to_string(), fs::read(entry.path())?));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}
