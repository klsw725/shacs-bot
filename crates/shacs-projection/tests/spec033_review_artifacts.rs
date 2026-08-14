mod spec033_review_artifacts_support;

use shacs_projection::{
    build_spec033_review_artifacts, write_spec033_review_artifacts, Spec033ArtifactTransformError,
    Spec033ReviewVerdict,
};
use spec033_review_artifacts_support::{artifact_input, write_source_artifacts};
use std::error::Error;

#[test]
fn artifact_transform_rejects_missing_redaction_evidence() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    write_source_artifacts(root.path())?;
    let mut input = artifact_input(root.path());
    input.redaction_evidence = None;

    // When
    let result = build_spec033_review_artifacts(input);

    // Then
    assert_eq!(
        result,
        Err(Spec033ArtifactTransformError::MissingRedactionEvidence)
    );
    Ok(())
}

#[test]
fn artifact_transform_rejects_tampered_or_missing_evidence() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    write_source_artifacts(root.path())?;
    let mut tampered = artifact_input(root.path());
    tampered.replay_result.digest = digest(b"different");
    let mut missing = artifact_input(root.path());
    missing.coverage.artifacts[0].locator = "evidence/missing.json".to_owned();

    // When
    let tampered_result = build_spec033_review_artifacts(tampered);
    let missing_result = build_spec033_review_artifacts(missing);

    // Then
    assert_eq!(
        tampered_result,
        Err(Spec033ArtifactTransformError::EvidenceDigestMismatch)
    );
    assert_eq!(
        missing_result,
        Err(Spec033ArtifactTransformError::MissingEvidenceArtifact)
    );
    Ok(())
}

#[test]
fn artifact_transform_keeps_cargo_evidence_separate_from_review_verdicts(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    write_source_artifacts(root.path())?;
    let mut failed = artifact_input(root.path());
    failed.cargo_commands[0].passed = false;
    let mut rejected = artifact_input(root.path());
    rejected.reviews[0].verdict = Spec033ReviewVerdict::Fail;
    let mut blocked = artifact_input(root.path());
    blocked.coverage.blockers.push("later".to_owned());
    let mut waived = artifact_input(root.path());
    waived.coverage.waivers.push("skip".to_owned());

    // When
    let failed_result = build_spec033_review_artifacts(failed);
    let rejected_result = build_spec033_review_artifacts(rejected);
    let blocked_result = build_spec033_review_artifacts(blocked);
    let waived_result = build_spec033_review_artifacts(waived);

    // Then
    assert_eq!(
        failed_result,
        Err(Spec033ArtifactTransformError::ReviewCommandFailed)
    );
    assert_eq!(
        rejected_result,
        Err(Spec033ArtifactTransformError::ReviewVerdictFailed)
    );
    assert_eq!(
        blocked_result,
        Err(Spec033ArtifactTransformError::ForbiddenBlocker)
    );
    assert_eq!(
        waived_result,
        Err(Spec033ArtifactTransformError::ForbiddenWaiver)
    );
    Ok(())
}

#[test]
fn artifact_transform_rejects_leaks_in_every_persisted_string() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    write_source_artifacts(root.path())?;
    let mut path_leak = artifact_input(root.path());
    path_leak.coverage.artifacts[0].locator = "/Users/private/result.json".to_owned();
    let mut token_leak = artifact_input(root.path());
    token_leak.trajectory_id = "token=sk-secret".to_owned();
    let mut command_leak = artifact_input(root.path());
    command_leak.cargo_commands[0].extra_arguments = vec!["; env".to_owned()];

    // When
    let path_result = build_spec033_review_artifacts(path_leak);
    let token_result = build_spec033_review_artifacts(token_leak);
    let command_result = build_spec033_review_artifacts(command_leak);

    // Then
    assert_eq!(
        path_result,
        Err(Spec033ArtifactTransformError::UnsafePersistedString)
    );
    assert_eq!(
        token_result,
        Err(Spec033ArtifactTransformError::UnsafePersistedString)
    );
    assert_eq!(
        command_result,
        Err(Spec033ArtifactTransformError::InvalidReviewCommand)
    );
    Ok(())
}

#[test]
fn real_artifact_tree_is_written_from_verified_provenance() -> Result<(), Box<dyn Error>> {
    // Given
    let source = tempfile::tempdir()?;
    let output = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
    let output_dir = output.path().join("artifacts");
    write_source_artifacts(source.path())?;
    let artifacts = build_spec033_review_artifacts(artifact_input(source.path()))?;

    // When
    write_spec033_review_artifacts(&output_dir, &artifacts)?;

    // Then
    for locator in &artifacts.artifact_paths {
        assert!(output_dir.join(locator).is_file());
    }
    let serialized = std::fs::read_to_string(output_dir.join("manifest.json"))?;
    assert!(!serialized.contains("sk-secret"));
    assert!(!serialized.contains(source.path().to_string_lossy().as_ref()));
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
