use super::live_tests::{connect, read_json, write_artifact};
use crate::{
    api_router_with_observer, ApiError, ChatCompletionAdapter, ChatCompletionInvocation,
    Spec031ChannelProjectionObserver,
};
use futures_util::SinkExt;
use serde_json::{json, Value};
use shacs_channels::WebSocketServerEvent;
use shacs_projection::Spec031Envelope;
use shacs_providers::LlmResponse;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::Message;

const BURST_EVENTS: usize = 96;
const BURST_TEXT_BYTES: usize = 256 * 1024;

struct QueueBurstAdapter {
    workspace: PathBuf,
    burst_complete: Arc<Barrier>,
    release_final: Arc<Barrier>,
    text: String,
}

impl ChatCompletionAdapter for QueueBurstAdapter {
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
        for _ in 0..BURST_EVENTS {
            on_event(WebSocketServerEvent::Delta {
                chat_id: default_chat_id.to_owned(),
                text: self.text.clone(),
                stream_id: Some("stream:queue".to_owned()),
            });
        }
        self.burst_complete.wait();
        self.release_final.wait();
        on_event(WebSocketServerEvent::Message {
            chat_id: default_chat_id.to_owned(),
            text: "final after bounded drops".to_owned(),
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
async fn production_slow_consumer_preserves_dropped_progress_and_delivered_final(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let burst_complete = Arc::new(Barrier::new(2));
    let release_final = Arc::new(Barrier::new(2));
    let adapter = Arc::new(QueueBurstAdapter {
        workspace: root.path().to_path_buf(),
        burst_complete: burst_complete.clone(),
        release_final: release_final.clone(),
        text: "x".repeat(BURST_TEXT_BYTES),
    });
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let dropped_tx = Arc::new(Mutex::new(Some(dropped_tx)));
    let (final_tx, final_rx) = oneshot::channel();
    let final_tx = Arc::new(Mutex::new(Some(final_tx)));
    let observations = Arc::new(Mutex::new(Vec::new()));
    let observed = observations.clone();
    let observer = Arc::new(move |envelope: Spec031Envelope| {
        if let Ok(value) = serde_json::to_value(envelope) {
            observed
                .lock()
                .expect("owner observations lock")
                .push(value.clone());
            let delivery = value["capability"]["details"]["delivery"].as_str();
            let sender = match delivery {
                Some("dropped") => Some(&dropped_tx),
                Some("final_delivered") => Some(&final_tx),
                _ => None,
            };
            if let Some(sender) = sender {
                if let Ok(mut sender) = sender.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(());
                    }
                }
            }
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

    let mut first = connect(addr).await?;
    let first_snapshot = read_json(&mut first).await?;
    assert_eq!(first_snapshot["metadata"]["generation"], "generation:1");
    let mut ports = vec![first.get_ref().local_addr()?.port()];
    first
        .send(Message::Text(
            json!({"type":"message","text":"burst"}).to_string().into(),
        ))
        .await?;
    let reached = burst_complete.clone();
    tokio::task::spawn_blocking(move || reached.wait()).await?;
    timeout(Duration::from_secs(5), dropped_rx).await??;
    let release = release_final.clone();
    tokio::task::spawn_blocking(move || release.wait()).await?;

    let mut delivered_deltas = 0_u64;
    loop {
        let frame = timeout(Duration::from_secs(5), read_json(&mut first)).await??;
        if frame["event"] == "delta" {
            assert_eq!(frame["stream_id"], "stream:queue");
            delivered_deltas = delivered_deltas.saturating_add(1);
        }
        if frame["capability"]["details"]["delivery"] == "final_delivered" {
            break;
        }
    }
    timeout(Duration::from_secs(5), final_rx).await??;
    first.close(None).await?;

    let mut second = connect(addr).await?;
    let snapshot = read_json(&mut second).await?;
    ports.push(second.get_ref().local_addr()?.port());
    assert_eq!(snapshot["metadata"]["generation"], "generation:2");
    let delivery = &snapshot["delivery_accounting"]["delivery"];
    assert_eq!(delivery["progress"], json!(["dropped"]));
    assert!(delivery["dropped_count"]["value"]
        .as_u64()
        .is_some_and(|value| value > 0));
    assert_eq!(delivery["final_delivery"]["state"], "final_delivered");
    assert_eq!(delivery["coalesced_count"]["availability"], "unavailable");
    assert_eq!(delivery["accepted_count"]["value"], delivered_deltas);
    assert_eq!(delivery["emitted_count"]["value"], delivered_deltas);
    let first_dropped = delivery["dropped_count"]["value"]
        .as_u64()
        .ok_or("missing drops")?;
    assert_eq!(
        delivered_deltas + first_dropped,
        u64::try_from(BURST_EVENTS)?
    );
    let mut snapshots = vec![first_snapshot, snapshot.clone()];
    let mut counts = vec![delivered_deltas];
    let mut total_emitted = delivered_deltas;
    let mut prior_dropped = first_dropped;
    for generation in 3..=4 {
        second
            .send(Message::Text(
                json!({"type":"message","text":"burst"}).to_string().into(),
            ))
            .await?;
        let reached = burst_complete.clone();
        tokio::task::spawn_blocking(move || reached.wait()).await?;
        let release = release_final.clone();
        tokio::task::spawn_blocking(move || release.wait()).await?;
        let mut emitted = 0_u64;
        loop {
            let frame = timeout(Duration::from_secs(5), read_json(&mut second)).await??;
            if frame["event"] == "delta" {
                assert_eq!(frame["stream_id"], "stream:queue");
                emitted += 1;
            }
            if frame["capability"]["details"]["delivery"] == "final_delivered" {
                break;
            }
        }
        total_emitted += emitted;
        counts.push(emitted);
        second.close(None).await?;
        second = connect(addr).await?;
        ports.push(second.get_ref().local_addr()?.port());
        let next = read_json(&mut second).await?;
        assert_eq!(next["type"], "tasks_snapshot");
        assert_eq!(
            next["metadata"]["generation"],
            format!("generation:{generation}")
        );
        assert_eq!(next["reconnect_gap"], true);
        let delivery = &next["delivery_accounting"]["delivery"];
        assert_eq!(delivery["progress"], json!(["dropped"]));
        assert_eq!(delivery["accepted_count"]["value"], total_emitted);
        assert_eq!(delivery["emitted_count"]["value"], total_emitted);
        let dropped = delivery["dropped_count"]["value"]
            .as_u64()
            .ok_or("missing cumulative drops")?;
        assert!(dropped > prior_dropped);
        assert_eq!(
            total_emitted + dropped,
            u64::try_from(BURST_EVENTS)? * (generation - 1)
        );
        assert_eq!(
            delivery["coalesced_count"],
            snapshot["delivery_accounting"]["delivery"]["coalesced_count"]
        );
        assert_eq!(delivery["final_delivery"]["state"], "final_delivered");
        prior_dropped = dropped;
        snapshots.push(next);
    }
    second.close(None).await?;
    let _ = shutdown_tx.send(());
    server.await??;
    write_artifact(
        "production-slow-websocket.json",
        &json!({
            "burst_events": BURST_EVENTS,
            "queue_capacity": 64,
            "delivered_deltas": delivered_deltas,
            "reconnect_snapshot": snapshot,
            "server": addr.to_string(), "source_ports": ports,
            "client_id": "client:slow", "session_id": "qa", "stream_id": "stream:queue",
            "delivered_per_round": counts, "snapshots": snapshots, "server_shutdown_awaited": true,
            "owner_observations": *observations.lock().map_err(|_| "owner observations lock failed")?,
        }),
    )?;

    Ok(())
}
