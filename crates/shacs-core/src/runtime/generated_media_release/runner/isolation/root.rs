use super::Spec034ReleaseArtifactError;
use super::root_identity::RootIdentity;
use super::root_monitor::RenameMonitor;
use rustix::fs::{fchmod, openat, statat, unlinkat, AtFlags, Dir, FileType, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::ffi::{CStr, OsString};
use std::fs::File;
use std::path::{Path, PathBuf};

#[cfg(test)]
type PreUnlinkHook = Box<dyn FnOnce(&Path)>;
#[cfg(test)]
type PreNestedUnlinkHook = Box<dyn FnOnce()>;
#[cfg(test)]
thread_local! {
    static PRE_UNLINK_HOOK: std::cell::RefCell<Option<PreUnlinkHook>> = const { std::cell::RefCell::new(None) };
    static PRE_NESTED_UNLINK_HOOK: std::cell::RefCell<Option<PreNestedUnlinkHook>> = const { std::cell::RefCell::new(None) };
}

pub(super) struct RetainedRoot {
    parent: File,
    parent_path: PathBuf,
    parent_identity: RootIdentity,
    root: File,
    root_name: OsString,
    #[cfg(test)]
    path: PathBuf,
    identity: RootIdentity,
    monitor: RenameMonitor,
    cleanup_on_drop: bool,
}

impl RetainedRoot {
    #[cfg(test)]
    pub(super) fn inject_next_pre_unlink_hook(hook: impl FnOnce(&Path) + 'static) {
        PRE_UNLINK_HOOK.with(|slot| slot.replace(Some(Box::new(hook))));
    }

    #[cfg(test)]
    pub(super) fn inject_next_pre_nested_unlink_hook(hook: impl FnOnce() + 'static) {
        PRE_NESTED_UNLINK_HOOK.with(|slot| slot.replace(Some(Box::new(hook))));
    }

    pub(super) fn capture(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        let parent_path = path.parent().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        let root_name = path.file_name().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?.to_owned();
        let parent = File::open(parent_path).map_err(Spec034ReleaseArtifactError::Io)?;
        let parent_identity = RootIdentity::capture(
            &parent
                .metadata()
                .map_err(Spec034ReleaseArtifactError::Io)?,
        )?;
        let root = File::from(openat(
            &parent,
            &root_name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ).map_err(io)?);
        let identity = RootIdentity::capture(&root.metadata().map_err(Spec034ReleaseArtifactError::Io)?)?;
        let monitor = RenameMonitor::arm(&root)?;
        let retained = Self {
            parent,
            parent_path: parent_path.to_path_buf(),
            parent_identity,
            root,
            root_name,
            #[cfg(test)]
            path: path.to_path_buf(),
            identity,
            monitor,
            cleanup_on_drop: true,
        };
        retained.verify_initial_identity()?;
        Ok(retained)
    }

    #[cfg(test)]
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(super) fn owner(&self) -> u32 {
        self.identity.owner
    }

    pub(super) fn cleanup(mut self, event_uncertain: bool) -> Result<String, Spec034ReleaseArtifactError> {
        self.cleanup_on_drop = false;
        if event_uncertain {
            return Err(residual());
        }
        self.cleanup_exact()
    }

    fn cleanup_exact(&self) -> Result<String, Spec034ReleaseArtifactError> {
        self.verify_parent().map_err(|_| residual())?;
        self.monitor.drain().map_err(|_| residual())?;
        self.verify_initial_identity().map_err(|_| residual())?;
        remove_contents(&self.root, self.identity.owner).map_err(|_| residual())?;
        self.monitor.drain().map_err(|_| residual())?;
        self.verify_path_entry().map_err(|_| residual())?;
        #[cfg(test)]
        PRE_UNLINK_HOOK.with(|slot| {
            if let Some(hook) = slot.take() {
                hook(&self.path);
            }
        });
        self.verify_parent().map_err(|_| residual())?;
        self.verify_path_entry().map_err(|_| residual())?;
        unlinkat(&self.parent, &self.root_name, AtFlags::REMOVEDIR).map_err(|_| residual())?;
        self.verify_unlinked().map_err(|_| residual())?;
        Ok(self.binding_digest())
    }

    fn verify_initial_identity(&self) -> Result<(), Spec034ReleaseArtifactError> {
        let descriptor = RootIdentity::capture(&self.root.metadata().map_err(Spec034ReleaseArtifactError::Io)?)?;
        if !descriptor.same_object(self.identity) {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        self.verify_path_entry()
    }

    fn verify_parent(&self) -> Result<(), Spec034ReleaseArtifactError> {
        let descriptor = RootIdentity::capture(
            &self
                .parent
                .metadata()
                .map_err(Spec034ReleaseArtifactError::Io)?,
        )?;
        let path = RootIdentity::capture(
            &std::fs::symlink_metadata(&self.parent_path)
                .map_err(Spec034ReleaseArtifactError::Io)?,
        )?;
        if !descriptor.same_object(self.parent_identity) || !path.same_object(self.parent_identity) {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        Ok(())
    }

    fn verify_path_entry(&self) -> Result<(), Spec034ReleaseArtifactError> {
        let entry = statat(&self.parent, &self.root_name, AtFlags::SYMLINK_NOFOLLOW).map_err(io)?;
        if stat_device(&entry)? != self.identity.device || entry.st_ino != self.identity.inode || entry.st_uid != self.identity.owner {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        Ok(())
    }

    fn verify_unlinked(&self) -> Result<(), Spec034ReleaseArtifactError> {
        use std::os::unix::fs::MetadataExt;
        self.monitor.confirm_unlinked()?;
        let metadata = self.root.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        let descriptor = RootIdentity::capture(&metadata)?;
        if !descriptor.same_object(self.identity) || !link_state_proves_unlinked(metadata.nlink()) {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        Ok(())
    }

    fn binding_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"spec034.completed-isolation-cleanup.v2\0");
        digest.update(self.parent_identity.bytes());
        digest.update(self.root_name.as_encoded_bytes());
        digest.update(self.identity.bytes());
        format!("sha256:{:x}", digest.finalize())
    }
}

impl Drop for RetainedRoot {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = self.cleanup_exact();
        }
    }
}

fn remove_contents(directory: &File, owner: u32) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = directory.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    if metadata.uid() != owner {
        return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
    }
    fchmod(directory, Mode::from_raw_mode(0o700)).map_err(io)?;
    let mut stream = Dir::read_from(directory).map_err(io)?;
    let mut names = Vec::new();
    while let Some(entry) = stream.read() {
        let entry = entry.map_err(io)?;
        if entry.file_name().to_bytes() != b"." && entry.file_name().to_bytes() != b".." {
            names.push(entry.file_name().to_owned());
        }
    }
    for name in names {
        remove_entry(directory, &name, owner)?;
    }
    Ok(())
}

fn remove_entry(parent: &File, name: &CStr, owner: u32) -> Result<(), Spec034ReleaseArtifactError> {
    let before = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io)?;
    if FileType::from_raw_mode(before.st_mode).is_dir() {
        let child = File::from(openat(parent, name, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(io)?);
        let opened = child.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != stat_device(&before)? || opened.ino() != before.st_ino || opened.uid() != owner {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        remove_contents(&child, owner)?;
        run_pre_nested_unlink_hook();
        let current = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io)?;
        if stat_device(&current)? != stat_device(&before)?
            || current.st_ino != before.st_ino
            || current.st_uid != owner
        {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(io)
    } else {
        run_pre_nested_unlink_hook();
        let current = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io)?;
        if stat_device(&current)? != stat_device(&before)?
            || current.st_ino != before.st_ino
            || current.st_uid != owner
        {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        unlinkat(parent, name, AtFlags::empty()).map_err(io)
    }
}

#[cfg(test)]
fn run_pre_nested_unlink_hook() {
    PRE_NESTED_UNLINK_HOOK.with(|slot| {
        if let Some(hook) = slot.take() {
            hook();
        }
    });
}

#[cfg(not(test))]
const fn run_pre_nested_unlink_hook() {}

fn io(error: rustix::io::Errno) -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::Io(std::io::Error::from_raw_os_error(error.raw_os_error()))
}

fn stat_device(stat: &rustix::fs::Stat) -> Result<u64, Spec034ReleaseArtifactError> {
    u64::try_from(stat.st_dev).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

fn residual() -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 }
}

#[cfg(target_vendor = "apple")]
const fn link_state_proves_unlinked(links: u64) -> bool {
    links == 2
}

#[cfg(not(target_vendor = "apple"))]
const fn link_state_proves_unlinked(links: u64) -> bool {
    links == 0
}
