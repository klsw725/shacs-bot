use axum::body::{to_bytes, Body};
use shacs_api::{api_router, api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_projection::{
    Spec033GoalStatus, Spec035TaskCount, Spec035TaskDetail, Spec035TaskOwnerKind,
    Spec035TasksProjection,
};
use shacs_providers::LlmResponse;
use shacs_session::{Session, SessionManager};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

struct WorkspaceAdapter {
    workspace: PathBuf,
    data_dir: PathBuf,
    owner_accesses: AtomicUsize,
}

impl WorkspaceAdapter {
    fn new(workspace: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            workspace,
            data_dir,
            owner_accesses: AtomicUsize::new(0),
        }
    }
}

impl ChatCompletionAdapter for WorkspaceAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<LlmResponse, shacs_api::ApiError> {
        unreachable!("tasks routes do not complete chat")
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        self.owner_accesses.fetch_add(1, Ordering::SeqCst);
        Some(self.workspace.clone())
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.data_dir.clone())
    }
}

async fn request(
    app: axum::Router,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(axum::http::StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))?;
    let response = app.oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await?.to_vec();
    Ok((status, bytes))
}

#[tokio::test]
async fn tasks_get_preserves_goal_accounting_and_marks_missing_sources_unavailable(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("api:default"))?;
    shacs_core::runtime::apply_goal_surface_action(
        &workspace,
        "api:default",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "ship task five".to_owned(),
            turn_budget: 13,
        },
        "1",
    )?;
    shacs_core::runtime::apply_goal_surface_action(
        &workspace,
        "api:default",
        shacs_core::runtime::GoalSurfaceAction::Done,
        "2",
    )?;

    // When
    let (status, bytes) = request(
        api_router(Arc::new(WorkspaceAdapter::new(workspace, data_dir))),
        "GET",
        "/v1/tasks",
        "",
    )
    .await?;

    // Then
    assert_eq!(status, axum::http::StatusCode::OK);
    let projection = Spec035TasksProjection::parse_json(std::str::from_utf8(&bytes)?)?;
    let goal = projection
        .rows()
        .iter()
        .find(|row| row.owner().kind() == Spec035TaskOwnerKind::Goal)
        .ok_or("goal row")?;
    let Spec035TaskDetail::Goal { fact } = goal.detail() else {
        return Err("goal detail".into());
    };
    assert_eq!(fact.status, Spec033GoalStatus::Done);
    assert_eq!(fact.stop_reason.as_deref(), Some("marked_done_by_user"));
    assert_eq!(fact.budget.turn_budget, 13);
    assert_eq!(fact.budget.remaining_turns, 13);
    assert_eq!(projection.coverage().child.count(), None);
    assert_eq!(projection.coverage().workflow.count(), None);
    Ok(())
}

#[tokio::test]
async fn unsupported_tasks_action_is_rejected_before_owner_access(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let adapter = Arc::new(WorkspaceAdapter::new(
        PathBuf::from("/must-not-be-read"),
        PathBuf::from("/must-not-be-read"),
    ));

    // When
    let (status, _) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        "/v1/tasks/actions",
        r#"{"owner":"workflow","action":"stop","locator":"workflow:one","session_id":"cli:direct"}"#,
    )
    .await?;

    // Then
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 0);
    Ok(())
}

fn copy_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &path)?;
        } else {
            std::fs::copy(entry.path(), path)?;
        }
    }
    Ok(())
}

fn tree(
    root: &Path,
    path: &Path,
    entries: &mut Vec<(PathBuf, Option<Vec<u8>>)>,
) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        entries.push((path.strip_prefix(root).unwrap_or(path).to_path_buf(), None));
        for entry in std::fs::read_dir(path)? {
            tree(root, &entry?.path(), entries)?;
        }
    } else {
        entries.push((
            path.strip_prefix(root).unwrap_or(path).to_path_buf(),
            Some(std::fs::read(path)?),
        ));
    }
    Ok(())
}

fn data_tree(data_dir: &Path) -> std::io::Result<Vec<(PathBuf, Option<Vec<u8>>)>> {
    let mut entries = Vec::new();
    tree(data_dir, data_dir, &mut entries)?;
    entries.sort();
    Ok(entries)
}

#[tokio::test]
async fn leftover_app_data_leaves_tasks_unchanged_and_app_actions_unsupported(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let empty = tempfile::tempdir()?;
    let leftover = tempfile::tempdir()?;
    let data_dir = leftover.path().join("data");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../shacs-core/tests/fixtures/spec043_leftover_apps"),
        &data_dir,
    )?;
    let before = data_tree(&data_dir)?;
    let adapter = Arc::new(WorkspaceAdapter::new(
        leftover.path().join("workspace"),
        data_dir.clone(),
    ));

    // When
    let (_, without) = request(
        api_router(Arc::new(WorkspaceAdapter::new(
            empty.path().join("workspace"),
            empty.path().join("data"),
        ))),
        "GET",
        "/v1/tasks?session_id=cli%3Adirect",
        "",
    )
    .await?;
    let (status, with) = request(
        api_router(adapter.clone()),
        "GET",
        "/v1/tasks?session_id=cli%3Adirect",
        "",
    )
    .await?;
    let mut rejections = Vec::new();
    for (action, locator) in [("stop", "demo.clock"), ("recover", "demo.recover")] {
        rejections.push(
            request(
                api_router_with_local_mutations(adapter.clone()),
                "POST",
                "/v1/tasks/actions",
                &format!(
                    r#"{{"owner":"app","action":"{action}","locator":"{locator}","session_id":"cli:direct","transport_hello":{{"client_id":"client:tasks-api","schema_versions":[1],"mutation_capabilities":["task_stop","task_recover"]}}}}"#
                ),
            )
            .await?,
        );
    }

    // Then
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(with, without);
    let projection = Spec035TasksProjection::parse_json(std::str::from_utf8(&with)?)?;
    assert_eq!(projection.coverage().app, Spec035TaskCount::Unavailable);
    assert!(projection
        .rows()
        .iter()
        .all(|row| row.owner().kind() != Spec035TaskOwnerKind::App));
    for (status, body) in rejections {
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert!(String::from_utf8(body)?.contains("unsupported tasks owner/action pair"));
    }
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 1);
    assert_eq!(data_tree(&data_dir)?, before);
    Ok(())
}
