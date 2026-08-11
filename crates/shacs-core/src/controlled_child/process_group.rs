use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

pub(super) const fn cleanup_capability_supported() -> bool {
    cfg!(unix)
}

#[cfg(unix)]
pub(super) fn configure(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
pub(super) fn configure(_command: &mut Command) {}

#[cfg(unix)]
pub(super) fn cleanup(child: &mut Child, grace: Duration) -> bool {
    let Ok(raw_pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    let group = nix::unistd::Pid::from_raw(raw_pid);
    let attempted = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGTERM).is_ok();
    let deadline = Instant::now() + grace;
    while group_exists(group) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if group_exists(group) {
        let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    attempted
}

#[cfg(unix)]
fn group_exists(group: nix::unistd::Pid) -> bool {
    let group_target = nix::unistd::Pid::from_raw(-group.as_raw());
    nix::sys::signal::kill(group_target, None).is_ok()
}

#[cfg(not(unix))]
pub(super) fn cleanup(child: &mut Child, _grace: Duration) -> bool {
    let _ = child.kill();
    let _ = child.wait();
    false
}
