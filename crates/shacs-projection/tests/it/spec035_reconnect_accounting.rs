use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031Count, Spec031Envelope,
    Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage, Spec031ObservedAtUnixMs,
    Spec031ProgressCapability, Spec031ProgressDelivery, Spec031ProjectionKind, Spec031Reason,
    Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Severity, Spec031Source,
    Spec031SourceOwner, Spec031SubjectRef, Spec035OwnerSurface, Spec035RevisedOwnerFacts,
};
use std::error::Error;

fn delivery(
    state: Spec031ProgressDelivery,
    emitted: Option<u64>,
    dropped: Option<u64>,
) -> Result<Spec031Envelope, Box<dyn Error>> {
    let mut capability = Spec031ProgressCapability::delivery(state);
    capability.emitted = emitted.map(Spec031Count::new);
    capability.dropped = dropped.map(Spec031Count::new);
    Ok(Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Progress,
        state: Spec031Availability::Degraded,
        severity: Spec031Severity::Info,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Degraded,
            safe_summary: Spec031SafeSummary::try_new("reconnect delivery accounting")?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new("subject:channel:websocket:progress")?,
            parent_ref: None,
            action_ref: Some(Spec031ActionRef::try_new(
                "action:channel:websocket:stream",
            )?),
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Channel,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(35)),
            freshness: Spec031Freshness::Current,
        },
        capability: Spec031Capability::Progress(capability),
        children: Vec::new(),
    })?)
}

#[test]
fn dropped_progress_and_final_delivered_coexist() -> Result<(), Box<dyn Error>> {
    // Given: independent progress loss and final-delivery owner facts.
    let progress = delivery(Spec031ProgressDelivery::Dropped, Some(3), Some(2))?;
    let final_delivery = delivery(Spec031ProgressDelivery::FinalDelivered, None, None)?;
    let runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);

    // When: both facts cross the revised projection boundary.
    let projection = project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Websocket, &runtime)
            .with_progress_delivery(&progress)
            .with_final_delivery(&final_delivery),
    )?;
    let value = serde_json::to_value(projection)?;

    // Then: loss is not cancelled by final delivery.
    assert_eq!(value["delivery"]["progress"][0], "dropped");
    assert_eq!(value["delivery"]["emitted_count"]["value"], 3);
    assert_eq!(value["delivery"]["dropped_count"]["value"], 2);
    assert_eq!(
        value["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    Ok(())
}

#[test]
fn delivered_progress_and_final_failed_coexist() -> Result<(), Box<dyn Error>> {
    // Given: delivered progress and a later owner-scoped final failure.
    let progress = delivery(Spec031ProgressDelivery::Live, Some(4), Some(0))?;
    let final_delivery = delivery(Spec031ProgressDelivery::FinalFailed, None, None)?;
    let runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);

    // When: both facts cross the revised projection boundary.
    let projection = project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Websocket, &runtime)
            .with_progress_delivery(&progress)
            .with_final_delivery(&final_delivery),
    )?;
    let value = serde_json::to_value(projection)?;

    // Then: progress delivery never implies terminal success.
    assert_eq!(value["delivery"]["progress"][0], "live");
    assert_eq!(value["delivery"]["emitted_count"]["value"], 4);
    assert_eq!(value["delivery"]["dropped_count"]["value"], 0);
    assert_eq!(value["delivery"]["final_delivery"]["state"], "final_failed");
    Ok(())
}

#[test]
fn unavailable_progress_counters_remain_unavailable() -> Result<(), Box<dyn Error>> {
    // Given: progress and final facts without owner-supplied counters.
    let progress = delivery(Spec031ProgressDelivery::Live, None, None)?;
    let final_delivery = delivery(Spec031ProgressDelivery::FinalPending, None, None)?;
    let runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);

    // When: reconnect projection combines the independent facts.
    let projection = project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Websocket, &runtime)
            .with_progress_delivery(&progress)
            .with_final_delivery(&final_delivery),
    )?;
    let value = serde_json::to_value(projection)?;

    // Then: missing counters are unavailable, never fabricated zeroes.
    assert_eq!(
        value["delivery"]["emitted_count"]["availability"],
        "unavailable"
    );
    assert_eq!(
        value["delivery"]["dropped_count"]["availability"],
        "unavailable"
    );
    assert_eq!(
        value["delivery"]["final_delivery"]["state"],
        "final_pending"
    );
    Ok(())
}
