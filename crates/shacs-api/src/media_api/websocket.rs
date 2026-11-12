use crate::websocket_runtime_turns::WebSocketRuntimeTurnSeat;
use crate::{
    send_websocket_event_with_observation, websocket_event_chat_id, ApiError, ApiRouterState,
    WebSocketQueueAccounting, WebSocketReconnectScope, WebSocketRuntimeTurn,
};
use axum::extract::ws::WebSocket;
use serde_json::Value;
use shacs_channels::{ChannelDeliveryObservation, WebSocketServerEvent};
use std::sync::{Arc, Condvar, Mutex};
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

/// How far the connection got through the queued events. The producer of an event waits
/// here to learn whether its event was written, so that "queued" is never taken for
/// "delivered".
#[derive(Default)]
struct WebSocketWriteLedger {
    progress: Mutex<WebSocketWriteProgress>,
    changed: Condvar,
}

#[derive(Default)]
struct WebSocketWriteProgress {
    written: u64,
    closed: bool,
}

impl WebSocketWriteLedger {
    fn record_written(&self) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.written += 1;
        }
        self.changed.notify_all();
    }

    fn close(&self) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.closed = true;
        }
        self.changed.notify_all();
    }

    /// Waits until the `sequence`-th queued event was written or the writer stopped,
    /// and returns whether it was written.
    fn wait_written(&self, sequence: u64) -> bool {
        let Ok(mut progress) = self.progress.lock() else {
            return false;
        };
        while progress.written < sequence && !progress.closed {
            progress = match self.changed.wait(progress) {
                Ok(progress) => progress,
                Err(_) => return false,
            };
        }
        progress.written >= sequence
    }
}

/// Stops the waiters however the writer ends: a failed write, a cancelled connection
/// task or a panic all leave no one to write the events still queued.
struct WebSocketWriterEnd(Arc<WebSocketWriteLedger>);

impl Drop for WebSocketWriterEnd {
    fn drop(&mut self) {
        self.0.close();
    }
}

fn reject_media_projection_frame(frame: &Value) -> Result<(), ApiError> {
    if frame.get("type").and_then(Value::as_str) == Some("media_projection") {
        return Err(ApiError::not_found("media projection is unavailable"));
    }
    Ok(())
}

pub(crate) async fn dispatch_websocket_frame(
    state: ApiRouterState,
    frame: Value,
    client_id: String,
    default_chat_id: String,
    socket: &mut WebSocket,
    reconnect_scope: &mut WebSocketReconnectScope,
    runtime_turns: Option<&WebSocketRuntimeTurnSeat>,
) -> Result<(), ApiError> {
    reject_media_projection_frame(&frame)?;
    let adapter = state.adapter.clone();
    let chat_id = default_chat_id.clone();
    dispatch_websocket_work(
        state,
        default_chat_id,
        socket,
        reconnect_scope,
        runtime_turns,
        move |emit| adapter.process_websocket_frame_streaming(frame, &client_id, &chat_id, emit),
    )
    .await
}

/// Runs a turn the adapter started by itself, the way a frame's work runs.
pub(crate) async fn dispatch_websocket_runtime_turn(
    state: ApiRouterState,
    turn: WebSocketRuntimeTurn,
    default_chat_id: String,
    socket: &mut WebSocket,
    reconnect_scope: &mut WebSocketReconnectScope,
    seat: &WebSocketRuntimeTurnSeat,
) -> Result<(), ApiError> {
    let adapter = state.adapter.clone();
    let mut run = seat.run(turn);
    dispatch_websocket_work(
        state,
        default_chat_id,
        socket,
        reconnect_scope,
        Some(seat),
        move |emit| {
            let end = adapter.process_websocket_runtime_turn_streaming(run.turn(), emit)?;
            run.ended(end);
            Ok(())
        },
    )
    .await
}

/// Runs `work` off the connection task and writes each event it emits to the connection.
/// `emit` returns whether its event was written.
async fn dispatch_websocket_work(
    state: ApiRouterState,
    default_chat_id: String,
    socket: &mut WebSocket,
    reconnect_scope: &mut WebSocketReconnectScope,
    runtime_turns: Option<&WebSocketRuntimeTurnSeat>,
    work: impl FnOnce(&mut dyn FnMut(WebSocketServerEvent) -> bool) -> Result<(), ApiError>
        + Send
        + 'static,
) -> Result<(), ApiError> {
    let fallback_chat_id = default_chat_id;
    let (event_tx, mut event_rx) = mpsc::channel::<(
        WebSocketServerEvent,
        ChannelDeliveryObservation,
    )>(WEBSOCKET_EVENT_QUEUE_CAPACITY);
    reconnect_scope.connection_for(
        &state.reconnect_tracker,
        &fallback_chat_id,
        &fallback_chat_id,
    );
    let mut queue_accounting =
        WebSocketQueueAccounting::new(&state, reconnect_scope, &fallback_chat_id);
    let ledger = Arc::new(WebSocketWriteLedger::default());
    let writer_end = WebSocketWriterEnd(ledger.clone());
    let producer_ledger = ledger.clone();
    let task = tokio::task::spawn_blocking(move || {
        let mut queued = 0_u64;
        let mut emit = move |event: WebSocketServerEvent| {
            let progress = matches!(event, WebSocketServerEvent::Delta { .. });
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
                return false;
            }
            queued += 1;
            // Progress is droppable and never waits for the connection.
            progress || producer_ledger.wait_written(queued)
        };
        work(&mut emit)
    });

    while let Some((event, mut observation)) = event_rx.recv().await {
        observation.queue_depth = u64::try_from(event_rx.len()).ok();
        let written_chat = runtime_turns.map(|_| {
            websocket_event_chat_id(&event)
                .unwrap_or(&fallback_chat_id)
                .to_owned()
        });
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
        if let (Some(seat), Some(chat_id)) = (runtime_turns, written_chat) {
            seat.wrote(&chat_id);
        }
        ledger.record_written();
    }
    drop(writer_end);

    task.await
        .unwrap_or_else(|_| Err(ApiError::internal("websocket frame task failed")))
}

#[cfg(test)]
mod tests;
