use super::*;

#[test]
fn nested_file_replacement_immediately_before_unlink_is_preserved(
) -> Result<(), Spec034ReleaseArtifactError> {
    // Given: retained root A contains a file displaced after its earlier identity read.
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent.path().canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let root = canonical.join("retained-root");
    let nested = root.join("nested");
    let displaced = canonical.join("displaced-nested-a");
    std::fs::create_dir(&root).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(&nested, b"A").map_err(Spec034ReleaseArtifactError::Io)?;
    let retained = RetainedRoot::capture(&root)?;
    let hook_nested = nested.clone();
    let hook_displaced = displaced.clone();
    RetainedRoot::inject_next_pre_nested_unlink_hook(move || {
        std::fs::rename(&hook_nested, &hook_displaced).expect("displace nested file A");
        std::fs::write(&hook_nested, b"B").expect("install nested replacement B");
    });

    // When: cleanup reaches the nested file's final pathname identity check.
    let result = retained.cleanup(false);

    // Then: replacement B and displaced A survive while cleanup proof is rejected.
    let replacement = std::fs::read(&nested).map_err(Spec034ReleaseArtifactError::Io)?;
    let original = std::fs::read(&displaced).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::remove_dir_all(&root).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::remove_file(&displaced).map_err(Spec034ReleaseArtifactError::Io)?;
    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 })
    ));
    assert_eq!(replacement, b"B");
    assert_eq!(original, b"A");
    Ok(())
}
