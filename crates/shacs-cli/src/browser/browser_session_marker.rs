//! One small file per session that uses the browser (Spec 050 PRD 000 scope
//! 6). The process that owns the session's browser holds an exclusive lock on
//! the file, so a restarted process can tell a browser that is gone from one
//! that another process still runs.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_core::runtime::BrowserError;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const DIRECTORY: &str = "runtime/browser-sessions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MarkerState {
    Open,
    ReplyWaitTimeout,
    Closed,
}

/// The whole file: no session key, no address, no page content.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    state: MarkerState,
    written_at_ms: u64,
}

/// The marker of a live browser. The lock lasts as long as this value.
pub(super) struct SessionMarker {
    file: File,
    path: PathBuf,
}

pub(super) enum Claim {
    Held(SessionMarker),
    /// Another process uses the browser under the same marker.
    HeldElsewhere,
}

pub(super) fn path(data_dir: &Path, owner_mode: &str, session: &str) -> PathBuf {
    let name = format!(
        "{:x}.json",
        Sha256::digest(format!("{owner_mode}\n{session}").as_bytes())
    );
    data_dir.join(DIRECTORY).join(name)
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    options
}

fn regular(file: &File) -> io::Result<()> {
    if file.metadata()?.file_type().is_file() {
        Ok(())
    } else {
        Err(io::Error::other("browser session marker is not a file"))
    }
}

pub(super) fn claim(path: &Path, now: u64) -> io::Result<Claim> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("browser session marker has no directory"))?;
    if let Some(parent) = directory.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    match builder.create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let mut options = options();
    options.write(true).create(true).truncate(false);
    let file = options.open(path)?;
    regular(&file)?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Claim::HeldElsewhere),
        Err(error) => return Err(error),
    }
    let mut marker = SessionMarker {
        file,
        path: path.to_owned(),
    };
    if let Err(error) = marker.write(MarkerState::Open, now) {
        // A half-written marker would later read as a closed browser.
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(Claim::Held(marker))
}

impl SessionMarker {
    /// Rewritten in place: replacing the file by rename would leave the lock
    /// on the old one.
    fn write(&mut self, state: MarkerState, now: u64) -> io::Result<()> {
        let bytes = serde_json::to_vec(&Record {
            state,
            written_at_ms: now,
        })
        .map_err(io::Error::other)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&bytes)?;
        self.file.set_len(bytes.len() as u64)?;
        self.file.sync_all()
    }

    /// The browser closed and the session has not been told: the file stays
    /// with the reason, and the lock ends with this value.
    pub(super) fn leave(mut self, state: MarkerState, now: u64) -> io::Result<()> {
        self.write(state, now)
    }

    pub(super) fn remove(self) -> io::Result<()> {
        fs::remove_file(&self.path)
    }
}

/// What the file says to a session that has no browser in this process.
/// A file whose lock another process holds says nothing and is not touched.
pub(super) fn undelivered(path: &Path) -> Option<BrowserError> {
    let file = options().open(path).ok()?;
    regular(&file).ok()?;
    FileExt::try_lock_exclusive(&file).ok()?;
    let mut text = String::new();
    let state = (&file)
        .take(256)
        .read_to_string(&mut text)
        .ok()
        .and_then(|_| serde_json::from_str::<Record>(&text).ok())
        .map(|record| record.state);
    // `open` means the owning process ended without closing its browser.
    Some(match state {
        Some(MarkerState::ReplyWaitTimeout) => BrowserError::ReplyWaitTimeout,
        Some(MarkerState::Open | MarkerState::Closed) | None => BrowserError::Closed,
    })
}

/// Remove the file of a delivered notice. A file another process holds is
/// left alone; an error means the file is still there and readable.
pub(super) fn forget(path: &Path) -> io::Result<()> {
    let file = match options().open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(error) => return Err(error),
    }
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
