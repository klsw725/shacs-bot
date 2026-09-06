use super::*;

#[test]
fn preoccupied_legacy_rejected_name_never_leaves_configured_evidence_visible(
) -> Result<(), Spec034ReleaseArtifactError> {
    let temporary = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let root = temporary
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let evidence = root.join("evidence");
    let displaced = root.join("validated-evidence");
    let replacement = root.join("replacement");
    std::fs::create_dir(&replacement).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(replacement.join("manifest.json"), b"replacement")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    std::fs::write(staging.path().join("manifest.json"), b"validated")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let mut legacy = staging.name().to_os_string();
    legacy.push(".rejected");
    std::fs::create_dir(root.join(&legacy)).map_err(Spec034ReleaseArtifactError::Io)?;
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
    assert!(root.join(legacy).is_dir());
    assert!(crate::runtime::audit_spec034_release_artifacts_against(
        &evidence,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
    .is_err());
    Ok(())
}

#[test]
fn forced_collisions_preserve_visible_bytes(
) -> Result<(), Spec034ReleaseArtifactError> {
    let temporary = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let root = temporary
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let evidence = root.join("evidence");
    let destination = EvidenceDestination::prepare(&evidence)?;
    std::fs::create_dir(&evidence).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(evidence.join("manifest.json"), b"replacement")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let names = [".collision-1", ".collision-2", ".collision-3"];
    for name in names {
        std::fs::create_dir(root.join(name)).map_err(Spec034ReleaseArtifactError::Io)?;
    }
    let mut generated = names
        .into_iter()
        .chain([".spec034-rejected-success"])
        .map(OsString::from);

    super::quarantine::quarantine_visible_with(
        destination.parent(),
        OsStr::new("evidence"),
        || generated.next().ok_or(Spec034ReleaseArtifactError::InvalidConfig),
    )?;

    assert!(!evidence.exists());
    assert_eq!(
        std::fs::read(root.join(".spec034-rejected-success/manifest.json"))
            .map_err(Spec034ReleaseArtifactError::Io)?,
        b"replacement"
    );
    assert!(crate::runtime::audit_spec034_release_artifacts_against(
        &evidence,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
    .is_err());
    Ok(())
}

#[test]
fn exhausted_collisions_return_explicit_quarantine_failure(
) -> Result<(), Spec034ReleaseArtifactError> {
    let temporary = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let root = temporary
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let evidence = root.join("evidence");
    let destination = EvidenceDestination::prepare(&evidence)?;
    std::fs::create_dir(&evidence).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::create_dir(root.join(".occupied")).map_err(Spec034ReleaseArtifactError::Io)?;

    let result = super::quarantine::quarantine_visible_with(
        destination.parent(),
        OsStr::new("evidence"),
        || Ok(OsString::from(".occupied")),
    );

    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::QuarantineFailure
        ))
    ));
    assert!(evidence.exists());
    Ok(())
}
