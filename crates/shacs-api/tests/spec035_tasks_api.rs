use axum::body::{to_bytes, Body};
use shacs_api::{api_router, api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_projection::{
    Spec033GoalStatus, Spec035TaskActionStatus, Spec035TaskDetail, Spec035TaskOwnerKind,
    Spec035TasksProjection,
};
use shacs_providers::LlmResponse;
use shacs_session::{Session, SessionManager};
use std::path::PathBuf;
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

#[tokio::test]
async fn app_stop_action_stays_requested_until_owner_terminal_receipt(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let data_dir = root.path().join("data");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    let app_id = shacs_core::app::AppId::parse("fixture-app")?;
    let registry = shacs_core::app::AppRegistry {
        entries: std::collections::BTreeMap::from([(
            app_id.clone(),
            shacs_core::app::AppRegistryEntry {
                app_id: app_id.clone(),
                version: "1".to_owned(),
                digest: "sha256:fixture".to_owned(),
                bundle_path: data_dir.join("apps/fixture-app.shacsapp"),
                lifecycle_state: shacs_core::app::AppLifecycleState::Enabled,
                permission_requests: Vec::new(),
                secret_requests: Vec::new(),
                resource_summaries: Vec::new(),
                grant_reference: None,
                unavailable_reasons: Vec::new(),
                process_snapshots: Vec::new(),
                installed_at_unix_ms: 1,
            },
        )]),
    };
    shacs_core::app::AppRegistryStore::new(&data_dir).save(&registry)?;
    let journal = shacs_core::app_lifecycle::AppSupervisorJournal::new(data_dir.join("apps"));
    let started = journal.request(
        &app_id,
        shacs_core::app_lifecycle::AppLifecycleAction::Start,
    )?;
    journal.complete(
        &started,
        shacs_core::app_lifecycle::AppProcessState::Running,
        "sha256:fixture",
        "runtime:fixture",
        Vec::new(),
    )?;
    let adapter = Arc::new(WorkspaceAdapter::new(workspace, data_dir));

    // When
    let (status, bytes) = request(
        api_router_with_local_mutations(adapter),
        "POST",
        "/v1/tasks/actions",
        r#"{"owner":"app","action":"stop","locator":"fixture-app","session_id":"cli:direct","transport_hello":{"client_id":"client:tasks-api","schema_versions":[1],"mutation_capabilities":["task_stop"]}}"#,
    )
    .await?;

    // Then
    assert_eq!(status, axum::http::StatusCode::OK);
    let projection = Spec035TasksProjection::parse_json(std::str::from_utf8(&bytes)?)?;
    let app = projection
        .rows()
        .iter()
        .find(|row| row.owner().kind() == Spec035TaskOwnerKind::App)
        .ok_or("app row")?;
    assert_eq!(
        app.action().map(|action| action.status),
        Some(Spec035TaskActionStatus::Requested)
    );
    Ok(())
}
