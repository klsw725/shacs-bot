use super::super::super::model::Spec034ReleaseArtifactError;
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
use rustix::fs::{flock, FlockOperation};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(super) fn lock_cache(root: &Path, key: &str) -> Result<File, Spec034ReleaseArtifactError> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("locks").join(format!("{key}.lock")))
        .map_err(Spec034ReleaseArtifactError::Io)?;
    flock(&lock, FlockOperation::LockExclusive)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    Ok(lock)
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(super) fn lock_target(
    repo: &Path,
    evidence: &Path,
) -> Result<File, Spec034ReleaseArtifactError> {
    let evidence = if evidence.is_absolute() {
        evidence.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(Spec034ReleaseArtifactError::Io)?
            .join(evidence)
    };
    let evidence = canonical_missing_path(&evidence)?;
    let mut digest = Sha256::new();
    digest.update(repo.as_os_str().as_encoded_bytes());
    digest.update([0]);
    digest.update(evidence.as_os_str().as_encoded_bytes());
    let directory = std::env::temp_dir().join("shacs-spec034-locks");
    std::fs::create_dir_all(&directory).map_err(Spec034ReleaseArtifactError::Io)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(format!("{:x}.lock", digest.finalize())))
        .map_err(Spec034ReleaseArtifactError::Io)?;
    flock(&lock, FlockOperation::LockExclusive)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    Ok(lock)
}

pub(super) fn canonical_missing_path(
    path: &Path,
) -> Result<PathBuf, Spec034ReleaseArtifactError> {
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        missing.push(
            cursor
                .file_name()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
                .to_os_string(),
        );
        cursor = cursor
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    }
    let mut canonical = cursor
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
pub(super) fn lock_target(
    _repo: &Path,
    _evidence: &Path,
) -> Result<File, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}
