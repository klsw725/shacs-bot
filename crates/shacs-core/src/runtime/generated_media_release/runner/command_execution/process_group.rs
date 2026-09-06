use super::Spec034ReleaseArtifactError;
const DISAPPEARANCE_POLLS: usize = 200;

pub(super) trait ReapChild {
    fn pid(&self) -> libc::pid_t;
    fn identity(&self) -> &super::super::super::tools::spawn::ProcessIdentity;
    fn wait_reaped(&mut self) -> std::io::Result<std::process::ExitStatus>;
}

pub(super) trait DescendantMonitor {
    fn terminate_and_verify(&mut self) -> Result<(), Spec034ReleaseArtifactError>;
    fn close(&mut self) -> Result<(), Spec034ReleaseArtifactError>;
}

impl DescendantMonitor for super::descendants::DescendantTracker {
    fn terminate_and_verify(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        super::descendants::DescendantTracker::terminate_and_verify(self)
    }

    fn close(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        super::descendants::DescendantTracker::close(self);
        Ok(())
    }
}

pub(super) trait ProcessGroupControl {
    fn capture_identity(
        &mut self,
        pid: i32,
    ) -> Result<super::super::super::tools::spawn::ProcessIdentity, Spec034ReleaseArtifactError>;
    fn kill_group(&mut self, pid: i32) -> Result<(), Spec034ReleaseArtifactError>;
    fn group_exists(&mut self, pid: i32) -> Result<bool, Spec034ReleaseArtifactError>;
}

impl ReapChild for super::super::super::tools::spawn::ExecutionChild {
    fn pid(&self) -> libc::pid_t {
        self.identity().pid
    }

    fn identity(&self) -> &super::super::super::tools::spawn::ProcessIdentity {
        self.identity()
    }

    fn wait_reaped(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.wait()
    }
}

#[cfg(all(test, unix))]
pub(super) fn configure_process_group(
    command: &mut std::process::Command,
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    Ok(())
}

#[cfg(all(test, not(unix)))]
pub(super) fn configure_process_group(
    _command: &mut std::process::Command,
) -> Result<(), Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

#[cfg(unix)]
pub(super) fn terminate_process_group<C: ReapChild, M: DescendantMonitor>(
    child: &mut C,
    descendants: &mut M,
) -> Result<(), Spec034ReleaseArtifactError> {
    terminate_process_group_with(child, descendants, &mut SystemProcessGroupControl)
}

#[cfg(unix)]
pub(super) fn terminate_process_group_with<
    C: ReapChild,
    M: DescendantMonitor,
    P: ProcessGroupControl,
>(
    child: &mut C,
    descendants: &mut M,
    control: &mut P,
) -> Result<(), Spec034ReleaseArtifactError> {
    let raw = child.pid();
    let mut failure = None;
    let root_alive = match control.capture_identity(raw) {
        Ok(current) if child.identity().same_process(&current) => true,
        Err(Spec034ReleaseArtifactError::Io(error))
            if error.raw_os_error() == Some(libc::ESRCH) =>
        {
            false
        }
        Ok(_) => {
            super::descendants::accumulate(
                &mut failure,
                Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch),
            );
            false
        }
        Err(error) => {
            super::descendants::accumulate(&mut failure, Err(error));
            false
        }
    };
    if root_alive {
        super::descendants::accumulate(&mut failure, control.kill_group(raw));
    }
    super::descendants::accumulate(
        &mut failure,
        child
            .wait_reaped()
            .map(|_| ())
            .map_err(Spec034ReleaseArtifactError::Io),
    );
    if root_alive {
        super::descendants::accumulate(
            &mut failure,
            wait_until_group_disappears(
                || control.group_exists(raw),
                || std::thread::sleep(std::time::Duration::from_millis(1)),
            ),
        );
    }
    super::descendants::accumulate(&mut failure, descendants.terminate_and_verify());
    super::descendants::accumulate(&mut failure, descendants.close());
    failure.map_or(Ok(()), Err)
}

#[cfg(unix)]
struct SystemProcessGroupControl;

#[cfg(unix)]
impl ProcessGroupControl for SystemProcessGroupControl {
    fn capture_identity(
        &mut self,
        pid: i32,
    ) -> Result<super::super::super::tools::spawn::ProcessIdentity, Spec034ReleaseArtifactError> {
        super::super::super::tools::spawn::capture_process_identity(pid)
    }

    fn kill_group(&mut self, pid: i32) -> Result<(), Spec034ReleaseArtifactError> {
        let group = rustix::process::Pid::from_raw(pid)
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        validate_group_kill(rustix::process::kill_process_group(
            group,
            rustix::process::Signal::KILL,
        ))
    }

    fn group_exists(&mut self, pid: i32) -> Result<bool, Spec034ReleaseArtifactError> {
        group_exists(pid)
    }
}

#[cfg(unix)]
fn validate_group_kill(result: rustix::io::Result<()>) -> Result<(), Spec034ReleaseArtifactError> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::SRCH => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn group_exists(raw: i32) -> Result<bool, Spec034ReleaseArtifactError> {
    let target = nix::unistd::Pid::from_raw(-raw);
    validate_group_probe(nix::sys::signal::kill(target, None))
}

#[cfg(unix)]
fn validate_group_probe(result: nix::Result<()>) -> Result<bool, Spec034ReleaseArtifactError> {
    match result {
        Ok(()) => Ok(true),
        Err(nix::errno::Errno::ESRCH) => Ok(false),
        Err(nix::errno::Errno::EPERM) => Ok(true),
        Err(error) => Err(Spec034ReleaseArtifactError::Io(std::io::Error::from_raw_os_error(
            error as i32,
        ))),
    }
}

fn wait_until_group_disappears(
    mut probe: impl FnMut() -> Result<bool, Spec034ReleaseArtifactError>,
    mut yield_control: impl FnMut(),
) -> Result<(), Spec034ReleaseArtifactError> {
    for _ in 0..DISAPPEARANCE_POLLS {
        if !probe()? {
            return Ok(());
        }
        yield_control();
    }
    Err(Spec034ReleaseArtifactError::CommandFailed)
}

#[cfg(unix)]
fn io_error(error: rustix::io::Errno) -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::Io(std::io::Error::from_raw_os_error(error.raw_os_error()))
}

#[cfg(not(unix))]
pub(super) fn terminate_process_group<C: ReapChild, M: DescendantMonitor>(
    child: &mut C,
    descendants: &mut M,
) -> Result<(), Spec034ReleaseArtifactError> {
    let mut failure = Some(Spec034ReleaseArtifactError::InvalidConfig);
    super::descendants::accumulate(
        &mut failure,
        child.wait_reaped().map(|_| ()).map_err(Spec034ReleaseArtifactError::Io),
    );
    super::descendants::accumulate(&mut failure, descendants.terminate_and_verify());
    super::descendants::accumulate(&mut failure, descendants.close());
    failure.map_or(Ok(()), Err)
}

#[cfg(all(test, unix))]
#[path = "process_group_test.rs"]
mod tests;
