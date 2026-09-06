use super::artifacts::{collect_digests, write_json};
use super::catalog;
use super::model::*;
use super::source;
use shacs_projection::SPEC034_REQUIREMENTS;
use std::path::Path;

mod command_validation;
mod command_specs;
mod command_execution;
mod config;
mod attestation;
#[cfg(test)]
use attestation::FreshExecutionAttestation;
mod fixture;
mod generation;
mod isolation;
mod path_safety;
mod publication;
mod validation;
#[cfg(test)]
mod test_hooks;

pub fn run_spec034_release_runner(
    config: &Spec034ReleaseConfig,
) -> Result<super::CommittedPublicationResult, Spec034ReleaseArtifactError> {
    run_with_final_validator(config, validation::validate_pending_with_git)
}

pub fn run_spec034_release_runner_with_linker_image(
    config: &Spec034ReleaseConfig,
    linker_image: &Path,
) -> Result<super::CommittedPublicationResult, Spec034ReleaseArtifactError> {
    run_with_publication_hooks_and_linker_image(
        config,
        Some(linker_image),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    )
}

fn run_with_final_validator(
    config: &Spec034ReleaseConfig,
    final_validator: impl FnOnce(
        validation::PendingValidationContext<'_>,
    ) -> Result<validation::ValidatedSnapshot, Spec034ReleaseArtifactError>,
) -> Result<super::CommittedPublicationResult, Spec034ReleaseArtifactError> {
    run_with_publication_hooks(config, final_validator, |_| {}, |_| {})
}

fn run_with_publication_hooks(
    config: &Spec034ReleaseConfig,
    final_validator: impl FnOnce(
        validation::PendingValidationContext<'_>,
    ) -> Result<validation::ValidatedSnapshot, Spec034ReleaseArtifactError>,
    before_final_verification: impl FnOnce(&Path),
    after_final_verification: impl FnOnce(&Path),
) -> Result<super::CommittedPublicationResult, Spec034ReleaseArtifactError> {
    run_with_publication_hooks_and_linker_image(
        config,
        None,
        final_validator,
        before_final_verification,
        after_final_verification,
    )
}

fn run_with_publication_hooks_and_linker_image(
    config: &Spec034ReleaseConfig,
    linker_image: Option<&Path>,
    final_validator: impl FnOnce(
        validation::PendingValidationContext<'_>,
    ) -> Result<validation::ValidatedSnapshot, Spec034ReleaseArtifactError>,
    before_final_verification: impl FnOnce(&Path),
    after_final_verification: impl FnOnce(&Path),
) -> Result<super::CommittedPublicationResult, Spec034ReleaseArtifactError> {
    config::validate(config)?;
    let mut isolation = Some(isolation::RunnerIsolation::prepare(
        &config.repo_root,
        &config.evidence_root,
        config.cache_root.as_deref(),
    )?);
    let result = (|| {
    let active = isolation
        .as_ref()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut destination = publication::EvidenceDestination::prepare(&config.evidence_root)?;
    let source_root = source::SourceRootContext::resolve_release(&config.repo_root)?;
    let mut source_snapshot = source::capture_context(&source_root)?;
    for locator in catalog::FIXTURES {
        source_snapshot.include(&source_root, locator)?;
    }
    let source = source_snapshot.manifest.clone();
    let fixture_digests = fixture::digests_from_source(&source_snapshot)?;
    let execution = source_snapshot.materialize_at(active.source_parent())?;
    let toolchain = super::tools::ResolvedToolchain::resolve_at(
        active.home().to_path_buf(),
        active.cargo_home().to_path_buf(),
        active.target().to_path_buf(),
        active.tools().to_path_buf(),
        active.cache_tools().to_path_buf(),
        Some(&execution.path().join("crates/Cargo.toml")),
        linker_image,
    )?;
    let staging = destination.staging()?;
    #[cfg(not(test))]
    let results = run_after_source_preflight(&source_root, &source_snapshot, || {
        generation::run_results(
            config,
            staging.path(),
            &execution,
            &source.digest,
            &toolchain,
        )
    })?;
    #[cfg(test)]
    let results = run_after_source_preflight(&source_root, &source_snapshot, || {
        generation::fixture_results(config, staging.path(), &source.digest, &toolchain)
    })?;
    let attestation = attestation::FreshExecutionAttestation::from_commands(
        &source.digest,
        &results.commands,
        toolchain.linker_attestation_digest()?,
    )?;
    let coverage = generation::coverage(config, &results.commands)?;
    generation::write_documents(
        config,
        staging.path(),
        &source,
        &fixture_digests,
        &coverage,
        &results,
    )?;
    let pre_cleanup = super::artifacts::ArtifactSnapshot::capture(staging.path())?;
    path_safety::validate_snapshot(&pre_cleanup)?;
    attestation.verify_snapshot(&source.digest, &pre_cleanup, &toolchain)?;
    drop(execution);
    drop(source_snapshot);
    source_root.verify()?;
    let toolchain = toolchain.retire()?;
    let cleanup = isolation
        .take()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .cleanup()?;
    generation::write_cleanup_receipt(config, staging.path(), &cleanup)?;
    #[cfg(test)]
    test_hooks::before_manifest_capture(staging.path());
    let manifest = Spec034ReleaseManifest {
        schema: RELEASE_SCHEMA.to_owned(),
        run_id: config.run_id.clone(),
        mode: config.mode,
        repo_root: source.repo_root.clone(),
        head_oid: source.head_oid.clone(),
        source,
        fixture_digests,
        artifact_digests: collect_digests(staging.path())?,
        requirement_count: SPEC034_REQUIREMENTS.len(),
        blocker_count: catalog::BLOCKERS.len(),
        runner_passed: true,
        runner_only: true,
        closure_eligible: false,
        execution_attested: false,
        structural_only: true,
        non_guarantees: catalog::non_guarantees(),
    };
    write_json(staging.path(), "manifest.json", &manifest)?;
    let final_validation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        final_validator(validation::PendingValidationContext::new(
            staging.path(),
            &source_root,
            &toolchain,
            &cleanup,
        ))
    }))
    .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)??;
    #[cfg(test)]
    let mut final_validation = final_validation;
    #[cfg(test)]
    test_hooks::after_pending_validation(final_validation.cleanup_receipt_mut());
    if final_validation.manifest() != &manifest {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let (approved, cleanup_receipt) = final_validation.into_parts();
    let staging = staging.finalize_approved_marker(
        &config.run_id,
        approved,
        publication::FinalSourceBinding::runner(
            source_root,
            manifest.source.clone(),
            manifest.fixture_digests.clone(),
            toolchain,
            cleanup,
            cleanup_receipt,
        ),
    )?;
    let staging_path = staging.path().to_path_buf();
    let identity = destination.publish_with_runner_hooks(
        staging,
        || before_final_verification(&staging_path),
        || after_final_verification(&staging_path),
    )?;
    Ok(super::CommittedPublicationResult { manifest, identity })
    })();
    let cleanup = isolation
        .take()
        .map(isolation::RunnerIsolation::cleanup)
        .transpose()
        .map(|_| ());
    Spec034ReleaseArtifactError::combine(result, cleanup)
}

fn run_after_source_preflight<T>(
    source_root: &source::SourceRootContext,
    adopted: &source::SourceSnapshot,
    executor: impl FnOnce() -> Result<T, Spec034ReleaseArtifactError>,
) -> Result<T, Spec034ReleaseArtifactError> {
    let mut current = source::capture_context(source_root)?;
    for locator in catalog::FIXTURES {
        current.include(source_root, locator)?;
    }
    (current == *adopted)
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
    executor()
}

pub fn audit_spec034_release_artifacts_against(
    root: &Path,
    repo_root: &Path,
) -> Result<Spec034StructuralAudit, Spec034ReleaseArtifactError> {
    validation::validate(root, repo_root)
}

pub fn audit_spec034_release_artifacts_against_expected(
    root: &Path,
    repo_root: &Path,
    expected: &super::CommittedPublicationResult,
) -> Result<Spec034StructuralAudit, Spec034ReleaseArtifactError> {
    let audit = validation::validate_expected(root, repo_root, expected)?;
    (audit.manifest == expected.manifest
        && audit.manifest.run_id == expected.manifest.run_id
        && audit.content_digest == expected.identity.content_digest)
        .then_some(audit)
        .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod tests;

#[cfg(test)]
#[path = "runner_binding_test.rs"]
mod binding_tests;

#[cfg(test)]
#[path = "runner_cleanup_test.rs"]
mod cleanup_tests;
