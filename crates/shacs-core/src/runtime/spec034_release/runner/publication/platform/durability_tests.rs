use super::*;

fn io<T>(result: std::io::Result<T>) -> Result<T, Spec034ReleaseArtifactError> {
    result.map_err(Spec034ReleaseArtifactError::Io)
}

#[test]
fn pre_rename_sync_failure_never_exposes_validated_marker(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let destination = EvidenceDestination::prepare(&evidence)?;
    let mut staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"manifest"))?;
    staging.inject_marker_sync_failure(MarkerSyncFailure::BeforeRenameDirectory);

    let result = staging.finalize_marker("run");

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DirectorySync
        ))
    ));
    assert!(!evidence.join("publication-status.json").exists());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn successful_marker_rename_publishes_complete_validated_document(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"manifest"))?;

    let staging = staging.finalize_marker("run")?;
    destination.publish(staging)?;

    let bytes = io(std::fs::read(evidence.join("publication-status.json")))?;
    let status: PublicationStatusDocument =
        serde_json::from_slice(&bytes).map_err(Spec034ReleaseArtifactError::Json)?;
    assert_eq!(status.status, PublicationStatus::Validated);
    assert!(!evidence.join(".publication-status.validated").exists());
    Ok(())
}

#[test]
fn post_rename_parent_sync_failure_is_commit_unknown() -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"manifest"))?;
    let staging = staging.finalize_marker("run")?;
    destination.inject_destination_sync_failure();

    let result = destination.publish(staging);

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DirectorySync
        ))
    ));
    Ok(())
}

#[test]
fn created_parent_sync_failure_is_commit_unknown() -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let canonical = io(root.path().canonicalize())?;
    let destination = canonical.join("parent/evidence");

    let result = EvidenceDestination::prepare_with(&destination, |_| {
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DirectorySync,
        ))
    });

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DirectorySync
        ))
    ));
    assert!(!root.path().join("parent").exists());
    Ok(())
}
