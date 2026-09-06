use super::*;

pub(super) fn resolve(
    repo: &Path,
    override_root: Option<&Path>,
) -> Result<PathBuf, Spec034ReleaseArtifactError> {
    let requested = override_root.map_or_else(default_root, |path| Ok(path.to_path_buf()))?;
    if !requested.is_absolute() {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    let root = canonical_missing_path(&requested)?;
    if root != requested {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    reject_cargo_target(repo, &root)?;
    std::fs::create_dir_all(&root).map_err(Spec034ReleaseArtifactError::Io)?;
    let canonical = root
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if canonical != root {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    validate_owned_directory(repo, &root)?;
    for child in ["locks", "objects"] {
        let path = root.join(child);
        std::fs::create_dir(&path).or_else(|error| {
            error
                .kind()
                .eq(&std::io::ErrorKind::AlreadyExists)
                .then_some(())
                .ok_or(error)
        }).map_err(Spec034ReleaseArtifactError::Io)?;
        validate_owned_directory(repo, &path)?;
    }
    Ok(root)
}

fn default_root() -> Result<PathBuf, Spec034ReleaseArtifactError> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    #[cfg(target_vendor = "apple")]
    let root = home.join("Library/Caches/shacs-bot/spec034-release");
    #[cfg(not(target_vendor = "apple"))]
    let root = std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("shacs-bot/spec034-release");
    Ok(root)
}

fn reject_cargo_target(repo: &Path, root: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    let default_target = canonical_missing_path(&repo.join("crates/target"))?;
    if root.starts_with(&default_target) {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    if let Some(configured) = std::env::var_os("CARGO_TARGET_DIR").filter(|value| !value.is_empty()) {
        let configured = PathBuf::from(configured);
        let configured = if configured.is_absolute() { configured } else { repo.join(configured) };
        if root.starts_with(canonical_missing_path(&configured)?) {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
    }
    Ok(())
}

fn validate_owned_directory(
    repo: &Path,
    directory: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(Spec034ReleaseArtifactError::Io)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let owner = std::fs::metadata(repo)
            .map_err(Spec034ReleaseArtifactError::Io)?
            .uid();
        if metadata.uid() != owner {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(Spec034ReleaseArtifactError::Io)?;
    }
    Ok(())
}
