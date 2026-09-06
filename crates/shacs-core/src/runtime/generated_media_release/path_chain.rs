use super::artifacts::digest_bytes;
use super::model::Spec034ReleaseArtifactError;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

#[derive(PartialEq, Eq)]
struct PathEntrySeal {
    path: PathBuf,
    device: u64,
    inode: u64,
    ctime_seconds: Option<i64>,
    ctime_nanoseconds: Option<i64>,
    mode: Option<u32>,
    links: Option<u64>,
    size: Option<u64>,
    digest: Option<String>,
}

pub(super) struct PathChainSeal {
    boundary: PathBuf,
    path: PathBuf,
    entries: Vec<PathEntrySeal>,
    mutable_leaf: bool,
}

impl PathChainSeal {
    #[cfg(test)]
    pub fn capture(path: &Path, digest_leaf: bool) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::capture_mode(Path::new("/"), path, digest_leaf, false)
    }

    pub fn capture_mutable(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        let boundary = path
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        Self::capture_mode(boundary, path, false, true)
    }

    pub fn capture_controlled(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        let boundary = path
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        Self::capture_mode(boundary, path, false, false)
    }

    pub fn capture_leaf(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::capture_mode(path, path, false, false)
    }

    pub fn capture_digest_leaf(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::capture_mode(path, path, true, false)
    }

    fn capture_mode(
        boundary: &Path,
        path: &Path,
        digest_leaf: bool,
        mutable_leaf: bool,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let boundary = boundary
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let path = path
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if !path.starts_with(&boundary) {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let mut components = path
            .ancestors()
            .take_while(|component| component.starts_with(&boundary))
            .map(Path::to_path_buf)
            .collect::<Vec<_>>();
        components.reverse();
        let leaf_index = components.len().saturating_sub(1);
        let entries = components
            .into_iter()
            .enumerate()
            .map(|(index, component)| {
                let metadata = std::fs::symlink_metadata(&component)
                    .map_err(Spec034ReleaseArtifactError::Io)?;
                let digest = (digest_leaf && index == leaf_index && metadata.is_file())
                    .then(|| std::fs::read(&component).map(|bytes| digest_bytes(&bytes)))
                    .transpose()
                    .map_err(Spec034ReleaseArtifactError::Io)?;
                Ok(entry_seal(
                    component,
                    &metadata,
                    digest,
                    mutable_leaf && index == leaf_index,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            boundary,
            path,
            entries,
            mutable_leaf,
        })
    }

    pub fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        let current = Self::capture_mode(
            &self.boundary,
            &self.path,
            self.entries.last().is_some_and(|row| row.digest.is_some()),
            self.mutable_leaf,
        )?;
        (current.entries == self.entries)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }

    pub fn reseal(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        let current = Self::capture_mode(
            &self.boundary,
            &self.path,
            self.entries.last().is_some_and(|row| row.digest.is_some()),
            self.mutable_leaf,
        )?;
        self.entries = current.entries;
        Ok(())
    }
}

#[cfg(unix)]
fn entry_seal(
    path: PathBuf,
    metadata: &Metadata,
    digest: Option<String>,
    mutable: bool,
) -> PathEntrySeal {
    use std::os::unix::fs::MetadataExt;
    PathEntrySeal {
        path,
        device: metadata.dev(),
        inode: metadata.ino(),
        ctime_seconds: (!mutable).then(|| metadata.ctime()),
        ctime_nanoseconds: (!mutable).then(|| metadata.ctime_nsec()),
        mode: Some(metadata.mode()),
        links: (!mutable).then(|| metadata.nlink()),
        size: (!mutable).then(|| metadata.size()),
        digest,
    }
}

#[cfg(not(unix))]
fn entry_seal(
    path: PathBuf,
    metadata: &Metadata,
    digest: Option<String>,
    mutable: bool,
) -> PathEntrySeal {
    use std::time::UNIX_EPOCH;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    PathEntrySeal {
        path,
        device: 0,
        inode: 0,
        ctime_seconds: (!mutable).then(|| i64::try_from(modified.as_secs()).unwrap_or(i64::MAX)),
        ctime_nanoseconds: (!mutable).then(|| i64::from(modified.subsec_nanos())),
        mode: Some(u32::from(metadata.permissions().readonly())),
        links: None,
        size: (!mutable).then(|| metadata.len()),
        digest,
    }
}
