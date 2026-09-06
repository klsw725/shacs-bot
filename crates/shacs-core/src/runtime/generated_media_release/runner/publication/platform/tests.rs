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
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"{}"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with(&staging, || {
        assert!(std::fs::rename(&ancestor, root.join("displaced")).is_ok());
        assert!(std::os::unix::fs::symlink(&outside, &ancestor).is_ok());
    });

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
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
    let staging = destination.staging()?;
    use std::os::unix::fs::MetadataExt;
    assert_eq!(io(staging.path().metadata())?.dev(), io(staging.path().parent().ok_or(
        Spec034ReleaseArtifactError::InvalidConfig,
    )?.metadata())?.dev());
    io(std::fs::write(staging.path().join("manifest.json"), b"{}"))?;
    let staging = staging.finalize_marker("run")?;

    destination.publish(staging)?;

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
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"{}"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with(&staging, || {
        assert!(std::os::unix::fs::symlink(&outside, &evidence).is_ok());
    });

    assert!(result.is_err());
    assert!(evidence.is_symlink());
    assert!(!outside.join("manifest.json").exists());
    assert!(staging.path().join("manifest.json").is_file());
    Ok(())
}

#[test]
fn staging_name_replacement_never_publishes_unverified_directory(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let displaced = root.join("displaced-staging");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"verified"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with(&staging, || {
        assert!(std::fs::rename(staging.path(), &displaced).is_ok());
        assert!(std::fs::create_dir(staging.path()).is_ok());
        assert!(std::fs::write(staging.path().join("manifest.json"), b"replacement").is_ok());
    });

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(!evidence.exists());
    assert!(crate::runtime::audit_spec034_release_artifacts_against(
        &evidence,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
    .is_err());
    assert_eq!(io(std::fs::read(displaced.join("manifest.json")))?, b"verified");
    Ok(())
}

#[test]
fn same_device_foreign_parent_is_rejected() -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let foreign_parent = root.join("foreign");
    io(std::fs::create_dir(&foreign_parent))?;
    let foreign = tempfile::tempdir_in(&foreign_parent)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    io(std::fs::write(foreign.path().join("manifest.json"), b"foreign"))?;
    let foreign = StagingDirectory::capture_for_test(foreign)?.finalize_marker("run")?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;

    assert!(destination.publish(foreign).is_err());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn drop_preserves_concurrent_file_in_created_ancestor() -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let ancestor = root.join("created");
    let destination = EvidenceDestination::prepare(&ancestor.join("child/evidence"))?;
    let marker = ancestor.join("concurrent-marker");
    io(std::fs::write(&marker, b"not runner owned"))?;

    drop(destination);

    assert_eq!(io(std::fs::read(marker))?, b"not runner owned");
    Ok(())
}

#[test]
fn post_mkdir_open_failure_cleans_empty_and_preserves_concurrent_marker(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let empty = root.join("empty/child/evidence");
    let empty_result = std::panic::catch_unwind(|| {
        EvidenceDestination::prepare_with(&empty, |_| {
            Err(Spec034ReleaseArtifactError::InvalidConfig)
        })
    });
    assert!(matches!(empty_result, Ok(Err(_))));
    assert!(!root.join("empty").exists());

    let marker = root.join("marked/concurrent-marker");
    let marked = root.join("marked/child/evidence");
    let marked_result = std::panic::catch_unwind(|| {
        EvidenceDestination::prepare_with(&marked, |_| {
            io(std::fs::write(&marker, b"not runner owned"))?;
            Err(Spec034ReleaseArtifactError::InvalidConfig)
        })
    });
    assert!(matches!(marked_result, Ok(Err(_))));
    assert_eq!(io(std::fs::read(marker))?, b"not runner owned");
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn cross_filesystem_staging_is_replaced_by_destination_sibling(
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let Some(other) = [Path::new("/dev/shm"), Path::new("/tmp")]
        .into_iter()
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|metadata| metadata.dev() != root.metadata().map_or(0, |root| root.dev()))
        })
    else {
        return Ok(());
    };
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let foreign = tempfile::tempdir_in(other).map_err(Spec034ReleaseArtifactError::Io)?;
    io(std::fs::write(foreign.path().join("manifest.json"), b"foreign"))?;
    let foreign = StagingDirectory::capture_for_test(foreign)?.finalize_marker("run")?;
    assert!(destination.publish(foreign).is_err());
    assert!(!evidence.exists());

    let mut sibling = destination.staging()?;
    io(std::fs::write(sibling.path().join("manifest.json"), b"sibling"))?;
    let sibling = sibling.finalize_marker("run")?;
    destination.publish(sibling)?;

    assert_eq!(io(std::fs::read(evidence.join("manifest.json")))?, b"sibling");
    Ok(())
}
