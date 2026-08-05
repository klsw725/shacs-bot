use super::*;

use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

use shacs_core::runtime::{
    AgentLoopCommandResult, SessionTurnAcquireError, SessionTurnCancelOutcome, SessionTurnLock,
    SessionTurnReservation,
};

struct ProductionLockExecutor {
    turn_lock: SessionTurnLock,
    observed: mpsc::Sender<String>,
    release_tx: mpsc::Sender<()>,
    release_rx: Mutex<Option<mpsc::Receiver<()>>>,
}

struct ProductionPermit {
    reservation: SessionTurnReservation,
    observed: mpsc::Sender<String>,
    release_rx: Mutex<Option<mpsc::Receiver<()>>>,
}

impl ReplTurnPermit for ProductionPermit {
    fn bind_to_current_thread(&self) {
        let Ok(mut release_rx) = self.release_rx.lock() else {
            return;
        };
        if let Some(release_rx) = release_rx.take() {
            let _ = self.observed.send("ordinary:bind-waiting".to_owned());
            let _ = release_rx.recv();
        }
        self.reservation.bind_to_current_thread();
        let _ = self.observed.send("ordinary:bind-attempted".to_owned());
    }
}

impl ReplTurnExecutor for ProductionLockExecutor {
    fn reserve_turn(&self, _input: &str) -> Result<Box<dyn ReplTurnPermit>, CliError> {
        self.observed
            .send("ordinary:reserved".to_owned())
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        let release_rx = self
            .release_rx
            .lock()
            .map_err(|_| CliError::InvalidArguments("release lock poisoned".to_owned()))?
            .take()
            .ok_or_else(|| CliError::InvalidArguments("release receiver missing".to_owned()))?;
        Ok(Box::new(ProductionPermit {
            reservation: self.turn_lock.reserve("cli:direct"),
            observed: self.observed.clone(),
            release_rx: Mutex::new(Some(release_rx)),
        }))
    }

    fn execute(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        match self.turn_lock.acquire("cli:direct") {
            Ok(_guard) => {
                self.observed
                    .send(format!("ordinary:started:{input}"))
                    .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
                Ok(ReplTurnOutcome {
                    content: "completed".to_owned(),
                    stop_reason: "completed".to_owned(),
                    command: None,
                })
            }
            Err(SessionTurnAcquireError::Cancelled { .. }) => Ok(ReplTurnOutcome {
                content: "cancelled".to_owned(),
                stop_reason: "cancelled".to_owned(),
                command: None,
            }),
            Err(SessionTurnAcquireError::AlreadyActive { session_key }) => {
                Err(CliError::InvalidArguments(format!(
                    "session turn already active: {session_key} (session_busy)"
                )))
            }
        }
    }

    fn execute_priority(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.observed
            .send(format!("priority:{input}"))
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        if input == "/stop" {
            assert_eq!(
                self.turn_lock.cancel_active_or_reserved("cli:direct"),
                SessionTurnCancelOutcome::Reserved
            );
        }
        match self.turn_lock.acquire_priority("cli:direct") {
            Ok(_guard) => {}
            Err(SessionTurnAcquireError::AlreadyActive { .. }) => {}
            Err(SessionTurnAcquireError::Cancelled { .. }) => {}
        }
        self.release_tx
            .send(())
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        Ok(outcome_for(input))
    }
}

fn outcome_for(input: &str) -> ReplTurnOutcome {
    ReplTurnOutcome {
        content: format!("outcome:{input}"),
        stop_reason: "completed".to_owned(),
        command: match input {
            "/status" => Some(AgentLoopCommandResult::Status),
            "/stop" => Some(AgentLoopCommandResult::StopRequested),
            _ => None,
        },
    }
}

#[test]
fn driver_reserves_ordinary_turn_before_active_stop_can_race(
) -> Result<(), Box<dyn std::error::Error>> {
    let (observed_tx, observed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let executor = Arc::new(ProductionLockExecutor {
        turn_lock: SessionTurnLock::new(),
        observed: observed_tx,
        release_tx,
        release_rx: Mutex::new(Some(release_rx)),
    });
    let mut output = Vec::new();

    run(Cursor::new("hello\n/stop\n"), &mut output, executor)?;

    let mut observed = Vec::new();
    while let Ok(event) = observed_rx.recv_timeout(Duration::from_secs(1)) {
        observed.push(event);
    }
    assert!(observed.contains(&"ordinary:reserved".to_owned()));
    assert!(observed.contains(&"ordinary:bind-waiting".to_owned()));
    assert!(observed.contains(&"ordinary:bind-attempted".to_owned()));
    assert!(observed.contains(&"priority:/stop".to_owned()));
    assert_eq!(
        observed
            .iter()
            .filter(|event| event.as_str() == "priority:/stop")
            .count(),
        1
    );
    assert_eq!(
        observed
            .iter()
            .filter(|event| event.starts_with("ordinary:started:"))
            .count(),
        0
    );
    let rendered = String::from_utf8(output)?;
    assert!(rendered.contains("Command: StopRequested"), "{rendered}");
    assert!(rendered.contains("status=cancelled"), "{rendered}");
    assert!(!rendered.contains("session_busy"), "{rendered}");
    Ok(())
}
