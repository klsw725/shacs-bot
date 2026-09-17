use super::live_tests::write_artifact;
use crate::{
    api_router_with_observer, ApiError, ChatCompletionAdapter, ChatCompletionInvocation,
    Spec031ChannelProjectionObserver,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shacs_channels::WebSocketServerEvent;
use shacs_projection::Spec031Envelope;
use shacs_providers::LlmResponse;
use socket2::SockRef;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_tungstenite::{client_async, tungstenite::Message, WebSocketStream};

struct ProgressAfterDisconnectAdapter {
    workspace: PathBuf,
    reached: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl ChatCompletionAdapter for ProgressAfterDisconnectAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(&self, _: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }

    fn process_websocket_frame_streaming(
        &self,
        _: Value,
        _: &str,
        default_chat_id: &str,
        on_event: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        self.reached.wait();
        self.release.wait();
        on_event(WebSocketServerEvent::Delta {
            chat_id: default_chat_id.to_owned(),
            text: "undelivered progress".to_owned(),
            stream_id: Some("stream:progress-failure".to_owned()),
        });
        Ok(())
    }
}

#[tokio::test]
async fn progress_send_failure_does_not_synthesize_terminal_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let adapter = Arc::new(ProgressAfterDisconnectAdapter {
        workspace: root.path().to_path_buf(),
        reached: reached.clone(),
        release: release.clone(),
    });
    let observations = Arc::new(Mutex::new(Vec::<Spec031Envelope>::new()));
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let dropped_tx = Arc::new(Mutex::new(Some(dropped_tx)));
    let observer = {
        let observations = observations.clone();
        Arc::new(move |envelope: Spec031Envelope| {
            let is_dropped = serde_json::to_value(&envelope)
                .is_ok_and(|value| value["capability"]["details"]["delivery"] == "dropped");
            if let Ok(mut observations) = observations.lock() {
                observations.push(envelope);
            }
            if is_dropped {
                if let Ok(mut sender) = dropped_tx.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(());
                    }
                }
            }
        }) as Spec031ChannelProjectionObserver
    };
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, api_router_with_observer(adapter, observer))
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let mut first = connect(addr).await?;
    let _ = read_json(&mut first).await?;
    first
        .send(Message::Text(
            json!({"type":"message","text":"progress-only"})
                .to_string()
                .into(),
        ))
        .await?;
    let reached_wait = reached.clone();
    tokio::task::spawn_blocking(move || reached_wait.wait()).await?;
    SockRef::from(first.get_ref()).set_linger(Some(Duration::ZERO))?;
    drop(first);
    tokio::task::spawn_blocking(move || release.wait()).await?;
    timeout(Duration::from_secs(5), dropped_rx).await??;

    let mut second = connect(addr).await?;
    let snapshot = read_json(&mut second).await?;
    let observed = observations
        .lock()
        .map_err(|_| "observations lock failed")?
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(observed
        .iter()
        .any(|value| value["capability"]["details"]["delivery"] == "dropped"));
    assert!(!observed
        .iter()
        .any(|value| value["capability"]["details"]["delivery"] == "final_failed"));
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["final_delivery"]["state"],
        "unknown"
    );
    write_artifact(
        "progress-failure-websocket.json",
        &json!({"observed_accounting": observed, "reconnect_snapshot": snapshot}),
    )?;

    second.close(None).await?;
    let _ = shutdown_tx.send(());
    server.await??;
    Ok(())
}

async fn connect(
    addr: std::net::SocketAddr,
) -> Result<WebSocketStream<TcpStream>, Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(addr).await?;
    let url = format!("ws://{addr}/ws?client_id=client:progress-failure&session_id=qa");
    Ok(client_async(url, stream).await?.0)
}

async fn read_json(
    socket: &mut WebSocketStream<TcpStream>,
) -> Result<Value, Box<dyn std::error::Error>> {
    match socket.next().await.ok_or("websocket closed")?? {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        _ => Err("expected text frame".into()),
    }
}
