use super::*;

pub(super) fn monitor_paths(root: &Path) -> Result<Vec<PathBuf>, Spec034ReleaseArtifactError> {
    let mut paths = vec![root.to_path_buf()];
    if root.is_dir() {
        let mut entries = std::fs::read_dir(root).map_err(Spec034ReleaseArtifactError::Io)?.collect::<Result<Vec<_>, _>>().map_err(Spec034ReleaseArtifactError::Io)?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries { paths.extend(monitor_paths(&entry.path())?); }
    }
    Ok(paths)
}

#[cfg(test)]
pub(crate) fn release_tempdir(kind: &str) -> Result<tempfile::TempDir, Spec034ReleaseArtifactError> {
    tempfile::Builder::new().prefix(&format!("shacs-spec034-{kind}-")).tempdir().map_err(Spec034ReleaseArtifactError::Io)
}

pub(super) fn reject_root_cargo_config(root: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    for locator in [".cargo/config.toml", ".cargo/config"] {
        match std::fs::symlink_metadata(root.join(locator)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Spec034ReleaseArtifactError::InvalidConfig),
        }
    }
    Ok(())
}

pub(super) fn minimal_command(executable: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(executable);
    command.current_dir(cwd).env_clear();
    if let Some(value) = std::env::var_os("TMPDIR") { command.env("TMPDIR", value); }
    let mut paths = executable.parent().into_iter().map(Path::to_path_buf).collect::<Vec<_>>();
    paths.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    if let Ok(path) = std::env::join_paths(paths) { command.env("PATH", path); }
    command.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null").env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").env("CARGO_TERM_COLOR", "never");
    command
}

#[cfg(unix)]
pub(super) fn set_read_only_closure(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)? { set_read_only_closure(&entry.map_err(Spec034ReleaseArtifactError::Io)?.path())?; }
    }
    let metadata = std::fs::metadata(path).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(metadata.mode() & !0o222)).map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(not(unix))]
pub(super) fn set_read_only_closure(_path: &Path) -> Result<(), Spec034ReleaseArtifactError> { Err(Spec034ReleaseArtifactError::InvalidConfig) }
