use axum::body::{to_bytes, Body};
use serde_json::{json, Value};
use shacs_api::{
    api_router, api_router_with_local_mutations, ApiError, ChatCompletionAdapter,
    ChatCompletionInvocation,
};
use shacs_providers::LlmResponse;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

struct ImprovementAdapter(PathBuf);

impl ChatCompletionAdapter for ImprovementAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(&self, _: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        unreachable!("improvement routes do not complete chat")
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }

    fn local_improvement(
        &self,
        action: &str,
        proposal_id: &str,
        body: Value,
    ) -> Result<Value, ApiError> {
        Ok(json!({"action": action, "proposal_id": proposal_id, "body": body}))
    }
}

#[tokio::test]
async fn actual_router_exposes_all_local_improvement_actions(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let app =
        api_router_with_local_mutations(Arc::new(ImprovementAdapter(root.path().to_path_buf())));

    // When / Then
    for (method, action, request_body) in [
        ("POST", "propose", r#"{"target_ref":"settings.json"}"#),
        ("GET", "inspect", "{}"),
        ("POST", "apply", "{}"),
        ("POST", "verify", "{}"),
        ("GET", "candidate", "{}"),
        ("POST", "rollback", "{}"),
    ] {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(format!("/v1/improvements/proposal%3A1/{action}"))
            .header("content-type", "application/json")
            .body(Body::from(request_body))?;
        let response = app.clone().oneshot(request).await?;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await?)?;
        assert_eq!(body["action"], action);
        assert_eq!(body["proposal_id"], "proposal%3A1");
        if action == "propose" {
            assert_eq!(body["body"]["target_ref"], "settings.json");
        }
    }
    Ok(())
}

#[tokio::test]
async fn default_router_denies_local_improvement_mutations(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let app = api_router(Arc::new(ImprovementAdapter(root.path().to_path_buf())));
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/improvements/proposal%3A1/apply")
        .header("content-type", "application/json")
        .body(Body::from("{}"))?;

    // When
    let response = app.oneshot(request).await?;

    // Then
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    Ok(())
}
