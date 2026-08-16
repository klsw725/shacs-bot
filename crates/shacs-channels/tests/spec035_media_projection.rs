use serde_json::{json, Value};
use shacs_channels::{project_spec035_media_for_channel, ChannelSpec035MediaDelivery};
use shacs_projection::{
    Spec031Freshness, Spec035MediaOwnerFactsInput, Spec035MediaOwnerUnavailableReason,
    Spec035MediaProjection, Spec035MediaState, Spec035MediaValidationErrorKind,
};
use std::error::Error;

#[path = "spec035_media_projection/support.rs"]
mod support;
use support::{included_input, projection_for_state};

#[test]
fn channel_projection_preserves_every_canonical_media_state_losslessly(
) -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            Spec035MediaState::Included,
            ChannelSpec035MediaDelivery::Pending,
        ),
        (
            Spec035MediaState::Unsupported,
            ChannelSpec035MediaDelivery::Unknown,
        ),
        (
            Spec035MediaState::ExtractionFailed,
            ChannelSpec035MediaDelivery::Unknown,
        ),
        (
            Spec035MediaState::AnalyzerMissing,
            ChannelSpec035MediaDelivery::Unavailable,
        ),
        (
            Spec035MediaState::Truncated,
            ChannelSpec035MediaDelivery::Pending,
        ),
        (
            Spec035MediaState::Unavailable,
            ChannelSpec035MediaDelivery::Unavailable,
        ),
    ];

    for (state, delivery) in cases {
        let canonical = projection_for_state(state)?;
        let canonical_json = serde_json::to_value(&canonical)?;

        let channel = project_spec035_media_for_channel(canonical.clone());
        let channel_json = serde_json::to_value(&channel)?;

        assert_eq!(channel.media_capability(), &canonical);
        assert_eq!(channel.delivery_status(), delivery);
        assert_eq!(channel_json["media_capability"], canonical_json);
    }
    Ok(())
}

#[test]
fn channel_projection_never_invents_delivery_success_from_media_success(
) -> Result<(), Box<dyn Error>> {
    for state in [Spec035MediaState::Included, Spec035MediaState::Truncated] {
        let projection = project_spec035_media_for_channel(projection_for_state(state)?);
        let serialized = serde_json::to_string(&projection)?;

        assert_eq!(
            projection.delivery_status(),
            ChannelSpec035MediaDelivery::Pending
        );
        assert!(!serialized.contains("delivered"));
        assert!(!serialized.contains("sent_hint"));
        assert!(!serialized.contains("success"));
    }
    Ok(())
}

#[test]
fn channel_projection_keeps_unsupported_unavailable_and_untrusted_explicit(
) -> Result<(), Box<dyn Error>> {
    let unsupported = serde_json::to_value(project_spec035_media_for_channel(
        projection_for_state(Spec035MediaState::Unsupported)?,
    ))?;
    let unavailable = serde_json::to_value(project_spec035_media_for_channel(
        projection_for_state(Spec035MediaState::Unavailable)?,
    ))?;
    let untrusted = serde_json::to_value(project_spec035_media_for_channel(projection_for_state(
        Spec035MediaState::Included,
    )?))?;

    assert_eq!(
        unsupported["media_capability"]["state"],
        json!("unsupported")
    );
    assert_eq!(unsupported["delivery_status"], json!("unknown"));
    assert_eq!(
        unavailable["media_capability"]["state"],
        json!("unavailable")
    );
    assert_eq!(unavailable["media_capability"]["freshness"], json!("stale"));
    assert_eq!(unavailable["delivery_status"], json!("unavailable"));
    assert_eq!(
        untrusted["media_capability"]["owner_facts"]["analyzer_source"]["trust"],
        json!("unknown")
    );
    Ok(())
}

#[test]
fn malformed_and_misleading_media_are_rejected_before_channel_projection(
) -> Result<(), Box<dyn Error>> {
    let malformed = json!({
        "schema_version": 1,
        "kind": "media_capability",
        "state": "included",
        "raw_provider_payload": "secret"
    });
    assert!(Spec035MediaProjection::from_json_value(malformed).is_err());

    let mut misleading = included_input()?;
    misleading.owner_facts = Spec035MediaOwnerFactsInput {
        freshness: Spec031Freshness::Stale,
        unavailable_reasons: vec![Spec035MediaOwnerUnavailableReason::StaleOwnerFacts],
        facts: Vec::new(),
    };
    assert_eq!(
        Spec035MediaProjection::try_new(misleading)
            .expect_err("stale included media must be rejected")
            .kind(),
        Spec035MediaValidationErrorKind::MisleadingSuccess
    );
    Ok(())
}

#[test]
fn channel_projection_is_deterministic_and_contains_no_forbidden_payload_fields(
) -> Result<(), Box<dyn Error>> {
    let canonical = projection_for_state(Spec035MediaState::Included)?;
    let expected = serde_json::to_string(&project_spec035_media_for_channel(canonical.clone()))?;

    for _ in 0..100 {
        assert_eq!(
            serde_json::to_string(&project_spec035_media_for_channel(canonical.clone()))?,
            expected
        );
    }
    let value: Value = serde_json::from_str(&expected)?;
    assert_eq!(value["media_capability"]["schema_version"], json!(1));
    for forbidden in [
        "raw_provider_payload",
        "https://",
        "Bearer ",
        "/Users/",
        "secret-token",
    ] {
        assert!(!expected.contains(forbidden));
    }
    Ok(())
}
