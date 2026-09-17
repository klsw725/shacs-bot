#![cfg(test)]

use super::*;
use crate::media_api::WEBSOCKET_EVENT_QUEUE_CAPACITY;
use crate::ApiReconnectTracker;
use serde_json::json;
use shacs_channels::ChannelDeliveryObservation;
use shacs_projection::Spec031ProgressDelivery;

#[test]
fn spec035_f2_stale_generation_cannot_overwrite_final_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    for websocket in [true, false] {
        let mut tracker = ApiReconnectTracker::new(4);
        let key = "spec035:f2-overlap";
        let mut first = if websocket {
            tracker.connect_ws(key)
        } else {
            tracker.connect_sse(key, None)
        };
        let mut second = if websocket {
            tracker.connect_ws(key)
        } else {
            tracker.connect_sse(key, None)
        };
        second.delivery.observe(
            "websocket",
            Spec031ProgressDelivery::FinalFailed,
            ChannelDeliveryObservation::unavailable(),
        )?;
        first.delivery.observe(
            "websocket",
            Spec031ProgressDelivery::FinalDelivered,
            ChannelDeliveryObservation::unavailable(),
        )?;

        if websocket {
            tracker.record_ws(&second, 1);
            tracker.record_ws(&first, 9);
        } else {
            tracker.record_sse(&second, 1);
            tracker.record_sse(&first, 9);
        }

        let stored = if websocket {
            &tracker.ws[key]
        } else {
            &tracker.sse[key]
        };
        let projection = stored.delivery.projection()?.ok_or("accounting")?;
        assert_eq!(
            projection["delivery"]["final_delivery"]["state"],
            "final_failed"
        );
        assert_eq!(stored.last_sequence, 1);
        let third = if websocket {
            tracker.connect_ws(key)
        } else {
            tracker.connect_sse(key, Some("sse:2:1"))
        };
        assert_eq!(third.generation, 3);
        assert_eq!(
            third.delivery.projection()?.ok_or("third accounting")?["delivery"]["final_delivery"]
                ["state"],
            "final_failed"
        );
    }
    Ok(())
}

#[test]
fn websocket_send_failure_classifies_only_message_as_terminal() {
    let delta = shacs_channels::WebSocketServerEvent::Delta {
        chat_id: "chat-a".to_owned(),
        text: "progress".to_owned(),
        stream_id: None,
    };
    let message = shacs_channels::WebSocketServerEvent::Message {
        chat_id: "chat-a".to_owned(),
        text: "final".to_owned(),
        buttons: Vec::new(),
        button_prompt: None,
        media: Vec::new(),
        reply_to: None,
        kind: None,
    };
    let stream_end = shacs_channels::WebSocketServerEvent::StreamEnd {
        chat_id: "chat-a".to_owned(),
        stream_id: None,
    };
    let error = shacs_channels::WebSocketServerEvent::Error {
        chat_id: Some("chat-a".to_owned()),
        detail: None,
    };

    assert_eq!(
        websocket_send_failure_delivery(&delta),
        Some(Spec031ProgressDelivery::Dropped)
    );
    assert_eq!(
        websocket_send_failure_delivery(&message),
        Some(Spec031ProgressDelivery::FinalFailed)
    );
    assert_eq!(
        websocket_send_failure_delivery(&stream_end),
        Some(Spec031ProgressDelivery::Dropped)
    );
    assert_eq!(websocket_send_failure_delivery(&error), None);
}

#[test]
fn reconnect_persists_queue_drop_when_connection_records_final(
) -> Result<(), Box<dyn std::error::Error>> {
    let key = "spec035:production-queue-drop";
    let mut tracker = ApiReconnectTracker::new(4);
    let mut connection = tracker.connect_ws(key);
    connection.delivery.observe(
        "websocket",
        Spec031ProgressDelivery::Live,
        ChannelDeliveryObservation {
            accepted: Some(1),
            emitted: Some(1),
            dropped: Some(0),
            ..ChannelDeliveryObservation::unavailable()
        },
    )?;

    tracker.observe_ws_delivery(
        key,
        Spec031ProgressDelivery::Dropped,
        ChannelDeliveryObservation {
            queue_depth: Some(64),
            queue_capacity: Some(64),
            reconnect_generation: Some(connection.generation),
            accepted: Some(0),
            dropped: Some(1),
            slow_consumer: Some(1),
            ..ChannelDeliveryObservation::unavailable()
        },
    )?;
    connection.delivery.observe(
        "websocket",
        Spec031ProgressDelivery::FinalDelivered,
        ChannelDeliveryObservation::unavailable(),
    )?;
    tracker.record_ws(&connection, 2);
    let projection = tracker
        .connect_ws(key)
        .delivery
        .projection()?
        .ok_or("missing reconnect accounting")?;

    assert_eq!(projection["delivery"]["progress"], json!(["dropped"]));
    assert_eq!(projection["delivery"]["accepted_count"]["value"], 1);
    assert_eq!(projection["delivery"]["emitted_count"]["value"], 1);
    assert_eq!(projection["delivery"]["dropped_count"]["value"], 1);
    assert_eq!(
        projection["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    Ok(())
}

#[test]
fn reconnect_preserves_progress_loss_with_delivered_final() -> Result<(), Box<dyn std::error::Error>>
{
    // Given: a dropped progress fact followed by owner-observed final delivery.
    let mut accounting = ReconnectDeliveryAccounting::default();
    accounting.observe(
        "websocket",
        Spec031ProgressDelivery::Dropped,
        ChannelDeliveryObservation {
            queue_depth: Some(1),
            queue_capacity: Some(64),
            accepted: Some(5),
            emitted: Some(3),
            coalesced: Some(1),
            dropped: Some(2),
            reconnect_generation: Some(1),
            reconnect_gap: Some(false),
            slow_consumer: Some(1),
        },
    )?;
    accounting.observe(
        "websocket",
        Spec031ProgressDelivery::FinalDelivered,
        ChannelDeliveryObservation::unavailable(),
    )?;

    // When: a reconnect carries the prior owner facts into generation two.
    let mut tracker = ApiReconnectTracker::new(4);
    let mut first = tracker.connect_ws("spec035:accounting-delivered");
    first.delivery = accounting;
    tracker.record_ws(&first, 2);
    let accounting = tracker.connect_ws("spec035:accounting-delivered").delivery;
    let projection = accounting
        .projection()?
        .ok_or("missing accounting projection")?;

    // Then: no counter resets and final success does not erase progress loss.
    assert_eq!(projection["delivery"]["progress"], json!(["dropped"]));
    assert_eq!(projection["delivery"]["accepted_count"]["value"], 5);
    assert_eq!(projection["delivery"]["emitted_count"]["value"], 3);
    assert_eq!(projection["delivery"]["coalesced_count"]["value"], 1);
    assert_eq!(projection["delivery"]["dropped_count"]["value"], 2);
    assert_eq!(
        projection["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    Ok(())
}

#[test]
fn reconnect_preserves_delivered_progress_with_failed_final(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: delivered progress followed by an owner-observed final failure.
    let mut accounting = ReconnectDeliveryAccounting::default();
    accounting.observe(
        "websocket",
        Spec031ProgressDelivery::Live,
        ChannelDeliveryObservation {
            emitted: Some(4),
            reconnect_generation: Some(1),
            reconnect_gap: Some(false),
            ..ChannelDeliveryObservation::unavailable()
        },
    )?;
    accounting.observe(
        "websocket",
        Spec031ProgressDelivery::FinalFailed,
        ChannelDeliveryObservation::unavailable(),
    )?;

    // When: reconnect metadata advances independently.
    let mut tracker = ApiReconnectTracker::new(4);
    let mut first = tracker.connect_ws("spec035:accounting-failed");
    first.delivery = accounting;
    tracker.record_ws(&first, 2);
    let accounting = tracker.connect_ws("spec035:accounting-failed").delivery;
    let projection = accounting
        .projection()?
        .ok_or("missing accounting projection")?;

    // Then: progress remains delivered while terminal failure remains authoritative.
    assert_eq!(projection["delivery"]["progress"], json!(["live"]));
    assert_eq!(projection["delivery"]["emitted_count"]["value"], 4);
    assert_eq!(
        projection["delivery"]["dropped_count"]["availability"],
        "unavailable"
    );
    assert_eq!(
        projection["delivery"]["final_delivery"]["state"],
        "final_failed"
    );
    Ok(())
}

#[test]
fn unavailable_owner_counters_survive_reconnect() -> Result<(), Box<dyn std::error::Error>> {
    // Given: final owner state without progress accounting.
    let mut accounting = ReconnectDeliveryAccounting::default();
    accounting.observe(
        "websocket",
        Spec031ProgressDelivery::FinalFailed,
        ChannelDeliveryObservation::unavailable(),
    )?;

    // When: reconnect metadata changes.
    let mut tracker = ApiReconnectTracker::new(4);
    let mut first = tracker.connect_ws("spec035:accounting-unavailable");
    first.delivery = accounting;
    tracker.record_ws(&first, 1);
    let accounting = tracker
        .connect_ws("spec035:accounting-unavailable")
        .delivery;
    let projection = accounting
        .projection()?
        .ok_or("missing accounting projection")?;

    // Then: absent owner counters remain unavailable rather than zero/success.
    assert_eq!(
        projection["delivery"]["accepted_count"]["availability"],
        "unavailable"
    );
    assert_eq!(
        projection["delivery"]["emitted_count"]["availability"],
        "unavailable"
    );
    assert_eq!(
        projection["delivery"]["dropped_count"]["availability"],
        "unavailable"
    );
    assert_eq!(
        projection["delivery"]["final_delivery"]["state"],
        "final_failed"
    );
    Ok(())
}

#[tokio::test]
async fn slow_consumer_queue_stays_at_production_bound() {
    // Given: the production WebSocket event queue capacity.
    let (sender, mut receiver) =
        tokio::sync::mpsc::channel::<usize>(WEBSOCKET_EVENT_QUEUE_CAPACITY);

    // When: a consumer is paused and producers fill every bounded slot.
    for sequence in 0..WEBSOCKET_EVENT_QUEUE_CAPACITY {
        assert!(sender.try_send(sequence).is_ok());
    }

    // Then: one more event is rejected and terminal state is not synthesized by the queue.
    assert_eq!(receiver.len(), WEBSOCKET_EVENT_QUEUE_CAPACITY);
    assert_eq!(sender.capacity(), 0);
    assert!(matches!(
        sender.try_send(WEBSOCKET_EVENT_QUEUE_CAPACITY),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_))
    ));
    assert_eq!(receiver.recv().await, Some(0));
}
