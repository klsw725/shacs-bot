mod spec035_reconnect_support;

use futures_util::StreamExt;
use serde_json::{json, Value};
use shacs_api::serve_api_listener;
use spec035_reconnect_support::{adapter, release_events};
use std::env;
use std::fs;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::{client_async, tungstenite::Message, WebSocketStream};

#[tokio::test]
async fn sse_reconnect_uses_stable_identity_across_two_source_ports(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let fixture = adapter(root.path().to_path_buf())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve_api_listener(listener, fixture.adapter, async {
        let _ = shutdown_rx.await;
    }));

    let (first_port, first) = sse_transcript(addr, &fixture.reached, &fixture.release).await?;
    let (second_port, second) = sse_transcript(addr, &fixture.reached, &fixture.release).await?;

    assert_ne!(first_port, second_port);
    assert_eq!(first[0]["type"], "tasks_snapshot");
    assert_eq!(first[0]["metadata"]["generation"], "generation:1");
    assert_eq!(first[0]["reconnect_gap"], false);
    assert_eq!(second[0]["metadata"]["generation"], "generation:2");
    assert_eq!(second[0]["reconnect_gap"], true);
    assert_filtered_then_applied(&second)?;
    write_artifact(
        "sse.json",
        &json!({
            "source_ports": [first_port, second_port],
            "first_connection": first,
            "second_connection": second,
        }),
    )?;
    let _ = shutdown_tx.send(());
    server.await??;
    Ok(())
}

#[tokio::test]
async fn websocket_reconnect_uses_stable_identity_across_two_source_ports(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let fixture = adapter(root.path().to_path_buf())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve_api_listener(listener, fixture.adapter, async {
        let _ = shutdown_rx.await;
    }));

    let (first_port, first) =
        websocket_transcript(addr, &fixture.reached, &fixture.release).await?;
    let (second_port, second) =
        websocket_transcript(addr, &fixture.reached, &fixture.release).await?;

    assert_ne!(first_port, second_port);
    assert_eq!(first[0]["metadata"]["generation"], "generation:1");
    assert_eq!(first[0]["reconnect_gap"], false);
    assert_eq!(second[0]["metadata"]["generation"], "generation:2");
    assert_eq!(second[0]["reconnect_gap"], true);
    assert_filtered_then_applied(&second)?;
    write_artifact(
        "websocket.json",
        &json!({
            "source_ports": [first_port, second_port],
            "first_connection": first,
            "second_connection": second,
        }),
    )?;
    let _ = shutdown_tx.send(());
    server.await??;
    Ok(())
}

async fn sse_transcript(
    addr: std::net::SocketAddr,
    reached: &std::sync::Arc<std::sync::Barrier>,
    release: &std::sync::Arc<std::sync::Barrier>,
) -> Result<(u16, Vec<Value>), Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr).await?;
    let source_port = stream.local_addr()?.port();
    let body =
        json!({"stream":true,"session_id":"qa","messages":[{"role":"user","content":"hello"}]})
            .to_string();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nx-shacs-spec035-client-id: client:sse-qa\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    while !response.windows(14).any(|part| part == b"tasks_snapshot") {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err("SSE closed before snapshot".into());
        }
        response.extend_from_slice(&buffer[..read]);
    }
    release_events(reached, release).await?;
    stream.read_to_end(&mut response).await?;
    let transcript = String::from_utf8(response)?;
    Ok((source_port, parse_sse_data(&transcript)?))
}

async fn websocket_transcript(
    addr: std::net::SocketAddr,
    reached: &std::sync::Arc<std::sync::Barrier>,
    release: &std::sync::Arc<std::sync::Barrier>,
) -> Result<(u16, Vec<Value>), Box<dyn std::error::Error>> {
    let stream = TcpStream::connect(addr).await?;
    let source_port = stream.local_addr()?.port();
    let url = format!("ws://{addr}/ws?client_id=client:ws-qa&session_id=qa");
    let (mut socket, _) = client_async(url, stream).await?;
    let mut frames = vec![read_ws_json(&mut socket).await?];
    release_events(reached, release).await?;
    for _ in 0..2 {
        frames.push(read_ws_json(&mut socket).await?);
    }
    socket.close(None).await?;
    Ok((source_port, frames))
}

async fn read_ws_json(
    socket: &mut WebSocketStream<TcpStream>,
) -> Result<Value, Box<dyn std::error::Error>> {
    match socket.next().await.ok_or("websocket closed")?? {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        _ => Err("expected text frame".into()),
    }
}

fn assert_filtered_then_applied(frames: &[Value]) -> Result<(), Box<dyn std::error::Error>> {
    assert!(frames
        .iter()
        .any(|frame| frame["reason"] == "reject_invalid_payload"));
    let deltas = frames
        .iter()
        .filter(|frame| frame["type"] == "tasks_delta")
        .collect::<Vec<_>>();
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0]["metadata"]["sequence"], 2);
    Ok(())
}

fn parse_sse_data(input: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    input
        .split("\n\n")
        .filter_map(|frame| frame.lines().find_map(|line| line.strip_prefix("data: ")))
        .filter(|payload| {
            *payload != "[DONE]" && payload.starts_with('{') && payload.contains("\"type\"")
        })
        .map(|payload| serde_json::from_str(payload).map_err(Into::into))
        .collect()
}

fn write_artifact(name: &str, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = env::var_os("SPEC035_RECONNECT_ARTIFACT_DIR") else {
        return Ok(());
    };
    fs::create_dir_all(&root)?;
    fs::write(
        PathBuf::from(root).join(name),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}
