use super::SurfaceActionError;
use fs2::FileExt;
use std::fs::{self, File};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const RUNTIME_OWNERSHIP_MUTATION_LOCK_ERROR: &str =
    "runtime ownership is being mutated by another process";

pub(super) struct RuntimeOwnershipMutationLock {
    _file: File,
}

impl RuntimeOwnershipMutationLock {
    pub(super) fn acquire(marker_path: &Path) -> Result<Self, SurfaceActionError> {
        let path = runtime_ownership_mutation_lock_path(marker_path);
        let parent = path.parent().ok_or_else(|| {
            SurfaceActionError::InvalidMarker(
                "runtime ownership mutation lock path has no parent directory".to_owned(),
            )
        })?;
        fs::create_dir_all(parent)?;
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&path)?;
        if !file.metadata()?.file_type().is_file() {
            return Err(SurfaceActionError::InvalidMarker(format!(
                "runtime ownership mutation lock is not a regular file: {}",
                path.display()
            )));
        }
        file.try_lock_exclusive()
            .map_err(|error| match error.kind() {
                io::ErrorKind::WouldBlock => SurfaceActionError::InvalidMarker(
                    RUNTIME_OWNERSHIP_MUTATION_LOCK_ERROR.to_owned(),
                ),
                io::ErrorKind::Other
                | io::ErrorKind::Interrupted
                | io::ErrorKind::PermissionDenied
                | io::ErrorKind::AlreadyExists
                | io::ErrorKind::NotFound
                | io::ErrorKind::InvalidInput
                | io::ErrorKind::InvalidData
                | io::ErrorKind::TimedOut
                | io::ErrorKind::WriteZero
                | io::ErrorKind::UnexpectedEof
                | io::ErrorKind::Unsupported
                | io::ErrorKind::OutOfMemory
                | _ => SurfaceActionError::Io(error),
            })?;
        Ok(Self { _file: file })
    }
}

fn runtime_ownership_mutation_lock_path(marker_path: &Path) -> PathBuf {
    marker_path.with_extension("json.lock")
}
