use serde_json::{json, Value};
use shacs_channels::project_spec035_media_for_channel;
use shacs_projection::{
    Spec031Freshness, Spec035MediaOwnerFactsInput, Spec035MediaOwnerUnavailableReason,
    Spec035MediaProjection, Spec035MediaState,
};
use std::error::Error;

#[path = "spec034_media_projection_fixture/support.rs"]
mod support;
use support::{included_input, projection_for_state};

fn main() -> Result<(), Box<dyn Error>> {
    let states = [
        Spec035MediaState::Included,
        Spec035MediaState::Unsupported,
        Spec035MediaState::ExtractionFailed,
        Spec035MediaState::AnalyzerMissing,
        Spec035MediaState::Truncated,
        Spec035MediaState::Unavailable,
    ];
    let mut projections = Vec::with_capacity(states.len());
    let mut canonical_match = true;
    for state in states {
        let canonical = projection_for_state(state)?;
        let canonical_json = serde_json::to_value(&canonical)?;
        let channel = project_spec035_media_for_channel(canonical);
        let channel_json = serde_json::to_value(channel)?;
        canonical_match &= channel_json["media_capability"] == canonical_json;
        projections.push(channel_json);
    }

    let malformed_rejected = Spec035MediaProjection::from_json_value(json!({
        "schema_version": 1,
        "kind": "media_capability",
        "state": "included",
        "raw_provider_payload": "forbidden"
    }))
    .is_err();
    let mut misleading = included_input()?;
    misleading.owner_facts = Spec035MediaOwnerFactsInput {
        freshness: Spec031Freshness::Stale,
        unavailable_reasons: vec![Spec035MediaOwnerUnavailableReason::StaleOwnerFacts],
        facts: Vec::new(),
    };
    let misleading_rejected = Spec035MediaProjection::try_new(misleading).is_err();
    let encoded = serde_json::to_string(&projections)?;
    let output = json!({
        "canonical_match": canonical_match,
        "projections": projections,
        "probes": {
            "malformed_rejected": malformed_rejected,
            "misleading_rejected": misleading_rejected,
            "deterministic": encoded == serde_json::to_string(&serde_json::from_str::<Value>(&encoded)?)?,
            "delivery_success_claimed": encoded.contains("delivered") || encoded.contains("sent_hint") || encoded.contains("success"),
            "external_calls": false,
            "bounded_iterations": states.len()
        }
    });
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
