use super::Spec034ReleaseArtifactError;
#[cfg(target_vendor = "apple")]
use super::digest_bytes;
#[cfg(target_vendor = "apple")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_vendor = "apple")]
#[path = "monitor/descriptor_limit.rs"]
mod descriptor_limit;

#[cfg(target_vendor = "apple")]
mod platform {
    use super::*;
    use std::ffi::CString;
    use std::fs::{File, Metadata};
    use std::io::{Read, Seek};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::macos::fs::MetadataExt as MacMetadataExt;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[path = "darwin_support.rs"]
    mod support;
    use support::{create_queue, empty_event, register, require_apfs};

    const FATAL_EVENTS: u32 = libc::NOTE_DELETE
        | libc::NOTE_WRITE
        | libc::NOTE_EXTEND
        | libc::NOTE_LINK
        | libc::NOTE_RENAME
        | libc::NOTE_REVOKE;
    const WATCH_EVENTS: u32 = FATAL_EVENTS | libc::NOTE_ATTRIB;

    pub(crate) struct ExecutionLedger {
        queue: ExecutionQueue,
        entries: Vec<Entry>,
        fatal: Arc<AtomicBool>,
    }

    #[derive(Clone)]
    pub(crate) struct ExecutionQueue {
        file: Arc<File>,
        fatal: Arc<AtomicBool>,
    }

    struct Entry {
        path: PathBuf,
        event: File,
        content: Option<File>,
        baseline: Baseline,
    }

    #[derive(PartialEq, Eq)]
    struct Baseline {
        device: u64,
        inode: u64,
        mode: u32,
        uid: u32,
        gid: u32,
        flags: u32,
        links: u64,
        size: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
        digest: Option<String>,
    }

    impl ExecutionLedger {
        pub(crate) fn new_queue() -> Result<ExecutionQueue, Spec034ReleaseArtifactError> {
            Ok(ExecutionQueue {
                file: Arc::new(create_queue()?),
                fatal: Arc::new(AtomicBool::new(false)),
            })
        }

        pub(crate) fn ensure_capacity(path_count: usize) -> Result<(), Spec034ReleaseArtifactError> {
            super::descriptor_limit::ensure(path_count)
        }

        pub(crate) fn arm(paths: &[PathBuf]) -> Result<Self, Spec034ReleaseArtifactError> {
            let queue = Self::new_queue()?;
            Self::arm_on(paths, queue)
        }

        pub(crate) fn arm_on(
            paths: &[PathBuf],
            queue: ExecutionQueue,
        ) -> Result<Self, Spec034ReleaseArtifactError> {
            require_apfs(paths.first().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?)?;
            super::descriptor_limit::ensure(paths.len())?;
            let mut entries = paths
                .iter()
                .map(|path| Entry::open(path))
                .collect::<Result<Vec<_>, _>>()?;
            register(&queue.file, &entries)?;
            for entry in &mut entries {
                entry.baseline = entry.capture()?;
            }
            let fatal = Arc::clone(&queue.fatal);
            let ledger = Self { queue, entries, fatal };
            ledger.verify()?;
            Ok(ledger)
        }

        pub(crate) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
            if self.fatal.load(Ordering::Acquire) {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
            let mut events = [empty_event(); 64];
            loop {
                let timeout = libc::timespec { tv_sec: 0, tv_nsec: 0 };
                // SAFETY: [Category 8 - FFI boundary] `events` is writable for its declared
                // length, `queue` is a live kqueue descriptor, and timeout is initialized.
                let count = unsafe {
                    libc::kevent(
                        self.queue.file.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        events.as_mut_ptr(),
                        i32::try_from(events.len())
                            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
                        &timeout,
                    )
                };
                if count < 0 {
                    self.fatal.store(true, Ordering::Release);
                    return Err(Spec034ReleaseArtifactError::Io(std::io::Error::last_os_error()));
                }
                if count == 0 {
                    break;
                }
                for event in events.iter().take(
                    usize::try_from(count).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
                ) {
                    if event.flags & (libc::EV_ERROR | libc::EV_EOF) != 0
                        || event.fflags & FATAL_EVENTS != 0
                    {
                        self.fatal.store(true, Ordering::Release);
                        return Err(Spec034ReleaseArtifactError::DigestMismatch);
                    }
                    if event.fflags & libc::NOTE_ATTRIB != 0 {
                        let Some(entry) = self
                            .entries
                            .iter()
                            .find(|entry| entry.event.as_raw_fd() as usize == event.ident)
                        else {
                            continue;
                        };
                        if entry.capture()? != entry.baseline {
                            self.fatal.store(true, Ordering::Release);
                            return Err(Spec034ReleaseArtifactError::DigestMismatch);
                        }
                    }
                }
            }
            for entry in &self.entries {
                if entry.capture()? != entry.baseline {
                    self.fatal.store(true, Ordering::Release);
                    return Err(Spec034ReleaseArtifactError::DigestMismatch);
                }
            }
            Ok(())
        }
    }

    impl Entry {
        fn open(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
            let raw = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
            // SAFETY: [Category 8 - FFI boundary] the C path is NUL-terminated and flags do
            // not require a mode argument. A negative descriptor is rejected before wrapping.
            let descriptor = unsafe {
                libc::open(raw.as_ptr(), libc::O_EVTONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            };
            if descriptor < 0 {
                return Err(Spec034ReleaseArtifactError::Io(std::io::Error::last_os_error()));
            }
            // SAFETY: [Category 12 - invalid free] successful `open` returns one owned file
            // descriptor and ownership is transferred exactly once into `File`.
            let event = unsafe { File::from_raw_fd(descriptor) };
            let content = event
                .metadata()
                .map_err(Spec034ReleaseArtifactError::Io)?
                .is_file()
                .then(|| File::open(path).map_err(Spec034ReleaseArtifactError::Io))
                .transpose()?;
            let mut entry = Self {
                path: path.to_path_buf(),
                event,
                content,
                baseline: Baseline::empty(),
            };
            entry.baseline = entry.capture()?;
            Ok(entry)
        }

        fn capture(&self) -> Result<Baseline, Spec034ReleaseArtifactError> {
            let descriptor = self.event.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
            let path = std::fs::symlink_metadata(&self.path)
                .map_err(|_| Spec034ReleaseArtifactError::DigestMismatch)?;
            if descriptor.dev() != path.dev() || descriptor.ino() != path.ino() {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
            let digest = self
                .content
                .as_ref()
                .map(|file| {
                    let mut file = file.try_clone().map_err(Spec034ReleaseArtifactError::Io)?;
                    file.rewind().map_err(Spec034ReleaseArtifactError::Io)?;
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes).map_err(Spec034ReleaseArtifactError::Io)?;
                    Ok::<_, Spec034ReleaseArtifactError>(digest_bytes(&bytes))
                })
                .transpose()?;
            Ok(Baseline::from_metadata(&descriptor, digest))
        }
    }

    impl Baseline {
        const fn empty() -> Self {
            Self {
                device: 0, inode: 0, mode: 0, uid: 0, gid: 0, flags: 0, links: 0, size: 0,
                modified_seconds: 0, modified_nanoseconds: 0, changed_seconds: 0,
                changed_nanoseconds: 0, digest: None,
            }
        }

        fn from_metadata(metadata: &Metadata, digest: Option<String>) -> Self {
            Self {
                device: metadata.dev(), inode: metadata.ino(), mode: metadata.mode(),
                uid: metadata.uid(), gid: metadata.gid(), flags: metadata.st_flags(),
                links: metadata.nlink(), size: metadata.size(),
                modified_seconds: metadata.mtime(), modified_nanoseconds: metadata.mtime_nsec(),
                changed_seconds: metadata.ctime(), changed_nanoseconds: metadata.ctime_nsec(), digest,
            }
        }
    }

}

#[cfg(not(target_vendor = "apple"))]
#[path = "monitor/unsupported.rs"]
mod platform;

pub(super) use platform::{ExecutionLedger, ExecutionQueue};
