use super::Spec034ReleaseArtifactError;
use std::fs::File;
#[cfg(any(target_os = "linux", target_vendor = "apple"))]
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

static PROCESS_CONTROL_LOCK: ProcessControlLock = ProcessControlLock {
    state: Mutex::new(ProcessControlState {
        owner: None,
        depth: 0,
    }),
    available: Condvar::new(),
};

struct ProcessControlLock {
    state: Mutex<ProcessControlState>,
    available: Condvar,
}

struct ProcessControlState {
    owner: Option<ThreadId>,
    depth: usize,
}

struct ProcessControlGuard {
    outermost: bool,
}

impl ProcessControlLock {
    fn lock(&'static self) -> Result<ProcessControlGuard, Spec034ReleaseArtifactError> {
        let current = std::thread::current().id();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        while state.owner.as_ref().is_some_and(|owner| owner != &current) {
            state = self
                .available
                .wait(state)
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        }
        let outermost = state.depth == 0;
        state.owner = Some(current);
        state.depth += 1;
        Ok(ProcessControlGuard { outermost })
    }
}

impl Drop for ProcessControlGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = PROCESS_CONTROL_LOCK.state.lock() {
            state.depth -= 1;
            if state.depth == 0 {
                state.owner = None;
                PROCESS_CONTROL_LOCK.available.notify_one();
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(in crate::runtime::generated_media_release) struct ControlLease {
    _directory: File,
    _process: ProcessControlGuard,
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
pub(in crate::runtime::generated_media_release) struct ControlLease;

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(in crate::runtime::generated_media_release) fn acquire_control(
    parent: &Path,
) -> Result<ControlLease, Spec034ReleaseArtifactError> {
    let process = PROCESS_CONTROL_LOCK.lock()?;
    let directory = File::open(parent).map_err(Spec034ReleaseArtifactError::Io)?;
    if process.outermost {
        rustix::fs::flock(&directory, rustix::fs::FlockOperation::LockExclusive)
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    }
    Ok(ControlLease {
        _directory: directory,
        _process: process,
    })
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
pub(in crate::runtime::generated_media_release) fn acquire_control(
    _parent: &std::path::Path,
) -> Result<ControlLease, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

pub(in crate::runtime::generated_media_release) struct ToolRoot(tempfile::TempDir);

impl ToolRoot {
    pub(in crate::runtime::generated_media_release) fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}

impl Drop for ToolRoot {
    fn drop(&mut self) {
        // The read-only tool closure makes TempDir's own removal fail silently.
        restore_owner_write(self.0.path());
    }
}

#[cfg(unix)]
fn restore_owner_write(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if !metadata.is_dir() {
        return;
    }
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o700);
    let _ = std::fs::set_permissions(path, permissions);
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            restore_owner_write(&entry.path());
        }
    }
}

#[cfg(not(unix))]
fn restore_owner_write(_path: &std::path::Path) {}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub(super) fn controlled_temp_root(
) -> Result<(ControlLease, ToolRoot), Spec034ReleaseArtifactError> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let parent = workspace
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let control = acquire_control(parent)?;
    let root = tempfile::Builder::new()
        .prefix(".shacs-spec034-tools-")
        .tempdir_in(parent)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    Ok((
        control,
        ToolRoot(root),
    ))
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
pub(super) fn controlled_temp_root(
) -> Result<(ControlLease, ToolRoot), Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}
