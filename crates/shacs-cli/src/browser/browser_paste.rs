//! The paste path of a takeover episode (Spec 050 PRD 010 scope 6).
//!
//! While an episode is open the manager listens on a Unix socket inside the
//! generation folder. `websockify` reaches it through the `paste` token and
//! nothing else does. One connection is one challenge–response with the view
//! password and at most one text. The text is used for the one insert and is
//! never stored or logged, and neither is its length.

use super::BrowserClock;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// One line, up to and including its `\n`.
const LINE_LIMIT: usize = 256;
/// The longest text, in bytes.
pub const PASTE_TEXT_LIMIT: usize = 20_000;
/// How long each of the `auth` line, the `paste` line and the text may take.
pub const PASTE_STAGE_LIMIT_MS: u64 = 10_000;
/// How long a wrong answer waits for its `error auth`.
pub const PASTE_AUTH_DELAY_MS: u64 = 1_000;
/// Wrong answers that close the episode's listener.
pub const PASTE_AUTH_FAILURES: usize = 10;
/// Connections served at once; one more is dropped as it is accepted.
pub const PASTE_CONNECTIONS: usize = 2;
/// How often a waiting thread looks at the clock and at the stop flag.
const POLL: Duration = Duration::from_millis(5);
const WRITE_LIMIT: Duration = Duration::from_secs(2);

/// What became of one text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteOutcome {
    Done,
    NotUser,
    BrowserClosed,
}

impl PasteOutcome {
    const fn line(self) -> &'static str {
        match self {
            Self::Done => "done\n",
            Self::NotUser => "error not_user\n",
            Self::BrowserClosed => "error browser_closed\n",
        }
    }
}

/// Where an authenticated text goes. `stopping` turns true when the listener
/// is being taken down; an implementation that waits must give up then,
/// because whoever takes the listener down waits for its threads.
pub trait PasteTarget: Send + Sync {
    fn paste_insert(&self, episode: &str, text: &str, stopping: &AtomicBool) -> PasteOutcome;
    /// The episode used up its wrong answers: the listener goes away, then
    /// the `paste` route.
    fn paste_lockout(&self, episode: &str, stopping: &AtomicBool);
}

struct Shared {
    /// No connection is served any more.
    closed: AtomicBool,
    /// The owner is taking the listener down and waits for the threads.
    stopping: AtomicBool,
    /// The listener goes because the browser closed, not because the
    /// episode ended.
    browser_closed: AtomicBool,
    connections: AtomicUsize,
    failures: AtomicUsize,
    threads: Mutex<Vec<JoinHandle<()>>>,
    episode: String,
    label: String,
    secret: String,
    path: PathBuf,
    clock: Arc<dyn BrowserClock>,
    target: Weak<dyn PasteTarget>,
}

impl Shared {
    fn over(&self) -> bool {
        self.closed.load(Ordering::Acquire) || self.stopping.load(Ordering::Acquire)
    }

    /// What a text that came too late is answered.
    fn late(&self) -> PasteOutcome {
        if self.browser_closed.load(Ordering::Acquire) {
            PasteOutcome::BrowserClosed
        } else {
            PasteOutcome::NotUser
        }
    }
}

/// The listener of one episode. Dropping it closes the socket, waits for
/// every connection and removes the socket file.
pub struct PasteListener {
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl PasteListener {
    pub fn open(
        path: PathBuf,
        episode: &str,
        label: &str,
        secret: &str,
        clock: Arc<dyn BrowserClock>,
        target: Weak<dyn PasteTarget>,
    ) -> Result<Self, String> {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("paste_socket_unavailable".to_owned()),
        }
        let listener =
            UnixListener::bind(&path).map_err(|_| "paste_socket_unavailable".to_owned())?;
        let ready = fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .and_then(|()| listener.set_nonblocking(true));
        if ready.is_err() {
            drop(listener);
            let _ = fs::remove_file(&path);
            return Err("paste_socket_unavailable".to_owned());
        }
        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            browser_closed: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            failures: AtomicUsize::new(0),
            threads: Mutex::new(Vec::new()),
            episode: episode.to_owned(),
            label: label.to_owned(),
            secret: secret.to_owned(),
            path,
            clock,
            target,
        });
        let accepting = Arc::clone(&shared);
        let accept = thread::Builder::new()
            .name("browser-paste".to_owned())
            .spawn(move || accept(&listener, &accepting));
        match accept {
            Ok(accept) => Ok(Self {
                shared,
                accept: Some(accept),
            }),
            Err(_) => {
                let _ = fs::remove_file(&shared.path);
                Err("paste_socket_unavailable".to_owned())
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.shared.path
    }

    /// Takes the listener down because the browser closed. A connection
    /// that authenticated and still waits is answered `browser_closed`.
    pub fn browser_closed(self) {
        self.shared.browser_closed.store(true, Ordering::Release);
    }
}

impl Drop for PasteListener {
    fn drop(&mut self) {
        // `stopping` first: a connection that sees the listener over because
        // of this must also see why.
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.closed.store(true, Ordering::Release);
        let me = thread::current().id();
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        // A poisoned list is skipped: its threads see `stopping` and end on
        // their own.
        let mut threads = Vec::new();
        if let Ok(mut running) = self.shared.threads.lock() {
            threads = std::mem::take(&mut *running);
        }
        for handle in threads {
            // The connection that used up the wrong answers takes the
            // listener down from its own thread.
            if handle.thread().id() != me {
                let _ = handle.join();
            }
        }
        let _ = fs::remove_file(&self.shared.path);
    }
}

struct Connected<'a>(&'a AtomicUsize);

impl Drop for Connected<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn accept(listener: &UnixListener, shared: &Arc<Shared>) {
    while !shared.over() {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) =>
            {
                thread::sleep(POLL);
                continue;
            }
            Err(_) => break,
        };
        if shared.connections.load(Ordering::Acquire) >= PASTE_CONNECTIONS {
            drop(stream);
            continue;
        }
        shared.connections.fetch_add(1, Ordering::AcqRel);
        let serving = Arc::clone(shared);
        let spawned = thread::Builder::new()
            .name("browser-paste-connection".to_owned())
            .spawn(move || {
                let _connected = Connected(&serving.connections);
                serve(stream, &serving);
            });
        match spawned {
            Ok(handle) => {
                let Ok(mut threads) = shared.threads.lock() else {
                    // Nobody can wait for a connection any more, so none is
                    // served: the one just started sees this too.
                    shared.closed.store(true, Ordering::Release);
                    break;
                };
                threads.retain(|thread| !thread.is_finished());
                threads.push(handle);
            }
            Err(_) => {
                shared.connections.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
    // The socket file goes with the listener, so nothing new finds it.
    let _ = fs::remove_file(&shared.path);
}

fn serve(mut stream: UnixStream, shared: &Arc<Shared>) {
    let ready = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(POLL)))
        .and_then(|()| stream.set_write_timeout(Some(WRITE_LIMIT)));
    if ready.is_err() {
        return;
    }
    let Some(nonce) = nonce() else {
        return;
    };
    // Each limit starts before the line that opens its stage is written,
    // so a peer that has that line is already on the clock.
    let until = shared.clock.now_ms().saturating_add(PASTE_STAGE_LIMIT_MS);
    if stream
        .write_all(format!("shacs-paste 1 {} {nonce}\n", shared.label).as_bytes())
        .is_err()
    {
        return;
    }
    let Some(line) = read_line(&mut stream, shared, until) else {
        return;
    };
    if !authentic(&line, &nonce, &shared.secret) {
        refuse(&mut stream, shared);
        return;
    }
    let until = shared.clock.now_ms().saturating_add(PASTE_STAGE_LIMIT_MS);
    if stream.write_all(b"ok\n").is_err() {
        return;
    }
    let Some(line) = read_line(&mut stream, shared, until) else {
        overtaken(&mut stream, shared);
        return;
    };
    let Some(length) = announced(&line) else {
        return;
    };
    if length > PASTE_TEXT_LIMIT {
        let _ = stream.write_all(b"error too_long\n");
        return;
    }
    if length == 0 {
        let _ = stream.write_all(b"error invalid_text\n");
        return;
    }
    let until = shared.clock.now_ms().saturating_add(PASTE_STAGE_LIMIT_MS);
    let Some(bytes) = read_exact(&mut stream, shared, length, until) else {
        overtaken(&mut stream, shared);
        return;
    };
    let Some(text) = pasted(bytes) else {
        let _ = stream.write_all(b"error invalid_text\n");
        return;
    };
    let outcome = if shared.over() {
        shared.late()
    } else {
        match shared.target.upgrade() {
            Some(target) => match target.paste_insert(&shared.episode, &text, &shared.stopping) {
                PasteOutcome::NotUser => shared.late(),
                outcome => outcome,
            },
            None => PasteOutcome::BrowserClosed,
        }
    };
    drop(text);
    let _ = stream.write_all(outcome.line().as_bytes());
}

/// An authenticated connection whose text had not come when the listener
/// was taken down is told why: its `paste` has nowhere to go.
fn overtaken(stream: &mut UnixStream, shared: &Shared) {
    if shared.stopping.load(Ordering::Acquire) {
        let _ = stream.write_all(shared.late().line().as_bytes());
    }
}

/// A wrong answer: it is counted and its reply waits a second by the clock.
/// The tenth stops the listener at once (no connection is served any more
/// and the accept loop takes the socket file with it); the `paste` route
/// goes once the wait is over, because taking the listener down ends every
/// wait.
fn refuse(stream: &mut UnixStream, shared: &Arc<Shared>) {
    let failures = shared.failures.fetch_add(1, Ordering::AcqRel) + 1;
    let last = failures >= PASTE_AUTH_FAILURES;
    if last {
        shared.closed.store(true, Ordering::Release);
    }
    let until = shared.clock.now_ms().saturating_add(PASTE_AUTH_DELAY_MS);
    while shared.clock.now_ms() < until && !shared.stopping.load(Ordering::Acquire) {
        thread::sleep(POLL);
    }
    if last {
        if let Some(target) = shared.target.upgrade() {
            target.paste_lockout(&shared.episode, &shared.stopping);
        }
    }
    let _ = stream.write_all(b"error auth\n");
}

fn nonce() -> Option<String> {
    let mut random = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .ok()?;
    Some(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The digest a client that knows the view password answers `nonce` with.
pub fn paste_digest(nonce: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"shacs-paste-v1\n");
    hasher.update(nonce.as_bytes());
    hasher.update(b"\n");
    hasher.update(secret.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn authentic(line: &[u8], nonce: &str, secret: &str) -> bool {
    let Some(answer) = line.strip_prefix(b"auth ") else {
        return false;
    };
    let expected = paste_digest(nonce, secret);
    let expected = expected.as_bytes();
    if answer.len() != expected.len() {
        return false;
    }
    // Every byte is compared, whatever the first difference is.
    answer
        .iter()
        .zip(expected)
        .fold(0, |different, (left, right)| different | (left ^ right))
        == 0
}

/// The `n` of a `paste <n>` line: plain decimal digits and nothing else.
fn announced(line: &[u8]) -> Option<usize> {
    let digits = line.strip_prefix(b"paste ")?;
    if digits.is_empty() || digits.len() > 6 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// The text as it is inserted: UTF-8 with `\r\n` as `\n`, and no NUL or
/// other C0 control character than `\n` and `\t`.
fn pasted(bytes: Vec<u8>) -> Option<String> {
    let text = String::from_utf8(bytes).ok()?.replace("\r\n", "\n");
    let allowed = text
        .chars()
        .all(|character| character >= ' ' || matches!(character, '\n' | '\t'));
    allowed.then_some(text)
}

/// Some bytes, or `None` when the peer is gone, the time is up or the
/// listener is closed.
fn read_some(
    stream: &mut UnixStream,
    shared: &Shared,
    buffer: &mut [u8],
    until_ms: u64,
) -> Option<usize> {
    loop {
        if shared.over() || shared.clock.now_ms() >= until_ms {
            return None;
        }
        match stream.read(buffer) {
            Ok(0) => return None,
            Ok(read) => return Some(read),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => return None,
        }
    }
}

/// One line without its `\n`. Reading stops at the limit; the bytes after a
/// line stay unread.
fn read_line(stream: &mut UnixStream, shared: &Shared, until_ms: u64) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    let mut byte = [0];
    while line.len() < LINE_LIMIT {
        read_some(stream, shared, &mut byte, until_ms)?;
        if byte[0] == b'\n' {
            return Some(line);
        }
        line.push(byte[0]);
    }
    None
}

fn read_exact(
    stream: &mut UnixStream,
    shared: &Shared,
    length: usize,
    until_ms: u64,
) -> Option<Vec<u8>> {
    let mut bytes = vec![0; length];
    let mut filled = 0;
    while filled < length {
        filled += read_some(stream, shared, &mut bytes[filled..], until_ms)?;
    }
    Some(bytes)
}
