use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use crate::agent_repl::input::{parse_line, ReplInput};
use crate::agent_repl::render::{self, ReplTurnOutcome};
use crate::agent_repl::state::{ReplAction, ReplState};
use crate::CliError;

pub trait ReplTurnExecutor: Send + Sync + 'static {
    fn reserve_turn(&self, _input: &str) -> Result<Box<dyn ReplTurnPermit>, CliError> {
        Ok(Box::new(NoopTurnPermit))
    }

    fn execute(&self, input: &str) -> Result<ReplTurnOutcome, CliError>;

    fn execute_priority(&self, input: &str) -> Result<ReplTurnOutcome, CliError> {
        self.execute(input)
    }
}

pub trait ReplTurnPermit: Send {
    fn bind_to_current_thread(&self);
}

struct NoopTurnPermit;

impl ReplTurnPermit for NoopTurnPermit {
    fn bind_to_current_thread(&self) {}
}

enum DriverEvent {
    Input(ReplInput),
    Completed(Result<ReplTurnOutcome, String>),
}

pub fn run<R, W, E>(reader: R, writer: &mut W, executor: Arc<E>) -> Result<(), CliError>
where
    R: BufRead + Send + 'static,
    W: Write,
    E: ReplTurnExecutor,
{
    let (tx, rx) = mpsc::channel();
    spawn_reader(reader, tx.clone());
    spawn_interrupt_watcher(tx.clone());
    writeln!(writer, "{}", render::welcome())?;
    write!(writer, "{}", render::prompt(false))?;
    writer.flush()?;
    drive(rx, tx, writer, executor)
}

pub fn run_stdio<W, E>(writer: &mut W, executor: Arc<E>) -> Result<(), CliError>
where
    W: Write,
    E: ReplTurnExecutor,
{
    run(io::BufReader::new(io::stdin()), writer, executor)
}

fn drive<W, E>(
    rx: mpsc::Receiver<DriverEvent>,
    tx: mpsc::Sender<DriverEvent>,
    writer: &mut W,
    executor: Arc<E>,
) -> Result<(), CliError>
where
    W: Write,
    E: ReplTurnExecutor,
{
    let mut state = ReplState::default();
    while let Ok(event) = rx.recv() {
        match event {
            DriverEvent::Input(input) => match state.handle_input(input) {
                ReplAction::None => {}
                ReplAction::Exit => {
                    writeln!(writer, "\n{}", render::eof())?;
                    return Ok(());
                }
                ReplAction::StartTurn(input) => {
                    spawn_turn(input, executor.clone(), tx.clone(), writer)?
                }
                ReplAction::RunPriority(input) => {
                    write_turn(writer, executor.execute_priority(&input))?
                }
                ReplAction::QueueFollowUp(message) => {
                    writeln!(writer, "{}", render::queued(&message))?
                }
                ReplAction::Malformed(raw) => writeln!(writer, "{}", render::malformed(&raw))?,
                ReplAction::RequestStop | ReplAction::RequestStopAndExit => {
                    writeln!(writer, "{}", render::stop_requested())?;
                    write_turn(writer, executor.execute_priority("/stop"))?;
                }
            },
            DriverEvent::Completed(result) => {
                write_turn(writer, result.map_err(CliError::InvalidArguments))?;
                if let Some(next) = state.finish_turn() {
                    spawn_turn(next, executor.clone(), tx.clone(), writer)?;
                } else if state.should_exit_after_completion() {
                    writeln!(writer, "{}", render::eof())?;
                    return Ok(());
                }
            }
        }
        write!(writer, "{}", render::prompt(state.is_active()))?;
        writer.flush()?;
    }
    Ok(())
}

fn spawn_turn<W, E>(
    input: String,
    executor: Arc<E>,
    tx: mpsc::Sender<DriverEvent>,
    writer: &mut W,
) -> Result<(), CliError>
where
    W: Write,
    E: ReplTurnExecutor,
{
    let permit = executor.reserve_turn(&input)?;
    thread::spawn(move || {
        permit.bind_to_current_thread();
        let result = executor.execute(&input).map_err(|error| error.to_string());
        let _ = tx.send(DriverEvent::Completed(result));
    });
    writeln!(writer, "Projection: kind=turn status=running")?;
    Ok(())
}

fn write_turn<W>(writer: &mut W, result: Result<ReplTurnOutcome, CliError>) -> Result<(), CliError>
where
    W: Write,
{
    match result {
        Ok(outcome) => writeln!(writer, "{}", render::turn(&outcome))?,
        Err(error) => writeln!(writer, "Projection: kind=error status=failed\n{error}")?,
    }
    Ok(())
}

fn spawn_reader<R>(reader: R, tx: mpsc::Sender<DriverEvent>)
where
    R: BufRead + Send + 'static,
{
    thread::spawn(move || {
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if tx.send(DriverEvent::Input(parse_line(&line))).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
        let _ = tx.send(DriverEvent::Input(ReplInput::Eof));
    });
}

fn spawn_interrupt_watcher(tx: mpsc::Sender<DriverEvent>) {
    thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        loop {
            if runtime.block_on(tokio::signal::ctrl_c()).is_err() {
                return;
            }
            if tx.send(DriverEvent::Input(ReplInput::Interrupt)).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
#[path = "driver_race_tests.rs"]
mod race_tests;

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
