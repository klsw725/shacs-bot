use serde_json::{json, Value};
use shacs_api::{ApiError, ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_channels::{project_spec031_channel_event, ChannelSpec031ProjectionInput};
use shacs_providers::LlmResponse;
use std::error::Error;
use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::{connect, Message};

#[derive(Clone)]
struct QaAdapter;

struct Server {
    addr: SocketAddr,
    shutdown_tx: Option<mpsc::Sender<()>>,
    join: Option<JoinHandle<Result<(), String>>>,
}

impl ChatCompletionAdapter for QaAdapter {
    fn configured_model(&self) -> &str {
        "spec031-black-box-qa"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse::default())
    }
}

impl Server {
    fn start() -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let join = std::thread::spawn(move || run_server(listener, shutdown_rx));
        Ok(Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            join: Some(join),
        })
    }

    fn shutdown_and_join(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| "QA server thread panicked")?
                .map_err(|error| format!("QA server failed: {error}"))?;
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.shutdown_and_join();
    }
}

#[test]
fn spec031_black_box_qa_surfaces_are_reachable_safe_and_explicit() -> Result<(), Box<dyn Error>> {
    assert_fixture_command_is_not_shipped()?;
    assert_cli_channels_status_reachable()?;

    let mut server = Server::start()?;
    let mut health_after_readiness = ureq::get(&format!("http://{}/health", server.addr)).call()?;
    assert_eq!(
        health_after_readiness.body_mut().read_to_string()?,
        r#"{"status":"ok"}"#
    );

    let mut diagnostics = ureq::get(&format!("http://{}/v1/diagnostics", server.addr)).call()?;
    let body: Value = serde_json::from_str(&diagnostics.body_mut().read_to_string()?)?;
    assert!(body.is_object());
    assert!(!serde_json::to_string(&body)?.contains("raw_provider_payload"));

    let mut readiness = ureq::get(&format!("http://{}/v1/readiness", server.addr)).call()?;
    let readiness_body: Value = serde_json::from_str(&readiness.body_mut().read_to_string()?)?;
    assert_eq!(readiness_body["kind"], "readiness");
    assert_eq!(readiness_body["state"], "unavailable");

    let mut health = ureq::get(&format!("http://{}/health", server.addr)).call()?;
    assert_eq!(health.body_mut().read_to_string()?, r#"{"status":"ok"}"#);

    let (mut socket, _) = connect(format!("ws://{}{}", server.addr, shacs_api::WEBSOCKET_PATH))?;
    socket.send(Message::Text(json!({"type":"message"}).to_string().into()))?;
    let _error_event = socket.read()?;
    let projection: Value = serde_json::from_str(&socket.read()?.into_text()?)?;
    assert_eq!(projection["reason"]["code"], "unsupported");
    let _ = socket.close(None);
    server.shutdown_and_join()?;

    let unsupported = project_spec031_channel_event(ChannelSpec031ProjectionInput::unsupported(
        "telegram",
        "progress_delta",
    ))?;
    let unsupported_json = serde_json::to_value(unsupported)?;
    assert_eq!(unsupported_json["state"], "unavailable");
    assert_eq!(unsupported_json["reason"]["code"], "unsupported");
    Ok(())
}

fn assert_fixture_command_is_not_shipped() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args(["spec031-fixture", "websocket-final"])
        .output()?;
    if output.status.success() {
        return Err("spec031-fixture unexpectedly succeeded".into());
    }
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command `spec031-fixture`"));
    Ok(())
}

fn assert_cli_channels_status_reachable() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args(["channels", "status"])
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Channel runtime status"));
    assert!(stdout.contains("Spec031 progress:"));
    Ok(())
}

fn run_server(listener: TcpListener, shutdown_rx: mpsc::Receiver<()>) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let listener =
            tokio::net::TcpListener::from_std(listener).map_err(|error| error.to_string())?;
        shacs_api::serve_api_listener_with_timeout_and_websocket_path(
            listener,
            Arc::new(QaAdapter),
            Duration::from_secs(10),
            shacs_api::WEBSOCKET_PATH,
            async move {
                let _ = tokio::task::spawn_blocking(move || shutdown_rx.recv()).await;
            },
        )
        .await
        .map_err(|error| error.to_string())
    })
}
