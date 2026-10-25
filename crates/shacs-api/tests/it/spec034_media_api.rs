use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shacs_api::{
    handle_api_request, serve_api_listener, ApiError, ApiHttpRequest, ChatCompletionAdapter,
    ChatCompletionInvocation, MEDIA_DIAGNOSTICS_PATH, WEBSOCKET_PATH,
};
use shacs_providers::types::text_response;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const LEFTOVER_RECORD: &str = r#"{"schema_version":1,"kind":"media_capability","state":"extraction_failed","reason":{"code":"extraction_failed","safe_summary":"media extraction failed"},"lineage":{"artifact_ref":"spec034://media/artifact/7b1dfd5bb5d3258aa9d9c556171a55c5b2a8dc847bb042faa489277a53f627ee"},"freshness":"unavailable","disclosure":{"status":"unavailable"},"owner_facts":{"unavailable_reasons":["missing_analyzer_owner_ref"]}}"#;

struct LeftoverRecordAdapter {
    data_dir: PathBuf,
}

impl ChatCompletionAdapter for LeftoverRecordAdapter {
    fn configured_model(&self) -> &str {
        "fixture-model"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(text_response("unused"))
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.data_dir.clone())
    }
}

#[tokio::test]
async fn media_diagnostics_stay_unavailable_with_a_leftover_record() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let record = root
        .path()
        .join("media")
        .join("projections")
        .join("current.json");
    fs::create_dir_all(record.parent().ok_or("record parent")?)?;
    fs::write(&record, LEFTOVER_RECORD)?;
    let adapter = Arc::new(LeftoverRecordAdapter {
        data_dir: root.path().to_path_buf(),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(serve_api_listener(listener, adapter.clone(), async {
        let _ = shutdown_rx.await;
    }));

    // When
    let http = handle_api_request(
        ApiHttpRequest::get(MEDIA_DIAGNOSTICS_PATH),
        adapter.as_ref(),
    );
    let (mut websocket, _) = connect_async(format!("ws://{addr}{WEBSOCKET_PATH}")).await?;
    websocket
        .send(Message::Text(
            json!({"type": "media_projection"}).to_string().into(),
        ))
        .await?;
    let frame = websocket.next().await.ok_or("missing media frame")??;
    let websocket_reply: Value = serde_json::from_str(&frame.into_text()?)?;
    let _ = websocket.close(None).await;
    let _ = shutdown_tx.send(());
    server.await??;

    // Then
    assert_eq!(http.status, 404);
    assert_eq!(
        http.body["error"]["message"],
        "media projection is unavailable"
    );
    assert_eq!(websocket_reply["event"], "error");
    assert_eq!(websocket_reply["detail"], "media projection is unavailable");
    assert_eq!(fs::read_to_string(&record)?, LEFTOVER_RECORD);
    Ok(())
}
