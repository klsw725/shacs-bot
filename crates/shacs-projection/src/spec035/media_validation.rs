use super::*;
use crate::Spec031Freshness;
use std::collections::BTreeSet;

pub(super) fn validate_media_input(
    input: &Spec035MediaProjectionInput,
) -> Result<(), Spec035MediaValidationError> {
    if input.reason.code != input.state.into() {
        return invalid(Spec035MediaValidationErrorKind::InconsistentState);
    }
    if input.reason.safe_summary.as_str().chars().count() > SPEC035_MEDIA_SAFE_SUMMARY_MAX_CHARS {
        return invalid(Spec035MediaValidationErrorKind::UnsafeOwnerFact);
    }
    if !input
        .lineage
        .artifact_ref
        .as_str()
        .starts_with("spec034://media/artifact/")
        || input.lineage.artifact_ref.as_str().len() > SPEC035_MEDIA_OWNER_REF_MAX_CHARS
        || input
            .lineage
            .analyzer_ref
            .as_ref()
            .is_some_and(|reference| {
                !reference.as_str().starts_with("spec034://media/analyzer/")
                    || reference.as_str().len() > SPEC035_MEDIA_OWNER_REF_MAX_CHARS
            })
    {
        return invalid(Spec035MediaValidationErrorKind::UnsafeOwnerFact);
    }
    validate_owner_facts(&input.owner_facts)?;
    validate_state(input)?;
    validate_lineage(input)
}

fn validate_owner_facts(
    input: &Spec035MediaOwnerFactsInput,
) -> Result<(), Spec035MediaValidationError> {
    if input.facts.len() > SPEC035_MEDIA_OWNER_FACTS_MAX
        || input.unavailable_reasons.len() > SPEC035_MEDIA_UNAVAILABLE_REASONS_MAX
    {
        return invalid(Spec035MediaValidationErrorKind::UnsafeOwnerFact);
    }
    let reason_count = input
        .unavailable_reasons
        .iter()
        .collect::<BTreeSet<_>>()
        .len();
    if reason_count != input.unavailable_reasons.len() {
        return invalid(Spec035MediaValidationErrorKind::DuplicateOwnerFact);
    }
    let kinds = input
        .facts
        .iter()
        .map(Spec035MediaOwnerFactInput::kind)
        .collect::<BTreeSet<_>>();
    if kinds.len() != input.facts.len() {
        return invalid(Spec035MediaValidationErrorKind::DuplicateOwnerFact);
    }
    match input.freshness {
        Spec031Freshness::Current => {
            if !input.unavailable_reasons.is_empty() {
                return invalid(Spec035MediaValidationErrorKind::InconsistentState);
            }
            if kinds != Spec035MediaOwnerFactKind::REQUIRED.into_iter().collect() {
                return invalid(Spec035MediaValidationErrorKind::MissingOwnerFact);
            }
            validate_current_facts(&input.facts)
        }
        Spec031Freshness::Stale | Spec031Freshness::Unavailable | Spec031Freshness::Unknown => {
            if !input.facts.is_empty() || input.unavailable_reasons.is_empty() {
                return invalid(Spec035MediaValidationErrorKind::InconsistentState);
            }
            Ok(())
        }
    }
}

fn validate_current_facts(
    facts: &[Spec035MediaOwnerFactInput],
) -> Result<(), Spec035MediaValidationError> {
    for fact in facts {
        match fact {
            Spec035MediaOwnerFactInput::AnalyzerSource { analyzer_ref, .. } => {
                if !analyzer_ref
                    .as_str()
                    .starts_with("spec034://media/analyzer/")
                    || analyzer_ref.as_str().len() > SPEC035_MEDIA_OWNER_REF_MAX_CHARS
                {
                    return invalid(Spec035MediaValidationErrorKind::UnsafeOwnerFact);
                }
            }
            Spec035MediaOwnerFactInput::Sandbox(sandbox) => {
                if sandbox.applied_adapters.len() > SPEC035_MEDIA_APPLIED_ADAPTERS_MAX {
                    return invalid(Spec035MediaValidationErrorKind::UnsafeOwnerFact);
                }
                if sandbox
                    .applied_adapters
                    .iter()
                    .enumerate()
                    .any(|(index, adapter)| sandbox.applied_adapters[..index].contains(adapter))
                {
                    return invalid(Spec035MediaValidationErrorKind::DuplicateOwnerFact);
                }
            }
            Spec035MediaOwnerFactInput::Disclosure(disclosure) => {
                if disclosure.surfaces.len() > SPEC035_MEDIA_DISCLOSURE_SURFACES_MAX {
                    return invalid(Spec035MediaValidationErrorKind::UnsafeOwnerFact);
                }
                if disclosure
                    .surfaces
                    .iter()
                    .enumerate()
                    .any(|(index, surface)| disclosure.surfaces[..index].contains(surface))
                {
                    return invalid(Spec035MediaValidationErrorKind::DuplicateOwnerFact);
                }
            }
            Spec035MediaOwnerFactInput::Credential(_)
            | Spec035MediaOwnerFactInput::Snapshot { .. } => {}
        }
    }
    Ok(())
}

fn validate_state(input: &Spec035MediaProjectionInput) -> Result<(), Spec035MediaValidationError> {
    match input.state {
        Spec035MediaState::Included | Spec035MediaState::Truncated => {
            if input.owner_facts.freshness != Spec031Freshness::Current {
                return invalid(Spec035MediaValidationErrorKind::MisleadingSuccess);
            }
            if input.lineage.evidence_digest.is_none() {
                return invalid(Spec035MediaValidationErrorKind::InconsistentState);
            }
        }
        Spec035MediaState::Unavailable => {
            if input.owner_facts.freshness == Spec031Freshness::Current
                || input.lineage.analyzer_ref.is_some()
                || input.lineage.snapshot_ref.is_some()
                || input.lineage.evidence_digest.is_some()
            {
                return invalid(Spec035MediaValidationErrorKind::InconsistentState);
            }
        }
        Spec035MediaState::AnalyzerMissing => {
            if input.owner_facts.freshness != Spec031Freshness::Unavailable
                || !input.owner_facts.facts.is_empty()
                || !input
                    .owner_facts
                    .unavailable_reasons
                    .contains(&Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef)
                || input.lineage.analyzer_ref.is_some()
                || input.lineage.snapshot_ref.is_some()
                || input.lineage.evidence_digest.is_some()
            {
                return invalid(Spec035MediaValidationErrorKind::InconsistentState);
            }
        }
        Spec035MediaState::Unsupported | Spec035MediaState::ExtractionFailed => {
            if input.lineage.evidence_digest.is_some() {
                return invalid(Spec035MediaValidationErrorKind::InconsistentState);
            }
        }
    }
    Ok(())
}

fn validate_lineage(
    input: &Spec035MediaProjectionInput,
) -> Result<(), Spec035MediaValidationError> {
    for fact in &input.owner_facts.facts {
        match fact {
            Spec035MediaOwnerFactInput::AnalyzerSource { analyzer_ref, .. }
                if input.lineage.analyzer_ref.as_ref() != Some(analyzer_ref) =>
            {
                return invalid(Spec035MediaValidationErrorKind::OwnerLineageMismatch);
            }
            Spec035MediaOwnerFactInput::Snapshot { snapshot_ref, .. }
                if input.lineage.snapshot_ref.as_ref() != Some(snapshot_ref) =>
            {
                return invalid(Spec035MediaValidationErrorKind::OwnerLineageMismatch);
            }
            Spec035MediaOwnerFactInput::AnalyzerSource { .. }
            | Spec035MediaOwnerFactInput::Sandbox(_)
            | Spec035MediaOwnerFactInput::Credential(_)
            | Spec035MediaOwnerFactInput::Disclosure(_)
            | Spec035MediaOwnerFactInput::Snapshot { .. } => {}
        }
    }
    Ok(())
}

fn invalid<T>(kind: Spec035MediaValidationErrorKind) -> Result<T, Spec035MediaValidationError> {
    Err(Spec035MediaValidationError::new(kind))
}
