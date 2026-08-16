use super::*;

fn io<T>(result: std::io::Result<T>) -> Result<T, Spec034ReleaseArtifactError> {
    result.map_err(Spec034ReleaseArtifactError::Io)
}

#[test]
fn direct_intermediate_and_final_symlinks_fail_closed() -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let real = root.join("real");
    io(std::fs::create_dir(&real))?;
    io(std::os::unix::fs::symlink(&real, root.join("link")))?;

    assert!(EvidenceDestination::prepare(&root.join("link/evidence")).is_err());
    assert!(EvidenceDestination::prepare(&root.join("link/child/evidence")).is_err());
    io(std::os::unix::fs::symlink(&real, root.join("evidence")))?;
    assert!(EvidenceDestination::prepare(&root.join("evidence")).is_err());
    assert!(!real.join("evidence").exists());
    Ok(())
}

#[test]
fn traversal_and_component_replacement_fail_without_publication(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    assert!(EvidenceDestination::prepare(&root.join("safe/../evidence")).is_err());
    let ancestor = root.join("ancestor");
    let outside = root.join("outside");
    io(std::fs::create_dir(&ancestor))?;
    io(std::fs::create_dir(&outside))?;
    let mut destination = EvidenceDestination::prepare(&ancestor.join("evidence"))?;
    let staging = io(tempfile::tempdir_in(&root))?;
    io(std::fs::write(staging.path().join("manifest.json"), b"{}"))?;

    let result = destination.publish_with(staging.path(), || {
        assert!(std::fs::rename(&ancestor, root.join("displaced")).is_ok());
        assert!(std::os::unix::fs::symlink(&outside, &ancestor).is_ok());
    });

    assert!(result.is_err());
    assert!(!outside.join("evidence").exists());
    assert!(!root.join("displaced/evidence").exists());
    Ok(())
}

#[test]
fn normal_absolute_nested_destination_publishes_atomically(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("nested/child/evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = io(tempfile::tempdir_in(&root))?;
    io(std::fs::write(staging.path().join("manifest.json"), b"{}"))?;

    destination.publish(staging.path())?;

    assert_eq!(io(std::fs::read(evidence.join("manifest.json")))?, b"{}");
    Ok(())
}

#[test]
fn final_entry_replacement_fails_without_overwrite() -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let outside = root.join("outside");
    io(std::fs::create_dir(&outside))?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = io(tempfile::tempdir_in(&root))?;
    io(std::fs::write(staging.path().join("manifest.json"), b"{}"))?;

    let result = destination.publish_with(staging.path(), || {
        assert!(std::os::unix::fs::symlink(&outside, &evidence).is_ok());
    });

    assert!(result.is_err());
    assert!(evidence.is_symlink());
    assert!(!outside.join("manifest.json").exists());
    assert!(staging.path().join("manifest.json").is_file());
    Ok(())
}
