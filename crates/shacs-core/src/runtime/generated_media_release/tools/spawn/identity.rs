use super::{live_cdhash, process_executable, static_cdhash, Spec034ReleaseArtifactError};
use crate::runtime::generated_media_release::artifacts::digest_bytes;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: libc::pid_t,
    pub(crate) parent_pid: libc::pid_t,
    pub(crate) start_seconds: u64,
    pub(crate) start_microseconds: u64,
    pub(crate) executable: PathBuf,
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) digest: String,
    pub(crate) cdhash: Vec<u8>,
}

impl ProcessIdentity {
    pub(crate) fn same_process(&self, current: &Self) -> bool {
        self.pid == current.pid
            && self.start_seconds == current.start_seconds
            && self.start_microseconds == current.start_microseconds
    }

    pub(crate) fn same_launch(&self, current: &Self) -> bool {
        self.same_process(current)
            && self.device == current.device
            && self.inode == current.inode
            && self.digest == current.digest
            && self.cdhash == current.cdhash
    }

    pub(crate) fn same_executable(&self, current: &Self) -> bool {
        self.device == current.device
            && self.inode == current.inode
            && self.digest == current.digest
            && self.cdhash == current.cdhash
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn capture_process_identity(
    pid: libc::pid_t,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    capture_process_identity_with(pid, true)
}

#[cfg(all(not(test), target_vendor = "apple"))]
pub(crate) fn capture_observed_process_identity(
    pid: libc::pid_t,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    capture_process_identity_with(pid, false)
}

#[cfg(target_vendor = "apple")]
fn capture_process_identity_with(
    pid: libc::pid_t,
    require_cdhash: bool,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;

    let executable = process_executable(pid)?;
    let mut file = File::open(&executable).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let (start_seconds, start_microseconds, parent_pid) = process_details(pid)?;
    let cdhash = match live_cdhash(pid) {
        Ok(cdhash) => cdhash,
        Err(Spec034ReleaseArtifactError::InvalidConfig) if !require_cdhash => Vec::new(),
        Err(error) => return Err(error),
    };
    Ok(ProcessIdentity {
        pid,
        parent_pid,
        start_seconds,
        start_microseconds,
        executable,
        device: metadata.dev(),
        inode: metadata.ino(),
        digest: digest_bytes(&bytes),
        cdhash,
    })
}

#[cfg(target_vendor = "apple")]
fn process_details(pid: libc::pid_t) -> Result<(u64, u64, libc::pid_t), Spec034ReleaseArtifactError> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    // SAFETY: [Category 8 - FFI boundary] `info` is writable for exactly `size` bytes;
    // a complete-size return is required before the value is assumed initialized.
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count != size {
        return Err(Spec034ReleaseArtifactError::Io(
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: [Category 4 - uninitialized memory] the exact-size return above proves
    // `proc_pidinfo` initialized the complete `proc_bsdinfo` value.
    let info = unsafe { info.assume_init() };
    Ok((
        info.pbi_start_tvsec,
        info.pbi_start_tvusec,
        i32::try_from(info.pbi_ppid).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
    ))
}

#[cfg(not(target_vendor = "apple"))]
pub(crate) fn capture_process_identity(
    _pid: libc::pid_t,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

#[cfg(all(not(test), not(target_vendor = "apple")))]
pub(crate) fn capture_observed_process_identity(
    _pid: libc::pid_t,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

pub(crate) fn static_identity_matches(
    identity: &ProcessIdentity,
) -> Result<bool, Spec034ReleaseArtifactError> {
    Ok(static_cdhash(&identity.executable)? == identity.cdhash)
}

#[cfg(target_vendor = "apple")]
pub(crate) fn capture_static_identity(
    executable: &std::path::Path,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let executable = executable.canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut file = File::open(&executable).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(ProcessIdentity {
        pid: 0,
        parent_pid: 0,
        start_seconds: 0,
        start_microseconds: 0,
        device: metadata.dev(),
        inode: metadata.ino(),
        digest: digest_bytes(&bytes),
        cdhash: static_cdhash(&executable)?,
        executable,
    })
}

#[cfg(not(target_vendor = "apple"))]
pub(crate) fn capture_static_identity(
    _executable: &std::path::Path,
) -> Result<ProcessIdentity, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}
