use super::*;

pub(super) fn collect_files(root: &Path, directory: &Path, rows: &mut Vec<CacheFile>) -> Result<(), Spec034ReleaseArtifactError> {
    let mut entries = std::fs::read_dir(directory).map_err(Spec034ReleaseArtifactError::Io)?.collect::<Result<Vec<_>, _>>().map_err(Spec034ReleaseArtifactError::Io)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.file_type().is_symlink() { return Err(Spec034ReleaseArtifactError::InvalidConfig); }
        if metadata.is_dir() {
            collect_files(root, &path, rows)?;
        } else if metadata.is_file() {
            let locator = path.strip_prefix(root).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?.to_str().ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?.to_owned();
            let bytes = std::fs::read(&path).map_err(Spec034ReleaseArtifactError::Io)?;
            rows.push(CacheFile { locator, digest: digest_bytes(&bytes), mode: file_mode(&metadata) });
        } else { return Err(Spec034ReleaseArtifactError::InvalidConfig); }
    }
    Ok(())
}

pub(super) fn publish_or_verify(stage: tempfile::TempDir, final_root: &Path, expected: &CacheManifest, expected_bytes: &[u8]) -> Result<(), Spec034ReleaseArtifactError> {
    let parent = final_root.parent().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let final_name = final_root.file_name().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    if final_root.exists() && verify_existing(final_root, expected, expected_bytes).is_ok() {
        make_writable(stage.path())?;
        return Ok(());
    }
    let parent_handle = File::open(parent).map_err(Spec034ReleaseArtifactError::Io)?;
    if final_root.exists() {
        let quarantine = format!(".toolchain-rejected-{}", std::process::id());
        renameat_with(&parent_handle, final_name, &parent_handle, quarantine.as_str(), RenameFlags::NOREPLACE).map_err(|_| Spec034ReleaseArtifactError::DigestMismatch)?;
        fsync(&parent_handle).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        std::fs::remove_dir_all(parent.join(quarantine)).map_err(Spec034ReleaseArtifactError::Io)?;
    }
    let stage_path = stage.keep();
    let stage_name = stage_path.file_name().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    renameat_with(&parent_handle, stage_name, &parent_handle, final_name, RenameFlags::NOREPLACE).map_err(|_| Spec034ReleaseArtifactError::DigestMismatch)?;
    fsync(&parent_handle).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

pub(super) fn make_writable(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)? { make_writable(&entry.map_err(Spec034ReleaseArtifactError::Io)?.path())?; }
    }
    let metadata = std::fs::symlink_metadata(path).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut permissions = metadata.permissions();
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; permissions.set_mode(permissions.mode() | 0o700); }
    #[cfg(not(unix))] permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions).map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(unix)]
pub(super) fn seal_cache_payload(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)? { seal_cache_payload(&entry.map_err(Spec034ReleaseArtifactError::Io)?.path())?; }
    }
    let metadata = std::fs::metadata(path).map_err(Spec034ReleaseArtifactError::Io)?;
    let mode = metadata.mode() & !0o222;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(not(unix))]
pub(super) fn seal_cache_payload(_path: &Path) -> Result<(), Spec034ReleaseArtifactError> { Err(Spec034ReleaseArtifactError::InvalidConfig) }

fn verify_existing(root: &Path, expected: &CacheManifest, expected_bytes: &[u8]) -> Result<(), Spec034ReleaseArtifactError> {
    let handle = crate::runtime::generated_media_release::source::ConfinedSourceReader::open(root)?.into_root();
    let bytes = read_file(&handle, std::ffi::OsStr::new("cache-manifest.json"), 4 * 1024 * 1024)?.ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
    if bytes != expected_bytes { return Err(Spec034ReleaseArtifactError::DigestMismatch); }
    let manifest: CacheManifest = serde_json::from_slice(&bytes).map_err(Spec034ReleaseArtifactError::Json)?;
    if manifest != *expected || digest_tree(&handle, TreeKind::Cache)? != expected.tree_digest { return Err(Spec034ReleaseArtifactError::DigestMismatch); }
    Ok(())
}

pub(super) fn sync_tree(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    let mut entries = std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)?.collect::<Result<Vec<_>, _>>().map_err(Spec034ReleaseArtifactError::Io)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() { sync_tree(&path)?; } else { File::open(&path).and_then(|file| file.sync_all()).map_err(Spec034ReleaseArtifactError::Io)?; }
    }
    File::open(path).and_then(|directory| directory.sync_all()).map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 { use std::os::unix::fs::MetadataExt; metadata.mode() & 0o777 }

#[cfg(not(unix))]
fn file_mode(metadata: &std::fs::Metadata) -> u32 { u32::from(metadata.permissions().readonly()) }
