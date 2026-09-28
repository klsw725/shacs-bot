mod spec035_accounting_support;
mod spec035_reconnect_support;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shacs_api::serve_api_listener;
use spec035_reconnect_support::{adapter, release_events};
use std::env;
use std::fs;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{client_async, tungstenite::Message, WebSocketStream};

#[tokio::test]
async fn normal_websocket_reconnect_preserves_progress_and_final_accounting(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: an isolated API and a normal WebSocket consumer.
    let root = tempfile::tempdir()?;
    let fixture = adapter(root.path().to_path_buf())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve_api_listener(listener, fixture.adapter, async {
        let _ = shutdown_rx.await;
    }));

    // When: the client consumes progress/final frames and reconnects with stable identity.
    let mut first = connect(addr).await?;
    let first_snapshot = read_json(&mut first).await?;
    assert_eq!(first_snapshot["type"], "tasks_snapshot");
    assert_eq!(first_snapshot["metadata"]["generation"], "generation:1");
    let mut ports = vec![first.get_ref().local_addr()?.port()];
    first
        .send(Message::Text(
            json!({"type":"message","text":"run"}).to_string().into(),
        ))
        .await?;
    let frames = read_many(&mut first, 4).await?;
    release_events(&fixture.reached, &fixture.release).await?;
    let _ = read_many(&mut first, 2).await?;
    first.close(None).await?;

    let mut second = connect(addr).await?;
    let snapshot = read_json(&mut second).await?;
    ports.push(second.get_ref().local_addr()?.port());

    // Then: snapshot remains first and prior accounting is not reset or inferred.
    assert_eq!(frames[0]["event"], "delta");
    assert_eq!(frames[2]["event"], "message");
    assert_eq!(snapshot["type"], "tasks_snapshot");
    assert_eq!(snapshot["metadata"]["generation"], "generation:2");
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["progress"],
        json!(["live"])
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["emitted_count"]["value"],
        1
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["dropped_count"]["value"],
        0
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    assert_eq!(
        snapshot["delivery_accounting"]["delivery"]["coalesced_count"]["availability"],
        "unavailable"
    );
    let mut snapshots = vec![first_snapshot, snapshot.clone()];
    let mut rounds = vec![frames.clone()];
    for generation in 3..=4 {
        second
            .send(Message::Text(
                json!({"type":"message","text":"run"}).to_string().into(),
            ))
            .await?;
        let current = read_many(&mut second, 4).await?;
        assert_eq!(current[0]["stream_id"], "stream:accounting");
        assert_eq!(current[2]["event"], "message");
        rounds.push(current);
        release_events(&fixture.reached, &fixture.release).await?;
        let _ = read_many(&mut second, 2).await?;
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
        assert_eq!(delivery["accepted_count"]["value"], generation - 1);
        assert_eq!(delivery["emitted_count"]["value"], generation - 1);
        assert_eq!(delivery["dropped_count"]["value"], 0);
        assert_eq!(
            delivery["coalesced_count"],
            snapshot["delivery_accounting"]["delivery"]["coalesced_count"]
        );
        assert_eq!(delivery["final_delivery"]["state"], "final_delivered");
        snapshots.push(next);
    }
    release_events(&fixture.reached, &fixture.release).await?;
    let _ = read_many(&mut second, 2).await?;
    second.close(None).await?;
    let _ = shutdown_tx.send(());
    server.await??;
    write_artifact(
        "normal-websocket.json",
        &json!({"server": addr.to_string(), "source_ports": ports,
            "client_id": "client:accounting", "session_id": "qa", "stream_id": "stream:accounting",
            "first_connection": frames, "reconnect_snapshot": snapshot,
            "rounds": rounds, "snapshots": snapshots, "server_shutdown_awaited": true}),
    )?;
    Ok(())
}

fn write_artifact(name: &str, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = env::var_os("SPEC035_ACCOUNTING_ARTIFACT_DIR") else {
        return Ok(());
    };
    fs::create_dir_all(&root)?;
    fs::write(
        std::path::PathBuf::from(root).join(name),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}

async fn connect(
    addr: std::net::SocketAddr,
) -> Result<WebSocketStream<TcpStream>, Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(addr).await?;
    let url = format!("ws://{addr}/ws?client_id=client:accounting&session_id=qa");
    Ok(client_async(url, stream).await?.0)
}

async fn read_many(
    socket: &mut WebSocketStream<TcpStream>,
    count: usize,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_json(socket).await?);
    }
    Ok(values)
}

async fn read_json(
    socket: &mut WebSocketStream<TcpStream>,
) -> Result<Value, Box<dyn std::error::Error>> {
    match socket.next().await.ok_or("websocket closed")?? {
        Message::Text(text) => {
            spec035_accounting_support::capture_payload(socket, &text)?;
            Ok(serde_json::from_str(&text)?)
        }
        _ => Err("expected text frame".into()),
    }
}
