//! Spec 052 PRD 010 acceptance 6: the echo guard puts the signal actions,
//! the signal mask and the terminal's echo back. It changes process-wide
//! signal actions, so the check runs alone in a child test process.

use super::unix::{restore_and_reraise, EchoOff};
use std::error::Error;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Command;
use std::ptr;

type TestResult = Result<(), Box<dyn Error>>;

const CHILD: &str = "SHACS_SPEC052_ECHO_GUARD_CHILD";

#[test]
fn the_echo_guard_puts_signal_actions_mask_and_echo_back() -> TestResult {
    let status = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "hidden_input::tests::echo_guard_child",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .status()?;
    assert!(status.success(), "child exited {status}");
    Ok(())
}

#[test]
fn echo_guard_child() -> TestResult {
    if std::env::var(CHILD).as_deref() != Ok("1") {
        return Ok(());
    }
    // Given: a terminal, and SIGUSR1 blocked before the guard.
    let (_master, slave) = open_pty()?;
    block(libc::SIGUSR1)?;
    let actions_before = [handler(libc::SIGINT)?, handler(libc::SIGTERM)?];
    let mask_before = blocked_signals()?;

    // When
    let guard = EchoOff::on(slave.as_raw_fd(), "")?;
    let actions_during = [handler(libc::SIGINT)?, handler(libc::SIGTERM)?];
    let echo_during = echo(slave.as_raw_fd())?;
    drop(guard);

    // Then
    let ours: extern "C" fn(libc::c_int) = restore_and_reraise;
    let ours = ours as libc::sighandler_t;
    assert_eq!(actions_during, [ours, ours]);
    assert!(!echo_during);
    assert_eq!(
        [handler(libc::SIGINT)?, handler(libc::SIGTERM)?],
        actions_before
    );
    assert_eq!(blocked_signals()?, mask_before);
    assert_eq!(mask_before, [true, false, false]);
    assert!(echo(slave.as_raw_fd())?);
    Ok(())
}

fn open_pty() -> Result<(OwnedFd, OwnedFd), Box<dyn Error>> {
    let mut master = -1;
    let mut slave = -1;
    let settings: *mut libc::termios = ptr::null_mut();
    let size: *mut libc::winsize = ptr::null_mut();
    // SAFETY: `openpty` writes two descriptors; null name, settings and size
    // pointers are allowed.
    let result = unsafe { libc::openpty(&mut master, &mut slave, ptr::null_mut(), settings, size) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: `openpty` succeeded, so both are open descriptors owned here.
    Ok(unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) })
}

fn block(signal: libc::c_int) -> TestResult {
    let mut set = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: `set` is valid for writing one signal set.
    unsafe { libc::sigemptyset(set.as_mut_ptr()) };
    // SAFETY: `sigemptyset` initialized it.
    let mut set = unsafe { set.assume_init() };
    // SAFETY: `set` is initialized and `signal` is a valid signal number.
    unsafe { libc::sigaddset(&mut set, signal) };
    // SAFETY: `set` is valid and a null old-mask pointer is allowed.
    let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, ptr::null_mut()) };
    if result != 0 {
        return Err(std::io::Error::from_raw_os_error(result).into());
    }
    Ok(())
}

/// Whether SIGUSR1, SIGINT and SIGTERM are blocked for this thread.
fn blocked_signals() -> Result<[bool; 3], Box<dyn Error>> {
    let mut mask = MaybeUninit::<libc::sigset_t>::uninit();
    // SAFETY: a null new-mask pointer only reads the mask into `mask`.
    let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, ptr::null(), mask.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::from_raw_os_error(result).into());
    }
    // SAFETY: `pthread_sigmask` succeeded and filled it.
    let mask = unsafe { mask.assume_init() };
    // SAFETY: `mask` is an initialized signal set.
    let member = |signal| unsafe { libc::sigismember(&mask, signal) } == 1;
    Ok([
        member(libc::SIGUSR1),
        member(libc::SIGINT),
        member(libc::SIGTERM),
    ])
}

fn handler(signal: libc::c_int) -> Result<libc::sighandler_t, Box<dyn Error>> {
    let mut action = MaybeUninit::<libc::sigaction>::uninit();
    // SAFETY: a null new-action pointer only reads the action into `action`.
    if unsafe { libc::sigaction(signal, ptr::null(), action.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: `sigaction` succeeded and filled it.
    Ok(unsafe { action.assume_init() }.sa_sigaction)
}

fn echo(terminal: libc::c_int) -> Result<bool, Box<dyn Error>> {
    let mut settings = MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `settings` is valid for writing one `termios`.
    if unsafe { libc::tcgetattr(terminal, settings.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: `tcgetattr` succeeded and filled it.
    Ok(unsafe { settings.assume_init() }.c_lflag & libc::ECHO != 0)
}
