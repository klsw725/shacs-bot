use super::*;
use crate::runtime::spec034_release::artifacts::ArtifactSnapshot;

fn io<T>(result: std::io::Result<T>) -> Result<T, Spec034ReleaseArtifactError> {
    result.map_err(Spec034ReleaseArtifactError::Io)
}

#[test]
fn approved_artifact_deletion_before_marker_is_rejected(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    let manifest = staging.path().join("manifest.json");
    io(std::fs::write(&manifest, b"approved"))?;
    let approved = ArtifactSnapshot::capture(staging.path())?;

    io(std::fs::remove_file(manifest))?;
    let result = staging.finalize_approved_marker(
        "run",
        approved,
        super::super::FinalSourceBinding::fixture(),
    );

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn approved_artifact_mutate_restore_before_marker_is_rejected(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    let manifest = staging.path().join("manifest.json");
    io(std::fs::write(&manifest, b"approved"))?;
    let approved = ArtifactSnapshot::capture(staging.path())?;

    io(std::fs::write(&manifest, b"changed!"))?;
    io(std::fs::write(&manifest, b"approved"))?;
    let result = staging.finalize_approved_marker(
        "run",
        approved,
        super::super::FinalSourceBinding::fixture(),
    );

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    assert!(!evidence.exists());
    Ok(())
}
