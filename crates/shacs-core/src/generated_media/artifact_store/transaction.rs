use super::{ArtifactId, ArtifactStoreError, STAGE_PREFIX};
use fs2::FileExt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct StagedArtifact {
    path: PathBuf,
    published: bool,
}

impl StagedArtifact {
    pub(super) fn create(path: PathBuf) -> Result<Self, ArtifactStoreError> {
        fs::create_dir(&path).map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => ArtifactStoreError::AlreadyExists,
            _ => ArtifactStoreError::Io(error),
        })?;
        Ok(Self {
            path,
            published: false,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for StagedArtifact {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ArtifactStoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(ArtifactStoreError::Io)?;
    file.write_all(bytes).map_err(ArtifactStoreError::Io)?;
    file.sync_all().map_err(ArtifactStoreError::Io)
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), ArtifactStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(ArtifactStoreError::SymlinkRejected)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ArtifactStoreError::Io(error)),
    }
}

pub(super) fn reject_existing(path: &Path) -> Result<(), ArtifactStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(ArtifactStoreError::SymlinkRejected)
        }
        Ok(_) => Err(ArtifactStoreError::AlreadyExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ArtifactStoreError::Io(error)),
    }
}

pub(super) fn sync_dir(path: &Path) -> Result<(), ArtifactStoreError> {
    sync_dir_io(path).map_err(ArtifactStoreError::Io)
}

pub(super) fn sync_dir_io(path: &Path) -> std::io::Result<()> {
    fs::File::open(path).and_then(|directory| directory.sync_all())
}

pub(super) fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        _ => "bin",
    }
}

pub(super) fn staging_path(artifacts: &Path, artifact_id: &ArtifactId) -> PathBuf {
    artifacts.join(format!(
        "{STAGE_PREFIX}{}-{}-{}",
        artifact_id.as_str(),
        std::process::id(),
        NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

pub(super) fn lock_store(root: &Path) -> Result<std::fs::File, ArtifactStoreError> {
    let path = root.join(".artifact.lock");
    reject_symlink(&path)?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(ArtifactStoreError::Io)?;
    lock.lock_exclusive().map_err(ArtifactStoreError::Io)?;
    Ok(lock)
}
