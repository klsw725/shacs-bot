use super::*;

#[test]
fn only_verified_tool_closure_uses_persistent_cache() -> Result<(), Spec034ReleaseArtifactError> {
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let evidence = canonical.join("evidence");
    let cache = canonical.join("cache");

    let first = RunnerIsolation::prepare(&repo, &evidence, Some(&cache))?;
    let first_target = first.target().to_path_buf();
    let first_cache_tools = first.cache_tools().to_path_buf();
    drop(first);
    let second = RunnerIsolation::prepare(&repo, &evidence, Some(&cache))?;

    assert_ne!(second.target(), first_target);
    assert_ne!(second.tools(), first_tools_for(&first_target));
    assert_eq!(second.cache_tools(), first_cache_tools);
    assert!(second.target().is_dir());
    assert!(second.tools().is_dir());
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_removes_immutable_owned_tree() -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(
        &repo,
        &canonical.join("evidence"),
        Some(&canonical.join("cache")),
    )?;
    let root = isolation
        .source_parent()
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .to_path_buf();
    let immutable = isolation.source_parent().join("immutable");
    std::fs::create_dir(&immutable).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::set_permissions(&immutable, std::fs::Permissions::from_mode(0o500))
        .map_err(Spec034ReleaseArtifactError::Io)?;

    let cleanup = isolation.cleanup()?;

    assert!(!root.exists());
    let receipt = cleanup.receipt("cleanup-success");
    assert!(receipt.raw_evidence_cleaned);
    assert_eq!(receipt.leak_count, 0);
    assert!(receipt.leak_summary.is_empty());
    assert_eq!(receipt.cleanup_binding_digest, cleanup.binding_digest());
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_accepts_expected_root_content_changes() -> Result<(), Spec034ReleaseArtifactError> {
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), None)?;
    let root = isolation
        .source_parent()
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .to_path_buf();
    std::fs::create_dir(root.join("expected-output"))
        .map_err(Spec034ReleaseArtifactError::Io)?;

    isolation.cleanup()?;

    assert!(!root.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_proves_normal_retained_root_unlink() -> Result<(), Spec034ReleaseArtifactError> {
    // Given: an unchanged retained isolation root with a normal parent entry.
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent.path().canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), None)?;
    let root = isolation.source_parent().parent().ok_or(
        Spec034ReleaseArtifactError::InvalidConfig,
    )?.to_path_buf();

    // When: cleanup unlinks the exact parent entry.
    let cleanup = isolation.cleanup()?;

    // Then: post-unlink retained-FD proof succeeds and the path is absent.
    assert!(!root.exists());
    assert!(cleanup.binding_digest().starts_with("sha256:"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_rejects_renamed_root_and_preserves_same_uid_replacement(
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    // Given: retained root A is moved away and same-owner replacement B takes its name.
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent.path().canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), None)?;
    let root = isolation.source_parent().parent().ok_or(
        Spec034ReleaseArtifactError::InvalidConfig,
    )?.to_path_buf();
    let displaced = canonical.join("displaced-a");
    std::fs::rename(&root, &displaced).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::create_dir(&root).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(root.join("replacement"), b"B").map_err(Spec034ReleaseArtifactError::Io)?;
    assert_eq!(root.metadata().map_err(Spec034ReleaseArtifactError::Io)?.uid(), displaced.metadata().map_err(Spec034ReleaseArtifactError::Io)?.uid());

    // When: cleanup attempts to retire the retained isolation.
    let result = isolation.cleanup();

    // Then: cleanup fails closed without deleting either the replacement or leaked A.
    assert!(matches!(result, Err(Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 })));
    assert_eq!(std::fs::read(root.join("replacement")).map_err(Spec034ReleaseArtifactError::Io)?, b"B");
    assert!(displaced.is_dir());
    Ok(())
}

#[cfg(target_vendor = "apple")]
#[test]
fn cleanup_rejects_root_a_b_a_swap() -> Result<(), Spec034ReleaseArtifactError> {
    // Given: A is displaced, B occupies A's name, B is restored, and A returns.
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent.path().canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), None)?;
    let root = isolation.source_parent().parent().ok_or(
        Spec034ReleaseArtifactError::InvalidConfig,
    )?.to_path_buf();
    let displaced = canonical.join("displaced-a");
    let replacement = canonical.join("replacement-b");
    std::fs::create_dir(&replacement).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(replacement.join("sentinel"), b"B").map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::rename(&root, &displaced).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::rename(&replacement, &root).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::rename(&root, &replacement).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::rename(&displaced, &root).map_err(Spec034ReleaseArtifactError::Io)?;

    // When: cleanup sees the original pathname and inode restored.
    let result = isolation.cleanup();

    // Then: rename history still invalidates cleanup and B remains untouched.
    assert!(matches!(result, Err(Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 })));
    assert_eq!(std::fs::read(replacement.join("sentinel")).map_err(Spec034ReleaseArtifactError::Io)?, b"B");
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_rejects_event_uncertainty_without_constructing_proof(
) -> Result<(), Spec034ReleaseArtifactError> {
    // Given: an intact retained root whose vnode event history is uncertain.
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent.path().canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), None)?;
    let root = isolation.source_parent().parent().ok_or(
        Spec034ReleaseArtifactError::InvalidConfig,
    )?.to_path_buf();
    RunnerIsolation::inject_next_monitor_uncertainty();

    // When: cleanup attempts to establish exact-root absence.
    let result = isolation.cleanup();

    // Then: no proof exists and the uncertain root is retained rather than deleted.
    assert!(matches!(result, Err(Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 })));
    assert!(root.is_dir());
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_rejects_replacement_installed_immediately_before_unlink(
) -> Result<(), Spec034ReleaseArtifactError> {
    // Given: final path verification passes before A is displaced and empty B takes its name.
    let parent = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = parent.path().canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let repo = canonical.join("repo");
    std::fs::create_dir(&repo).map_err(Spec034ReleaseArtifactError::Io)?;
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), None)?;
    let root = isolation.source_parent().parent().ok_or(
        Spec034ReleaseArtifactError::InvalidConfig,
    )?.to_path_buf();
    let displaced = canonical.join("late-displaced-a");
    let hook_displaced = displaced.clone();
    RunnerIsolation::inject_next_pre_unlink_hook(move |root| {
        std::fs::rename(root, &hook_displaced).expect("displace verified root");
        std::fs::create_dir(root).expect("install empty replacement root");
    });

    // When: cleanup reaches its final pathname identity check.
    let result = isolation.cleanup();

    // Then: cleanup fails closed while preserving both replacement B and displaced A.
    let original_remained_linked = displaced.is_dir();
    let replacement_preserved = root.is_dir();
    std::fs::remove_dir_all(&root).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::remove_dir_all(&displaced).map_err(Spec034ReleaseArtifactError::Io)?;
    assert!(matches!(result, Err(Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 })));
    assert!(original_remained_linked);
    assert!(replacement_preserved);
    Ok(())
}

fn first_tools_for(target: &Path) -> PathBuf {
    target
        .parent()
        .map_or_else(PathBuf::new, |root| root.join("toolchain/tools"))
}
