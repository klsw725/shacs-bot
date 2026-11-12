//! Closures a session has not been told about yet (Spec 050 PRD 000 scope 6).
//! The marker files are the record that survives a restart; the table in
//! memory is a copy so the same process does not read a file twice.
use super::browser_session_marker::{self, Claim, MarkerState, SessionMarker};
use shacs_core::runtime::{BrowserClosureNotice, BrowserError};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Default)]
struct Table {
    pending: HashMap<String, BrowserClosureNotice>,
    /// Sessions whose delivered notice could not be removed from disk.
    delivered: HashSet<String>,
    serial: u64,
}

pub(super) struct BrowserNotices {
    /// Where the marker files live. A manager without a data directory keeps
    /// its notices in memory only.
    data_dir: Option<PathBuf>,
    owner_mode: String,
    /// Makes the tokens of this process differ from an earlier one's.
    seed: u64,
    table: Mutex<Table>,
}

impl BrowserNotices {
    pub(super) fn new(data_dir: Option<PathBuf>, owner_mode: &str, seed: u64) -> Self {
        Self {
            data_dir,
            owner_mode: owner_mode.to_owned(),
            seed,
            table: Mutex::new(Table::default()),
        }
    }

    fn marker_path(&self, session: &str) -> Option<PathBuf> {
        self.data_dir
            .as_ref()
            .map(|data_dir| browser_session_marker::path(data_dir, &self.owner_mode, session))
    }

    fn insert(
        &self,
        table: &mut Table,
        session: &str,
        error: BrowserError,
    ) -> BrowserClosureNotice {
        table.serial += 1;
        let notice = BrowserClosureNotice {
            error,
            // Opaque: no session key and no page content.
            token: format!("n{:x}-{}", self.seed, table.serial),
        };
        table.pending.insert(session.to_owned(), notice.clone());
        notice
    }

    /// The notice this process already knows of. Never reads a file.
    pub(super) fn remembered(
        &self,
        session: &str,
    ) -> Result<Option<BrowserClosureNotice>, BrowserError> {
        let table = self
            .table
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        Ok(table.pending.get(session).cloned())
    }

    /// The session's notice, from memory or from its marker file. Only for a
    /// session that has no browser in this process: its own live marker is
    /// locked by this process and would read as another process's.
    pub(super) fn stored(
        &self,
        session: &str,
    ) -> Result<Option<BrowserClosureNotice>, BrowserError> {
        let mut table = self
            .table
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        if let Some(notice) = table.pending.get(session) {
            return Ok(Some(notice.clone()));
        }
        if table.delivered.contains(session) {
            return Ok(None);
        }
        let error = self
            .marker_path(session)
            .and_then(|path| browser_session_marker::undelivered(&path));
        Ok(error.map(|error| self.insert(&mut table, session, error)))
    }

    pub(super) fn leave(&self, session: &str, error: BrowserError) -> Result<(), BrowserError> {
        let mut table = self
            .table
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        table.delivered.remove(session);
        self.insert(&mut table, session, error);
        Ok(())
    }

    /// Remove the notice `token` names. A stale or unknown token removes
    /// nothing.
    pub(super) fn acknowledge(&self, session: &str, token: &str) {
        let Ok(mut table) = self.table.lock() else {
            return;
        };
        if !table
            .pending
            .get(session)
            .is_some_and(|notice| notice.token == token)
        {
            return;
        }
        table.pending.remove(session);
        let Some(path) = self.marker_path(session) else {
            return;
        };
        if browser_session_marker::forget(&path).is_err() {
            // The file is still there; this process does not show it again.
            table.delivered.insert(session.to_owned());
        }
    }

    /// Lock the marker of a session that is about to own the browser. `None`
    /// means the browser runs without one: no data directory, or another
    /// process holds the same marker.
    pub(super) fn claim(
        &self,
        session: &str,
        now: u64,
    ) -> Result<Option<SessionMarker>, BrowserError> {
        let Some(path) = self.marker_path(session) else {
            return Ok(None);
        };
        let claim = browser_session_marker::claim(&path, now)
            .map_err(|_| BrowserError::StateUnavailable)?;
        let Claim::Held(marker) = claim else {
            return Ok(None);
        };
        let mut table = self
            .table
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        table.delivered.remove(session);
        Ok(Some(marker))
    }

    pub(super) fn pending(&self) -> Result<usize, BrowserError> {
        self.table
            .lock()
            .map(|table| table.pending.len())
            .map_err(|_| BrowserError::StateUnavailable)
    }
}

pub(super) fn marker_state(error: BrowserError) -> MarkerState {
    if error == BrowserError::ReplyWaitTimeout {
        MarkerState::ReplyWaitTimeout
    } else {
        MarkerState::Closed
    }
}
