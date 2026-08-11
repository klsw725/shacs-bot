use super::MAX_OUTPUT;
use crate::controlled_child::{
    ControlledChildAbort, ControlledChildCommand, ControlledChildOutcome, ControlledChildReceipt,
    ControlledChildStream,
};
use crate::runtime::sandbox_adapter::{execute_bash, SandboxExecutionFact, SandboxPlan};
use crate::runtime::{
    ProcessRedactedSpawnSummary, ProcessRedactedStatus, ProcessRedactedStreamKind,
    ProcessRedactedStreamSummary, ProcessSpawnAuthorization, ProcessTerminalOutcome,
};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

pub(super) fn run_shell(
    _authorization: ProcessSpawnAuthorization,
    command: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
    timeout: Duration,
    abort: Option<&ControlledChildAbort>,
    sandbox: Option<&SandboxPlan>,
) -> Result<ShellRunResult, ShellRunFailure> {
    let argv = if cfg!(windows) {
        vec!["cmd.exe", "/c", command]
    } else {
        vec!["/bin/bash", "-l", "-c", command]
    };
    let mut child = ControlledChildCommand::new(argv, cwd, timeout);
    child.inherit_env = false;
    child.env = env
        .iter()
        .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
        .collect();
    child.output_limit = MAX_OUTPUT;
    let local_abort = ControlledChildAbort::new();
    let execution =
        execute_bash(sandbox, &child, abort.unwrap_or(&local_abort)).map_err(|error| {
            ShellRunFailure {
                message: error.to_string(),
                sandbox: error.fact().cloned(),
            }
        })?;
    shell_result(
        execution.receipt,
        timeout,
        execution.fact,
        execution.warning,
    )
}

fn shell_result(
    receipt: ControlledChildReceipt,
    timeout: Duration,
    sandbox: SandboxExecutionFact,
    sandbox_warning: Option<String>,
) -> Result<ShellRunResult, ShellRunFailure> {
    let terminal_outcome = match receipt.outcome {
        ControlledChildOutcome::Succeeded { .. } => ProcessTerminalOutcome::Succeeded,
        ControlledChildOutcome::Failed { .. } => ProcessTerminalOutcome::Failed,
        ControlledChildOutcome::TimedOut => ProcessTerminalOutcome::TimedOut,
        ControlledChildOutcome::Aborted => ProcessTerminalOutcome::Cancelled,
        ControlledChildOutcome::InvalidCwd => {
            return Err(ShellRunFailure {
                message: "working_dir could not be resolved".to_owned(),
                sandbox: Some(sandbox),
            })
        }
    };
    let output = match receipt.outcome {
        ControlledChildOutcome::TimedOut => format!(
            "Error: Command timed out after {} seconds",
            timeout.as_secs()
        ),
        ControlledChildOutcome::Aborted => "Error: Command aborted".to_owned(),
        ControlledChildOutcome::Succeeded { code } | ControlledChildOutcome::Failed { code } => {
            format_controlled_output(&receipt.stdout, &receipt.stderr, code)
        }
        ControlledChildOutcome::InvalidCwd => {
            return Err(ShellRunFailure {
                message: "working_dir could not be resolved".to_owned(),
                sandbox: Some(sandbox),
            })
        }
    };
    Ok(ShellRunResult {
        output,
        terminal_outcome,
        redacted_summary: shell_output_summary(
            terminal_outcome,
            usize::try_from(receipt.stdout.total_bytes).unwrap_or(usize::MAX),
            usize::try_from(receipt.stderr.total_bytes).unwrap_or(usize::MAX),
        ),
        sandbox,
        sandbox_warning,
        controlled_child_receipt: receipt,
    })
}

pub(super) struct ShellRunResult {
    pub output: String,
    pub terminal_outcome: ProcessTerminalOutcome,
    pub redacted_summary: ProcessRedactedSpawnSummary,
    pub sandbox: SandboxExecutionFact,
    pub sandbox_warning: Option<String>,
    pub controlled_child_receipt: ControlledChildReceipt,
}

pub(super) struct ShellRunFailure {
    pub message: String,
    pub sandbox: Option<SandboxExecutionFact>,
}

fn shell_output_summary(
    terminal_outcome: ProcessTerminalOutcome,
    stdout_len: usize,
    stderr_len: usize,
) -> ProcessRedactedSpawnSummary {
    let (code, summary) = match terminal_outcome {
        ProcessTerminalOutcome::Succeeded => ("completed_success", "shell process completed"),
        ProcessTerminalOutcome::Failed => {
            ("completed_failed", "shell process exited unsuccessfully")
        }
        ProcessTerminalOutcome::Denied => ("denied", "shell process was denied"),
        ProcessTerminalOutcome::ReplaySkipped => ("replay_skipped", "shell process replay skipped"),
        ProcessTerminalOutcome::TimedOut => ("timed_out", "shell process timed out"),
        ProcessTerminalOutcome::Cancelled => ("cancelled", "shell process was cancelled"),
        ProcessTerminalOutcome::Interrupted => ("interrupted", "shell process was interrupted"),
    };
    ProcessRedactedSpawnSummary {
        status: Some(ProcessRedactedStatus {
            code: code.to_owned(),
            summary: summary.to_owned(),
        }),
        stdout: stream_count(ProcessRedactedStreamKind::Stdout, stdout_len),
        stderr: stream_count(ProcessRedactedStreamKind::Stderr, stderr_len),
    }
}

pub(super) fn process_status_summary(code: &str, summary: &str) -> ProcessRedactedSpawnSummary {
    ProcessRedactedSpawnSummary {
        status: Some(ProcessRedactedStatus {
            code: code.to_owned(),
            summary: summary.to_owned(),
        }),
        stdout: ProcessRedactedStreamSummary::empty(ProcessRedactedStreamKind::Stdout),
        stderr: ProcessRedactedStreamSummary::empty(ProcessRedactedStreamKind::Stderr),
    }
}

fn stream_count(
    stream: ProcessRedactedStreamKind,
    byte_count: usize,
) -> ProcessRedactedStreamSummary {
    ProcessRedactedStreamSummary {
        stream,
        byte_count,
        redacted_preview: None,
        evidence_refs: if byte_count == 0 {
            Vec::new()
        } else {
            vec!["exec_process_redacted_stream_count.v1".to_owned()]
        },
    }
}

fn format_controlled_output(
    stdout: &ControlledChildStream,
    stderr: &ControlledChildStream,
    exit_code: Option<i32>,
) -> String {
    let mut parts = Vec::new();
    if !stdout.captured.is_empty() {
        parts.push(stream_text(stdout));
    }
    if !stderr.captured.is_empty() {
        let stderr_text = stream_text(stderr);
        if !stderr_text.trim().is_empty() {
            parts.push(format!("STDERR:\n{stderr_text}"));
        }
    }
    parts.push(format!("\nExit code: {}", exit_code.unwrap_or(-1)));
    truncate_output(parts.join("\n"))
}

fn stream_text(stream: &ControlledChildStream) -> String {
    let text = String::from_utf8_lossy(&stream.captured);
    if stream.truncated {
        format!("{text}\n\n... (output chars truncated) ...")
    } else {
        text.into_owned()
    }
}

fn truncate_output(result: String) -> String {
    let char_count = result.chars().count();
    if char_count <= MAX_OUTPUT {
        return result;
    }
    let half = MAX_OUTPUT / 2;
    let first = result.chars().take(half).collect::<String>();
    let last = result
        .chars()
        .rev()
        .take(half)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{}\n\n... ({} chars truncated) ...\n\n{}",
        first,
        char_count - MAX_OUTPUT,
        last
    )
}
