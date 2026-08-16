use super::ChatCompletionAdapter;
use crate::{
    observe_progress_delivery, send_websocket_event, ApiError, ApiRouterState,
    WebSocketReconnectScope,
};
use axum::extract::ws::{Message, WebSocket};
use serde_json::Value;
use shacs_channels::{ChannelDeliveryObservation, WebSocketServerEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

async fn send_projection(
    frame: &Value,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    socket: &mut WebSocket,
) -> Result<bool, ApiError> {
    if frame.get("type").and_then(Value::as_str) != Some("media_projection") {
        return Ok(false);
    }
    let projection = tokio::task::spawn_blocking(move || adapter.media_projection())
        .await
        .map_err(|_| ApiError::internal("media projection task failed"))?
        .ok_or_else(|| ApiError::not_found("media projection is unavailable"))?;
    let payload = serde_json::to_string(&projection).map_err(|error| {
        ApiError::internal(format!("media projection could not be serialized: {error}"))
    })?;
    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ApiError::internal("websocket client disconnected"))?;
    Ok(true)
}

pub(crate) async fn dispatch_websocket_frame(
    state: ApiRouterState,
    frame: Value,
    client_id: String,
    default_chat_id: String,
    socket: &mut WebSocket,
    reconnect_scope: &mut WebSocketReconnectScope,
) -> Result<(), ApiError> {
    if send_projection(&frame, state.adapter.clone(), socket).await? {
        return Ok(());
    }
    let fallback_chat_id = default_chat_id.clone();
    let (event_tx, mut event_rx) = mpsc::channel::<WebSocketServerEvent>(64);
    let adapter = state.adapter.clone();
    let spec031_channel_observer = state.spec031_channel_observer.clone();
    let task = tokio::task::spawn_blocking(move || {
        let mut emit = move |event| {
            if event_tx.blocking_send(event).is_err() {
                observe_progress_delivery(
                    spec031_channel_observer.as_ref(),
                    shacs_channels::WEBSOCKET_CHANNEL,
                    shacs_projection::Spec031ProgressDelivery::Dropped,
                    ChannelDeliveryObservation {
                        dropped: Some(1),
                        slow_consumer: Some(1),
                        ..ChannelDeliveryObservation::unavailable()
                    },
                );
            }
        };
        adapter.process_websocket_frame_streaming(frame, &client_id, &default_chat_id, &mut emit)
    });

    while let Some(event) = event_rx.recv().await {
        send_websocket_event(
            socket,
            event,
            &fallback_chat_id,
            state.spec031_channel_observer.as_ref(),
            state.reconnect_tracker.clone(),
            reconnect_scope,
        )
        .await?;
    }

    task.await
        .unwrap_or_else(|_| Err(ApiError::internal("websocket frame task failed")))
}
