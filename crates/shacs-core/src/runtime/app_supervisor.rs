use crate::app::{AppId, AppLifecycleState};
use crate::app_lifecycle::{
    AppLifecycleAction, AppLifecycleBlocker, AppLifecycleError, AppLifecycleReceipt,
    AppProcessOutcomeEvidence, AppProcessState, AppSupervisorJournal,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStartFacts {
    pub app_id: AppId,
    pub lifecycle: AppLifecycleState,
    pub expected_manifest_digest: String,
    pub current_manifest_digest: String,
    pub trusted_runtime_ref: Option<String>,
    pub workspace_trusted: bool,
    pub credential_source_statuses: Vec<String>,
    pub missing_credentials: Vec<String>,
    pub activation_blockers: Vec<AppLifecycleBlocker>,
    pub process_authorized: bool,
    pub activation_refs: Vec<String>,
    pub execution_snapshot_ref: Option<String>,
}

impl AppStartFacts {
    pub fn blockers(&self) -> Vec<AppLifecycleBlocker> {
        let mut blockers = Vec::new();
        if self.lifecycle != AppLifecycleState::Enabled {
            blockers.push(AppLifecycleBlocker::AppNotEnabled);
        }
        if self.expected_manifest_digest != self.current_manifest_digest {
            blockers.push(AppLifecycleBlocker::ManifestDigestMismatch);
        }
        if !self.workspace_trusted {
            blockers.push(AppLifecycleBlocker::WorkspaceUntrusted);
        }
        if self.trusted_runtime_ref.is_none() {
            blockers.push(AppLifecycleBlocker::TrustedRuntimeUnavailable);
        }
        blockers.extend(
            self.missing_credentials
                .iter()
                .cloned()
                .map(|name| AppLifecycleBlocker::CredentialMissing { name }),
        );
        blockers.extend(self.activation_blockers.clone());
        if !self.process_authorized {
            blockers.push(AppLifecycleBlocker::ProcessPermissionDenied);
        }
        blockers
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSupervisorTerminal {
    Stopped,
    RestartRequested,
    Failed,
    RecoveryNeeded,
}

pub trait AppProcessDriver {
    fn run(&mut self) -> AppProcessRunOutcome;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProcessRunOutcome {
    pub terminal: AppSupervisorTerminal,
    pub evidence: AppProcessOutcomeEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSupervisorRun {
    pub requested: AppLifecycleReceipt,
    pub running: Option<AppLifecycleReceipt>,
    pub terminal: AppLifecycleReceipt,
    pub dispatch_count: usize,
    pub restart_requested: bool,
}

pub struct AppSupervisor<'a> {
    journal: &'a AppSupervisorJournal,
}

impl<'a> AppSupervisor<'a> {
    pub const fn new(journal: &'a AppSupervisorJournal) -> Self {
        Self { journal }
    }

    pub fn start(
        &self,
        facts: AppStartFacts,
        driver: &mut impl AppProcessDriver,
    ) -> Result<AppSupervisorRun, AppSupervisorError> {
        let requested = self
            .journal
            .request(&facts.app_id, AppLifecycleAction::Start)?;
        self.start_reserved(requested, facts, driver)
    }

    pub fn start_reserved(
        &self,
        mut requested: AppLifecycleReceipt,
        facts: AppStartFacts,
        driver: &mut impl AppProcessDriver,
    ) -> Result<AppSupervisorRun, AppSupervisorError> {
        if requested.app_id != facts.app_id
            || requested.action != AppLifecycleAction::Start
            || requested.current_state != AppProcessState::Starting
            || requested.completed
        {
            return Err(AppSupervisorError::InvalidReservation);
        }
        self.journal.attach_start_evidence(
            &mut requested,
            facts.activation_refs.clone(),
            facts.execution_snapshot_ref.clone(),
        );
        let blockers = facts.blockers();
        if !blockers.is_empty() {
            let terminal = self.journal.block(&requested, blockers)?;
            return Ok(AppSupervisorRun {
                requested,
                running: None,
                terminal,
                dispatch_count: 0,
                restart_requested: false,
            });
        }
        let trusted_runtime_ref = facts
            .trusted_runtime_ref
            .as_deref()
            .ok_or(AppSupervisorError::MissingTrustedRuntime)?;
        let running = self.journal.complete(
            &requested,
            AppProcessState::Running,
            &facts.current_manifest_digest,
            trusted_runtime_ref,
            facts.credential_source_statuses,
        )?;
        let outcome = driver.run();
        let requested_action = if outcome.terminal == AppSupervisorTerminal::RestartRequested {
            AppLifecycleAction::Restart
        } else {
            AppLifecycleAction::Stop
        };
        let mut stop = self
            .journal
            .pending_request(&facts.app_id, requested_action)?
            .map_or_else(|| self.journal.request(&facts.app_id, requested_action), Ok)?;
        stop.activation_refs = running.activation_refs.clone();
        stop.execution_snapshot_ref = running.execution_snapshot_ref.clone();
        stop.process_outcome = Some(outcome.evidence);
        let state = match outcome.terminal {
            AppSupervisorTerminal::Stopped | AppSupervisorTerminal::RestartRequested => {
                AppProcessState::Stopped
            }
            AppSupervisorTerminal::Failed => AppProcessState::Failed,
            AppSupervisorTerminal::RecoveryNeeded => AppProcessState::RecoveryNeeded,
        };
        let terminal = self.journal.complete(
            &stop,
            state,
            &facts.current_manifest_digest,
            trusted_runtime_ref,
            Vec::new(),
        )?;
        Ok(AppSupervisorRun {
            requested,
            running: Some(running),
            terminal,
            dispatch_count: 1,
            restart_requested: outcome.terminal == AppSupervisorTerminal::RestartRequested,
        })
    }

    pub fn recover(
        &self,
        app_id: &AppId,
        cleanup_confirmed: bool,
    ) -> Result<AppLifecycleReceipt, AppSupervisorError> {
        let requested = self.journal.request(app_id, AppLifecycleAction::Recover)?;
        let state = if cleanup_confirmed {
            AppProcessState::Stopped
        } else {
            AppProcessState::RecoveryNeeded
        };
        Ok(self
            .journal
            .complete(&requested, state, "", "", Vec::new())?)
    }
}

#[derive(Debug)]
pub enum AppSupervisorError {
    Lifecycle(AppLifecycleError),
    MissingTrustedRuntime,
    InvalidReservation,
}

impl fmt::Display for AppSupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lifecycle(error) => write!(formatter, "{error}"),
            Self::MissingTrustedRuntime => {
                formatter.write_str("trusted runtime reference is unavailable")
            }
            Self::InvalidReservation => formatter.write_str("invalid app start reservation"),
        }
    }
}

impl std::error::Error for AppSupervisorError {}

impl From<AppLifecycleError> for AppSupervisorError {
    fn from(error: AppLifecycleError) -> Self {
        Self::Lifecycle(error)
    }
}
