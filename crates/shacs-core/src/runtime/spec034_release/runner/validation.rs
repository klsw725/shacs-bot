use super::super::artifacts::{validate_digest_rows, ArtifactSnapshot};
use super::super::catalog;
use super::super::model::*;
use super::super::source;
use super::generation::{command_passed, command_ref};
use std::path::Path;

#[path = "validation/documents.rs"]
mod documents;

const ARTIFACT_LOCATORS: [&str; 12] = [
    "cleanup-receipt.json",
    "coverage-matrix.json",
    "failure-triage.json",
    "owner-audits.json",
    "reproducibility-observations.json",
    "results.json",
    "review-records.json",
    "spec034-schema-contract.stderr",
    "spec034-schema-contract.stdout",
    "spec034-sequential-integration.stderr",
    "spec034-sequential-integration.stdout",
    "summary.json",
];

pub(super) struct ValidatedSnapshot {
    manifest: Spec034ReleaseManifest,
    snapshot: ArtifactSnapshot,
    cleanup_receipt: CleanupReceipt,
}

#[derive(Clone, Copy)]
pub(super) struct PendingValidationContext<'a> {
    root: &'a Path,
    source_root: &'a source::SourceRootContext,
    toolchain: &'a super::super::tools::RetiredToolchain,
    cleanup: &'a super::isolation::CompletedIsolationCleanup,
}

impl<'a> PendingValidationContext<'a> {
    pub(super) const fn new(
        root: &'a Path,
        source_root: &'a source::SourceRootContext,
        toolchain: &'a super::super::tools::RetiredToolchain,
        cleanup: &'a super::isolation::CompletedIsolationCleanup,
    ) -> Self {
        Self { root, source_root, toolchain, cleanup }
    }

    #[cfg(test)]
    pub(super) const fn root(self) -> &'a Path {
        self.root
    }
}

impl ValidatedSnapshot {
    pub(super) fn manifest(&self) -> &Spec034ReleaseManifest {
        &self.manifest
    }

    pub(super) fn into_parts(self) -> (ArtifactSnapshot, CleanupReceipt) {
        (self.snapshot, self.cleanup_receipt)
    }

    #[cfg(test)]
    pub(super) fn cleanup_receipt_mut(&mut self) -> &mut CleanupReceipt {
        &mut self.cleanup_receipt
    }
}

pub fn validate(
    root: &Path,
    repo_root: &Path,
) -> Result<Spec034StructuralAudit, Spec034ReleaseArtifactError> {
    let source_root = source::SourceRootContext::resolve_release(repo_root)?;
    let snapshot = ArtifactSnapshot::capture(root)?;
    let status = publication_status(&snapshot)?;
    validate_captured(snapshot, repo_root, source_root, status)
}

pub fn validate_expected(
    root: &Path,
    repo_root: &Path,
    expected: &super::super::CommittedPublicationResult,
) -> Result<Spec034StructuralAudit, Spec034ReleaseArtifactError> {
    let source_root = source::SourceRootContext::resolve_release(repo_root)?;
    let snapshot = ArtifactSnapshot::capture(root)?;
    let status = publication_status(&snapshot)?;
    if status.content_digest != expected.identity.content_digest {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    validate_captured(snapshot, repo_root, source_root, status)
}

fn publication_status(
    snapshot: &ArtifactSnapshot,
) -> Result<PublicationStatusDocument, Spec034ReleaseArtifactError> {
    snapshot.json("publication-status.json").map_err(|_| {
        Spec034ReleaseArtifactError::CommitStatusUnknown(PublicationStage::MarkerRename)
    })
}

fn validate_captured(
    snapshot: ArtifactSnapshot,
    repo_root: &Path,
    source_root: source::SourceRootContext,
    status: PublicationStatusDocument,
) -> Result<Spec034StructuralAudit, Spec034ReleaseArtifactError> {
    let (manifest, commands, _) = validate_snapshot_structure(&snapshot, &source_root)?;
    let toolchain = super::super::tools::ResolvedToolchain::resolve()?;
    let final_source_root = source::SourceRootContext::resolve_release(repo_root)?;
    if source::collect_context(&final_source_root)? != manifest.source {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    super::command_validation::validate_resolved(
        &snapshot,
        &commands,
        &manifest.source.digest,
        &toolchain,
    )?;
    validate_publication_status(&snapshot, &status, &manifest)?;
    Ok(Spec034StructuralAudit {
        manifest,
        content_digest: status.content_digest,
        execution_attested: false,
        structural_only: true,
    })
}

fn validate_publication_status(
    snapshot: &ArtifactSnapshot,
    status: &PublicationStatusDocument,
    manifest: &Spec034ReleaseManifest,
) -> Result<(), Spec034ReleaseArtifactError> {
    if status.schema != PUBLICATION_STATUS_SCHEMA
        || status.run_id != manifest.run_id
        || status.status != PublicationStatus::Validated
        || status.content_digest != snapshot.publication_digest()
    {
        return Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::MarkerRename,
        ));
    }
    Ok(())
}

pub(super) fn validate_pending_with_git(
    context: PendingValidationContext<'_>,
) -> Result<ValidatedSnapshot, Spec034ReleaseArtifactError> {
    let snapshot = ArtifactSnapshot::capture(context.root)?;
    let (manifest, cleanup_receipt) =
        validate_snapshot(&snapshot, context.source_root, context.toolchain)?;
    context.cleanup.verify_receipt(&cleanup_receipt)?;
    Ok(ValidatedSnapshot { manifest, snapshot, cleanup_receipt })
}

fn validate_snapshot(
    snapshot: &ArtifactSnapshot,
    source_root: &source::SourceRootContext,
    toolchain: &super::super::tools::RetiredToolchain,
) -> Result<(Spec034ReleaseManifest, CleanupReceipt), Spec034ReleaseArtifactError> {
    let (manifest, commands, cleanup_receipt) = validate_snapshot_structure(snapshot, source_root)?;
    super::command_validation::validate(
        snapshot,
        &commands,
        &manifest.source.digest,
        toolchain,
    )?;
    Ok((manifest, cleanup_receipt))
}

fn validate_snapshot_structure(
    snapshot: &ArtifactSnapshot,
    source_root: &source::SourceRootContext,
) -> Result<(Spec034ReleaseManifest, Vec<CommandEvidence>, CleanupReceipt), Spec034ReleaseArtifactError> {
    let manifest: Spec034ReleaseManifest = snapshot.json("manifest.json")?;
    if !super::config::valid_run_id(&manifest.run_id)
        || manifest.schema != RELEASE_SCHEMA
        || manifest.repo_root != "."
        || manifest.head_oid != manifest.source.head_oid
        || manifest.requirement_count != 22
        || manifest.blocker_count != 8
        || !manifest.runner_passed
        || !manifest.runner_only
        || manifest.closure_eligible
        || manifest.execution_attested
        || !manifest.structural_only
        || manifest.non_guarantees != catalog::non_guarantees()
        || manifest
            .artifact_digests
            .iter()
            .map(|row| row.locator.as_str())
            .ne(ARTIFACT_LOCATORS)
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    validate_digest_rows(snapshot, &manifest.artifact_digests)?;
    let (commands, cleanup_receipt) = documents::validate(snapshot, &manifest)?;
    super::path_safety::validate_snapshot(snapshot)?;
    source_root.verify()?;
    super::fixture::validate(source_root.root(), &manifest.fixture_digests)?;
    Ok((manifest, commands, cleanup_receipt))
}
