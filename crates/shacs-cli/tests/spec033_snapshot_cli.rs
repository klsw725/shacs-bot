use shacs_cli::{parse_cli_args, run_command};
use shacs_projection::{Spec033Availability, Spec033GoalStatus, Spec033Snapshot};
use shacs_session::{Session, SessionManager};
use std::process::Command;
use std::sync::Arc;
use tower::ServiceExt;

struct WorkspaceAdapter(std::path::PathBuf);

impl shacs_api::ChatCompletionAdapter for WorkspaceAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<shacs_providers::LlmResponse, shacs_api::ApiError> {
        unreachable!("goal snapshot does not complete chat")
    }

    fn session_workspace(&self) -> Option<std::path::PathBuf> {
        Some(self.0.clone())
    }
}

fn run_goal(
    workspace: &std::path::Path,
    action: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["goal".to_owned()];
    args.extend(action.iter().map(|value| (*value).to_owned()));
    args.extend([
        "--workspace".to_owned(),
        workspace.display().to_string(),
        "--session".to_owned(),
        "cli:direct".to_owned(),
    ]);
    Ok(run_command(parse_cli_args(args)?)?)
}

#[test]
fn cli_exercises_persisted_goal_lifecycle_through_owner_transitions(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;

    // When / Then
    run_goal(&workspace, &["set", "ship it"])?;
    run_goal(&workspace, &["pause"])?;
    run_goal(&workspace, &["resume"])?;
    run_goal(&workspace, &["blocked", "needs input"])?;
    run_goal(&workspace, &["resume"])?;
    run_goal(&workspace, &["done"])?;
    let done: Spec033Snapshot = serde_json::from_str(&run_goal(&workspace, &["status"])?)?;
    assert_eq!(done.goal.availability, Spec033Availability::Available);
    let done_fact = done.goal.fact.ok_or("goal fact")?;
    assert_eq!(done_fact.status, Spec033GoalStatus::Done);
    assert_eq!(
        done_fact.stop_reason.as_deref(),
        Some("marked_done_by_user")
    );
    assert_eq!(done_fact.budget.remaining_turns, 8);
    assert!(!done_fact.user_interrupted);
    assert!(done_fact.latest_transition.is_some());

    run_goal(&workspace, &["clear"])?;
    let cleared: Spec033Snapshot = serde_json::from_str(&run_goal(&workspace, &["inspect"])?)?;
    assert_eq!(
        cleared.goal.fact.ok_or("goal fact")?.status,
        Spec033GoalStatus::Cleared
    );
    Ok(())
}

#[test]
fn built_cli_independently_reads_the_canonical_persisted_snapshot(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    run_goal(&workspace, &["set", "ship it"])?;

    // When
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "goal",
            "inspect",
            "--workspace",
            &workspace.display().to_string(),
            "--session",
            "cli:direct",
        ])
        .output()?;

    // Then
    assert!(output.status.success());
    let binary_snapshot: Spec033Snapshot = serde_json::from_slice(&output.stdout)?;
    let library_snapshot: Spec033Snapshot =
        serde_json::from_str(&run_goal(&workspace, &["inspect"])?)?;
    assert_eq!(
        serde_json::to_vec(&binary_snapshot)?,
        serde_json::to_vec(&library_snapshot)?
    );
    Ok(())
}

#[tokio::test]
async fn built_cli_and_actual_api_router_return_byte_equivalent_canonical_snapshots(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    run_goal(&workspace, &["set", "ship it"])?;

    // When
    let cli = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "goal",
            "inspect",
            "--workspace",
            &workspace.display().to_string(),
            "--session",
            "cli:direct",
        ])
        .output()?;
    let request = axum::http::Request::builder()
        .uri("/v1/sessions/cli%3Adirect/goal-snapshot")
        .body(axum::body::Body::empty())?;
    let response = shacs_api::api_router(Arc::new(WorkspaceAdapter(workspace)))
        .oneshot(request)
        .await?;
    let api = axum::body::to_bytes(response.into_body(), 1 << 20).await?;

    // Then
    assert!(cli.status.success());
    let cli_snapshot: Spec033Snapshot = serde_json::from_slice(&cli.stdout)?;
    let api_snapshot: Spec033Snapshot = serde_json::from_slice(&api)?;
    assert_eq!(
        serde_json::to_vec(&cli_snapshot)?,
        serde_json::to_vec(&api_snapshot)?
    );
    Ok(())
}
