use super::{parse_json, parse_line, CHAT_ID, FINAL_TEXT, OBSERVED_AT_UNIX_MS, REPLY_ID};
use crate::spec031_cli::render;
use serde_json::{json, Value};
use shacs_api::{ApiError, ApiHttpRequest, ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_channels::{
    project_spec031_channel_event, ChannelSpec031ProjectionInput, WebSocketServerEvent,
};
use shacs_projection::{
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031Envelope,
    Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage, Spec031ObservedAtUnixMs,
    Spec031ParentRef, Spec031ProgressCapability, Spec031ProgressDelivery, Spec031ProjectionKind,
    Spec031Reason, Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Severity,
    Spec031Source, Spec031SourceOwner, Spec031SubjectRef,
};
use shacs_providers::LlmResponse;
use std::error::Error;
use std::net::TcpListener;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tungstenite::{connect, Message};

#[derive(Clone)]
struct StaticAdapter;

impl ChatCompletionAdapter for StaticAdapter {
    fn configured_model(&self) -> &str {
        "spec031-parity"
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse {
            content: Some(FINAL_TEXT.to_owned()),
            finish_reason: "stop".to_owned(),
            ..LlmResponse::default()
        })
    }

    fn spec031_projection(
        &self,
        _projection: shacs_api::Spec031ApiProjection,
    ) -> Result<Option<Spec031Envelope>, ApiError> {
        canonical_envelope()
            .map(Some)
            .map_err(|error| ApiError::internal(format!("parity projection failed: {error}")))
    }

    fn process_websocket_frame(
        &self,
        _frame: Value,
        _client_id: &str,
        _default_chat_id: &str,
    ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
        Ok(vec![owner_event()])
    }
}

pub fn cli() -> Result<super::CanonicalFields, Box<dyn Error>> {
    parse_line(&render::envelope_line("progress", &canonical_envelope()?))
}

pub fn api() -> Result<super::CanonicalFields, Box<dyn Error>> {
    let response =
        shacs_api::handle_api_request(ApiHttpRequest::get("/v1/diagnostics"), &StaticAdapter);
    assert_eq!(response.status, 200);
    Ok(parse_json(&response.body))
}

pub fn websocket() -> Result<super::CanonicalFields, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let join = std::thread::spawn(move || server_thread(listener, shutdown_rx));
    let (mut socket, _) = connect(format!("ws://{addr}{}", shacs_api::WEBSOCKET_PATH))?;
    socket.send(Message::Text(
        json!({"type":"message","chat_id":CHAT_ID})
            .to_string()
            .into(),
    ))?;
    let _event = socket.read()?;
    let projection = parse_json(&serde_json::from_str::<Value>(
        &socket.read()?.into_text()?,
    )?);
    let _ = socket.close(None);
    let _ = shutdown_tx.send(());
    join.join()
        .map_err(|_| "websocket server thread panicked")??;
    Ok(projection)
}

pub fn channel() -> Result<super::CanonicalFields, Box<dyn Error>> {
    Ok(parse_json(&serde_json::to_value(
        project_spec031_channel_event(ChannelSpec031ProjectionInput::websocket_event(
            owner_event(),
        ))?,
    )?))
}

fn server_thread(listener: TcpListener, shutdown_rx: mpsc::Receiver<()>) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let listener =
            tokio::net::TcpListener::from_std(listener).map_err(|error| error.to_string())?;
        shacs_api::serve_api_listener_with_timeout_and_websocket_path(
            listener,
            Arc::new(StaticAdapter),
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

fn canonical_envelope() -> Result<Spec031Envelope, Box<dyn Error>> {
    Ok(Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Progress,
        state: Spec031Availability::Ready,
        severity: Spec031Severity::Info,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Included,
            safe_summary: Spec031SafeSummary::try_new("included final channel message delivered")?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new("subject:channel:websocket:message")?,
            parent_ref: Some(Spec031ParentRef::try_new(&format!(
                "parent:channel:websocket:chat:{CHAT_ID}"
            ))?),
            action_ref: Some(Spec031ActionRef::try_new(&format!(
                "action:channel:websocket:reply:{REPLY_ID}"
            ))?),
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Channel,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(OBSERVED_AT_UNIX_MS)),
            freshness: Spec031Freshness::Current,
        },
        capability: Spec031Capability::Progress(Spec031ProgressCapability::delivery(
            Spec031ProgressDelivery::FinalDelivered,
        )),
        children: Vec::new(),
    })?)
}

fn owner_event() -> WebSocketServerEvent {
    WebSocketServerEvent::Message {
        chat_id: CHAT_ID.to_owned(),
        text: FINAL_TEXT.to_owned(),
        buttons: Vec::new(),
        button_prompt: None,
        media: Vec::new(),
        reply_to: Some(REPLY_ID.to_owned()),
        kind: None,
    }
}
