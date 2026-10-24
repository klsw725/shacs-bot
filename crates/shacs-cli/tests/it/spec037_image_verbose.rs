//! The gateway runtime (`run --verbose`, as in the default Compose service)
//! prints response previews to stderr; an image request's model reply must
//! not appear there, nor in the answer.

use serde_json::json;
use shacs_config::{save_config_to_path, Config};
use shacs_providers::LlmResponse;
use std::error::Error;
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message};

#[test]
fn verbose_runtime_logs_show_none_of_an_image_reply() -> Result<(), Box<dyn Error>> {
    // Given
    let reply = LlmResponse {
        content: Some("The blue cat is ready.".to_owned()),
        ..LlmResponse::default()
    };
    // When
    let (frames, stderr) = verbose_run(&[reply.clone(), reply])?;
    // Then
    assert!(
        frames.contains("No image was generated for this request"),
        "{frames}"
    );
    assert!(stderr.contains("LLM response preview"), "{stderr}");
    for shown in [&frames, &stderr] {
        assert!(!shown.contains("blue cat"), "{shown}");
    }
    Ok(())
}

#[test]
fn verbose_runtime_logs_show_no_words_of_an_image_call() -> Result<(), Box<dyn Error>> {
    // Given
    let mut generate = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        ..LlmResponse::default()
    };
    generate
        .tool_calls
        .push(shacs_providers::ToolCallRequest::new(
            "i1",
            "image_generate",
            serde_json::Map::from_iter([("prompt".to_owned(), json!("a green dog"))]),
        ));
    // When
    let (frames, stderr) = verbose_run(&[generate])?;
    // Then
    assert!(
        frames.contains("Permission approval required before running tool `image_generate`"),
        "{frames}"
    );
    assert!(
        stderr.contains("Tool call image_generate args="),
        "{stderr}"
    );
    assert!(!stderr.contains("green dog"), "{stderr}");
    Ok(())
}

/// Runs the gateway runtime with `--verbose` on scripted provider replies and
/// sends it an image request: the frames sent back and everything on stderr.
fn verbose_run(replies: &[LlmResponse]) -> Result<(String, String), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    let config: Config = serde_json::from_value(json!({
        "agents":{"defaults":{"workspace":workspace,"provider":"custom","model":"test-model"}},
        "providers":{"custom":{"apiKey":"sk-isolated-fixture","apiBase":"http://127.0.0.1:1/v1"},
            "openai":{"apiKey":"sk-local-image-fixture","apiBase":"http://127.0.0.1:1/v1"}},
        "tools":{"exec":{"enable":false},"web":{"enable":false},
            "imageGeneration":{"enable":true,"provider":"openai","model":"gpt-image-1"}}
    }))?;
    let config_path = root.path().join("config.json");
    save_config_to_path(&config, &config_path)?;
    let responses = root.path().join("responses.json");
    fs::write(&responses, serde_json::to_vec(replies)?)?;
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--config")
        .arg(&config_path)
        .args(["run", "--websocket-port", &port.to_string(), "--verbose"])
        .env("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", &responses)
        .env("HOME", root.path())
        .env_remove("SHACS_RUNTIME_PACKAGE")
        .env_remove("OPENAI_API_KEY")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let sink = stderr.clone();
    let mut pipe = child.stderr.take().ok_or("missing stderr")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(count) = pipe.read(&mut bytes) {
            if count == 0 {
                return;
            }
            sink.lock().unwrap().extend_from_slice(&bytes[..count]);
        }
    });
    let frames = exchange(port, "Generate an image of a red square");
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    let stderr = String::from_utf8_lossy(&stderr.lock().unwrap()).into_owned();
    Ok((frames?, stderr))
}

/// Sends one message over the runtime's WebSocket and collects the frames up
/// to the method footer.
fn exchange(port: u16, text: &str) -> Result<String, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut socket = loop {
        match connect(format!("ws://127.0.0.1:{port}/")) {
            Ok((socket, _)) => break socket,
            Err(error) if Instant::now() > deadline => return Err(error.into()),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    };
    if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    }
    let message = json!({"type": "message", "text": text, "chat_id": "qa"});
    socket.send(Message::Text(message.to_string().into()))?;
    let mut frames = String::new();
    while !frames.contains("Method: image_generation_review") {
        let frame = socket.read()?;
        frames.push_str(&frame.into_text()?);
        frames.push('\n');
    }
    let _ = socket.close(None);
    Ok(frames)
}
