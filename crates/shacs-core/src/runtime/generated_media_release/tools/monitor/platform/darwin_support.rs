use super::{Entry, Spec034ReleaseArtifactError, WATCH_EVENTS};
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub(super) fn require_apfs(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    let raw = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: [Category 4 - uninitialized memory] `statfs` initializes the complete output
    // on success; the value is only assumed initialized after a zero return status.
    let result = unsafe { libc::statfs(raw.as_ptr(), status.as_mut_ptr()) };
    if result != 0 {
        return Err(Spec034ReleaseArtifactError::Io(
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: [Category 4 - uninitialized memory] the successful call above initialized it.
    let status = unsafe { status.assume_init() };
    let kind = status
        .f_fstypename
        .iter()
        .map(|byte| *byte as u8)
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    (kind == b"apfs")
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)
}

pub(super) fn create_queue() -> Result<File, Spec034ReleaseArtifactError> {
    // SAFETY: [Category 8 - FFI boundary] `kqueue` has no pointer arguments; negative
    // results are rejected and successful ownership is transferred once.
    let descriptor = unsafe { libc::kqueue() };
    if descriptor < 0 {
        return Err(Spec034ReleaseArtifactError::Io(
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: [Category 12 - invalid free] successful `kqueue` returns one owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

pub(super) fn register(
    queue: &File,
    entries: &[Entry],
) -> Result<(), Spec034ReleaseArtifactError> {
    let changes = entries
        .iter()
        .map(|entry| libc::kevent {
            ident: entry.event.as_raw_fd() as usize,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR | libc::EV_RECEIPT,
            fflags: WATCH_EVENTS,
            data: 0,
            udata: std::ptr::null_mut(),
        })
        .collect::<Vec<_>>();
    let mut receipts = vec![empty_event(); changes.len()];
    // SAFETY: [Category 8 - FFI boundary] both slices are initialized and valid for the
    // lengths supplied; the queue descriptor is live for this call.
    let count = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            changes.as_ptr(),
            i32::try_from(changes.len())
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
            receipts.as_mut_ptr(),
            i32::try_from(receipts.len())
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
            std::ptr::null(),
        )
    };
    if count
        != i32::try_from(changes.len())
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?
        || receipts
            .iter()
            .any(|event| event.flags & libc::EV_ERROR == 0 || event.data != 0)
    {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    Ok(())
}

pub(super) const fn empty_event() -> libc::kevent {
    libc::kevent {
        ident: 0,
        filter: 0,
        flags: 0,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}
