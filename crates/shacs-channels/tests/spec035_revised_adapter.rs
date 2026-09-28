use shacs_channels::{
    project_spec031_channel_event, project_spec035_revised_channel_event,
    project_spec035_revised_json_for_channel, ChannelSpec031ProjectionInput, WebSocketServerEvent,
};
use shacs_projection::{Spec030RuntimeProjection, Spec030UnavailableReason};
use std::error::Error;

const SENSITIVE_VALUES: [(&str, &str); 6] = [
    ("secret", "sk-spec035-adapter-secret"),
    (
        "credential_url",
        "https://user:password@example.invalid/path",
    ),
    ("absolute_path", "/tmp/spec035-private"),
    ("process_handle", "spec035-process-handle-4242"),
    ("stdout", "spec035-raw-stdout"),
    ("stderr", "spec035-raw-stderr"),
];

#[test]
fn channel_adapter_preserves_canonical_projection_and_rejects_sensitive_input(
) -> Result<(), Box<dyn Error>> {
    // Given: an actual channel owner envelope and independent forged sensitive fields.
    let delivery = project_spec031_channel_event(ChannelSpec031ProjectionInput::websocket_event(
        WebSocketServerEvent::StreamEnd {
            chat_id: "chat-a".to_owned(),
            stream_id: None,
        },
    ))?;
    let trusted_runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    let canonical = serde_json::to_string(&project_spec035_revised_channel_event(
        &trusted_runtime,
        &delivery,
    )?)?;

    // When: the owner output crosses the channel adapter boundary.
    let projected = project_spec035_revised_json_for_channel(&canonical)?;

    // Then: canonical fields are unchanged and every sensitive field is rejected independently.
    assert_eq!(
        serde_json::to_value(projected)?,
        serde_json::from_str::<serde_json::Value>(&canonical)?
    );
    for (field, sentinel) in SENSITIVE_VALUES {
        let mut sensitive: serde_json::Value = serde_json::from_str(&canonical)?;
        sensitive[field] = sentinel.into();
        let rejected =
            project_spec035_revised_json_for_channel(&serde_json::to_string(&sensitive)?)
                .expect_err("unsafe fixture must fail");
        assert!(!rejected.to_string().contains(sentinel));
    }
    Ok(())
}
