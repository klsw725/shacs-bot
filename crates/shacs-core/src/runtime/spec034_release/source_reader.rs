use super::Spec034ReleaseArtifactError;
use rustix::fd::OwnedFd;
use rustix::fs::{openat, Mode, OFlags, CWD};
use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

pub(in crate::runtime::spec034_release) struct ConfinedSourceReader {
    root: File,
}

impl ConfinedSourceReader {
    pub(in crate::runtime::spec034_release) fn from_root(root: File) -> Self {
        Self { root }
    }

    pub(in crate::runtime::spec034_release) fn open(
        root: &Path,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let absolute = root.is_absolute();
        let anchor = if absolute { Path::new("/") } else { Path::new(".") };
        let descriptor = openat(CWD, anchor, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let mut current: File = descriptor.into();
        for component in root.components() {
            let name = match component {
                Component::RootDir if absolute => continue,
                Component::CurDir if !absolute => continue,
                Component::Normal(name) => name,
                _ => return Err(Spec034ReleaseArtifactError::InvalidEvidence),
            };
            let directory = open_component(&current, name, DIRECTORY_FLAGS)?
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            current = directory.into();
        }
        Ok(Self { root: current })
    }

    pub(in crate::runtime::spec034_release) fn root_metadata(
        &self,
    ) -> Result<Metadata, Spec034ReleaseArtifactError> {
        self.root.metadata().map_err(Spec034ReleaseArtifactError::Io)
    }

    pub(in crate::runtime::spec034_release) fn into_root(self) -> File {
        self.root
    }

    pub(in crate::runtime::spec034_release) fn read(
        &self,
        locator: &str,
        max_bytes: u64,
    ) -> Result<Option<Vec<u8>>, Spec034ReleaseArtifactError> {
        self.read_with(locator, max_bytes, || {})
    }

    pub(in crate::runtime::spec034_release) fn entry_names_with(
        &self,
        after_first_entry: impl FnOnce(),
    ) -> Result<Vec<String>, Spec034ReleaseArtifactError> {
        let directory = cap_primitives::fs::read_base_dir(&self.root)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let mut after_first_entry = Some(after_first_entry);
        let mut names = Vec::new();
        for entry in directory {
            let entry = entry.map_err(Spec034ReleaseArtifactError::Io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
            names.push(name);
            if let Some(hook) = after_first_entry.take() {
                hook();
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub(in crate::runtime::spec034_release) fn read_with(
        &self,
        locator: &str,
        max_bytes: u64,
        after_first_ancestor: impl FnOnce(),
    ) -> Result<Option<Vec<u8>>, Spec034ReleaseArtifactError> {
        self.read_with_hooks(locator, max_bytes, after_first_ancestor, || {})
    }

    pub(in crate::runtime::spec034_release) fn read_with_hooks(
        &self,
        locator: &str,
        max_bytes: u64,
        after_first_ancestor: impl FnOnce(),
        after_file_open: impl FnOnce(),
    ) -> Result<Option<Vec<u8>>, Spec034ReleaseArtifactError> {
        Ok(self
            .read_with_metadata_hooks(
                locator,
                max_bytes,
                after_first_ancestor,
                after_file_open,
            )?
            .map(|(bytes, _)| bytes))
    }

    pub(in crate::runtime::spec034_release) fn read_with_metadata_hooks(
        &self,
        locator: &str,
        max_bytes: u64,
        after_first_ancestor: impl FnOnce(),
        after_file_open: impl FnOnce(),
    ) -> Result<Option<(Vec<u8>, Metadata)>, Spec034ReleaseArtifactError> {
        self.read_with_all_hooks(
            locator,
            max_bytes,
            after_first_ancestor,
            after_file_open,
            || {},
        )
    }

    #[cfg(test)]
    pub(super) fn read_with_content_hook(
        &self,
        locator: &str,
        max_bytes: u64,
        after_file_open: impl FnOnce(),
        after_bounded_read: impl FnOnce(),
    ) -> Result<Option<(Vec<u8>, Metadata)>, Spec034ReleaseArtifactError> {
        self.read_with_all_hooks(locator, max_bytes, || {}, after_file_open, after_bounded_read)
    }

    fn read_with_all_hooks(
        &self,
        locator: &str,
        max_bytes: u64,
        after_first_ancestor: impl FnOnce(),
        after_file_open: impl FnOnce(),
        after_bounded_read: impl FnOnce(),
    ) -> Result<Option<(Vec<u8>, Metadata)>, Spec034ReleaseArtifactError> {
        let components = Path::new(locator)
            .components()
            .map(|component| match component {
                Component::Normal(name) => Ok(name),
                _ => Err(Spec034ReleaseArtifactError::InvalidEvidence),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (file_name, ancestors) = components
            .split_last()
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        let mut current = self
            .root
            .try_clone()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let mut after_first_ancestor = Some(after_first_ancestor);
        for ancestor in ancestors {
            let Some(directory) = open_component(&current, ancestor, DIRECTORY_FLAGS)? else {
                return Ok(None);
            };
            current = directory.into();
            if let Some(hook) = after_first_ancestor.take() {
                hook();
            }
        }
        let Some(descriptor) = open_component(&current, file_name, FILE_FLAGS)? else {
            return Ok(None);
        };
        let mut file: File = descriptor.into();
        after_file_open();
        let before = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if !before.is_file() || before.len() > max_bytes {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let limit = max_bytes
            .checked_add(1)
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(limit)
            .read_to_end(&mut bytes)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        after_bounded_read();
        if u64::try_from(bytes.len()).map_or(true, |length| length > max_bytes) {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        let after = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        file.seek(SeekFrom::Start(0))
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let mut confirmed = Vec::new();
        file.by_ref()
            .take(limit)
            .read_to_end(&mut confirmed)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let confirmed_metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if bytes != confirmed
            || !same_file(&before, &after)
            || !same_file(&after, &confirmed_metadata)
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok(Some((bytes, confirmed_metadata)))
    }
}

#[cfg(unix)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
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
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

fn open_component(
    parent: &File,
    name: &OsStr,
    flags: OFlags,
) -> Result<Option<OwnedFd>, Spec034ReleaseArtifactError> {
    match openat(parent, name, flags, Mode::empty()) {
        Ok(descriptor) => Ok(Some(descriptor)),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(None),
        Err(_) => Err(Spec034ReleaseArtifactError::InvalidConfig),
    }
}
