use shacs_core::app::{AppId, AppLifecycleState};
use shacs_core::app_lifecycle::{
    AppLifecycleBlocker, AppProcessOutcomeEvidence, AppProcessState, AppSupervisorJournal,
};
use shacs_core::runtime::{
    AppProcessDriver, AppProcessRunOutcome, AppStartFacts, AppSupervisor, AppSupervisorTerminal,
};
use std::error::Error;

struct Driver {
    terminal: AppSupervisorTerminal,
    calls: usize,
}

impl AppProcessDriver for Driver {
    fn run(&mut self) -> AppProcessRunOutcome {
        self.calls += 1;
        AppProcessRunOutcome {
            terminal: self.terminal,
            evidence: AppProcessOutcomeEvidence {
                outcome: format!("{:?}", self.terminal),
                duration_ms: 1,
                cleanup_attempted: true,
                descendant_cleanup_supported: cfg!(unix),
            },
        }
    }
}

#[test]
fn blocked_start_never_dispatches_process_driver() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let supervisor = AppSupervisor::new(&journal);
    let mut driver = Driver {
        terminal: AppSupervisorTerminal::Stopped,
        calls: 0,
    };
    let result = supervisor.start(
        facts(AppLifecycleState::Disabled, false, vec!["TOKEN"]),
        &mut driver,
    )?;

    assert_eq!(result.dispatch_count, 0);
    assert_eq!(driver.calls, 0);
    assert_eq!(result.terminal.current_state, AppProcessState::Failed);
    assert!(!result.restart_requested);
    assert!(result
        .terminal
        .blockers
        .contains(&AppLifecycleBlocker::AppNotEnabled));
    assert!(result
        .terminal
        .blockers
        .contains(&AppLifecycleBlocker::CredentialMissing {
            name: "TOKEN".to_owned()
        }));
    Ok(())
}

#[test]
fn successful_start_records_running_and_terminal_receipts() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let supervisor = AppSupervisor::new(&journal);
    let mut driver = Driver {
        terminal: AppSupervisorTerminal::Stopped,
        calls: 0,
    };
    let result = supervisor.start(
        facts(AppLifecycleState::Enabled, true, Vec::new()),
        &mut driver,
    )?;

    assert_eq!(result.dispatch_count, 1);
    assert_eq!(driver.calls, 1);
    assert_eq!(
        result.running.map(|receipt| receipt.current_state),
        Some(AppProcessState::Running)
    );
    assert_eq!(result.terminal.current_state, AppProcessState::Stopped);
    assert!(result.terminal.process_outcome.is_some());
    assert_eq!(
        result.terminal.execution_snapshot_ref.as_deref(),
        Some("execution:app:test")
    );
    assert_eq!(
        journal.replay(&AppId::parse("runner.app")?)?.dispatch_count,
        0
    );
    Ok(())
}

#[test]
fn uncertain_cleanup_remains_recovery_needed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let supervisor = AppSupervisor::new(&journal);
    let app_id = AppId::parse("runner.app")?;
    let recovered = supervisor.recover(&app_id, false)?;
    assert_eq!(recovered.current_state, AppProcessState::RecoveryNeeded);
    Ok(())
}

#[test]
fn restart_uses_requested_receipt_and_requires_a_new_generation() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let journal = AppSupervisorJournal::new(root.path());
    let supervisor = AppSupervisor::new(&journal);
    let mut driver = Driver {
        terminal: AppSupervisorTerminal::RestartRequested,
        calls: 0,
    };

    let first = supervisor.start(
        facts(AppLifecycleState::Enabled, true, Vec::new()),
        &mut driver,
    )?;
    assert!(first.restart_requested);
    assert_eq!(first.terminal.current_state, AppProcessState::Stopped);
    assert_eq!(first.requested.generation, 1);

    driver.terminal = AppSupervisorTerminal::Stopped;
    let second = supervisor.start(
        facts(AppLifecycleState::Enabled, true, Vec::new()),
        &mut driver,
    )?;
    assert!(!second.restart_requested);
    assert_eq!(second.requested.generation, 2);
    assert_eq!(driver.calls, 2);
    Ok(())
}

fn facts(
    lifecycle: AppLifecycleState,
    trusted: bool,
    missing_credentials: Vec<&str>,
) -> AppStartFacts {
    AppStartFacts {
        app_id: AppId::parse("runner.app").unwrap_or_else(|_| unreachable!()),
        lifecycle,
        expected_manifest_digest: "sha256:manifest".to_owned(),
        current_manifest_digest: "sha256:manifest".to_owned(),
        trusted_runtime_ref: trusted.then(|| "trusted-runtime:local".to_owned()),
        workspace_trusted: trusted,
        credential_source_statuses: Vec::new(),
        missing_credentials: missing_credentials.into_iter().map(str::to_owned).collect(),
        activation_blockers: Vec::new(),
        process_authorized: trusted,
        activation_refs: Vec::new(),
        execution_snapshot_ref: Some("execution:app:test".to_owned()),
    }
}
