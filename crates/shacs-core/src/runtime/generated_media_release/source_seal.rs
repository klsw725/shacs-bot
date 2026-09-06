use super::super::artifacts::digest_bytes;
use super::super::model::Spec034ReleaseArtifactError;
use std::collections::BTreeMap;
use std::fs::Metadata;
use std::path::Path;

#[derive(PartialEq, Eq)]
struct EntrySeal {
    kind: u8,
    device: u64,
    inode: u64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
    size: u64,
    digest: Option<String>,
}

pub(super) struct ExecutionSeal(BTreeMap<String, EntrySeal>);

impl ExecutionSeal {
    pub fn capture(root: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        let mut entries = BTreeMap::new();
        capture_entry(root, Path::new(""), &mut entries)?;
        Ok(Self(entries))
    }

    pub fn verify(&self, root: &Path) -> Result<(), Spec034ReleaseArtifactError> {
        let current = Self::capture(root)?;
        (current.0 == self.0)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }

    pub fn matches_files(&self, files: &[(String, Vec<u8>)]) -> bool {
        let mut expected = BTreeMap::from([(".".to_owned(), None)]);
        for (locator, bytes) in files {
            let path = Path::new(locator);
            for parent in path.ancestors().skip(1) {
                let Some(locator) = parent.to_str() else {
                    return false;
                };
                expected.insert(
                    if locator.is_empty() { "." } else { locator }.to_owned(),
                    None,
                );
            }
            expected.insert(locator.clone(), Some(digest_bytes(bytes)));
        }
        self.0
            .iter()
            .map(|(locator, entry)| (locator.clone(), entry.digest.clone()))
            .eq(expected)
    }
}

pub(super) fn set_read_only(
    root: &Path,
    read_only: bool,
) -> Result<(), Spec034ReleaseArtifactError> {
    set_entry_permissions(root, read_only)
}

fn capture_entry(
    root: &Path,
    relative: &Path,
    entries: &mut BTreeMap<String, EntrySeal>,
) -> Result<(), Spec034ReleaseArtifactError> {
    let path = root.join(relative);
    let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let locator = if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        relative
            .to_str()
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?
            .to_owned()
    };
    let digest = file_type
        .is_file()
        .then(|| std::fs::read(&path).map(|bytes| digest_bytes(&bytes)))
        .transpose()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    entries.insert(locator, entry_seal(&metadata, digest));
    if file_type.is_dir() {
        let mut children = std::fs::read_dir(&path)
            .map_err(Spec034ReleaseArtifactError::Io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            capture_entry(root, &relative.join(child.file_name()), entries)?;
        }
    }
    Ok(())
}

fn set_entry_permissions(
    path: &Path,
    read_only: bool,
) -> Result<(), Spec034ReleaseArtifactError> {
    let metadata = std::fs::symlink_metadata(path).map_err(Spec034ReleaseArtifactError::Io)?;
    if metadata.is_dir() {
        for child in std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)? {
            set_entry_permissions(
                &child.map_err(Spec034ReleaseArtifactError::Io)?.path(),
                read_only,
            )?;
        }
    }
    set_permissions(path, metadata.is_dir(), read_only)
}

#[cfg(unix)]
fn set_permissions(
    path: &Path,
    directory: bool,
    read_only: bool,
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match (directory, read_only) {
        (true, true) => 0o500,
        (true, false) => 0o700,
        (false, true) => 0o400,
        (false, false) => 0o600,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(not(unix))]
fn set_permissions(
    path: &Path,
    _directory: bool,
    read_only: bool,
) -> Result<(), Spec034ReleaseArtifactError> {
    let mut permissions = std::fs::metadata(path)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .permissions();
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(unix)]
fn entry_seal(metadata: &Metadata, digest: Option<String>) -> EntrySeal {
    use std::os::unix::fs::MetadataExt;
    EntrySeal {
        kind: u8::from(metadata.is_file()),
        device: metadata.dev(),
        inode: metadata.ino(),
        ctime_seconds: metadata.ctime(),
        ctime_nanoseconds: metadata.ctime_nsec(),
        size: metadata.size(),
        digest,
    }
}

#[cfg(not(unix))]
fn entry_seal(metadata: &Metadata, digest: Option<String>) -> EntrySeal {
    use std::time::UNIX_EPOCH;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    EntrySeal {
        kind: u8::from(metadata.is_file()),
        device: 0,
        inode: 0,
        ctime_seconds: i64::try_from(modified.as_secs()).unwrap_or(i64::MAX),
        ctime_nanoseconds: i64::from(modified.subsec_nanos()),
        size: metadata.len(),
        digest,
    }
}
