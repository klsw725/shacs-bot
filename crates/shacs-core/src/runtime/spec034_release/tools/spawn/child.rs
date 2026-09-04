use super::Spec034ReleaseArtifactError;
use std::process::ExitStatus;

pub(crate) struct ExecutionChild {
    pub(super) pid: libc::pid_t,
    pub(super) identity: super::ProcessIdentity,
    pub(super) status: Option<ExitStatus>,
    pub(super) cleaned: bool,
}

impl ExecutionChild {
    #[cfg(not(test))]
    pub(crate) fn id(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or_default()
    }

    pub(crate) fn identity(&self) -> &super::ProcessIdentity {
        &self.identity
    }

    #[cfg(not(test))]
    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        let mut raw = 0;
        // SAFETY: [Category 8 - FFI boundary] `raw` is writable and `pid` was returned by
        // posix_spawn; WNOHANG cannot block or write beyond the status integer.
        let result = unsafe { libc::waitpid(self.pid, &mut raw, libc::WNOHANG) };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if result == 0 {
            return Ok(None);
        }
        let status = exit_status(raw);
        self.status = Some(status);
        Ok(Some(status))
    }

    pub(crate) fn wait(&mut self) -> std::io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let mut raw = 0;
        // SAFETY: [Category 8 - FFI boundary] `raw` is writable and this object exclusively
        // owns the wait/reap responsibility for the spawned PID.
        let result = unsafe { libc::waitpid(self.pid, &mut raw, 0) };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let status = exit_status(raw);
        self.status = Some(status);
        Ok(status)
    }

    pub(crate) fn terminate_and_reap(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        if self.cleaned {
            return Ok(());
        }
        self.terminate_and_reap_with(super::capture_process_identity, signal_process_group)
    }

    fn terminate_and_reap_with(
        &mut self,
        mut capture: impl FnMut(libc::pid_t) -> Result<super::ProcessIdentity, Spec034ReleaseArtifactError>,
        mut signal: impl FnMut(libc::pid_t) -> Result<(), Spec034ReleaseArtifactError>,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        let termination = match capture(self.pid) {
            Ok(current) if self.identity.same_process(&current) => signal(self.pid),
            Err(Spec034ReleaseArtifactError::Io(error))
                if error.raw_os_error() == Some(libc::ESRCH) =>
            {
                Ok(())
            }
            Ok(_) => Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch),
            Err(error) => Err(error),
        };
        let reaped = self
            .wait()
            .map(|_| ())
            .map_err(Spec034ReleaseArtifactError::Io);
        if reaped.is_ok() {
            self.cleaned = true;
        }
        Spec034ReleaseArtifactError::combine(termination, reaped)
    }
}

fn signal_process_group(pid: libc::pid_t) -> Result<(), Spec034ReleaseArtifactError> {
    let group = rustix::process::Pid::from_raw(pid)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    match rustix::process::kill_process_group(group, rustix::process::Signal::KILL) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::SRCH => Ok(()),
        Err(error) => Err(Spec034ReleaseArtifactError::Io(
            std::io::Error::from_raw_os_error(error.raw_os_error()),
        )),
    }
}

impl Drop for ExecutionChild {
    fn drop(&mut self) {
        let _ = self.terminate_and_reap();
    }
}

#[cfg(unix)]
fn exit_status(raw: libc::c_int) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(raw)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    #[cfg(target_vendor = "apple")]
    #[test]
    fn panic_drop_terminates_and_reaps_process_group() -> Result<(), Box<dyn std::error::Error>> {
        let mut command = Command::new("/bin/bash");
        command.args(["-c", "sleep 60 & wait"]);
        command.process_group(0);
        let child = command.spawn()?;
        let pid = i32::try_from(child.id())?;
        let identity = super::super::capture_process_identity(pid)?;
        std::mem::forget(child);

        let panic = std::panic::catch_unwind(|| {
            let _guard = ExecutionChild { pid, identity, status: None, cleaned: false };
            panic!("exercise guard drop");
        });

        assert!(panic.is_err());
        for _ in 0..200 {
            let target = nix::unistd::Pid::from_raw(pid);
            if matches!(nix::sys::signal::kill(target, None), Err(nix::errno::Errno::ESRCH)) {
                return Ok(());
            }
            std::thread::yield_now();
        }
        Err("guard left a live child".into())
    }

    #[test]
    fn primary_and_cleanup_failures_are_both_retained() {
        let combined = Spec034ReleaseArtifactError::combine::<()>(
            Err(Spec034ReleaseArtifactError::CommandFailed),
            Err(Spec034ReleaseArtifactError::DigestMismatch),
        );

        assert!(matches!(
            combined,
            Err(Spec034ReleaseArtifactError::CombinedFailure { primary, cleanup })
                if matches!(*primary, Spec034ReleaseArtifactError::CommandFailed)
                    && matches!(*cleanup, Spec034ReleaseArtifactError::DigestMismatch)
        ));
    }

    #[test]
    fn group_signal_failure_still_reaps_owned_child() -> Result<(), Box<dyn std::error::Error>> {
        let child = Command::new("/usr/bin/true").spawn()?;
        let pid = i32::try_from(child.id())?;
        std::mem::forget(child);
        let expected = identity(pid, 1);
        let mut child = ExecutionChild {
            pid,
            identity: expected.clone(),
            status: None,
            cleaned: false,
        };

        let result = child.terminate_and_reap_with(
            |_| Ok(expected.clone()),
            |_| Err(Spec034ReleaseArtifactError::Io(std::io::Error::other("signal"))),
        );

        assert!(result.is_err());
        assert!(child.status.is_some());
        Ok(())
    }

    #[test]
    fn identity_mismatch_never_signals_unrelated_pid() -> Result<(), Box<dyn std::error::Error>> {
        let child = Command::new("/usr/bin/true").spawn()?;
        let pid = i32::try_from(child.id())?;
        std::mem::forget(child);
        let expected = identity(pid, 1);
        let mut child = ExecutionChild {
            pid,
            identity: expected,
            status: None,
            cleaned: false,
        };
        let mut signals = 0;

        let result = child.terminate_and_reap_with(
            |_| Ok(identity(pid, 2)),
            |_| {
                signals += 1;
                Ok(())
            },
        );

        assert!(matches!(result, Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch)));
        assert_eq!(signals, 0);
        assert!(child.status.is_some());
        Ok(())
    }

    fn identity(pid: i32, start: u64) -> super::super::ProcessIdentity {
        super::super::ProcessIdentity {
            pid,
            parent_pid: 0,
            start_seconds: start,
            start_microseconds: 0,
            executable: PathBuf::from("/test"),
            device: 1,
            inode: 1,
            digest: "sha256:test".to_owned(),
            cdhash: vec![1],
        }
    }
}
