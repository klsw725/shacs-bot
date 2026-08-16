use super::artifacts::{collect_digests, write_json};
use super::catalog;
use super::model::*;
use super::source;
use shacs_projection::SPEC034_REQUIREMENTS;
use std::path::Path;

mod generation;
mod publication;
mod validation;

pub fn run_spec034_release_runner(
    config: &Spec034ReleaseConfig,
) -> Result<Spec034ReleaseManifest, Spec034ReleaseArtifactError> {
    validate_config(config)?;
    let mut destination = publication::EvidenceDestination::prepare(&config.evidence_root)?;
    let source = source::collect(&config.repo_root)?;
    let fixture_digests = generation::fixture_digests(&config.repo_root)?;
    let staging = tempfile::Builder::new()
        .prefix(".spec034-release-")
        .tempdir()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let results = generation::run_results(config, staging.path())?;
    let coverage = generation::coverage(config, &results.commands)?;
    generation::write_documents(
        config,
        staging.path(),
        &source,
        &fixture_digests,
        &coverage,
        &results,
    )?;
    if source::collect(&config.repo_root)? != source {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
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
        non_guarantees: catalog::non_guarantees(),
    };
    write_json(staging.path(), "manifest.json", &manifest)?;
    validation::validate(staging.path(), &config.repo_root)?;
    destination.publish(staging.path())?;
    Ok(manifest)
}

pub fn validate_spec034_release_artifacts_against(
    root: &Path,
    repo_root: &Path,
) -> Result<Spec034ReleaseManifest, Spec034ReleaseArtifactError> {
    validation::validate(root, repo_root)
}

fn validate_config(config: &Spec034ReleaseConfig) -> Result<(), Spec034ReleaseArtifactError> {
    let valid_id = !config.run_id.is_empty()
        && config
            .run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    valid_id
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)
}
