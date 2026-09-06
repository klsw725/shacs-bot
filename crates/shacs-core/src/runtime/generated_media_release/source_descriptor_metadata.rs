use super::Spec034ReleaseArtifactError;
use std::fs::Metadata;
use std::path::Path;

#[cfg(unix)]
pub(super) fn same_snapshot(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
        && left.mode() == right.mode()
        && left.nlink() == right.nlink()
        && left.size() == right.size()
}

#[cfg(not(unix))]
pub(super) fn same_snapshot(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

#[cfg(unix)]
pub(super) fn mode(metadata: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o777
}

#[cfg(not(unix))]
pub(super) fn mode(metadata: &Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
pub(super) fn preserve_mode(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode(metadata)))
        .map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(not(unix))]
pub(super) fn preserve_mode(
    _path: &Path,
    _metadata: &Metadata,
) -> Result<(), Spec034ReleaseArtifactError> {
    Ok(())
}
