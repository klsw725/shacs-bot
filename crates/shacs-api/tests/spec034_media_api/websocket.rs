use crate::support::{projection_for, MediaAdapter, StoreMediaAdapter};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shacs_api::{
    handle_api_request, serve_api_listener, ApiHttpRequest, MEDIA_DIAGNOSTICS_PATH, WEBSOCKET_PATH,
};
use std::error::Error;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn websocket_media_projection_matches_http_and_omits_raw_material(
) -> Result<(), Box<dyn Error>> {
    let projection = projection_for("included")?;
    let expected = serde_json::to_value(&projection)?;
    let adapter = Arc::new(MediaAdapter {
        projection: Some(projection),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(serve_api_listener(listener, adapter, async {
        let _ = shutdown_rx.await;
    }));
    let (mut websocket, _) = connect_async(format!("ws://{addr}{WEBSOCKET_PATH}")).await?;

    websocket
        .send(Message::Text(
            json!({"type": "media_projection"}).to_string().into(),
        ))
        .await?;
    let frame = websocket.next().await.ok_or("missing media frame")??;
    let actual: Value = serde_json::from_str(&frame.into_text()?)?;

    assert_eq!(actual, expected);
    for forbidden in [
        "data:image",
        "base64",
        "?token=",
        "provider raw body",
        "/Users/",
    ] {
        assert!(!actual.to_string().contains(forbidden), "{forbidden}");
    }
    let _ = websocket.close(None).await;
    let _ = shutdown_tx.send(());
    server.await??;
    Ok(())
}

#[tokio::test]
async fn websocket_media_projection_reports_unavailable_without_inventing_success(
) -> Result<(), Box<dyn Error>> {
    // Given
    let adapter = Arc::new(MediaAdapter { projection: None });
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(serve_api_listener(listener, adapter, async {
        let _ = shutdown_rx.await;
    }));
    let (mut websocket, _) = connect_async(format!("ws://{addr}{WEBSOCKET_PATH}")).await?;

    // When
    websocket
        .send(Message::Text(
            json!({"type": "media_projection"}).to_string().into(),
        ))
        .await?;
    let frame = websocket.next().await.ok_or("missing media frame")??;
    let actual: Value = serde_json::from_str(&frame.into_text()?)?;
    let _ = websocket.close(None).await;
    let _ = shutdown_tx.send(());
    server.await??;

    // Then
    assert_eq!(actual["event"], "error");
    assert_eq!(actual["chat_id"], "default");
    assert_eq!(actual["detail"], "media projection is unavailable");
    assert!(!actual.to_string().contains("included"));
    Ok(())
}

#[tokio::test]
async fn production_store_backs_http_and_websocket_without_projection_override(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let projection = projection_for("included")?;
    shacs_core::runtime::Spec035MediaProjectionStore::new(root.path()).publish(&projection)?;
    let adapter = Arc::new(StoreMediaAdapter {
        data_dir: root.path().to_path_buf(),
    });
    let expected = serde_json::to_value(&projection)?;
    let http = handle_api_request(
        ApiHttpRequest::get(MEDIA_DIAGNOSTICS_PATH),
        adapter.as_ref(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(serve_api_listener(listener, adapter, async {
        let _ = shutdown_rx.await;
    }));

    // When
    let (mut websocket, _) = connect_async(format!("ws://{addr}{WEBSOCKET_PATH}")).await?;
    websocket
        .send(Message::Text(
            json!({"type": "media_projection"}).to_string().into(),
        ))
        .await?;
    let frame = websocket.next().await.ok_or("missing media frame")??;
    let websocket_projection: Value = serde_json::from_str(&frame.into_text()?)?;
    let _shutdown = shutdown_tx.send(());
    server.await??;

    // Then
    assert_eq!(http.status, 200);
    assert_eq!(http.body, expected);
    assert_eq!(websocket_projection, expected);
    Ok(())
}
