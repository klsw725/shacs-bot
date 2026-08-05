use serde_json::{json, Map, Value};
use shacs_channels::{
    channel_delivery_observation_from_metadata, project_spec031_channel_event,
    ChannelDeliveryObservation, ChannelSpec031ProjectionInput, ChannelSpec031ProjectionKind,
    OutboundMessage, WebSocketServerEvent, WEBSOCKET_CHANNEL,
};
use shacs_projection::spec031::{
    Spec031Availability, Spec031ProgressDelivery, Spec031ReasonCode, Spec031Severity,
};
use std::error::Error;

#[test]
fn spec031_websocket_progress_and_final_are_distinct() -> Result<(), Box<dyn Error>> {
    let progress = project_spec031_channel_event(ChannelSpec031ProjectionInput::websocket_event(
        WebSocketServerEvent::Delta {
            chat_id: "chat-a".to_owned(),
            text: "delta".to_owned(),
            stream_id: Some("stream-a".to_owned()),
        },
    ))?;
    let stream_end = project_spec031_channel_event(
        ChannelSpec031ProjectionInput::websocket_event(WebSocketServerEvent::StreamEnd {
            chat_id: "chat-a".to_owned(),
            stream_id: Some("stream-a".to_owned()),
        }),
    )?;
    let final_message = project_spec031_channel_event(
        ChannelSpec031ProjectionInput::websocket_event(WebSocketServerEvent::Message {
            chat_id: "chat-a".to_owned(),
            text: "final".to_owned(),
            buttons: Vec::new(),
            button_prompt: None,
            media: Vec::new(),
            reply_to: Some("reply-1".to_owned()),
            kind: None,
        }),
    )?;

    assert_eq!(progress.state(), Spec031Availability::Degraded);
    assert_eq!(progress.reason().code, Spec031ReasonCode::Degraded);
    assert_eq!(stream_end.state(), Spec031Availability::Degraded);
    assert_eq!(stream_end.reason().code, Spec031ReasonCode::Degraded);
    assert_eq!(final_message.state(), Spec031Availability::Ready);
    assert_eq!(final_message.reason().code, Spec031ReasonCode::Included);
    assert_eq!(
        progress.lineage().subject_ref.as_str(),
        "subject:channel:websocket:delta"
    );
    assert_eq!(
        stream_end.lineage().subject_ref.as_str(),
        "subject:channel:websocket:stream_end"
    );
    assert_eq!(
        final_message
            .lineage()
            .parent_ref
            .as_ref()
            .map(|value| value.as_str()),
        Some("parent:channel:websocket:chat:chat-a")
    );
    assert_eq!(
        final_message
            .lineage()
            .action_ref
            .as_ref()
            .map(|value| value.as_str()),
        Some("action:channel:websocket:reply:reply-1")
    );

    let progress_json = serde_json::to_value(progress)?;
    let stream_end_json = serde_json::to_value(stream_end)?;
    let final_json = serde_json::to_value(final_message)?;
    assert_eq!(
        progress_json["capability"]["details"]["delivery"],
        json!("live")
    );
    assert_eq!(
        stream_end_json["capability"]["details"]["delivery"],
        json!("final_pending")
    );
    assert_eq!(
        final_json["capability"]["details"]["delivery"],
        json!("final_delivered")
    );
    Ok(())
}

#[test]
fn spec031_external_channel_final_preserves_reply_thread_metadata() -> Result<(), Box<dyn Error>> {
    let mut metadata = Map::new();
    metadata.insert("thread_id".to_owned(), Value::String("thread-1".to_owned()));
    let envelope = project_spec031_channel_event(ChannelSpec031ProjectionInput::external_final(
        OutboundMessage::new("discord", "chat-a", "done")
            .with_reply_to("message-1")
            .with_metadata(metadata),
    ))?;

    assert_eq!(envelope.state(), Spec031Availability::Ready);
    assert_eq!(envelope.severity(), Spec031Severity::Info);
    assert_eq!(envelope.reason().code, Spec031ReasonCode::Included);
    assert_eq!(
        envelope
            .lineage()
            .parent_ref
            .as_ref()
            .map(|value| value.as_str()),
        Some("parent:channel:discord:thread")
    );
    assert_eq!(
        envelope
            .lineage()
            .action_ref
            .as_ref()
            .map(|value| value.as_str()),
        Some("action:channel:discord:reply")
    );
    Ok(())
}

#[test]
fn spec031_external_channel_delivery_metadata_projects_conservative_statuses(
) -> Result<(), Box<dyn Error>> {
    let pending = external_delivery_status("pending")?;
    let unknown = external_delivery_status("unknown")?;
    let failed = external_delivery_status("failed_hint")?;

    assert_eq!(pending.state(), Spec031Availability::Degraded);
    assert_eq!(unknown.state(), Spec031Availability::Unknown);
    assert_eq!(failed.state(), Spec031Availability::Blocked);

    let pending_json = serde_json::to_value(pending)?;
    let unknown_json = serde_json::to_value(unknown)?;
    let failed_json = serde_json::to_value(failed)?;
    assert_eq!(
        pending_json["capability"]["details"]["delivery"],
        json!("final_pending")
    );
    assert_eq!(
        unknown_json["capability"]["details"]["delivery"],
        json!("final_unknown")
    );
    assert_eq!(
        failed_json["capability"]["details"]["delivery"],
        json!("final_failed")
    );
    assert_eq!(
        pending_json["lineage"]["parent_ref"],
        json!("parent:channel:discord:thread")
    );
    assert!(unknown_json["reason"]["safe_summary"]
        .as_str()
        .unwrap_or_default()
        .contains("unknown"));
    Ok(())
}

#[test]
fn spec031_external_channel_parses_real_delivery_metadata_records() -> Result<(), Box<dyn Error>> {
    let sent = external_delivery_record(json!({
        "status": "sent",
        "delivery_projection_status": "sent_hint",
        "writer_instance_ref": "process:current"
    }))?;
    let dedupe = external_delivery_record(json!({
        "status": "pending",
        "delivery_projection_status": "dedupe_candidate",
        "work_ref": "work:opaque"
    }))?;
    let malformed = external_delivery_record(json!({
        "status": "not-a-real-status",
        "writer_instance_ref": "process:prior"
    }))?;

    let sent_json = serde_json::to_value(sent)?;
    let dedupe_json = serde_json::to_value(dedupe)?;
    let malformed_json = serde_json::to_value(malformed)?;
    assert_eq!(
        sent_json["capability"]["details"]["delivery"],
        json!("final_delivered")
    );
    assert_eq!(sent_json["capability"]["details"]["emitted"], json!(1));
    assert_eq!(
        dedupe_json["capability"]["details"]["delivery"],
        json!("final_unknown")
    );
    assert_eq!(dedupe_json["capability"]["details"]["coalesced"], json!(1));
    assert_eq!(
        malformed_json["capability"]["details"]["delivery"],
        json!("final_unknown")
    );
    assert_eq!(malformed_json["state"], json!("unavailable"));

    let failed_observation = channel_delivery_observation_from_metadata(&json!({
        "delivery_projection_status": "failed_hint"
    }));
    assert_eq!(failed_observation.dropped, Some(1));
    assert_eq!(failed_observation.slow_consumer, Some(1));
    Ok(())
}

#[test]
fn spec031_unsupported_malformed_and_disconnect_are_explicit() -> Result<(), Box<dyn Error>> {
    let unsupported = project_spec031_channel_event(ChannelSpec031ProjectionInput::unsupported(
        "telegram",
        "send_delta",
    ))?;
    let malformed = project_spec031_channel_event(ChannelSpec031ProjectionInput::malformed_frame(
        WEBSOCKET_CHANNEL,
        "websocket frame must be valid JSON",
    ))?;
    let disconnected = project_spec031_channel_event(ChannelSpec031ProjectionInput::disconnected(
        WEBSOCKET_CHANNEL,
        Some("chat-a"),
    ))?;

    assert_eq!(unsupported.state(), Spec031Availability::Unavailable);
    assert_eq!(unsupported.reason().code, Spec031ReasonCode::Unsupported);
    assert_eq!(malformed.state(), Spec031Availability::Blocked);
    assert_eq!(malformed.reason().code, Spec031ReasonCode::ExtractionFailed);
    assert_eq!(disconnected.state(), Spec031Availability::Blocked);
    assert_eq!(disconnected.reason().code, Spec031ReasonCode::Blocked);

    let disconnected_json = serde_json::to_value(disconnected)?;
    assert_eq!(
        disconnected_json["capability"]["details"]["delivery"],
        json!("final_failed")
    );
    assert_eq!(
        disconnected_json["lineage"]["parent_ref"],
        json!("parent:channel:websocket:chat:chat-a")
    );
    Ok(())
}

#[test]
fn spec031_channel_projection_rejects_raw_payload_path_and_ref_leaks() {
    let leaky = ChannelSpec031ProjectionInput {
        kind: ChannelSpec031ProjectionKind::Unsupported {
            channel: "slack".to_owned(),
            capability: "/Users/spec031-channel-path-secret/raw.json".to_owned(),
        },
        observed_at_unix_ms: Some(31),
        delivery_observation: ChannelDeliveryObservation::unavailable(),
    };

    let error = project_spec031_channel_event(leaky).expect_err("raw path must be rejected");
    let rendered = error.to_string();
    assert!(!rendered.contains("spec031-channel-path-secret"));
}

#[test]
fn spec031_channel_projection_keeps_degraded_unknown_and_dropped_words(
) -> Result<(), Box<dyn Error>> {
    let coalesced =
        project_spec031_channel_event(ChannelSpec031ProjectionInput::progress_delivery(
            "websocket",
            Spec031ProgressDelivery::Dropped,
            Some("stream-a"),
        ))?;

    assert_eq!(coalesced.state(), Spec031Availability::Degraded);
    assert_eq!(coalesced.reason().code, Spec031ReasonCode::Degraded);
    assert!(coalesced.reason().safe_summary.as_str().contains("dropped"));
    Ok(())
}

#[test]
fn spec031_channel_projection_preserves_independent_delivery_accounting(
) -> Result<(), Box<dyn Error>> {
    let envelope = project_spec031_channel_event(
        ChannelSpec031ProjectionInput::progress_delivery(
            "websocket",
            Spec031ProgressDelivery::FinalDelivered,
            Some("stream-a"),
        )
        .with_delivery_observation(ChannelDeliveryObservation {
            queue_depth: Some(1),
            queue_capacity: Some(4),
            accepted: Some(5),
            emitted: Some(4),
            coalesced: Some(2),
            dropped: Some(1),
            reconnect_generation: Some(3),
            reconnect_gap: Some(true),
            slow_consumer: Some(1),
        }),
    )?;

    let projected = serde_json::to_value(envelope)?;
    let details = &projected["capability"]["details"];
    assert_eq!(details["delivery"], json!("final_delivered"));
    assert_eq!(details["coalesced"], json!(2));
    assert_eq!(details["dropped"], json!(1));
    assert_eq!(details["reconnect_generation"], json!(3));
    assert_eq!(details["reconnect_gap"], json!(true));
    assert_eq!(details["slow_consumer"], json!(1));
    Ok(())
}

#[test]
fn spec031_channel_projection_keeps_unavailable_counters_missing_not_zero(
) -> Result<(), Box<dyn Error>> {
    let envelope = project_spec031_channel_event(
        ChannelSpec031ProjectionInput::progress_delivery(
            "websocket",
            Spec031ProgressDelivery::Dropped,
            Some("stream-a"),
        )
        .with_delivery_observation(ChannelDeliveryObservation::unavailable()),
    )?;

    let projected = serde_json::to_value(envelope)?;
    let details = projected["capability"]["details"]
        .as_object()
        .ok_or("capability details should be an object")?;
    assert!(!details.contains_key("dropped"));
    assert!(!details.contains_key("coalesced"));
    assert!(!details.contains_key("queue_depth"));
    Ok(())
}

fn external_delivery_status(
    status: &str,
) -> Result<shacs_projection::Spec031Envelope, Box<dyn Error>> {
    let mut metadata = Map::new();
    metadata.insert("thread_id".to_owned(), Value::String("thread-1".to_owned()));
    metadata.insert(
        "delivery_status".to_owned(),
        Value::String(status.to_owned()),
    );
    Ok(project_spec031_channel_event(
        ChannelSpec031ProjectionInput::external_final(
            OutboundMessage::new("discord", "chat-a", "done")
                .with_reply_to("message-1")
                .with_metadata(metadata),
        ),
    )?)
}

fn external_delivery_record(
    record: Value,
) -> Result<shacs_projection::Spec031Envelope, Box<dyn Error>> {
    let metadata = record
        .as_object()
        .cloned()
        .ok_or("delivery record fixture must be an object")?;
    Ok(project_spec031_channel_event(
        ChannelSpec031ProjectionInput::external_final(
            OutboundMessage::new("discord", "chat-a", "done").with_metadata(metadata),
        ),
    )?)
}
