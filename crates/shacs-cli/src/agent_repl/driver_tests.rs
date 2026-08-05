use super::*;

use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

use shacs_core::runtime::AgentLoopCommandResult;

struct BlockingExecutor {
    observed: mpsc::Sender<String>,
    release_tx: mpsc::Sender<()>,
    release_rx: Mutex<mpsc::Receiver<()>>,
}

impl ReplTurnExecutor for BlockingExecutor {
    fn execute(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.observed
            .send(input.to_owned())
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        match input {
            "hello" => self
                .release_rx
                .lock()
                .map_err(|_| CliError::InvalidArguments("release lock poisoned".to_owned()))?
                .recv()
                .map_err(|error| CliError::InvalidArguments(error.to_string()))?,
            "/stop" => self
                .release_tx
                .send(())
                .map_err(|error| CliError::InvalidArguments(error.to_string()))?,
            _ => {}
        }
        Ok(ReplTurnOutcome {
            content: format!("outcome:{input}"),
            stop_reason: "completed".to_owned(),
            command: match input {
                "/status" => Some(AgentLoopCommandResult::Status),
                "/stop" => Some(AgentLoopCommandResult::StopRequested),
                _ => None,
            },
        })
    }
}

struct PriorityOnlyStopExecutor {
    observed: mpsc::Sender<String>,
    release_tx: mpsc::Sender<()>,
    release_rx: Mutex<mpsc::Receiver<()>>,
}

impl ReplTurnExecutor for PriorityOnlyStopExecutor {
    fn execute(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.observed
            .send(format!("normal:{input}"))
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        match input {
            "hello" => self
                .release_rx
                .lock()
                .map_err(|_| CliError::InvalidArguments("release lock poisoned".to_owned()))?
                .recv()
                .map_err(|error| CliError::InvalidArguments(error.to_string()))?,
            "/stop" => {
                return Err(CliError::InvalidArguments(
                    "session turn already active: cli:direct (session_busy)".to_owned(),
                ));
            }
            _ => {}
        }
        Ok(outcome_for(input))
    }

    fn execute_priority(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.observed
            .send(format!("priority:{input}"))
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        if input == "/stop" {
            self.release_tx
                .send(())
                .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        }
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
fn driver_runs_priority_and_eof_stop_while_turn_is_active() -> Result<(), Box<dyn std::error::Error>>
{
    let (observed_tx, observed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let executor = Arc::new(BlockingExecutor {
        observed: observed_tx,
        release_tx,
        release_rx: Mutex::new(release_rx),
    });
    let mut output = Vec::new();

    run(Cursor::new("hello\n/status\n"), &mut output, executor)?;

    let mut observed = vec![
        observed_rx.recv()?,
        observed_rx.recv()?,
        observed_rx.recv()?,
    ];
    observed.sort();
    assert_eq!(
        observed,
        vec!["/status".to_owned(), "/stop".to_owned(), "hello".to_owned()]
    );
    let rendered = String::from_utf8(output)?;
    assert!(rendered.contains("Projection: kind=turn status=running"));
    assert!(rendered.contains("Command: Status"));
    assert!(rendered.contains("Command: StopRequested"));
    assert!(rendered.contains("REPL closed."));
    Ok(())
}

#[test]
fn driver_routes_active_stop_through_priority_executor() -> Result<(), Box<dyn std::error::Error>> {
    let (observed_tx, observed_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let executor = Arc::new(PriorityOnlyStopExecutor {
        observed: observed_tx,
        release_tx,
        release_rx: Mutex::new(release_rx),
    });
    let mut output = Vec::new();

    run(Cursor::new("hello\n/stop\n"), &mut output, executor)?;

    let mut observed = Vec::new();
    while let Ok(event) = observed_rx.recv_timeout(Duration::from_secs(1)) {
        observed.push(event);
    }
    assert!(observed.contains(&"normal:hello".to_owned()));
    assert!(observed.contains(&"priority:/stop".to_owned()));
    assert!(!observed.contains(&"normal:/stop".to_owned()));
    let rendered = String::from_utf8(output)?;
    assert!(!rendered.contains("session_busy"));
    assert!(rendered.contains("Command: StopRequested"));
    Ok(())
}
