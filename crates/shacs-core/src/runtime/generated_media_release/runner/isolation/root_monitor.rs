use super::Spec034ReleaseArtifactError;
use std::fs::File;

#[cfg(target_vendor = "apple")]
pub(super) struct RenameMonitor {
    queue: nix::sys::event::Kqueue,
}

#[cfg(target_vendor = "apple")]
impl RenameMonitor {
    pub(super) fn arm(root: &File) -> Result<Self, Spec034ReleaseArtifactError> {
        use nix::sys::event::{EvFlags, EventFilter, FilterFlag, KEvent, Kqueue};
        use std::os::fd::AsRawFd;
        let queue = Kqueue::new().map_err(nix_io)?;
        let event = KEvent::new(
            usize::try_from(root.as_raw_fd())
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
            EventFilter::EVFILT_VNODE,
            EvFlags::EV_ADD | EvFlags::EV_ENABLE | EvFlags::EV_CLEAR | EvFlags::EV_RECEIPT,
            FilterFlag::NOTE_DELETE | FilterFlag::NOTE_RENAME | FilterFlag::NOTE_REVOKE,
            0,
            0,
        );
        let mut receipt = [empty_event()];
        let count = queue.kevent(&[event], &mut receipt, None).map_err(nix_io)?;
        if count != 1 || !receipt[0].flags().contains(EvFlags::EV_ERROR) || receipt[0].data() != 0 {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        Ok(Self { queue })
    }

    pub(super) fn drain(&self) -> Result<(), Spec034ReleaseArtifactError> {
        use nix::sys::event::{EvFlags, FilterFlag};
        let mut events = [empty_event(); 8];
        let timeout = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let count = self.queue.kevent(&[], &mut events, Some(timeout)).map_err(nix_io)?;
        let fatal = FilterFlag::NOTE_DELETE | FilterFlag::NOTE_RENAME | FilterFlag::NOTE_REVOKE;
        if events[..count].iter().any(|event| {
            event.flags().intersects(EvFlags::EV_ERROR | EvFlags::EV_EOF)
                || event.fflags().intersects(fatal)
        }) {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        Ok(())
    }

    pub(super) fn confirm_unlinked(&self) -> Result<(), Spec034ReleaseArtifactError> {
        use nix::sys::event::{EvFlags, FilterFlag};
        let mut events = [empty_event(); 2];
        let timeout = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let count = self.queue.kevent(&[], &mut events, Some(timeout)).map_err(nix_io)?;
        let valid = count == 1
            && !events[0].flags().contains(EvFlags::EV_ERROR)
            && events[0].fflags().contains(FilterFlag::NOTE_DELETE)
            && !events[0]
                .fflags()
                .intersects(FilterFlag::NOTE_RENAME | FilterFlag::NOTE_REVOKE);
        if !valid {
            return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
        }
        Ok(())
    }
}

#[cfg(target_vendor = "apple")]
fn empty_event() -> nix::sys::event::KEvent {
    use nix::sys::event::{EvFlags, EventFilter, FilterFlag, KEvent};
    KEvent::new(
        0,
        EventFilter::EVFILT_VNODE,
        EvFlags::empty(),
        FilterFlag::empty(),
        0,
        0,
    )
}

#[cfg(target_vendor = "apple")]
fn nix_io(error: nix::errno::Errno) -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::Io(error.into())
}

#[cfg(not(target_vendor = "apple"))]
pub(super) struct RenameMonitor;

#[cfg(not(target_vendor = "apple"))]
impl RenameMonitor {
    pub(super) fn arm(_root: &File) -> Result<Self, Spec034ReleaseArtifactError> {
        Ok(Self)
    }

    pub(super) fn drain(&self) -> Result<(), Spec034ReleaseArtifactError> {
        Ok(())
    }


    pub(super) fn confirm_unlinked(&self) -> Result<(), Spec034ReleaseArtifactError> {
        Ok(())
    }
}
