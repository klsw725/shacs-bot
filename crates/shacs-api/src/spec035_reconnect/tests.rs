use super::*;
use shacs_core::runtime::{assemble_spec035_tasks, Spec035TasksOwnerSnapshots};
use shacs_projection::Spec031Freshness;

#[test]
fn spec035_f2_stale_queue_failure_cannot_overwrite_current_delivery(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tracker = crate::ApiReconnectTracker::new(4);
    let first = tracker.connect_ws("spec035:queue-overlap");
    let mut second = tracker.connect_ws(&first.key);
    second.delivery.observe(
        "websocket",
        shacs_projection::Spec031ProgressDelivery::FinalDelivered,
        shacs_channels::ChannelDeliveryObservation::unavailable(),
    )?;
    tracker.record_ws(&second, 1);

    tracker.observe_ws_delivery(
        &first.key,
        shacs_projection::Spec031ProgressDelivery::FinalFailed,
        shacs_channels::ChannelDeliveryObservation {
            reconnect_generation: Some(first.generation),
            dropped: Some(1),
            ..shacs_channels::ChannelDeliveryObservation::unavailable()
        },
    )?;

    let third = tracker.connect_ws(&first.key);
    assert_eq!(
        third.delivery.projection()?.ok_or("accounting")?["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    Ok(())
}

#[test]
fn spec035_f2_queue_producer_and_socket_share_fenced_generation(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        ApiRouterState, NoopMediaAdapter, WebSocketQueueAccounting, WebSocketReconnectScope,
    };
    use shacs_channels::{ChannelDeliveryObservation, WebSocketServerEvent};
    use shacs_projection::Spec031ProgressDelivery;
    use std::sync::Arc;

    for chat_id in ["default", "named:chat"] {
        let state = ApiRouterState::new(Arc::new(NoopMediaAdapter));
        let mut scope = WebSocketReconnectScope::new("client:queue", Some("session:queue"));
        scope.connection_for(&state.reconnect_tracker, "default", "default");
        let mut queue = WebSocketQueueAccounting::new(&state, &scope, "default");
        let event = WebSocketServerEvent::Message {
            chat_id: chat_id.to_owned(),
            text: "final".to_owned(),
            buttons: Vec::new(),
            button_prompt: None,
            media: Vec::new(),
            reply_to: None,
            kind: None,
        };
        let queued_generation = queue.generation_for(&event);
        let first = scope.connection_for(&state.reconnect_tracker, chat_id, "default");
        assert_eq!(queued_generation, first.generation);
        let key = first.key.clone();
        let observation = ChannelDeliveryObservation {
            reconnect_generation: Some(queued_generation),
            dropped: Some(1),
            ..ChannelDeliveryObservation::unavailable()
        };
        queue.observe_failure(&event, observation);
        let mut second = state
            .reconnect_tracker
            .lock()
            .expect("tracker")
            .connect_ws(&key);
        assert_eq!(
            second
                .delivery
                .projection()?
                .ok_or("current queue failure")?["delivery"]["final_delivery"]["state"],
            "final_failed"
        );
        second.delivery.observe(
            "websocket",
            Spec031ProgressDelivery::FinalDelivered,
            ChannelDeliveryObservation::unavailable(),
        )?;
        state
            .reconnect_tracker
            .lock()
            .expect("tracker")
            .record_ws(&second, 1);

        queue.observe_failure(&event, observation);

        assert_eq!(queue.generation_for(&event), queued_generation);
        let third = state
            .reconnect_tracker
            .lock()
            .expect("tracker")
            .connect_ws(&key);
        assert_eq!(third.generation, 3);
        assert_eq!(
            third.delivery.projection()?.ok_or("current delivery")?["delivery"]["final_delivery"]
                ["state"],
            "final_delivered"
        );
    }
    Ok(())
}

#[test]
fn spec035_f2_colon_pairs_have_independent_reconnect_state(
) -> Result<(), Box<dyn std::error::Error>> {
    let first = crate::spec035_websocket_identity(Some("client_id=client:a&session_id=b:c"))?
        .ok_or("first identity")?;
    let second = crate::spec035_websocket_identity(Some("client_id=client:a:b&session_id=c"))?
        .ok_or("second identity")?;
    let first_key = stable_reconnect_key(&first.0, &first.1);
    let second_key = stable_reconnect_key(&second.0, &second.1);
    let mut tracker = crate::ApiReconnectTracker::new(4);
    let mut first_connection = tracker.connect_ws(&first_key);
    first_connection.delivery.observe(
        "websocket",
        shacs_projection::Spec031ProgressDelivery::FinalFailed,
        shacs_channels::ChannelDeliveryObservation::unavailable(),
    )?;
    tracker.record_ws(&first_connection, 1);

    let second_connection = tracker.connect_ws(&second_key);

    assert_ne!(first_key, second_key);
    assert_eq!(second_connection.generation, 1);
    assert!(!second_connection.gap);
    assert!(second_connection.delivery.projection()?.is_none());
    let first_scope = crate::WebSocketReconnectScope::new(&first.0, None);
    let second_scope = crate::WebSocketReconnectScope::new(&second.0, None);
    assert_ne!(
        first_scope.key_for(&first.1, "default"),
        second_scope.key_for(&second.1, "default")
    );
    Ok(())
}

fn payload() -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::to_value(assemble_spec035_tasks(
        Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable),
    )?)?)
}

fn event(
    generation: Spec035TransportGeneration,
    sequence: u64,
    payload: Value,
) -> Result<Spec035TasksStreamEvent, Box<dyn std::error::Error>> {
    Ok(Spec035TasksStreamEvent::new(
        serde_json::to_value(Spec035TransportEventMetadata::try_new(
            Spec035TransportEventKind::Delta,
            generation,
            Spec035TransportSequence::new(sequence),
        )?)?,
        payload,
    ))
}

#[test]
fn transport_rejections_only_reach_accounting_consumer() -> Result<(), Box<dyn std::error::Error>> {
    let generation = transport_generation(2)?;
    let server = server_hello(&generation)?;
    let mut connection = Spec035ReconnectConnection {
        ordering: Spec035TransportOrdering::new(),
        generation: generation.clone(),
        next_sequence: 2,
        reconnect_gap: false,
    };
    assert_eq!(
        connection.ordering.accept_hello(&server),
        Spec035TransportDecision::AcceptHello
    );
    let pre_snapshot = connection.observe(event(generation.clone(), 1, payload()?)?);
    assert_eq!(pre_snapshot["reason"], "reject_pre_snapshot");
    assert_eq!(pre_snapshot["counters"]["pre_snapshot_deltas"], 1);

    let snapshot = Spec035TransportEventMetadata::try_new(
        Spec035TransportEventKind::Snapshot,
        generation.clone(),
        Spec035TransportSequence::new(1),
    )?;
    assert_eq!(
        connection.ordering.observe(&snapshot),
        Spec035TransportDecision::ApplySnapshot
    );
    let invalid = connection.observe(event(generation.clone(), 2, json!({"schema_version":999}))?);
    let stale = connection.observe(event(transport_generation(1)?, 2, payload()?)?);
    let duplicate = connection.observe(event(generation.clone(), 1, payload()?)?);
    let gap = connection.observe(event(generation.clone(), 3, payload()?)?);
    let applied = connection.observe(event(generation, 2, payload()?)?);

    assert_eq!(invalid["reason"], "reject_invalid_payload");
    assert_eq!(stale["reason"], "reject_stale_generation");
    assert_eq!(duplicate["reason"], "reject_duplicate");
    assert_eq!(gap["reason"], "reject_gap");
    assert_eq!(applied["type"], "tasks_delta");
    assert_eq!(applied["counters"]["applied_deltas"], 1);
    if let Some(root) = std::env::var_os("SPEC035_RECONNECT_ARTIFACT_DIR") {
        std::fs::create_dir_all(&root)?;
        std::fs::write(
            std::path::PathBuf::from(root).join("state-machine.json"),
            serde_json::to_vec_pretty(&json!({
                "pre_snapshot": pre_snapshot,
                "invalid_payload": invalid,
                "stale_generation": stale,
                "duplicate_sequence": duplicate,
                "sequence_gap": gap,
                "applied_delta": applied,
            }))?,
        )?;
    }
    Ok(())
}

#[test]
fn client_id_validation_rejects_unstable_or_unbounded_values() {
    assert!(parse_client_id("").is_err());
    assert!(parse_client_id("client/with/path").is_err());
    assert!(parse_client_id(&"x".repeat(129)).is_err());
    assert!(parse_client_id("client:stable-1").is_ok());
}
