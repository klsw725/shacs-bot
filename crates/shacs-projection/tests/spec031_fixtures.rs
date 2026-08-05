use serde_json::json;
use shacs_projection::*;
use std::error::Error;

#[test]
fn spec031_canonical_fixture_registry_covers_todo4_families() -> Result<(), Box<dyn Error>> {
    let registry = spec031_canonical_fixture_registry()?;

    assert_eq!(registry.len(), 13);
    assert!(registry
        .iter()
        .any(|fixture| fixture.family() == Spec031FixtureFamily::Recovery));
    assert!(registry
        .iter()
        .any(|fixture| fixture.family() == Spec031FixtureFamily::Extension));
    assert!(registry
        .iter()
        .any(|fixture| fixture.family() == Spec031FixtureFamily::Delivery));

    for fixture in registry {
        let envelope = fixture.envelope();
        assert_eq!(envelope.schema_version(), Spec031SchemaVersion::CURRENT);
        assert_ne!(envelope.reason().safe_summary.as_str(), "");
        assert_ne!(envelope.lineage().subject_ref.as_str(), "");
        assert!(envelope.source().observed_at_unix_ms.is_some());
        assert_ne!(envelope.source().freshness, Spec031Freshness::Unknown);
    }

    Ok(())
}

#[test]
fn spec031_canonical_fixtures_preserve_zero_distinct_from_missing() -> Result<(), Box<dyn Error>> {
    let registry = spec031_canonical_fixture_registry()?;
    let session = registry
        .iter()
        .find(|fixture| fixture.family() == Spec031FixtureFamily::Session)
        .ok_or("missing session fixture")?
        .envelope();
    let turn = registry
        .iter()
        .find(|fixture| fixture.family() == Spec031FixtureFamily::Turn)
        .ok_or("missing turn fixture")?
        .envelope();
    let serialized_session = serde_json::to_value(session)?;
    let serialized_turn = serde_json::to_value(turn)?;

    assert_eq!(
        serialized_session["capability"]["details"]["active_turn_count"],
        json!(0)
    );
    assert!(serialized_turn["capability"]["details"]
        .get("turn_index")
        .is_none());

    Ok(())
}

#[test]
fn spec031_owner_missing_external_evidence_blocks_app_media_and_readiness(
) -> Result<(), Box<dyn Error>> {
    for family in [
        Spec031FixtureFamily::ExternalAppOwner,
        Spec031FixtureFamily::ExternalMediaOwner,
        Spec031FixtureFamily::Readiness,
    ] {
        let envelope = spec031_missing_external_owner_evidence(family)?;
        let serialized = serde_json::to_value(&envelope)?;

        assert_ne!(envelope.state(), Spec031Availability::Ready);
        assert_eq!(envelope.state(), Spec031Availability::Unavailable);
        assert_eq!(envelope.severity(), Spec031Severity::Error);
        assert_eq!(
            envelope.reason().code,
            Spec031ReasonCode::MissingExternalOwnerEvidence
        );
        assert_ne!(envelope.reason().safe_summary.as_str(), "");
        let parsed = Spec031Envelope::from_json_value(serialized)?;
        assert_eq!(
            parsed.reason().code,
            Spec031ReasonCode::MissingExternalOwnerEvidence
        );
        assert_eq!(parsed, envelope);
    }

    Ok(())
}

#[test]
fn spec031_owner_record_conversion_accepts_safe_summary_only() -> Result<(), Box<dyn Error>> {
    let envelope = spec031_project_owner_record(Spec031OwnerRecordProjectionInput {
        family: Spec031FixtureFamily::Tool,
        subject_ref: Spec031SubjectRef::try_new("subject:tool:owner-record")?,
        parent_ref: Some(Spec031ParentRef::try_new("parent:turn:owner-record")?),
        action_ref: Some(Spec031ActionRef::try_new("action:tool:owner-record")?),
        digest: Some(Spec031Digest::try_new("sha256:toolownerrecord")?),
        owner: Spec031SourceOwner::Spec030,
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
        freshness: Spec031Freshness::Current,
        state: Spec031Availability::Blocked,
        severity: Spec031Severity::Warning,
        reason_code: Spec031ReasonCode::Blocked,
        safe_summary: Spec031SafeSummary::try_new("tool owner blocked safely")?,
        capability: Spec031Capability::Tool(Spec031ToolCapability {
            attempt_count: Some(Spec031Count::new(0)),
        }),
    })?;

    let serialized = serde_json::to_value(&envelope)?;
    assert_eq!(
        serialized["capability"]["details"]["attempt_count"],
        json!(0)
    );
    assert_eq!(serialized["reason"]["code"], json!("blocked"));

    Ok(())
}
