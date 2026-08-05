use shacs_core::runtime::{SurfaceActionOutcome, SurfaceActionOutcomeKind};
use shacs_session::{SessionRuntimeExecutionProjection, SessionRuntimeWorkflowProjection};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey(String);

impl SessionKey {
    pub fn new(value: impl Into<String>) -> Result<Self, TuiStateError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TuiStateError::EmptySessionKey);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalLineage(String);

impl ApprovalLineage {
    pub fn new(value: impl Into<String>) -> Result<Self, TuiStateError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TuiStateError::EmptyApprovalLineage);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub sessions: Vec<RuntimeSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSession {
    pub key: SessionKey,
    pub updated_at: Option<String>,
    pub message_count: usize,
    pub recovery_markers: Vec<String>,
    pub checkpoint_phase: Option<String>,
    pub diagnostics_ref_count: usize,
    pub workflow: Option<SessionRuntimeWorkflowProjection>,
    pub execution: Option<SessionRuntimeExecutionProjection>,
    pub pending_approval: Option<PendingApproval>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub lineage: ApprovalLineage,
    pub tool_name: String,
    pub status: ApprovalStatus,
    pub expires_at_unix_ms: Option<u64>,
    pub action: ApprovalActionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalActionState {
    Actionable { target_owner_id: String },
    Unavailable { reason: String },
}

impl ApprovalActionState {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Executing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiState {
    pub sessions: Vec<RuntimeSession>,
    pub selected: usize,
    pub status: UiStatus,
    pub terminal_size: TerminalSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiStatus {
    Ready,
    Empty,
    InvalidAction(String),
    ActionUnavailable(String),
    ActionOutcome(SurfaceActionOutcome),
    SourceError(String),
    Exiting,
}

pub fn action_outcome_label(kind: SurfaceActionOutcomeKind) -> &'static str {
    match kind {
        SurfaceActionOutcomeKind::Requested => "requested",
        SurfaceActionOutcomeKind::Completed => "completed",
        SurfaceActionOutcomeKind::Unavailable => "unavailable",
        SurfaceActionOutcomeKind::StaleLineage => "stale lineage",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiStateError {
    EmptySessionKey,
    EmptyApprovalLineage,
}

impl TuiState {
    pub fn from_snapshot(snapshot: RuntimeSnapshot, preferred: Option<&SessionKey>) -> Self {
        let selected = preferred
            .and_then(|key| {
                snapshot
                    .sessions
                    .iter()
                    .position(|session| &session.key == key)
            })
            .unwrap_or_default();
        let status = if snapshot.sessions.is_empty() {
            UiStatus::Empty
        } else {
            UiStatus::Ready
        };
        Self {
            sessions: snapshot.sessions,
            selected,
            status,
            terminal_size: TerminalSize {
                columns: 100,
                rows: 30,
            },
        }
    }

    pub fn selected_session(&self) -> Option<&RuntimeSession> {
        self.sessions.get(self.selected)
    }
}

impl fmt::Display for TuiStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySessionKey => formatter.write_str("session key is empty"),
            Self::EmptyApprovalLineage => formatter.write_str("approval lineage is empty"),
        }
    }
}

impl std::error::Error for TuiStateError {}
