use axum::body::{to_bytes, Body};
use shacs_api::{api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_providers::LlmResponse;
use shacs_session::{Session, SessionManager};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

struct CountingAdapter {
    workspace: PathBuf,
    owner_accesses: AtomicUsize,
}

impl ChatCompletionAdapter for CountingAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<LlmResponse, shacs_api::ApiError> {
        unreachable!("capability routes do not complete chat")
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        self.owner_accesses.fetch_add(1, Ordering::SeqCst);
        Some(self.workspace.clone())
    }
}

#[tokio::test]
async fn live_hello_reports_supported_and_unsupported_capabilities(
) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = Arc::new(CountingAdapter {
        workspace: PathBuf::from("/must-not-be-read"),
        owner_accesses: AtomicUsize::new(0),
    });
    let (status, body) = post(
        api_router_with_local_mutations(adapter.clone()),
        "/v1/transport/hello",
        r#"{"client_id":"client:api","schema_versions":[1],"mutation_capabilities":["task_stop","task_retry"]}"#,
    )
    .await?;

    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["capability_decisions"][0]["capability"], "task_stop");
    assert_eq!(
        value["capability_decisions"][0]["support"]["status"],
        "supported"
    );
    assert_eq!(value["capability_decisions"][1]["capability"], "task_retry");
    assert_eq!(
        value["capability_decisions"][1]["support"]["status"],
        "unsupported"
    );
    assert_eq!(
        value["capability_decisions"][1]["support"]["reason"],
        "capability_unavailable"
    );
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn hello_schema_mismatch_uses_canonical_blocked_reason(
) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = Arc::new(CountingAdapter {
        workspace: PathBuf::from("/must-not-be-read"),
        owner_accesses: AtomicUsize::new(0),
    });

    let (status, body) = post(
        api_router_with_local_mutations(adapter.clone()),
        "/v1/transport/hello",
        r#"{"client_id":"client:api-schema","schema_versions":[2],"mutation_capabilities":["task_stop"]}"#,
    )
    .await?;

    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["reason"], "schema_mismatch");
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn unsupported_mutation_returns_before_owner_access() -> Result<(), Box<dyn std::error::Error>>
{
    let adapter = Arc::new(CountingAdapter {
        workspace: PathBuf::from("/must-not-be-read"),
        owner_accesses: AtomicUsize::new(0),
    });
    let (status, body) = post(
        api_router_with_local_mutations(adapter.clone()),
        "/v1/tasks/actions",
        &task_action(&[]),
    )
    .await?;

    assert_eq!(status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(value["status"], "unsupported");
    assert_eq!(value["reason"], "capability_unavailable");
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn supported_mutation_reaches_existing_handler_once() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let mut sessions = SessionManager::new(root.path())?;
    sessions.save(&Session::new("cli:direct"))?;
    shacs_core::runtime::apply_goal_surface_action(
        root.path(),
        "cli:direct",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "capability gate".to_owned(),
            turn_budget: 3,
        },
        "1",
    )?;
    let goal_id =
        shacs_core::runtime::build_spec033_snapshot_from(root.path(), root.path(), "cli:direct")?
            .goal
            .fact
            .ok_or("missing goal fact")?
            .goal_id;
    let adapter = Arc::new(CountingAdapter {
        workspace: root.path().to_path_buf(),
        owner_accesses: AtomicUsize::new(0),
    });

    let (status, _) = post(
        api_router_with_local_mutations(adapter.clone()),
        "/v1/tasks/actions",
        &task_action_for_locator(&goal_id, &["task_pause"]),
    )
    .await?;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 1);
    let snapshot =
        shacs_core::runtime::build_spec033_snapshot_from(root.path(), root.path(), "cli:direct")?;
    assert_eq!(
        snapshot.goal.fact.ok_or("missing goal fact")?.status,
        shacs_projection::Spec033GoalStatus::Paused
    );
    Ok(())
}

#[tokio::test]
async fn schema_mismatch_is_not_reported_as_capability_unavailable(
) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = Arc::new(CountingAdapter {
        workspace: PathBuf::from("/must-not-be-read"),
        owner_accesses: AtomicUsize::new(0),
    });
    let (status, body) = post(
        api_router_with_local_mutations(adapter.clone()),
        "/v1/tasks/actions",
        &task_action_with_schema(2, &["task_pause"]),
    )
    .await?;

    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let value: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["reason"], "schema_mismatch");
    assert!(value.get("error").is_none());
    assert_eq!(adapter.owner_accesses.load(Ordering::SeqCst), 0);
    Ok(())
}

fn task_action(capabilities: &[&str]) -> String {
    task_action_for_locator("goal:cli:direct", capabilities)
}

fn task_action_for_locator(locator: &str, capabilities: &[&str]) -> String {
    task_action_with_schema_and_locator(1, locator, capabilities)
}

fn task_action_with_schema(schema: u32, capabilities: &[&str]) -> String {
    task_action_with_schema_and_locator(schema, "goal:cli:direct", capabilities)
}

fn task_action_with_schema_and_locator(
    schema: u32,
    locator: &str,
    capabilities: &[&str],
) -> String {
    serde_json::json!({
        "owner": "goal",
        "action": "pause",
        "locator": locator,
        "session_id": "cli:direct",
        "transport_hello": {
            "client_id": "client:api",
            "schema_versions": [schema],
            "mutation_capabilities": capabilities,
        }
    })
    .to_string()
}

async fn post(
    app: axum::Router,
    path: &str,
    body: &str,
) -> Result<(axum::http::StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))?;
    let response = app.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 20).await?.to_vec();
    Ok((status, body))
}
