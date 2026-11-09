//! Reads a secret from standard input without showing it on the terminal
//! (Spec 052 PRD 010 scope 4). However the read ends, the terminal's echo
//! comes back, also when SIGINT or SIGTERM ends the process.

use std::io::{self, IsTerminal, Read};

/// Standard input to its end when it is not a terminal; otherwise one line
/// typed with the terminal's echo off, after `prompt` on standard error.
pub(crate) fn read_secret(prompt: &str) -> io::Result<String> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut text = String::new();
        stdin.lock().read_to_string(&mut text)?;
        return Ok(text);
    }
    read_hidden_line(prompt)
}

#[cfg(unix)]
fn read_hidden_line(prompt: &str) -> io::Result<String> {
    use std::io::{BufRead, Write};

    let echo_off = EchoOff::on(libc::STDIN_FILENO, prompt)?;
    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line);
    drop(echo_off);
    writeln!(io::stderr())?;
    read.map(|_| line)
}

#[cfg(not(unix))]
fn read_hidden_line(prompt: &str) -> io::Result<String> {
    use std::io::{BufRead, Write};

    write!(io::stderr(), "{prompt}")?;
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

#[cfg(unix)]
use unix::EchoOff;

#[cfg(unix)]
mod unix {
    use std::io::{self, Write};
    use std::mem::MaybeUninit;
    use std::ptr;
    use std::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

    const SIGNALS: [libc::c_int; 2] = [libc::SIGINT, libc::SIGTERM];

    /// The settings the signal handler puts back while echo is off, and the
    /// terminal they belong to. `EchoOff` owns the settings and clears this
    /// pointer, with the signals blocked, before it lets them go.
    static SAVED_SETTINGS: AtomicPtr<libc::termios> = AtomicPtr::new(ptr::null_mut());
    static SAVED_TERMINAL: AtomicI32 = AtomicI32::new(-1);

    /// Echo turned off on a terminal until this is dropped.
    pub(crate) struct EchoOff {
        terminal: libc::c_int,
        saved: Box<libc::termios>,
        previous_actions: [libc::sigaction; 2],
    }

    impl EchoOff {
        /// Turns echo off on `terminal`, with SIGINT and SIGTERM putting it
        /// back, then writes `prompt` to standard error.
        pub(crate) fn on(terminal: libc::c_int, prompt: &str) -> io::Result<Self> {
            let blocked = SignalsBlocked::block()?;
            let mut saved = Box::new(settings(terminal)?);
            SAVED_TERMINAL.store(terminal, Ordering::SeqCst);
            SAVED_SETTINGS.store(&mut *saved, Ordering::SeqCst);
            let previous_actions = match install_handlers() {
                Ok(previous_actions) => previous_actions,
                Err(error) => {
                    SAVED_SETTINGS.store(ptr::null_mut(), Ordering::SeqCst);
                    return Err(error);
                }
            };
            let echo_off = Self {
                terminal,
                saved,
                previous_actions,
            };
            let mut hidden = *echo_off.saved;
            hidden.c_lflag &= !libc::ECHO;
            apply(terminal, &hidden)?;
            let mut stderr = io::stderr();
            write!(stderr, "{prompt}")?;
            stderr.flush()?;
            drop(blocked);
            Ok(echo_off)
        }
    }

    impl Drop for EchoOff {
        fn drop(&mut self) {
            let blocked = SignalsBlocked::block();
            // SAFETY: `self.saved` holds the settings `tcgetattr` returned for
            // this descriptor, and the pointer is valid for the call.
            unsafe { libc::tcsetattr(self.terminal, libc::TCSANOW, &*self.saved) };
            for (signal, previous) in SIGNALS.iter().zip(&self.previous_actions) {
                // SAFETY: `previous` is the action `sigaction` returned for
                // this signal; a null old-action pointer is allowed.
                unsafe { libc::sigaction(*signal, previous, ptr::null_mut()) };
            }
            SAVED_SETTINGS.store(ptr::null_mut(), Ordering::SeqCst);
            drop(blocked);
        }
    }

    /// SIGINT and SIGTERM blocked for this thread until dropped, which puts
    /// the thread's previous signal mask back exactly.
    struct SignalsBlocked {
        previous: libc::sigset_t,
    }

    impl SignalsBlocked {
        fn block() -> io::Result<Self> {
            let mut set = MaybeUninit::<libc::sigset_t>::uninit();
            // SAFETY: `set` is valid for writing one signal set.
            unsafe { libc::sigemptyset(set.as_mut_ptr()) };
            // SAFETY: `sigemptyset` initialized it.
            let mut set = unsafe { set.assume_init() };
            for signal in SIGNALS {
                // SAFETY: `set` is an initialized signal set and the signal
                // numbers are valid.
                unsafe { libc::sigaddset(&mut set, signal) };
            }
            let mut previous = MaybeUninit::<libc::sigset_t>::uninit();
            // SAFETY: both pointers are valid for the call, which fills
            // `previous` on success.
            let result =
                unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, previous.as_mut_ptr()) };
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result));
            }
            Ok(Self {
                // SAFETY: `pthread_sigmask` succeeded and filled it.
                previous: unsafe { previous.assume_init() },
            })
        }
    }

    impl Drop for SignalsBlocked {
        fn drop(&mut self) {
            // SAFETY: `self.previous` is the mask `pthread_sigmask` returned;
            // a null old-mask pointer is allowed.
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, ptr::null_mut()) };
        }
    }

    pub(crate) extern "C" fn restore_and_reraise(signal: libc::c_int) {
        let saved = SAVED_SETTINGS.load(Ordering::SeqCst);
        if !saved.is_null() {
            // SAFETY: `tcsetattr` is async-signal-safe, and `saved` points into
            // the `EchoOff` that clears this pointer before releasing it.
            unsafe { libc::tcsetattr(SAVED_TERMINAL.load(Ordering::SeqCst), libc::TCSANOW, saved) };
        }
        // SAFETY: `signal` and `raise` are async-signal-safe; with the default
        // action back, the raised signal ends the process once this returns.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }

    fn settings(terminal: libc::c_int) -> io::Result<libc::termios> {
        let mut settings = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `settings` is valid for writing one `termios`.
        if unsafe { libc::tcgetattr(terminal, settings.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `tcgetattr` succeeded and filled it.
        Ok(unsafe { settings.assume_init() })
    }

    fn apply(terminal: libc::c_int, settings: &libc::termios) -> io::Result<()> {
        // SAFETY: `settings` is a valid `termios` for the call.
        if unsafe { libc::tcsetattr(terminal, libc::TCSANOW, settings) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Installs the handler for both signals and returns their previous
    /// actions; on failure the ones already installed are put back.
    fn install_handlers() -> io::Result<[libc::sigaction; 2]> {
        // SAFETY: an all-zero `sigaction` is a valid empty action.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        let handler: extern "C" fn(libc::c_int) = restore_and_reraise;
        action.sa_sigaction = handler as libc::sighandler_t;
        // SAFETY: `action.sa_mask` is valid for writing one signal set.
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        // SAFETY: an all-zero `sigaction` is a valid value to be overwritten.
        let mut previous: [libc::sigaction; 2] = unsafe { std::mem::zeroed() };
        for (index, signal) in SIGNALS.iter().enumerate() {
            // SAFETY: both pointers are valid for the call.
            if unsafe { libc::sigaction(*signal, &action, &mut previous[index]) } != 0 {
                let error = io::Error::last_os_error();
                for (installed, old) in SIGNALS.iter().zip(&previous).take(index) {
                    // SAFETY: `old` is the action `sigaction` returned for it.
                    unsafe { libc::sigaction(*installed, old, ptr::null_mut()) };
                }
                return Err(error);
            }
        }
        Ok(previous)
    }
}

#[cfg(all(test, unix))]
#[path = "hidden_input_tests.rs"]
mod tests;
