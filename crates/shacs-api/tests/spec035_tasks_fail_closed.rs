use axum::body::{to_bytes, Body};
use serde_json::json;
use shacs_api::{api_router_with_local_mutations, ChatCompletionAdapter};
use shacs_providers::LlmResponse;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, SESSION_TURN_ACCEPTED,
};
use shacs_session::durable_replay::{
    evaluate_durable_recovery, DurableCheckpointStore, DurableRecoveryStatus,
};
use shacs_session::{Session, SessionManager};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;

struct WorkspaceAdapter {
    workspace: PathBuf,
    data_dir: PathBuf,
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
        Some(self.workspace.clone())
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.data_dir.clone())
    }
}

#[derive(Clone, Copy)]
enum RecoveryFixture {
    Healthy,
    Recoverable,
}

#[tokio::test]
async fn healthy_recovery_without_advertised_action_returns_non_success(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = task_fixture(RecoveryFixture::Healthy)?;
    let before = owner_bytes(root.path())?;

    // When
    let (status, body) = recover_request(root.path()).await?;

    // Then
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(error_message(&body)?, "tasks action is not advertised");
    assert_eq!(owner_bytes(root.path())?, before);
    Ok(())
}

#[tokio::test]
async fn recoverable_recovery_without_advertised_action_returns_non_success(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = task_fixture(RecoveryFixture::Recoverable)?;
    let admission = evaluate_durable_recovery(
        root.path().join("runtime/durable-events"),
        root.path().join("runtime/durable-checkpoints"),
    );
    assert_eq!(admission.status, DurableRecoveryStatus::Recoverable);
    let before = owner_bytes(root.path())?;

    // When
    let (status, body) = recover_request(root.path()).await?;

    // Then
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(error_message(&body)?, "tasks action is not advertised");
    assert_eq!(owner_bytes(root.path())?, before);
    Ok(())
}

fn task_fixture(kind: RecoveryFixture) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let mut manager = SessionManager::new(&workspace)?;
    manager.save(&Session::new("api:default"))?;
    let event_root = root.path().join("runtime/durable-events");
    let checkpoint_root = root.path().join("runtime/durable-checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    match kind {
        RecoveryFixture::Healthy => {}
        RecoveryFixture::Recoverable => {
            let mut input = DurableEventInput::new(
                "api:default",
                SESSION_TURN_ACCEPTED,
                DurableEventPayload::inline(
                    "orchestrator_fact",
                    json!({"content_hash":"sha256:one","media_count":0}),
                ),
            );
            input.turn_id = Some("turn-1".to_owned());
            events.append(input)?;
            let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
                .state
                .ok_or("missing replay state")?;
            let checkpoints = DurableCheckpointStore::open(&checkpoint_root)?;
            checkpoints.write(&state)?;
            let checkpoint = checkpoints
                .candidate_paths()?
                .into_iter()
                .next()
                .ok_or("missing checkpoint")?;
            std::fs::write(checkpoint, b"{malformed")?;
        }
    }
    Ok(root)
}

async fn recover_request(
    root: &Path,
) -> Result<(axum::http::StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/tasks/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"owner":"recovery","action":"recover","locator":"runtime:recovery","session_id":"api:default","transport_hello":{"client_id":"client:tasks-api","schema_versions":[1],"mutation_capabilities":["task_recover"]}}"#,
        ))?;
    let response = api_router_with_local_mutations(Arc::new(WorkspaceAdapter {
        workspace: root.join("workspace"),
        data_dir: root.to_path_buf(),
    }))
    .oneshot(request)
    .await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 20).await?.to_vec();
    Ok((status, body))
}

fn owner_bytes(root: &Path) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
    let events = std::fs::read(root.join("runtime/durable-events/events.log"))?;
    let checkpoints = DurableCheckpointStore::open(root.join("runtime/durable-checkpoints"))?;
    let mut bytes = vec![events];
    for path in checkpoints.candidate_paths()? {
        bytes.push(std::fs::read(path)?);
    }
    Ok(bytes)
}

fn error_message(body: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_slice(body)?;
    value["error"]["message"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "missing error message".into())
}
