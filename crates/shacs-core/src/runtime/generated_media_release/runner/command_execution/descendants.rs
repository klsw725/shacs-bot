use super::Spec034ReleaseArtifactError;
#[cfg(not(test))]
use crate::runtime::generated_media_release::tools::spawn::capture_observed_process_identity;
use crate::runtime::generated_media_release::tools::spawn::{capture_process_identity, ProcessIdentity};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) struct DescendantTracker {
    root: i32,
    tracked: BTreeMap<i32, ProcessIdentity>,
    closed: bool,
}

impl DescendantTracker {
    pub(super) fn new(root: u32) -> Result<Self, Spec034ReleaseArtifactError> {
        Ok(Self {
            root: i32::try_from(root).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
            tracked: BTreeMap::new(),
            closed: false,
        })
    }

    #[cfg(test)]
    pub(super) fn observe(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        self.observe_with(direct_children, |_| Ok(()), capture_process_identity)
    }

    #[cfg(not(test))]
    pub(super) fn observe_verified(
        &mut self,
        toolchain: &super::super::super::tools::ResolvedToolchain,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.observe_with(
            direct_children,
            |identity| toolchain.verify_descendant_identity(identity),
            capture_observed_process_identity,
        )
    }

    fn observe_with(
        &mut self,
        mut children_of: impl FnMut(i32) -> Result<Vec<i32>, Spec034ReleaseArtifactError>,
        mut verify: impl FnMut(&ProcessIdentity) -> Result<(), Spec034ReleaseArtifactError>,
        mut identity: impl FnMut(i32) -> Result<ProcessIdentity, Spec034ReleaseArtifactError>,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        if self.closed {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let mut pending = VecDeque::from([self.root]);
        let mut visited = BTreeSet::new();
        while let Some(parent) = pending.pop_front() {
            if !visited.insert(parent) {
                continue;
            }
            for child in children_of(parent)? {
                if child > 1 && child != std::process::id() as i32 {
                    let identity = match identity(child) {
                        Ok(identity) => identity,
                        Err(Spec034ReleaseArtifactError::Io(error))
                            if error.raw_os_error() == Some(libc::ESRCH) =>
                        {
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    verify(&identity)?;
                    self.tracked.insert(child, identity);
                    pending.push_back(child);
                }
            }
        }
        Ok(())
    }

    pub(super) fn terminate_and_verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.terminate_with(capture_process_identity, signal, exists)
    }

    pub(super) fn close(&mut self) {
        self.closed = true;
    }

    fn terminate_with(
        &self,
        mut capture: impl FnMut(i32) -> Result<ProcessIdentity, Spec034ReleaseArtifactError>,
        mut terminate: impl FnMut(i32) -> Result<(), Spec034ReleaseArtifactError>,
        mut present: impl FnMut(i32) -> Result<bool, Spec034ReleaseArtifactError>,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        let mut failure = None;
        for (&pid, expected) in self.tracked.iter().rev() {
            match capture(pid) {
                Ok(current) if expected.same_launch(&current) => {
                    accumulate(&mut failure, terminate(pid));
                }
                Err(Spec034ReleaseArtifactError::Io(error))
                    if error.raw_os_error() == Some(libc::ESRCH) => {}
                Ok(_) | Err(_) => accumulate(
                    &mut failure,
                    Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch),
                ),
            }
        }
        for (&pid, expected) in &self.tracked {
            let absent = match capture(pid) {
                Err(Spec034ReleaseArtifactError::Io(error))
                    if error.raw_os_error() == Some(libc::ESRCH) => true,
                Ok(current) if expected.same_launch(&current) => match present(pid) {
                    Ok(present) => !present,
                    Err(error) => {
                        accumulate(&mut failure, Err(error));
                        false
                    }
                },
                Ok(_) | Err(_) => {
                    accumulate(
                        &mut failure,
                        Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch),
                    );
                    false
                }
            };
            if !absent {
                accumulate(&mut failure, Err(Spec034ReleaseArtifactError::CommandFailed));
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

pub(super) fn accumulate(
    failure: &mut Option<Spec034ReleaseArtifactError>,
    result: Result<(), Spec034ReleaseArtifactError>,
) {
    if let Err(next) = result {
        *failure = Some(match failure.take() {
            None => next,
            Some(previous) => Spec034ReleaseArtifactError::CombinedFailure {
                primary: Box::new(previous),
                cleanup: Box::new(next),
            },
        });
    }
}

#[cfg(target_vendor = "apple")]
fn direct_children(parent: i32) -> Result<Vec<i32>, Spec034ReleaseArtifactError> {
    let mut children = vec![0_i32; 4096];
    let size = i32::try_from(children.len() * std::mem::size_of::<i32>())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    // SAFETY: [Category 8 - FFI boundary] `children` is writable for `size` bytes and
    // `proc_listchildpids` cannot retain the pointer after returning.
    let count = unsafe { libc::proc_listchildpids(parent, children.as_mut_ptr().cast(), size) };
    if count < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(Vec::new());
        }
        return Err(Spec034ReleaseArtifactError::Io(error));
    }
    let count = usize::try_from(count).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    children.truncate(count.min(children.len()));
    children.retain(|pid| *pid > 0);
    Ok(children)
}

#[cfg(not(target_vendor = "apple"))]
fn direct_children(_parent: i32) -> Result<Vec<i32>, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

fn signal(raw: i32) -> Result<(), Spec034ReleaseArtifactError> {
    let pid = rustix::process::Pid::from_raw(raw)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    match rustix::process::kill_process(pid, rustix::process::Signal::KILL) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::SRCH => Ok(()),
        Err(error) => Err(Spec034ReleaseArtifactError::Io(
            std::io::Error::from_raw_os_error(error.raw_os_error()),
        )),
    }
}

fn exists(raw: i32) -> Result<bool, Spec034ReleaseArtifactError> {
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(raw), None) {
        Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(true),
        Err(nix::errno::Errno::ESRCH) => Ok(false),
        Err(error) => Err(Spec034ReleaseArtifactError::Io(std::io::Error::from_raw_os_error(error as i32))),
    }
}

#[cfg(test)]
#[path = "descendants_test.rs"]
mod tests;
