use super::{catalog, command_passed, command_ref, ArtifactSnapshot};
use super::super::{
    CleanupReceipt, CommandEvidence, CoverageDocument, DigestRow, ObservationsDocument,
    OwnerAuditDocument, ResultsDocument, ReviewDocument, Spec034ReleaseArtifactError,
    Spec034ReleaseManifest, Spec034ReleaseMode, SummaryDocument, TriageDocument,
};

pub(super) fn validate(
    snapshot: &ArtifactSnapshot,
    manifest: &Spec034ReleaseManifest,
) -> Result<(Vec<CommandEvidence>, CleanupReceipt), Spec034ReleaseArtifactError> {
    let results: ResultsDocument = snapshot.json("results.json")?;
    let coverage: CoverageDocument = snapshot.json("coverage-matrix.json")?;
    let reviews: ReviewDocument = snapshot.json("review-records.json")?;
    let owners: OwnerAuditDocument = snapshot.json("owner-audits.json")?;
    let cleanup: CleanupReceipt = snapshot.json("cleanup-receipt.json")?;
    let observations: ObservationsDocument = snapshot.json("reproducibility-observations.json")?;
    let triage: TriageDocument = snapshot.json("failure-triage.json")?;
    let summary: SummaryDocument = snapshot.json("summary.json")?;
    let run_ids = [
        &results.run_id,
        &coverage.run_id,
        &reviews.run_id,
        &owners.run_id,
        &cleanup.run_id,
        &observations.run_id,
        &triage.run_id,
        &summary.run_id,
    ];
    if run_ids.iter().any(|run_id| *run_id != &manifest.run_id)
        || results.schema != "spec034.results.v2"
        || coverage.schema != "spec034.coverage.v1"
        || reviews.schema != "spec034.runner_reviews.v1"
        || owners.schema != "spec034.runner_owner_audits.v1"
        || cleanup.schema != "spec034.cleanup.v2"
        || observations.schema != "spec034.observations.v1"
        || triage.schema != "spec034.triage.v1"
        || summary.schema != "spec034.summary.v1"
        || results.mode != manifest.mode
        || !results.runner_passed
        || results.closure_eligible
        || results.execution_attested
        || !results.structural_only
        || results.commands.len() != 2
        || results
            .commands
            .iter()
            .any(|command| !command_passed(command))
        || observations.source != manifest.source
        || observations.fixture_digests != manifest.fixture_digests
        || observations.dirty_worktree_recorded != manifest.source.worktree_dirty
        || !triage.command_failures.is_empty()
        || !triage.open_blockers.is_empty()
        || !cleanup.raw_evidence_cleaned
        || cleanup.leak_count != 0
        || !cleanup.leak_summary.is_empty()
        || !valid_digest(&cleanup.cleanup_binding_digest)
        || summary.label != "runner-mechanics-only"
        || !summary.runner_passed
        || summary.closure_eligible
        || summary.execution_attested
        || !summary.structural_only
        || summary.non_guarantees != catalog::non_guarantees()
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    validate_coverage(&coverage, &results.commands)?;
    let schema_evidence = command_ref(&results.commands, "schema-contract")?;
    let integration_evidence = command_ref(&results.commands, "sequential-integration")?;
    validate_reviews(&reviews, manifest.mode, &schema_evidence)?;
    validate_owners(&owners, &integration_evidence)?;
    Ok((results.commands, cleanup))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_coverage(
    coverage: &CoverageDocument,
    commands: &[CommandEvidence],
) -> Result<(), Spec034ReleaseArtifactError> {
    let requirements = catalog::requirements(&command_ref(commands, "sequential-integration")?);
    let blockers = catalog::blockers(&command_ref(commands, "schema-contract")?);
    if coverage.requirements != requirements || coverage.blockers != blockers {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}

fn validate_reviews(
    reviews: &ReviewDocument,
    mode: Spec034ReleaseMode,
    evidence: &DigestRow,
) -> Result<(), Spec034ReleaseArtifactError> {
    let expected = catalog::reviews(evidence, mode == Spec034ReleaseMode::SuccessFixture);
    if reviews.records != expected {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}

fn validate_owners(
    owners: &OwnerAuditDocument,
    evidence: &DigestRow,
) -> Result<(), Spec034ReleaseArtifactError> {
    if owners.audits != catalog::owner_audits(evidence) {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}
