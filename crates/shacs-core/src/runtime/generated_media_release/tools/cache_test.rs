use super::*;

fn paths() -> Result<(tempfile::TempDir, PathBuf), Spec034ReleaseArtifactError> {
    let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = root
        .path()
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    Ok((root, canonical.join("toolchain/tools")))
}

#[test]
fn malicious_preseeded_tool_cache_is_rebuilt_from_source_closure(
) -> Result<(), Spec034ReleaseArtifactError> {
    let (_root, tools) = paths()?;
    std::fs::create_dir_all(&tools).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(tools.join("cargo"), b"forged")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(
        tools
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
            .join("cache-manifest.json"),
        b"{}",
    )
    .map_err(Spec034ReleaseArtifactError::Io)?;

    let (cargo, _, _, binding) = resolve(&tools)?;

    cargo.verify()?;
    assert_ne!(cargo.identity().executable_digest, digest_bytes(b"forged"));
    assert!(binding.manifest_digest.starts_with("sha256:"));
    assert!(binding.tree_digest.starts_with("sha256:"));
    Ok(())
}

#[test]
fn cache_root_a_b_a_during_fresh_verification_is_rejected(
) -> Result<(), Spec034ReleaseArtifactError> {
    let (_root, tools) = paths()?;
    let _ = resolve(&tools)?;
    let cache = tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .to_path_buf();
    let parent = cache
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let displaced = parent.join("toolchain-a");
    let replacement = parent.join("toolchain-b");

    let result = resolve_with_hook(&tools, &tools, || {
        assert!(std::fs::rename(&cache, &displaced).is_ok());
        assert!(std::fs::create_dir(&replacement).is_ok());
        assert!(std::fs::rename(&replacement, &cache).is_ok());
        assert!(std::fs::remove_dir(&cache).is_ok());
        assert!(std::fs::rename(&displaced, &cache).is_ok());
    });

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}

#[test]
fn verified_cache_hit_removes_read_only_stage() -> Result<(), Spec034ReleaseArtifactError> {
    let (_root, tools) = paths()?;
    let _ = resolve(&tools)?;
    let _ = resolve(&tools)?;
    let parent = tools
        .parent()
        .and_then(Path::parent)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut stages = std::fs::read_dir(parent)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".toolchain-stage-"));

    assert!(stages.next().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn published_cache_files_and_directories_are_non_writable(
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::PermissionsExt;

    let (_root, tools) = paths()?;
    let _ = resolve(&tools)?;
    let cache = tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut pending = vec![cache.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
        assert_eq!(metadata.permissions().mode() & 0o222, 0, "writable cache path: {}", path.display());
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)? {
                pending.push(entry.map_err(Spec034ReleaseArtifactError::Io)?.path());
            }
        }
    }
    Ok(())
}

#[test]
fn cache_binding_rejects_directory_injection() -> Result<(), Spec034ReleaseArtifactError> {
    let (_root, tools) = paths()?;
    let (_, _, _, binding) = resolve(&tools)?;
    let cache = tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    make_writable(cache)?;
    std::fs::create_dir(cache.join("injected")).map_err(Spec034ReleaseArtifactError::Io)?;

    assert!(matches!(binding.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}
