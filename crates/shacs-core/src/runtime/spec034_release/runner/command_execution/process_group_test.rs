use super::*;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn group_disappearance_is_polled_to_esrch() -> Result<(), Spec034ReleaseArtifactError> {
    let mut observations = [true, true, false].into_iter();
    wait_until_group_disappears(|| Ok(observations.next().unwrap_or(false)), || {})
}

#[test]
fn group_probe_error_and_timeout_fail_cleanup() {
    assert!(matches!(
        wait_until_group_disappears(
            || Err(Spec034ReleaseArtifactError::Io(std::io::Error::other("probe"))),
            || {}
        ),
        Err(Spec034ReleaseArtifactError::Io(_))
    ));
    assert!(matches!(
        wait_until_group_disappears(|| Ok(true), || {}),
        Err(Spec034ReleaseArtifactError::CommandFailed)
    ));
}

#[test]
fn permission_denied_probe_keeps_polling_for_esrch() {
    assert!(matches!(
        validate_group_probe(Err(nix::errno::Errno::EPERM)),
        Ok(true)
    ));
}

#[test]
fn later_cleanup_is_aggregated_after_group_probe_failure() {
    let mut failure = None;
    super::super::descendants::accumulate(
        &mut failure,
        Err(Spec034ReleaseArtifactError::Io(std::io::Error::other("group probe"))),
    );
    super::super::descendants::accumulate(
        &mut failure,
        Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch),
    );
    assert!(matches!(
        failure,
        Some(Spec034ReleaseArtifactError::CombinedFailure { .. })
    ));
}

#[cfg(target_vendor = "apple")]
#[test]
fn process_group_termination_reaps_descendants() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let pid_file = root.path().join("descendant.pid");
    let mut command = std::process::Command::new("/bin/bash");
    command.args(["-c", &format!("sleep 60 & echo $! > '{}' && wait", pid_file.display())]);
    configure_process_group(&mut command)?;
    let child = command.spawn()?;
    let identity = super::super::super::super::tools::spawn::capture_process_identity(
        i32::try_from(child.id())?,
    )?;
    let mut child = ObservedChild { child, identity };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !pid_file.exists() {
        if Instant::now() >= deadline {
            child.child.kill()?;
            return Err("descendant pid was not recorded".into());
        }
        std::thread::yield_now();
    }
    let mut descendants = super::super::descendants::DescendantTracker::new(child.child.id())?;
    descendants.observe()?;
    terminate_process_group(&mut child, &mut descendants)?;
    Ok(())
}

#[cfg(target_vendor = "apple")]
struct ObservedChild {
    child: std::process::Child,
    identity: super::super::super::super::tools::spawn::ProcessIdentity,
}

#[cfg(target_vendor = "apple")]
impl ReapChild for ObservedChild {
    fn pid(&self) -> libc::pid_t {
        self.identity.pid
    }

    fn identity(&self) -> &super::super::super::super::tools::spawn::ProcessIdentity {
        &self.identity
    }

    fn wait_reaped(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}

#[test]
fn root_identity_mismatch_still_runs_descendant_cleanup_and_monitor_close() {
    let mut child = FakeChild::new(identity(41, 1));
    let mut monitor = FakeMonitor::default();
    let mut control = FakeControl::identity(identity(41, 2));

    let result = terminate_process_group_with(&mut child, &mut monitor, &mut control);

    assert!(result.is_err());
    assert_eq!(child.waits, 1);
    assert_eq!(monitor.cleanups, 1);
    assert_eq!(monitor.closes, 1);
}

#[test]
fn root_identity_capture_error_still_attempts_wait() {
    let mut child = FakeChild::new(identity(42, 1));
    let mut monitor = FakeMonitor::default();
    let mut control = FakeControl::capture_error();

    let result = terminate_process_group_with(&mut child, &mut monitor, &mut control);

    assert!(result.is_err());
    assert_eq!(child.waits, 1);
    assert_eq!(monitor.cleanups, 1);
    assert_eq!(monitor.closes, 1);
}

struct FakeChild {
    identity: super::super::super::super::tools::spawn::ProcessIdentity,
    waits: usize,
}

impl FakeChild {
    fn new(identity: super::super::super::super::tools::spawn::ProcessIdentity) -> Self {
        Self { identity, waits: 0 }
    }
}

impl ReapChild for FakeChild {
    fn pid(&self) -> libc::pid_t {
        self.identity.pid
    }

    fn identity(&self) -> &super::super::super::super::tools::spawn::ProcessIdentity {
        &self.identity
    }

    fn wait_reaped(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.waits += 1;
        Ok(std::process::ExitStatus::from_raw(0))
    }
}

#[derive(Default)]
struct FakeMonitor {
    cleanups: usize,
    closes: usize,
}

impl DescendantMonitor for FakeMonitor {
    fn terminate_and_verify(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        self.cleanups += 1;
        Ok(())
    }

    fn close(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
        self.closes += 1;
        Ok(())
    }
}

struct FakeControl {
    captured: Option<Result<super::super::super::super::tools::spawn::ProcessIdentity, Spec034ReleaseArtifactError>>,
}

impl FakeControl {
    fn identity(identity: super::super::super::super::tools::spawn::ProcessIdentity) -> Self {
        Self { captured: Some(Ok(identity)) }
    }

    fn capture_error() -> Self {
        Self {
            captured: Some(Err(Spec034ReleaseArtifactError::Io(std::io::Error::other("capture")))),
        }
    }
}

impl ProcessGroupControl for FakeControl {
    fn capture_identity(
        &mut self,
        _pid: i32,
    ) -> Result<super::super::super::super::tools::spawn::ProcessIdentity, Spec034ReleaseArtifactError> {
        self.captured.take().unwrap_or(Err(Spec034ReleaseArtifactError::InvalidConfig))
    }

    fn kill_group(&mut self, _pid: i32) -> Result<(), Spec034ReleaseArtifactError> {
        Ok(())
    }

    fn group_exists(&mut self, _pid: i32) -> Result<bool, Spec034ReleaseArtifactError> {
        Ok(false)
    }
}

fn identity(
    pid: i32,
    start: u64,
) -> super::super::super::super::tools::spawn::ProcessIdentity {
    super::super::super::super::tools::spawn::ProcessIdentity {
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
