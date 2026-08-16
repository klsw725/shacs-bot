use shacs_projection::*;
use std::error::Error;

#[path = "spec035_media_model/support.rs"]
mod support;
use support::included_input;

fn analyzer_missing_input() -> Result<Spec035MediaProjectionInput, Box<dyn Error>> {
    let mut input = included_input()?;
    input.state = Spec035MediaState::AnalyzerMissing;
    input.reason.code = Spec035MediaReasonCode::AnalyzerMissing;
    input.lineage.evidence_digest = None;
    Ok(input)
}

fn unavailable_input() -> Result<Spec035MediaProjectionInput, Box<dyn Error>> {
    let mut input = analyzer_missing_input()?;
    input.lineage.analyzer_ref = None;
    input.lineage.snapshot_ref = None;
    input.owner_facts = Spec035MediaOwnerFactsInput {
        freshness: Spec031Freshness::Unavailable,
        unavailable_reasons: vec![Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef],
        facts: Vec::new(),
    };
    Ok(input)
}

fn sandbox_mut(
    input: &mut Spec035MediaProjectionInput,
) -> Result<&mut SandboxStatusProjection, Box<dyn Error>> {
    input
        .owner_facts
        .facts
        .iter_mut()
        .find_map(|fact| match fact {
            Spec035MediaOwnerFactInput::Sandbox(sandbox) => Some(sandbox),
            Spec035MediaOwnerFactInput::AnalyzerSource { .. }
            | Spec035MediaOwnerFactInput::Credential(_)
            | Spec035MediaOwnerFactInput::Disclosure(_)
            | Spec035MediaOwnerFactInput::Snapshot { .. } => None,
        })
        .ok_or_else(|| "sandbox fixture missing".into())
}

fn disclosure_mut(
    input: &mut Spec035MediaProjectionInput,
) -> Result<&mut Spec035MediaDisclosureFact, Box<dyn Error>> {
    input
        .owner_facts
        .facts
        .iter_mut()
        .find_map(|fact| match fact {
            Spec035MediaOwnerFactInput::Disclosure(disclosure) => Some(disclosure),
            Spec035MediaOwnerFactInput::AnalyzerSource { .. }
            | Spec035MediaOwnerFactInput::Sandbox(_)
            | Spec035MediaOwnerFactInput::Credential(_)
            | Spec035MediaOwnerFactInput::Snapshot { .. } => None,
        })
        .ok_or_else(|| "disclosure fixture missing".into())
}

#[test]
fn analyzer_missing_rejects_current_analyzer_owner_and_lineage() -> Result<(), Box<dyn Error>> {
    let input = analyzer_missing_input()?;

    assert_eq!(
        Spec035MediaProjection::try_new(input)
            .expect_err("analyzer_missing cannot carry configured analyzer facts")
            .kind(),
        Spec035MediaValidationErrorKind::InconsistentState
    );
    Ok(())
}

#[test]
fn analyzer_missing_accepts_only_explicit_missing_owner_shape() -> Result<(), Box<dyn Error>> {
    let projection = Spec035MediaProjection::try_new(unavailable_input()?)?;

    assert_eq!(projection.state(), Spec035MediaState::AnalyzerMissing);
    assert_eq!(projection.freshness(), Spec031Freshness::Unavailable);
    assert!(projection.owner_facts().analyzer_source.is_none());
    Ok(())
}

#[test]
fn media_projection_rejects_oversized_user_facing_values() -> Result<(), Box<dyn Error>> {
    assert_eq!(SPEC035_MEDIA_SAFE_SUMMARY_MAX_CHARS, 240);
    assert_eq!(SPEC035_MEDIA_OWNER_REF_MAX_CHARS, 160);
    assert_eq!(SPEC035_MEDIA_OPAQUE_REF_MAX_CHARS, 160);
    assert_eq!(SPEC035_MEDIA_DIGEST_CHARS, 71);
    assert_eq!(SPEC035_MEDIA_OWNER_FACTS_MAX, 5);
    assert_eq!(SPEC035_MEDIA_UNAVAILABLE_REASONS_MAX, 10);
    assert_eq!(SPEC035_MEDIA_APPLIED_ADAPTERS_MAX, 7);
    assert_eq!(SPEC035_MEDIA_DISCLOSURE_SURFACES_MAX, 5);

    let mut summary = included_input()?;
    summary.reason.safe_summary = Spec031SafeSummary::try_new(&"s".repeat(241))?;
    assert_eq!(
        Spec035MediaProjection::try_new(summary)
            .expect_err("summary over 240 chars must fail")
            .kind(),
        Spec035MediaValidationErrorKind::UnsafeOwnerFact
    );

    let mut adapters = included_input()?;
    sandbox_mut(&mut adapters)?.applied_adapters = vec![ProcessAdapterKind::GenericExec; 8];
    assert_eq!(
        Spec035MediaProjection::try_new(adapters)
            .expect_err("adapter list over seven entries must fail")
            .kind(),
        Spec035MediaValidationErrorKind::UnsafeOwnerFact
    );

    let mut surfaces = included_input()?;
    disclosure_mut(&mut surfaces)?.surfaces = vec![DataSurface::Session; 6];
    assert_eq!(
        Spec035MediaProjection::try_new(surfaces)
            .expect_err("surface list over five entries must fail")
            .kind(),
        Spec035MediaValidationErrorKind::UnsafeOwnerFact
    );

    let mut reasons = unavailable_input()?;
    reasons.owner_facts.unavailable_reasons =
        vec![Spec035MediaOwnerUnavailableReason::OwnerFactsUnavailable; 11];
    assert_eq!(
        Spec035MediaProjection::try_new(reasons)
            .expect_err("unavailable reason list over ten entries must fail")
            .kind(),
        Spec035MediaValidationErrorKind::UnsafeOwnerFact
    );

    let mut facts = included_input()?;
    let repeated = facts
        .owner_facts
        .facts
        .first()
        .ok_or("owner fact fixture")?
        .clone();
    facts.owner_facts.facts.push(repeated);
    assert_eq!(
        Spec035MediaProjection::try_new(facts)
            .expect_err("owner fact list over five entries must fail")
            .kind(),
        Spec035MediaValidationErrorKind::UnsafeOwnerFact
    );
    Ok(())
}

#[test]
fn media_projection_rejects_oversized_lineage_at_input_boundary() {
    let oversized_artifact = format!("spec034://media/artifact/{}", "a".repeat(160));
    assert!(Spec031ExternalOwnerRef::try_new(&oversized_artifact).is_err());
    assert!(Spec035MediaOpaqueRef::try_new(&"s".repeat(161)).is_err());
}

#[test]
fn media_projection_rejects_duplicate_set_members() -> Result<(), Box<dyn Error>> {
    let mut adapters = included_input()?;
    sandbox_mut(&mut adapters)?.applied_adapters = vec![
        ProcessAdapterKind::GenericExec,
        ProcessAdapterKind::GenericExec,
    ];
    assert_eq!(
        Spec035MediaProjection::try_new(adapters)
            .expect_err("duplicate adapters must fail")
            .kind(),
        Spec035MediaValidationErrorKind::DuplicateOwnerFact
    );

    let mut surfaces = included_input()?;
    disclosure_mut(&mut surfaces)?.surfaces = vec![DataSurface::Trace, DataSurface::Trace];
    assert_eq!(
        Spec035MediaProjection::try_new(surfaces)
            .expect_err("duplicate surfaces must fail")
            .kind(),
        Spec035MediaValidationErrorKind::DuplicateOwnerFact
    );
    Ok(())
}

#[test]
fn equivalent_set_permutations_serialize_identically() -> Result<(), Box<dyn Error>> {
    let mut first = included_input()?;
    sandbox_mut(&mut first)?.applied_adapters =
        vec![ProcessAdapterKind::GenericExec, ProcessAdapterKind::Bash];
    disclosure_mut(&mut first)?.surfaces = vec![DataSurface::Trace, DataSurface::Session];

    let mut second = included_input()?;
    sandbox_mut(&mut second)?.applied_adapters =
        vec![ProcessAdapterKind::Bash, ProcessAdapterKind::GenericExec];
    disclosure_mut(&mut second)?.surfaces = vec![DataSurface::Session, DataSurface::Trace];
    second.owner_facts.facts.reverse();

    assert_eq!(
        serde_json::to_string(&Spec035MediaProjection::try_new(first)?)?,
        serde_json::to_string(&Spec035MediaProjection::try_new(second)?)?
    );
    Ok(())
}

#[test]
fn unavailable_reason_permutations_serialize_identically() -> Result<(), Box<dyn Error>> {
    let mut first = unavailable_input()?;
    first.owner_facts.unavailable_reasons = vec![
        Spec035MediaOwnerUnavailableReason::OwnerFactsUnavailable,
        Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef,
    ];
    let mut second = unavailable_input()?;
    second.owner_facts.unavailable_reasons = vec![
        Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef,
        Spec035MediaOwnerUnavailableReason::OwnerFactsUnavailable,
    ];

    assert_eq!(
        serde_json::to_string(&Spec035MediaProjection::try_new(first)?)?,
        serde_json::to_string(&Spec035MediaProjection::try_new(second)?)?
    );
    Ok(())
}
