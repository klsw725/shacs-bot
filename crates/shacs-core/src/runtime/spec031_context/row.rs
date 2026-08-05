use super::super::context_files::ContextFileProjection;
use super::super::context_handoff::{
    ContextBudgetDecision, ContextBudgetEvidence, ContextProviderHandoff,
};
use super::super::context_refs::ResolvedContextArtifact;
use super::envelope::envelope_from_row;
use super::reason::{evidence_reason, reason_for_file, reason_for_inline, summary};
use super::types::{
    Spec031ContextEvidenceInput, Spec031ContextEvidenceProjection, Spec031ContextEvidenceReason,
    Spec031ContextEvidenceRow, Spec031ContextEvidenceRowKind, Spec031ContextOwnerRef,
};
use sha2::{Digest, Sha256};
use shacs_projection::{Spec031ConstructionError, Spec031InclusionReason, Spec031SafeSummary};

pub fn project_spec031_context_evidence(
    input: Spec031ContextEvidenceInput<'_>,
) -> Result<Spec031ContextEvidenceProjection, Spec031ConstructionError> {
    let mut rows = Vec::new();
    for (order, artifact) in input.inline_artifacts.iter().enumerate() {
        rows.push(row_from_inline(order, artifact, input.provider_handoff)?);
    }
    for file in input.context_files {
        rows.push(row_from_file(file, input.provider_handoff)?);
    }
    if rows.is_empty() {
        rows.push(prompt_absent_row()?);
    }

    let parent = input.batch_ref.as_ref().map(Spec031ContextOwnerRef::as_str);
    let envelopes = rows
        .iter()
        .map(|row| envelope_from_row(row, parent, input.owner_freshness))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Spec031ContextEvidenceProjection { rows, envelopes })
}

fn row_from_inline(
    order: usize,
    artifact: &ResolvedContextArtifact,
    handoff: Option<&ContextProviderHandoff>,
) -> Result<Spec031ContextEvidenceRow, Spec031ConstructionError> {
    let budget = budget_for(handoff, &format!("inline:{}", artifact.source));
    let mut reason = reason_for_inline(artifact);
    if matches!(
        budget.map(|evidence| evidence.decision),
        Some(ContextBudgetDecision::SkippedBudget)
    ) && reason == Spec031InclusionReason::Included
    {
        reason = Spec031InclusionReason::Skipped;
    }
    row(
        Spec031ContextEvidenceRowKind::InlineReference,
        order,
        reason,
        digest_suffix(artifact.digest.as_deref(), &artifact.source),
        budget,
    )
}

fn row_from_file(
    file: &ContextFileProjection,
    handoff: Option<&ContextProviderHandoff>,
) -> Result<Spec031ContextEvidenceRow, Spec031ConstructionError> {
    let source_label = format!("context-file:{}", file.path.display());
    row(
        Spec031ContextEvidenceRowKind::ContextFile,
        file.order,
        reason_for_file(file.status),
        digest_suffix(
            file.digest.as_ref().map(|digest| digest.sha256.as_str()),
            &source_label,
        ),
        budget_for(handoff, &source_label),
    )
}

fn prompt_absent_row() -> Result<Spec031ContextEvidenceRow, Spec031ConstructionError> {
    row(
        Spec031ContextEvidenceRowKind::InlineReference,
        0,
        Spec031InclusionReason::Missing,
        "promptabsent".to_owned(),
        None,
    )
    .map(|mut row| {
        row.evidence_reason = Spec031ContextEvidenceReason::PromptAbsent;
        row
    })
}

fn row(
    kind: Spec031ContextEvidenceRowKind,
    order: usize,
    reason: Spec031InclusionReason,
    suffix: String,
    budget: Option<&ContextBudgetEvidence>,
) -> Result<Spec031ContextEvidenceRow, Spec031ConstructionError> {
    let kind_label = match kind {
        Spec031ContextEvidenceRowKind::ContextFile => "file",
        Spec031ContextEvidenceRowKind::InlineReference => "inline",
    };
    Ok(Spec031ContextEvidenceRow {
        opaque_ref: Spec031ContextOwnerRef::try_new(&format!(
            "subject:context:{kind_label}:{order}:{suffix}"
        ))?,
        kind,
        order,
        reason,
        evidence_reason: evidence_reason(reason),
        included: reason == Spec031InclusionReason::Included,
        budget_decision: budget.map(|evidence| evidence.decision),
        budget_estimated_tokens: budget.and_then(|evidence| evidence.estimated_tokens),
        result_summary: Spec031SafeSummary::try_new(summary(reason))?,
    })
}

fn budget_for<'a>(
    handoff: Option<&'a ContextProviderHandoff>,
    source_label: &str,
) -> Option<&'a ContextBudgetEvidence> {
    handoff.and_then(|handoff| {
        handoff
            .evidence
            .iter()
            .find(|evidence| evidence.source_label == source_label)
    })
}

fn digest_suffix(digest: Option<&str>, fallback: &str) -> String {
    digest.and_then(safe_digest_suffix).unwrap_or_else(|| {
        safe_digest_suffix(&sha256_hex(fallback.as_bytes())).unwrap_or("missing".to_owned())
    })
}

fn safe_digest_suffix(value: &str) -> Option<String> {
    let suffix = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(16)
        .collect::<String>()
        .to_ascii_lowercase();
    (!suffix.is_empty()).then_some(suffix)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
