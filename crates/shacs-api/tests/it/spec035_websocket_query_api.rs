use futures_util::StreamExt;
use serde_json::{json, Value};
use shacs_api::{serve_api_listener, ApiError, ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_providers::LlmResponse;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite};

struct SnapshotAdapter(PathBuf);

impl ChatCompletionAdapter for SnapshotAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(&self, _: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        Err(ApiError::not_implemented("snapshot-only fixture"))
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

#[tokio::test]
async fn websocket_query_encoded_and_raw_ids_share_snapshot_reconnect_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut manager = shacs_session::SessionManager::new(root.path())?;
    manager.save(&shacs_session::Session::new("cli:direct"))?;
    shacs_core::runtime::apply_goal_surface_action(
        root.path(),
        "cli:direct",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "verify query identity".to_owned(),
            turn_budget: 8,
        },
        "1",
    )?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve_api_listener(
        listener,
        Arc::new(SnapshotAdapter(root.path().to_path_buf())),
        async {
            let _ = shutdown_rx.await;
        },
    ));
    let expected = serde_json::to_value(shacs_core::runtime::build_spec035_tasks_projection(
        root.path(),
        root.path(),
        "cli:direct",
    )?)?;
    assert!(!expected["rows"].as_array().ok_or("tasks rows")?.is_empty());

    for (query, generation) in [
        ("client_id=client:api&session_id=cli:direct", 1),
        ("client_id=client%3Aapi&session_id=cli%3Adirect", 2),
        ("client%5Fid=client%3aapi&session%5fid=%63li%3adirect", 3),
        ("client_id=client:api&session_id=cli:other", 1),
        ("client_id=client:other&session_id=cli:direct", 1),
        ("client_id=client:api:cli&session_id=direct", 1),
        ("client_id=client:api&session_id=cli:direct", 4),
    ] {
        let url = format!("ws://{addr}/ws?{query}");
        println!("handshake {url}");
        let (mut socket, response) = timeout(Duration::from_secs(5), connect_async(&url)).await??;
        assert_eq!(response.status(), 101);
        let frame = timeout(Duration::from_secs(5), socket.next())
            .await?
            .ok_or("websocket closed before snapshot")??;
        let snapshot: Value = serde_json::from_str(frame.to_text()?)?;
        socket.close(None).await?;
        println!(
            "{}",
            json!({"url": url, "status": 101, "snapshot": snapshot})
        );
        assert_eq!(snapshot["type"], "tasks_snapshot");
        assert_eq!(snapshot["metadata"]["sequence"], 1);
        assert_eq!(
            snapshot["metadata"]["generation"],
            format!("generation:{generation}")
        );
        assert_eq!(snapshot["reconnect_gap"], generation > 1);
        if generation > 1 || query == "client_id=client:api&session_id=cli:direct" {
            assert_eq!(snapshot["payload"], expected);
        }
    }

    let _ = shutdown_tx.send(());
    server.await??;
    Ok(())
}

#[tokio::test]
async fn websocket_query_rejects_malformed_duplicate_and_invalid_decoded_ids(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve_api_listener(
        listener,
        Arc::new(SnapshotAdapter(root.path().to_path_buf())),
        async {
            let _ = shutdown_rx.await;
        },
    ));

    let oversized = "%61".repeat(129);
    let mut queries = vec![
        "client_id=client%&session_id=cli:direct".to_owned(),
        "client_id=client%3&session_id=cli:direct".to_owned(),
        "client_id=client%GG&session_id=cli:direct".to_owned(),
        "client_id=client:api&session_id=cli%FFdirect".to_owned(),
        "client_id=client:api&session_id=cli:direct&ignored=%GG".to_owned(),
        "client%GGid=client:api&session_id=cli:direct".to_owned(),
        "client_id=client:api&client_id=client:other&session_id=cli:direct".to_owned(),
        "client_id=client:api&client%5Fid=client:api&session_id=cli:direct".to_owned(),
        "client%5Fid=client:api&client_id=client:other&session_id=cli:direct".to_owned(),
        "client_id=client:api&session_id=cli:direct&session%5fid=cli:direct".to_owned(),
        "client_id=client:api".to_owned(),
        "session_id=cli:direct".to_owned(),
        "client_id=client:api&session_id".to_owned(),
        "client%255Fid=client:api&session_id=cli:direct".to_owned(),
        "client_id=client:api&session%255Fid=cli:direct".to_owned(),
    ];
    for invalid in [
        "", "%253A", "%2F", "%26", "%3D", "%00", "%0A", "%20", "+", "%2B", "%C3%A9", &oversized,
    ] {
        queries.push(format!("client_id={invalid}&session_id=cli:direct"));
        queries.push(format!("client_id=client:api&session_id={invalid}"));
    }
    for query in queries {
        let url = format!("ws://{addr}/ws?{query}");
        let error = timeout(Duration::from_secs(5), connect_async(&url))
            .await?
            .expect_err(&format!("query must fail closed: {query}"));
        let tungstenite::Error::Http(response) = error else {
            return Err(format!("expected HTTP rejection for {url}: {error}").into());
        };
        let body: Value = serde_json::from_slice(response.body().as_deref().ok_or("error body")?)?;
        println!(
            "{}",
            json!({"url": url, "status": response.status().as_u16(), "body": body})
        );
        assert_eq!(response.status(), 400, "{query}");
        assert_eq!(body["error"]["type"], "invalid_request_error", "{query}");
        assert_eq!(body["error"]["code"], 400, "{query}");
    }

    let _ = shutdown_tx.send(());
    server.await??;
    Ok(())
}
