use super::ChatCompletionAdapter;
use crate::{
    send_websocket_event_with_observation, ApiError, ApiRouterState, WebSocketQueueAccounting,
    WebSocketReconnectScope,
};
use axum::extract::ws::{Message, WebSocket};
use serde_json::Value;
use shacs_channels::{ChannelDeliveryObservation, WebSocketServerEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) const WEBSOCKET_EVENT_QUEUE_CAPACITY: usize = 64;

#[derive(Debug)]
enum WebSocketQueueFailure {
    ProgressFull(Box<(WebSocketServerEvent, ChannelDeliveryObservation)>),
    Closed(Box<(WebSocketServerEvent, ChannelDeliveryObservation)>),
}

fn enqueue_websocket_event(
    sender: &mpsc::Sender<(WebSocketServerEvent, ChannelDeliveryObservation)>,
    event: WebSocketServerEvent,
    observation: ChannelDeliveryObservation,
) -> Result<(), WebSocketQueueFailure> {
    if matches!(event, WebSocketServerEvent::Delta { .. }) {
        return sender
            .try_send((event, observation))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full((event, observation)) => {
                    WebSocketQueueFailure::ProgressFull(Box::new((event, observation)))
                }
                mpsc::error::TrySendError::Closed((event, observation)) => {
                    WebSocketQueueFailure::Closed(Box::new((event, observation)))
                }
            });
    }
    sender.blocking_send((event, observation)).map_err(|error| {
        let (event, observation) = error.0;
        WebSocketQueueFailure::Closed(Box::new((event, observation)))
    })
}

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
    let (event_tx, mut event_rx) = mpsc::channel::<(
        WebSocketServerEvent,
        ChannelDeliveryObservation,
    )>(WEBSOCKET_EVENT_QUEUE_CAPACITY);
    let adapter = state.adapter.clone();
    reconnect_scope.connection_for(
        &state.reconnect_tracker,
        &fallback_chat_id,
        &fallback_chat_id,
    );
    let mut queue_accounting =
        WebSocketQueueAccounting::new(&state, reconnect_scope, &fallback_chat_id);
    let task = tokio::task::spawn_blocking(move || {
        let mut emit = move |event| {
            let generation = queue_accounting.generation_for(&event);
            let available = event_tx.capacity();
            let slow_consumer = u64::from(available == 0);
            let observation = ChannelDeliveryObservation {
                queue_depth: u64::try_from(
                    WEBSOCKET_EVENT_QUEUE_CAPACITY
                        .saturating_sub(available)
                        .saturating_add(1),
                )
                .ok()
                .map(|depth| depth.min(WEBSOCKET_EVENT_QUEUE_CAPACITY as u64)),
                queue_capacity: Some(WEBSOCKET_EVENT_QUEUE_CAPACITY as u64),
                accepted: Some(1),
                dropped: Some(0),
                slow_consumer: Some(slow_consumer),
                reconnect_generation: Some(generation),
                ..ChannelDeliveryObservation::unavailable()
            };
            if let Err(failure) = enqueue_websocket_event(&event_tx, event, observation) {
                let (event, observation, slow_consumer) = match failure {
                    WebSocketQueueFailure::ProgressFull(failure) => {
                        let (event, observation) = *failure;
                        (event, observation, 1)
                    }
                    WebSocketQueueFailure::Closed(failure) => {
                        let (event, observation) = *failure;
                        (event, observation, 0)
                    }
                };
                queue_accounting.observe_failure(
                    &event,
                    ChannelDeliveryObservation {
                        queue_depth: Some(WEBSOCKET_EVENT_QUEUE_CAPACITY as u64),
                        queue_capacity: Some(WEBSOCKET_EVENT_QUEUE_CAPACITY as u64),
                        accepted: Some(0),
                        dropped: Some(1),
                        slow_consumer: Some(slow_consumer),
                        ..observation
                    },
                );
            }
        };
        adapter.process_websocket_frame_streaming(frame, &client_id, &default_chat_id, &mut emit)
    });

    while let Some((event, mut observation)) = event_rx.recv().await {
        observation.queue_depth = u64::try_from(event_rx.len()).ok();
        send_websocket_event_with_observation(
            socket,
            event,
            &fallback_chat_id,
            state.spec031_channel_observer.as_ref(),
            state.reconnect_tracker.clone(),
            reconnect_scope,
            observation,
        )
        .await?;
    }

    task.await
        .unwrap_or_else(|_| Err(ApiError::internal("websocket frame task failed")))
}

#[cfg(test)]
mod tests;
