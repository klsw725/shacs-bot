use axum::body::{to_bytes, Body};
use shacs_api::{api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_projection::{Spec033Availability, Spec033GoalStatus, Spec033Snapshot};
use shacs_providers::LlmResponse;
use shacs_session::{Session, SessionManager};
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

struct WorkspaceAdapter(PathBuf);

impl ChatCompletionAdapter for WorkspaceAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }
    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<LlmResponse, shacs_api::ApiError> {
        unreachable!("goal routes do not complete chat")
    }
    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
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
async fn actual_router_exercises_goal_lifecycle_and_reads_persisted_snapshot(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("cli:direct"))?;
    let adapter = Arc::new(WorkspaceAdapter(workspace));

    // When
    for (action, body) in [
        ("set", r#"{"text":"ship it"}"#),
        ("pause", "{}"),
        ("resume", "{}"),
        ("blocked", r#"{"reason":"needs input"}"#),
        ("resume", "{}"),
        ("done", "{}"),
    ] {
        let (status, _) = request(
            api_router_with_local_mutations(adapter.clone()),
            "POST",
            &format!("/v1/sessions/cli%3Adirect/goal/{action}"),
            body,
        )
        .await?;
        assert_eq!(status, axum::http::StatusCode::OK);
    }
    let (status, bytes) = request(
        api_router_with_local_mutations(adapter),
        "GET",
        "/v1/sessions/cli%3Adirect/goal-snapshot",
        "",
    )
    .await?;

    // Then
    assert_eq!(status, axum::http::StatusCode::OK);
    let snapshot: Spec033Snapshot = serde_json::from_slice(&bytes)?;
    assert_eq!(snapshot.goal.availability, Spec033Availability::Available);
    let fact = snapshot.goal.fact.ok_or("goal fact")?;
    assert_eq!(fact.status, Spec033GoalStatus::Done);
    assert_eq!(fact.stop_reason.as_deref(), Some("marked_done_by_user"));
    assert_eq!(fact.budget.remaining_turns, 8);
    assert!(!fact.user_interrupted);
    assert!(fact.latest_transition.is_some());
    assert_eq!(
        snapshot.automation.availability,
        Spec033Availability::Unavailable
    );
    assert_eq!(
        snapshot.replay.availability,
        Spec033Availability::Unavailable
    );
    Ok(())
}
