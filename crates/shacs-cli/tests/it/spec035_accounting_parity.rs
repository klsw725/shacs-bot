use shacs_api::spec035_revised_projection_json_response;
use shacs_channels::{
    project_spec031_channel_event, project_spec035_revised_json_for_channel,
    ChannelDeliveryObservation, ChannelSpec031ProjectionInput, WebSocketServerEvent,
    WEBSOCKET_CHANNEL,
};
use shacs_cli::render_spec035_revised_json;
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec031ProgressDelivery, Spec035OwnerSurface, Spec035RevisedOwnerFacts,
};
use std::error::Error;

#[test]
fn cli_api_websocket_and_channel_preserve_canonical_accounting_fixture(
) -> Result<(), Box<dyn Error>> {
    let progress = project_spec031_channel_event(
        ChannelSpec031ProjectionInput::progress_delivery(
            WEBSOCKET_CHANNEL,
            Spec031ProgressDelivery::Dropped,
            None,
        )
        .with_delivery_observation(ChannelDeliveryObservation {
            queue_depth: Some(64),
            queue_capacity: Some(64),
            accepted: Some(65),
            emitted: Some(64),
            coalesced: Some(2),
            dropped: Some(1),
            reconnect_generation: Some(2),
            reconnect_gap: Some(true),
            slow_consumer: Some(1),
        }),
    )?;
    let final_delivery = project_spec031_channel_event(
        ChannelSpec031ProjectionInput::websocket_event(WebSocketServerEvent::Message {
            chat_id: "chat-parity".to_owned(),
            text: "final".to_owned(),
            buttons: Vec::new(),
            button_prompt: None,
            media: Vec::new(),
            reply_to: None,
            kind: None,
        }),
    )?;
    let runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    let canonical = serde_json::to_value(project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Channel, &runtime)
            .with_progress_delivery(&progress)
            .with_final_delivery(&final_delivery),
    )?)?;
    let serialized = serde_json::to_string(&canonical)?;

    let cli =
        serde_json::from_str::<serde_json::Value>(&render_spec035_revised_json(&serialized)?)?;
    let api = spec035_revised_projection_json_response(&serialized);
    let channel = serde_json::to_value(project_spec035_revised_json_for_channel(&serialized)?)?;

    assert_eq!(cli, canonical);
    assert_eq!(api.status, 200);
    assert_eq!(api.body, canonical);
    assert_eq!(channel, canonical);
    assert_eq!(
        canonical["delivery"]["progress"],
        serde_json::json!(["dropped"])
    );
    assert_eq!(canonical["delivery"]["coalesced_count"]["value"], 2);
    assert_eq!(
        canonical["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    Ok(())
}
