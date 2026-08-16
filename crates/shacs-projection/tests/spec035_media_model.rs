use serde_json::json;
use shacs_projection::*;
use std::error::Error;

#[path = "spec035_media_model/support.rs"]
mod support;
use support::{current_owner_facts, included_input};

#[test]
fn spec035_media_fixture_serializes_deterministically() -> Result<(), Box<dyn Error>> {
    let projection = Spec035MediaProjection::try_new(included_input()?)?;
    let first = serde_json::to_string(&projection)?;
    let second = serde_json::to_string(&projection)?;

    assert_eq!(first, second);
    assert_eq!(Spec035MediaProjection::parse_json(&first)?, projection);
    assert!(first.contains("\"schema_version\":1"));
    assert!(first.contains("\"kind\":\"media_capability\""));
    assert!(first.contains("\"state\":\"included\""));
    assert!(first.contains("\"freshness\":\"current\""));
    assert!(first.contains("\"raw_content_possible\":true"));
    Ok(())
}

#[test]
fn spec035_media_model_covers_canonical_states_and_reasons() -> Result<(), Box<dyn Error>> {
    let cases = [
        (Spec035MediaState::Included, "included"),
        (Spec035MediaState::Unsupported, "unsupported"),
        (Spec035MediaState::ExtractionFailed, "extraction_failed"),
        (Spec035MediaState::AnalyzerMissing, "analyzer_missing"),
        (Spec035MediaState::Truncated, "truncated"),
        (Spec035MediaState::Unavailable, "unavailable"),
    ];

    for (state, expected) in cases {
        assert_eq!(serde_json::to_value(state)?, json!(expected));
        assert_eq!(
            serde_json::to_value(Spec035MediaReasonCode::from(state))?,
            json!(expected)
        );

        let mut input = included_input()?;
        input.state = state;
        input.reason.code = state.into();
        match state {
            Spec035MediaState::Included | Spec035MediaState::Truncated => {}
            Spec035MediaState::Unsupported | Spec035MediaState::ExtractionFailed => {
                input.lineage.evidence_digest = None;
            }
            Spec035MediaState::AnalyzerMissing | Spec035MediaState::Unavailable => {
                input.lineage.analyzer_ref = None;
                input.lineage.snapshot_ref = None;
                input.lineage.evidence_digest = None;
                input.owner_facts = Spec035MediaOwnerFactsInput {
                    freshness: Spec031Freshness::Unavailable,
                    unavailable_reasons: vec![
                        Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef,
                    ],
                    facts: Vec::new(),
                };
            }
        }
        assert_eq!(Spec035MediaProjection::try_new(input)?.state(), state);
    }
    Ok(())
}

#[test]
fn spec035_media_model_rejects_unknown_version_field_and_state() -> Result<(), Box<dyn Error>> {
    let projection = Spec035MediaProjection::try_new(included_input()?)?;
    let value = serde_json::to_value(projection)?;

    let mut unknown_version = value.clone();
    unknown_version["schema_version"] = json!(2);
    assert_eq!(
        Spec035MediaProjection::from_json_value(unknown_version)
            .expect_err("unknown version must fail")
            .kind(),
        Spec035MediaParseErrorKind::InvalidSchema
    );

    let mut unknown_field = value.clone();
    unknown_field["raw_provider_body"] = json!("secret-token");
    assert!(Spec035MediaProjection::from_json_value(unknown_field).is_err());

    let mut unknown_state = value;
    unknown_state["state"] = json!("success");
    assert!(Spec035MediaProjection::from_json_value(unknown_state).is_err());
    Ok(())
}

#[test]
fn spec035_media_model_rejects_missing_and_duplicate_owner_facts() -> Result<(), Box<dyn Error>> {
    let mut missing = included_input()?;
    missing.owner_facts.facts.pop();
    assert_eq!(
        Spec035MediaProjection::try_new(missing)
            .expect_err("current facts must be complete")
            .kind(),
        Spec035MediaValidationErrorKind::MissingOwnerFact
    );

    let mut duplicate = included_input()?;
    duplicate.owner_facts.facts.pop();
    duplicate.owner_facts.facts.push(
        duplicate
            .owner_facts
            .facts
            .first()
            .ok_or("owner fact fixture")?
            .clone(),
    );
    assert_eq!(
        Spec035MediaProjection::try_new(duplicate)
            .expect_err("owner fact kinds must be unique")
            .kind(),
        Spec035MediaValidationErrorKind::DuplicateOwnerFact
    );
    Ok(())
}

#[test]
fn spec035_media_model_rejects_unsafe_refs_and_summaries() -> Result<(), Box<dyn Error>> {
    assert!(Spec035MediaOpaqueRef::try_new("https://user:secret@example.test/video").is_err());
    assert!(Spec035MediaOpaqueRef::try_new("/Users/private/video.mp4").is_err());
    assert!(Spec035MediaDigest::try_new("sha256:not-a-digest").is_err());
    let sanitized = Spec031SafeSummary::try_new("Bearer secret-token")?;
    assert!(!sanitized.as_str().contains("secret-token"));

    let projection = Spec035MediaProjection::try_new(included_input()?)?;
    let serialized = serde_json::to_string(&projection)?;
    for forbidden in ["secret-token", "/Users/private", "raw_provider_body"] {
        assert!(!serialized.contains(forbidden));
    }
    Ok(())
}

#[test]
fn spec035_media_model_rejects_stale_or_unavailable_success() -> Result<(), Box<dyn Error>> {
    for freshness in [
        Spec031Freshness::Stale,
        Spec031Freshness::Unavailable,
        Spec031Freshness::Unknown,
    ] {
        let mut input = included_input()?;
        input.owner_facts = Spec035MediaOwnerFactsInput {
            freshness,
            unavailable_reasons: vec![Spec035MediaOwnerUnavailableReason::OwnerFactsUnavailable],
            facts: Vec::new(),
        };
        assert_eq!(
            Spec035MediaProjection::try_new(input)
                .expect_err("non-current success must fail")
                .kind(),
            Spec035MediaValidationErrorKind::MisleadingSuccess
        );
    }
    Ok(())
}

#[test]
fn spec035_media_model_accepts_explicit_unavailable_without_inventing_facts(
) -> Result<(), Box<dyn Error>> {
    let mut input = included_input()?;
    input.state = Spec035MediaState::Unavailable;
    input.reason.code = Spec035MediaReasonCode::Unavailable;
    input.owner_facts = Spec035MediaOwnerFactsInput {
        freshness: Spec031Freshness::Stale,
        unavailable_reasons: vec![Spec035MediaOwnerUnavailableReason::StaleOwnerFacts],
        facts: Vec::new(),
    };
    input.lineage.analyzer_ref = None;
    input.lineage.snapshot_ref = None;
    input.lineage.evidence_digest = None;

    let projection = Spec035MediaProjection::try_new(input)?;
    assert_eq!(projection.state(), Spec035MediaState::Unavailable);
    assert_eq!(projection.freshness(), Spec031Freshness::Stale);
    assert_eq!(
        projection.disclosure(),
        &Spec035MediaDisclosure::Unavailable
    );
    assert!(projection.owner_facts().analyzer_source.is_none());
    Ok(())
}

#[test]
fn spec035_media_model_rejects_owner_lineage_mismatch() -> Result<(), Box<dyn Error>> {
    let mut input = included_input()?;
    input.lineage.analyzer_ref = Some(Spec031ExternalOwnerRef::try_new(
        "spec034://media/analyzer/other",
    )?);

    assert_eq!(
        Spec035MediaProjection::try_new(input)
            .expect_err("lineage must preserve owner refs")
            .kind(),
        Spec035MediaValidationErrorKind::OwnerLineageMismatch
    );
    Ok(())
}

#[test]
fn spec035_media_owner_input_is_data_only_and_does_not_mutate_source() -> Result<(), Box<dyn Error>>
{
    let source = current_owner_facts()?;
    let before = source.clone();
    let mut input = included_input()?;
    input.owner_facts = source;

    let _projection = Spec035MediaProjection::try_new(input)?;
    assert_eq!(before, current_owner_facts()?);
    Ok(())
}
