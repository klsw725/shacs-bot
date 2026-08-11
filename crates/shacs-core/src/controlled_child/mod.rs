mod output;
mod process_group;
mod run;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub use run::{
    run_bash, run_configured_credential_command, run_configured_load_check,
    run_configured_package_command, run_generic_argv,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlledChildAdapter {
    Bash,
    GenericArgv,
    CredentialCommand,
    PackageCommand,
    LoadCheck,
}

#[derive(Debug, Clone)]
pub struct ControlledChildCommand {
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub inherit_env: bool,
    pub timeout: Duration,
    pub termination_grace: Duration,
    pub output_limit: usize,
}

impl ControlledChildCommand {
    pub fn new<I, S>(argv: I, cwd: impl Into<PathBuf>, timeout: Duration) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
            inherit_env: true,
            timeout,
            termination_grace: Duration::from_millis(250),
            output_limit: 10_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ControlledChildAbort {
    requested: Arc<AtomicBool>,
    propagated: bool,
}

impl ControlledChildAbort {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn abort(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    pub fn is_aborted(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub(crate) fn from_flag(requested: Arc<AtomicBool>) -> Self {
        Self {
            requested,
            propagated: true,
        }
    }

    pub const fn is_propagated(&self) -> bool {
        self.propagated
    }
}

impl PartialEq for ControlledChildAbort {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.requested, &other.requested)
    }
}

impl Eq for ControlledChildAbort {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlledChildOutcome {
    Succeeded { code: Option<i32> },
    Failed { code: Option<i32> },
    TimedOut,
    Aborted,
    InvalidCwd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlledChildStream {
    pub captured: Vec<u8>,
    pub total_bytes: u64,
    pub truncated: bool,
}

impl ControlledChildStream {
    const fn empty() -> Self {
        Self {
            captured: Vec::new(),
            total_bytes: 0,
            truncated: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescendantCleanupCapability {
    Supported,
    Unsupported,
}

pub const fn descendant_cleanup_capability() -> DescendantCleanupCapability {
    if cfg!(unix) {
        DescendantCleanupCapability::Supported
    } else {
        DescendantCleanupCapability::Unsupported
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlledChildReceipt {
    pub adapter: ControlledChildAdapter,
    pub outcome: ControlledChildOutcome,
    pub stdout: ControlledChildStream,
    pub stderr: ControlledChildStream,
    pub duration_ms: u64,
    pub descendant_cleanup: DescendantCleanupCapability,
    pub cleanup_attempted: bool,
    pub abort_capable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlledChildError {
    EmptyArgv,
    Spawn(String),
    Wait(String),
    MissingPipe,
    OutputRead(String),
    OutputThread,
}

impl fmt::Display for ControlledChildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArgv => formatter.write_str("controlled child argv is empty"),
            Self::Spawn(error) => write!(formatter, "controlled child spawn failed: {error}"),
            Self::Wait(error) => write!(formatter, "controlled child wait failed: {error}"),
            Self::MissingPipe => formatter.write_str("controlled child output pipe is missing"),
            Self::OutputRead(error) => {
                write!(formatter, "controlled child output read failed: {error}")
            }
            Self::OutputThread => formatter.write_str("controlled child output thread panicked"),
        }
    }
}

impl Error for ControlledChildError {}

fn empty_receipt(
    adapter: ControlledChildAdapter,
    outcome: ControlledChildOutcome,
) -> ControlledChildReceipt {
    ControlledChildReceipt {
        adapter,
        outcome,
        stdout: ControlledChildStream::empty(),
        stderr: ControlledChildStream::empty(),
        duration_ms: 0,
        descendant_cleanup: descendant_cleanup_capability(),
        cleanup_attempted: false,
        abort_capable: false,
    }
}
