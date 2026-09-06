use super::*;

fn io<T>(result: std::io::Result<T>) -> Result<T, Spec034ReleaseArtifactError> {
    result.map_err(Spec034ReleaseArtifactError::Io)
}

#[test]
fn pre_commit_source_replacement_never_succeeds() -> Result<(), Spec034ReleaseArtifactError>
{
    // Given: a verified staging handle and a valid-looking replacement directory.
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let displaced = root.join("displaced-staging");
    let replacement = root.join("replacement");
    io(std::fs::create_dir(&replacement))?;
    io(std::fs::write(
        replacement.join("manifest.json"),
        br#"{"schema":"spec034.release_runner.v2","run_id":"replacement"}"#,
    ))?;
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(
        staging.path().join("manifest.json"),
        b"verified",
    ))?;
    let staging = staging.finalize_marker("run")?;

    // When: the source name changes after its final verification but before rename.
    let result = destination.publish_with(&staging, || {
        assert!(std::fs::rename(staging.path(), &displaced).is_ok());
        assert!(std::fs::rename(&replacement, staging.path()).is_ok());
    });

    // Then
    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(crate::runtime::audit_spec034_release_artifacts_against(
        &evidence,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
    .is_err());
    assert_eq!(
        io(std::fs::read(displaced.join("manifest.json")))?,
        b"verified"
    );
    Ok(())
}

#[test]
fn post_verification_identity_match_publishes_normally() -> Result<(), Spec034ReleaseArtifactError> {
    // Given
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"verified"))?;
    let staging = staging.finalize_marker("run")?;

    // When
    destination.publish_with(&staging, || {})?;

    // Then
    assert_eq!(io(std::fs::read(evidence.join("manifest.json")))?, b"verified");
    Ok(())
}

#[test]
fn physical_commit_can_be_finalized_through_preserved_handle(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"verified"))?;

    let staging = staging.finalize_marker("run")?;
    destination.publish(staging)?;

    assert!(evidence.join("publication-status.json").is_file());
    Ok(())
}

#[test]
fn artifact_inserted_after_final_seal_is_never_published(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"verified"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with_hooks(
        &staging,
        || {
            assert!(std::fs::write(staging.path().join("extra.json"), b"{}").is_ok());
        },
        || {},
        || {},
        || {},
    );

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn marker_overwrite_before_final_verification_never_succeeds(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"verified"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with_hooks(
        &staging,
        || {
            assert!(std::fs::write(
                staging.path().join("publication-status.json"),
                b"overwritten marker",
            )
            .is_ok());
        },
        || {},
        || {},
        || {},
    );

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(!evidence.exists());
    assert!(crate::runtime::audit_spec034_release_artifacts_against(
        staging.path(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
    .is_err());
    Ok(())
}

#[test]
fn manifest_overwrite_after_final_verification_is_commit_unknown(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"verified"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with_hooks(
        &staging,
        || {},
        || {
            assert!(std::fs::write(staging.path().join("manifest.json"), b"overwritten").is_ok());
        },
        || {},
        || {},
    );

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(!evidence.exists());
    assert_eq!(io(std::fs::read(staging.path().join("manifest.json")))?, b"overwritten");
    assert!(crate::runtime::audit_spec034_release_artifacts_against(
        staging.path(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
    .is_err());
    Ok(())
}

#[test]
fn post_rename_replacement_is_quarantined_from_configured_destination(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let displaced = root.join("validated-evidence");
    let replacement = root.join("replacement");
    io(std::fs::create_dir(&replacement))?;
    io(std::fs::write(replacement.join("manifest.json"), b"replacement"))?;
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"validated"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with_post_rename_hook(&staging, || {
        assert!(std::fs::rename(&evidence, &displaced).is_ok());
        assert!(std::fs::rename(&replacement, &evidence).is_ok());
    });

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(!evidence.exists());
    assert_eq!(io(std::fs::read(displaced.join("manifest.json")))?, b"validated");
    assert!(root
        .read_dir()
        .map_err(Spec034ReleaseArtifactError::Io)?
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".spec034-rejected-")));
    Ok(())
}

#[test]
fn post_fsync_mutation_between_aggregate_captures_is_durably_quarantined(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"validated"))?;
    let staging = staging.finalize_marker("run")?;

    let result = destination.publish_with_post_fsync_hook(&staging, || {
        assert!(std::fs::write(evidence.join("manifest.json"), b"mutated").is_ok());
    });

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::DestinationIdentity
        ))
    ));
    assert!(!evidence.exists());
    assert!(root
        .read_dir()
        .map_err(Spec034ReleaseArtifactError::Io)?
        .filter_map(Result::ok)
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".spec034-rejected-")));
    Ok(())
}
