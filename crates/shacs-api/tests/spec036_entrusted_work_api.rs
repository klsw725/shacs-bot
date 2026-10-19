use axum::body::{to_bytes, Body};
use serde_json::{json, Value};
use shacs_api::{api_router, api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_core::runtime::{EntrustedWorkSnapshot, PersistentGoalStatus};
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
        unreachable!("entrusted work routes do not complete chat")
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

async fn request(
    app: axum::Router,
    method: &str,
    path: &str,
    body: Value,
) -> Result<(axum::http::StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let response = app.oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await?.to_vec();
    Ok((status, bytes))
}

fn create_body() -> Value {
    json!({
        "goal_text": "watch the release",
        "turn_budget": 8,
        "consent": {
            "kind": "explicit_request",
            "evidence_ref": "api:user-command",
            "recorded_at": "100"
        },
        "conditions": {
            "instructions": "check the release page",
            "budget": "daily",
            "prior_decisions": [],
            "remaining_questions": []
        },
        "linked_targets": ["api:linked"],
        "observed_at": "100"
    })
}

#[tokio::test]
async fn opted_in_router_persists_work_lifecycle_across_router_restart(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("api:owner"))?;
    let adapter = Arc::new(WorkspaceAdapter(workspace));

    // When
    let (status, bytes) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        "/v1/sessions/api%3Aowner/entrusted-work",
        create_body(),
    )
    .await?;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let created: EntrustedWorkSnapshot = serde_json::from_slice(&bytes)?;
    let work_id = created.work.work_id;
    let revise_path = format!("/v1/sessions/api%3Aowner/entrusted-work/{work_id}/revise");
    let (revised_status, _) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        &revise_path,
        json!({
            "expected_revision": 1,
            "conditions": {"instructions": "check release and changelog"},
            "changed_at": "200"
        }),
    )
    .await?;
    assert_eq!(revised_status, axum::http::StatusCode::OK);
    for action in ["pause", "resume", "done"] {
        let path = format!("/v1/sessions/api%3Aowner/entrusted-work/{work_id}/{action}");
        let (transition_status, _) = request(
            api_router_with_local_mutations(adapter.clone()),
            "POST",
            &path,
            json!({"observed_at": format!("300-{action}")}),
        )
        .await?;
        assert_eq!(transition_status, axum::http::StatusCode::OK);
    }
    let read_path = format!("/v1/sessions/api%3Aowner/entrusted-work/{work_id}");
    let (read_status, read_bytes) = request(
        api_router_with_local_mutations(adapter.clone()),
        "GET",
        &read_path,
        json!({}),
    )
    .await?;

    // Then
    assert_eq!(read_status, axum::http::StatusCode::OK);
    let restarted: EntrustedWorkSnapshot = serde_json::from_slice(&read_bytes)?;
    assert_eq!(restarted.work.work_id, work_id);
    assert_eq!(restarted.work.current_revision, 2);
    assert_eq!(restarted.goal_status, PersistentGoalStatus::Done);
    let (second_status, second_bytes) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        "/v1/sessions/api%3Aowner/entrusted-work",
        json!({
            "goal_text": "clearable work",
            "consent": {
                "kind": "confirmed_proposal",
                "evidence_ref": "api:confirmed-proposal",
                "recorded_at": "400"
            },
            "conditions": {"instructions": "clear this work"},
            "observed_at": "400"
        }),
    )
    .await?;
    assert_eq!(second_status, axum::http::StatusCode::CREATED);
    let second: EntrustedWorkSnapshot = serde_json::from_slice(&second_bytes)?;
    let clear_path = format!(
        "/v1/sessions/api%3Aowner/entrusted-work/{}/clear",
        second.work.work_id
    );
    let (clear_status, clear_bytes) = request(
        api_router_with_local_mutations(adapter),
        "POST",
        &clear_path,
        json!({"observed_at": "500"}),
    )
    .await?;
    assert_eq!(clear_status, axum::http::StatusCode::OK);
    let cleared: EntrustedWorkSnapshot = serde_json::from_slice(&clear_bytes)?;
    assert_eq!(cleared.goal_status, PersistentGoalStatus::Cleared);
    Ok(())
}

#[tokio::test]
async fn requester_url_identity_enforces_links_and_stale_revision_preserves_data(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("api:owner"))?;
    manager.save(&Session::new("api:linked"))?;
    manager.save(&Session::new("api:stranger"))?;
    let adapter = Arc::new(WorkspaceAdapter(workspace));
    let (_, bytes) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        "/v1/sessions/api%3Aowner/entrusted-work",
        create_body(),
    )
    .await?;
    let created: EntrustedWorkSnapshot = serde_json::from_slice(&bytes)?;
    let work_id = created.work.work_id;

    // When
    let linked_path = format!("/v1/sessions/api%3Alinked/entrusted-work/api%3Aowner/{work_id}");
    let stranger_path = format!("/v1/sessions/api%3Astranger/entrusted-work/api%3Aowner/{work_id}");
    let (linked_status, _) = request(
        api_router_with_local_mutations(adapter.clone()),
        "GET",
        &linked_path,
        json!({"requester_target": "api:stranger"}),
    )
    .await?;
    let (stranger_status, _) = request(
        api_router_with_local_mutations(adapter.clone()),
        "GET",
        &stranger_path,
        json!({"requester_target": "api:linked"}),
    )
    .await?;
    let stale_path = format!("{linked_path}/revise");
    let (stale_status, _) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        &stale_path,
        json!({
            "expected_revision": 0,
            "conditions": {"instructions": "must not replace current"},
            "changed_at": "200"
        }),
    )
    .await?;
    let (_, current_bytes) = request(
        api_router_with_local_mutations(adapter),
        "GET",
        &linked_path,
        json!({}),
    )
    .await?;

    // Then
    assert_eq!(linked_status, axum::http::StatusCode::OK);
    assert_eq!(stranger_status, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(stale_status, axum::http::StatusCode::CONFLICT);
    let current: EntrustedWorkSnapshot = serde_json::from_slice(&current_bytes)?;
    assert_eq!(current.work.current_revision, 1);
    assert_eq!(
        current
            .work
            .current_conditions()
            .ok_or("current conditions")?
            .instructions,
        "check the release page"
    );
    Ok(())
}

#[tokio::test]
async fn default_router_and_bad_inputs_fail_before_mutation(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("api:owner"))?;
    let adapter = Arc::new(WorkspaceAdapter(workspace));

    // When
    let (forbidden, _) = request(
        api_router(adapter.clone()),
        "POST",
        "/v1/sessions/api%3Aowner/entrusted-work",
        create_body(),
    )
    .await?;
    let (invalid, _) = request(
        api_router_with_local_mutations(adapter.clone()),
        "POST",
        "/v1/sessions/api%3Aowner/entrusted-work",
        json!({"goal_text": "missing consent and conditions"}),
    )
    .await?;
    let (missing, _) = request(
        api_router_with_local_mutations(adapter),
        "POST",
        "/v1/sessions/api%3Amissing/entrusted-work",
        create_body(),
    )
    .await?;

    // Then
    assert_eq!(forbidden, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(invalid, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(missing, axum::http::StatusCode::NOT_FOUND);
    Ok(())
}
