use super::super::artifacts::{digest_file, read_json, validate_digest_rows, validated_file};
use super::super::catalog;
use super::super::model::*;
use super::super::source;
use super::generation::{command_passed, command_ref, fixture_digests};
use shacs_projection::{Spec034OwnerFactKind, Spec034ReviewKind};
use std::collections::BTreeSet;
use std::path::Path;

pub fn validate(
    root: &Path,
    repo_root: &Path,
) -> Result<Spec034ReleaseManifest, Spec034ReleaseArtifactError> {
    let manifest: Spec034ReleaseManifest = read_json(root, "manifest.json")?;
    let canonical_repo = repo_root
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if manifest.schema != RELEASE_SCHEMA
        || manifest.repo_root != canonical_repo.display().to_string()
        || manifest.head_oid != manifest.source.head_oid
        || manifest.requirement_count != 22
        || manifest.blocker_count != 8
        || !manifest.runner_passed
        || !manifest.runner_only
        || manifest.closure_eligible
        || manifest.non_guarantees != catalog::non_guarantees()
        || source::collect(repo_root)? != manifest.source
        || fixture_digests(repo_root)? != manifest.fixture_digests
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    validate_digest_rows(root, &manifest.artifact_digests)?;
    validate_documents(root, &manifest)?;
    Ok(manifest)
}

fn validate_documents(
    root: &Path,
    manifest: &Spec034ReleaseManifest,
) -> Result<(), Spec034ReleaseArtifactError> {
    let results: ResultsDocument = read_json(root, "results.json")?;
    let coverage: CoverageDocument = read_json(root, "coverage-matrix.json")?;
    let reviews: ReviewDocument = read_json(root, "review-records.json")?;
    let owners: OwnerAuditDocument = read_json(root, "owner-audits.json")?;
    let cleanup: CleanupReceipt = read_json(root, "cleanup-receipt.json")?;
    let observations: ObservationsDocument = read_json(root, "reproducibility-observations.json")?;
    let triage: TriageDocument = read_json(root, "failure-triage.json")?;
    let summary: SummaryDocument = read_json(root, "summary.json")?;
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
        || results.mode != manifest.mode
        || !results.runner_passed
        || results.closure_eligible
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
        || !cleanup.staging_atomically_published
        || !cleanup.leaked_paths.is_empty()
        || summary.label != "runner-mechanics-only"
        || !summary.runner_passed
        || summary.closure_eligible
        || summary.non_guarantees != catalog::non_guarantees()
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    validate_coverage(&coverage, &results.commands)?;
    validate_commands(root, &results.commands)?;
    validate_reviews(&reviews, manifest.mode)?;
    validate_owners(&owners)
}

fn validate_commands(
    root: &Path,
    commands: &[CommandEvidence],
) -> Result<(), Spec034ReleaseArtifactError> {
    let expected = [
        (
            "schema-contract",
            "shacs-projection",
            "spec034_evidence_schema",
        ),
        (
            "sequential-integration",
            "shacs-core",
            "spec034_sequential_integration",
        ),
    ];
    for (kind, package, target) in expected {
        let command = commands
            .iter()
            .find(|command| command.kind == kind)
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        let argv = [
            "cargo",
            "test",
            "--manifest-path",
            "crates/Cargo.toml",
            "--locked",
            "-p",
            package,
            "--test",
            target,
        ]
        .map(str::to_owned)
        .to_vec();
        let stdout = validated_file(root, &command.command.stdout_path)?;
        let stderr = validated_file(root, &command.command.stderr_path)?;
        if command.command.id != format!("spec034-{kind}")
            || command.command.argv != argv
            || command.command.package.as_deref() != Some(package)
            || digest_file(&stdout)? != command.stdout_digest
            || digest_file(&stderr)? != command.stderr_digest
        {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
    }
    Ok(())
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
) -> Result<(), Spec034ReleaseArtifactError> {
    let kinds = reviews
        .records
        .iter()
        .map(|record| record.kind)
        .collect::<BTreeSet<_>>();
    if kinds != Spec034ReviewKind::required().into_iter().collect()
        || reviews.records.len() != 5
        || reviews.records.iter().any(|record| {
            record.final_review
                || record.fixture_only != (mode == Spec034ReleaseMode::SuccessFixture)
        })
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}

fn validate_owners(owners: &OwnerAuditDocument) -> Result<(), Spec034ReleaseArtifactError> {
    let kinds = owners
        .audits
        .iter()
        .map(|audit| audit.kind)
        .collect::<BTreeSet<_>>();
    if kinds != Spec034OwnerFactKind::required().into_iter().collect()
        || owners.audits.len() != 8
        || owners
            .audits
            .iter()
            .any(|audit| audit.status != "command_observed")
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}
