use super::*;

#[test]
fn validation_rejects_symlinked_artifact() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let release = root.path().join("release");
    std::fs::create_dir(&release)?;
    std::fs::write(root.path().join("outside.json"), b"{}")?;
    std::os::unix::fs::symlink(
        root.path().join("outside.json"),
        release.join("manifest.json"),
    )?;

    // When
    let result = audit_spec034_release_artifacts_against(&release, Path::new("."));

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn runner_rejects_intermediate_ancestor_symlink_without_publication() -> Result<(), Box<dyn Error>>
{
    // Given
    let _baseline = support::release_evidence()?;
    let root = tempfile::tempdir()?;
    let root = root.path().canonicalize()?;
    let real = root.join("nested-real");
    let linked = root.join("nested-link");
    std::fs::create_dir(&real)?;
    std::os::unix::fs::symlink(&real, &linked)?;
    let evidence = linked.join("child/evidence");
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;

    // When
    let result = run_spec034_release_runner(&Spec034ReleaseConfig {
        run_id: "spec034-intermediate-symlink".to_owned(),
        repo_root: repo,
        evidence_root: evidence,
        cache_root: Some(root.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    });
    support::restore_cache_root(&root.join("cache"))?;
    std::fs::remove_dir_all(root.join("cache"))?;

    // Then
    assert!(result.is_err());
    assert!(!real.join("child/evidence").exists());
    Ok(())
}
