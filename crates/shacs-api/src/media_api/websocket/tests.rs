#![cfg(test)]

use super::*;
use std::sync::{mpsc as std_mpsc, Arc, Barrier};

fn delta(sequence: usize) -> WebSocketServerEvent {
    WebSocketServerEvent::Delta {
        chat_id: "chat-a".to_owned(),
        text: sequence.to_string(),
        stream_id: Some("stream-a".to_owned()),
    }
}

fn observation() -> ChannelDeliveryObservation {
    ChannelDeliveryObservation {
        queue_capacity: Some(WEBSOCKET_EVENT_QUEUE_CAPACITY as u64),
        accepted: Some(1),
        dropped: Some(0),
        ..ChannelDeliveryObservation::unavailable()
    }
}

#[test]
fn production_enqueue_drops_progress_when_queue_is_full() {
    let (sender, receiver) = mpsc::channel(WEBSOCKET_EVENT_QUEUE_CAPACITY);
    for sequence in 0..WEBSOCKET_EVENT_QUEUE_CAPACITY {
        assert!(enqueue_websocket_event(&sender, delta(sequence), observation()).is_ok());
    }

    let failure = enqueue_websocket_event(
        &sender,
        delta(WEBSOCKET_EVENT_QUEUE_CAPACITY),
        observation(),
    )
    .expect_err("full production queue must drop progress");

    assert!(matches!(failure, WebSocketQueueFailure::ProgressFull(_)));
    assert_eq!(receiver.len(), WEBSOCKET_EVENT_QUEUE_CAPACITY);
}

#[test]
fn production_enqueue_retains_terminal_backpressure_when_queue_is_full() {
    let (sender, mut receiver) = mpsc::channel(WEBSOCKET_EVENT_QUEUE_CAPACITY);
    for sequence in 0..WEBSOCKET_EVENT_QUEUE_CAPACITY {
        assert!(enqueue_websocket_event(&sender, delta(sequence), observation()).is_ok());
    }
    let ready = Arc::new(Barrier::new(2));
    let worker_ready = ready.clone();
    let (completed_tx, completed_rx) = std_mpsc::channel();
    let worker = std::thread::spawn(move || {
        worker_ready.wait();
        let result = enqueue_websocket_event(
            &sender,
            WebSocketServerEvent::Message {
                chat_id: "chat-a".to_owned(),
                text: "final".to_owned(),
                buttons: Vec::new(),
                button_prompt: None,
                media: Vec::new(),
                reply_to: None,
                kind: None,
            },
            observation(),
        );
        assert!(completed_tx.send(result).is_ok());
    });
    ready.wait();
    assert!(completed_rx.try_recv().is_err());

    assert!(receiver.blocking_recv().is_some());
    assert!(matches!(completed_rx.recv(), Ok(Ok(()))));
    assert!(worker.join().is_ok());
    assert_eq!(receiver.len(), WEBSOCKET_EVENT_QUEUE_CAPACITY);
}
