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
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::{client_async, tungstenite::Message, WebSocketStream};

struct SlowAdapter {
    workspace: PathBuf,
    progress_sent: Arc<Barrier>,
    release_final: Arc<Barrier>,
}

impl ChatCompletionAdapter for SlowAdapter {
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
        on_event(WebSocketServerEvent::Delta {
            chat_id: default_chat_id.to_owned(),
            text: "progress".to_owned(),
            stream_id: Some("stream:slow".to_owned()),
        });
        self.progress_sent.wait();
        self.release_final.wait();
        on_event(WebSocketServerEvent::Message {
            chat_id: default_chat_id.to_owned(),
            text: "final".to_owned(),
            buttons: Vec::new(),
            button_prompt: None,
            media: Vec::new(),
            reply_to: None,
            kind: None,
        });
        Ok(())
    }
}

#[tokio::test]
async fn slow_websocket_reconnect_keeps_progress_and_final_failure_independent(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: progress is delivered before an explicitly gated final event.
    let root = tempfile::tempdir()?;
    let progress_sent = Arc::new(Barrier::new(2));
    let release_final = Arc::new(Barrier::new(2));
    let adapter = Arc::new(SlowAdapter {
        workspace: root.path().to_path_buf(),
        progress_sent: progress_sent.clone(),
        release_final: release_final.clone(),
    });
    let (failed_tx, mut failed_rx) = mpsc::channel(4);
    let final_failure = Arc::new(Mutex::new(None));
    let observed_final_failure = final_failure.clone();
    let observer = Arc::new(move |envelope: Spec031Envelope| {
        if let Ok(value) = serde_json::to_value(&envelope) {
            if value["capability"]["details"]["delivery"] != "final_failed" {
                return;
            }
            if let Ok(mut observed) = observed_final_failure.lock() {
                *observed = Some(value.clone());
            }
            failed_tx
                .try_send(value)
                .expect("bounded final observations");
        }
    }) as Spec031ChannelProjectionObserver;
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

    // When: the slow client stops before final delivery, then reconnects.
    let mut first = connect(addr).await?;
    let first_snapshot = read_json(&mut first).await?;
    assert_eq!(first_snapshot["type"], "tasks_snapshot");
    assert_eq!(first_snapshot["metadata"]["generation"], "generation:1");
    let mut ports = vec![first.get_ref().local_addr()?.port()];
    first
        .send(Message::Text(
            json!({"type":"message","text":"slow"}).to_string().into(),
        ))
        .await?;
    let raw_progress = read_json(&mut first).await?;
    let progress_accounting = read_json(&mut first).await?;
    let reached = progress_sent.clone();
    tokio::task::spawn_blocking(move || reached.wait()).await?;
    SockRef::from(first.get_ref()).set_linger(Some(Duration::ZERO))?;
    drop(first);
    let release = release_final.clone();
    tokio::task::spawn_blocking(move || release.wait()).await?;
    timeout(Duration::from_secs(5), failed_rx.recv())
        .await?
        .ok_or("missing final failure")?;
    let mut second = connect(addr).await?;
    let snapshot = read_json(&mut second).await?;
    ports.push(second.get_ref().local_addr()?.port());
    assert_eq!(snapshot["metadata"]["generation"], "generation:2");
    let final_failure = final_failure
        .lock()
        .map_err(|_| "final failure observation lock failed")?
        .clone()
        .ok_or("final failure observation missing")?;

    // Then: queue bounds/progress survive and final failure stays authoritative.
    assert_eq!(raw_progress["event"], "delta");
    assert_eq!(
        progress_accounting["capability"]["details"]["queue_capacity"],
        64
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["progress"],
        json!(["live"])
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["emitted_count"]["value"],
        1
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["final_delivery"]["state"],
        "final_failed"
    );
    assert_eq!(final_failure["capability"]["details"]["dropped"], 1);
    assert_eq!(final_failure["capability"]["details"]["slow_consumer"], 1);
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["coalesced_count"]["availability"],
        "unavailable"
    );
    let mut snapshots = vec![first_snapshot, snapshot.clone()];
    let mut rounds = vec![
        json!({"progress": raw_progress, "accounting": progress_accounting, "failure": final_failure}),
    ];
    for generation in 3..=4 {
        second
            .send(Message::Text(
                json!({"type":"message","text":"slow"}).to_string().into(),
            ))
            .await?;
        let progress = read_json(&mut second).await?;
        let accounting = read_json(&mut second).await?;
        let reached = progress_sent.clone();
        tokio::task::spawn_blocking(move || reached.wait()).await?;
        SockRef::from(second.get_ref()).set_linger(Some(Duration::ZERO))?;
        drop(second);
        let release = release_final.clone();
        tokio::task::spawn_blocking(move || release.wait()).await?;
        let failure = timeout(Duration::from_secs(5), failed_rx.recv())
            .await?
            .ok_or("missing repeated final failure")?;
        second = connect(addr).await?;
        ports.push(second.get_ref().local_addr()?.port());
        let next = read_json(&mut second).await?;
        assert_eq!(next["type"], "tasks_snapshot");
        assert_eq!(
            next["metadata"]["generation"],
            format!("generation:{generation}")
        );
        assert_eq!(next["reconnect_gap"], true);
        assert_eq!(progress["stream_id"], "stream:slow");
        let delivery = &next["delivery_accounting"]["delivery"];
        assert_eq!(delivery["emitted_count"]["value"], generation - 1);
        assert_eq!(delivery["accepted_count"]["value"], generation - 1);
        assert_eq!(delivery["dropped_count"]["value"], 0);
        assert_eq!(
            delivery["coalesced_count"],
            snapshot["delivery_accounting"]["delivery"]["coalesced_count"]
        );
        assert_eq!(delivery["final_delivery"]["state"], "final_failed");
        assert_eq!(failure["capability"]["details"]["dropped"], 1);
        assert_eq!(
            failure["capability"]["details"]["reconnect_generation"],
            generation - 1
        );
        snapshots.push(next);
        rounds.push(json!({"progress": progress, "accounting": accounting, "failure": failure}));
    }
    second.close(None).await?;
    let _ = shutdown_tx.send(());
    server.await??;
    write_artifact(
        "slow-websocket.json",
        &json!({
            "progress_frame": raw_progress,
            "progress_accounting": progress_accounting,
            "final_failure_accounting": final_failure,
            "reconnect_snapshot": snapshot,
            "server": addr.to_string(), "source_ports": ports,
            "client_id": "client:slow", "session_id": "qa", "stream_id": "stream:slow",
            "rounds": rounds, "snapshots": snapshots, "server_shutdown_awaited": true,
        }),
    )?;

    Ok(())
}

pub(super) fn write_artifact(name: &str, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = env::var_os("SPEC035_ACCOUNTING_ARTIFACT_DIR") else {
        return Ok(());
    };
    fs::create_dir_all(&root)?;
    fs::write(
        PathBuf::from(root).join(name),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}

pub(super) async fn connect(
    addr: std::net::SocketAddr,
) -> Result<WebSocketStream<TcpStream>, Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(addr).await?;
    let url = format!("ws://{addr}/ws?client_id=client:slow&session_id=qa");
    Ok(client_async(url, stream).await?.0)
}

pub(super) async fn read_json(
    socket: &mut WebSocketStream<TcpStream>,
) -> Result<Value, Box<dyn std::error::Error>> {
    match socket.next().await.ok_or("websocket closed")?? {
        Message::Text(text) => {
            capture::capture_payload(socket, &text)?;
            Ok(serde_json::from_str(&text)?)
        }
        _ => Err("expected text frame".into()),
    }
}
#[path = "../../tests/spec035_accounting_support/mod.rs"]
mod capture;
