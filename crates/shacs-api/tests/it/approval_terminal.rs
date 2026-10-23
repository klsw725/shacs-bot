use axum::body::{to_bytes, Body};
use shacs_api::{api_router, ChatCompletionAdapter};
use shacs_session::{
    PermissionApprovalReceipt, PermissionApprovalTerminalState, Session, SessionManager,
};
use std::{path::PathBuf, sync::Arc};
use tower::ServiceExt;

struct WorkspaceAdapter(PathBuf);

impl ChatCompletionAdapter for WorkspaceAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }
    fn complete_chat(
        &self,
        _: shacs_api::ChatCompletionInvocation,
    ) -> Result<shacs_providers::LlmResponse, shacs_api::ApiError> {
        unreachable!("session reads do not call providers")
    }
    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

#[tokio::test]
async fn session_detail_reads_same_terminal_receipt_without_pending_or_authority(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    let mut session = Session::new("cli:terminal");
    session.record_permission_approval_receipt(PermissionApprovalReceipt {
        approval_request_id: "approval_terminal".to_owned(),
        action_digest: "a".repeat(64),
        snapshot_digest: "b".repeat(64),
        state: PermissionApprovalTerminalState::Consumed,
        recorded_at_unix_ms: 123,
    });
    manager.save(&session)?;
    let router = api_router(Arc::new(WorkspaceAdapter(root.path().to_path_buf())));
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/sessions/cli%3Aterminal")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), 200);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await?)?;
    assert_eq!(
        value["permission_approval_receipts"],
        serde_json::json!(session.permission_approval_receipts())
    );
    assert!(value["metadata_keys"]
        .as_array()
        .ok_or("metadata keys")?
        .iter()
        .all(|key| key != "pending_permission_approval"));
    Ok(())
}
