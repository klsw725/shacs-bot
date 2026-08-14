use super::RecordedTrajectoryStoreError;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) struct StagedTrajectory {
    path: PathBuf,
    published: bool,
}

impl StagedTrajectory {
    pub(super) fn create(path: PathBuf) -> Result<Self, RecordedTrajectoryStoreError> {
        fs::create_dir_all(&path).map_err(RecordedTrajectoryStoreError::Io)?;
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

impl Drop for StagedTrajectory {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
            if let Some(parent) = self.path.parent() {
                let _ = fs::remove_dir(parent);
            }
        }
    }
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), RecordedTrajectoryStoreError> {
    let parent = path
        .parent()
        .ok_or(RecordedTrajectoryStoreError::InvalidRecord)?;
    fs::create_dir_all(parent).map_err(RecordedTrajectoryStoreError::Io)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(RecordedTrajectoryStoreError::Io)?;
    file.write_all(bytes)
        .map_err(RecordedTrajectoryStoreError::Io)?;
    file.sync_all().map_err(RecordedTrajectoryStoreError::Io)
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), RecordedTrajectoryStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(RecordedTrajectoryStoreError::InvalidRecord)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RecordedTrajectoryStoreError::Io(error)),
    }
}

pub(super) fn reject_existing(path: &Path) -> Result<(), RecordedTrajectoryStoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(RecordedTrajectoryStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "trajectory already exists",
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RecordedTrajectoryStoreError::Io(error)),
    }
}

pub(super) fn create_safe_dir(path: &Path) -> Result<(), RecordedTrajectoryStoreError> {
    reject_symlink(path)?;
    fs::create_dir_all(path).map_err(RecordedTrajectoryStoreError::Io)?;
    reject_symlink(path)
}

pub(super) fn sync_tree(path: &Path) -> Result<(), RecordedTrajectoryStoreError> {
    for entry in fs::read_dir(path).map_err(RecordedTrajectoryStoreError::Io)? {
        let entry = entry.map_err(RecordedTrajectoryStoreError::Io)?;
        if entry
            .file_type()
            .map_err(RecordedTrajectoryStoreError::Io)?
            .is_dir()
        {
            sync_tree(&entry.path())?;
        }
    }
    sync_dir(path)
}

pub(super) fn sync_dir(path: &Path) -> Result<(), RecordedTrajectoryStoreError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(RecordedTrajectoryStoreError::Io)
}
