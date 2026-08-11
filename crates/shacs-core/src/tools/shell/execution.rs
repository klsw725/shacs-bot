use super::process::{process_status_summary, run_shell};
use super::{
    ExecTool, ExecToolProcessResult, JsonMap, ToolCallExecutionContext, Value, MAX_TIMEOUT_SECONDS,
};
use crate::runtime::sandbox_adapter::{SandboxBackend, SandboxPlan};
use crate::runtime::{ProcessGate, ProcessSpawnReport, ProcessTerminalOutcome};
use std::time::Duration;

impl ExecTool {
    pub(super) fn execute_command(
        &self,
        command: &str,
        working_dir: Option<&str>,
        timeout_seconds: u64,
        context: &ToolCallExecutionContext,
    ) -> Result<String, String> {
        self.execute_command_with_receipt(command, working_dir, timeout_seconds, context)
            .map(|result| result.output)
    }

    pub fn execute_with_receipt(
        &self,
        params: JsonMap,
        context: &ToolCallExecutionContext,
    ) -> Result<ExecToolProcessResult, String> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if command.trim().is_empty() {
            return Err("Unknown command".to_owned());
        }
        let working_dir = params.get("working_dir").and_then(Value::as_str);
        let timeout = params
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(self.config.timeout_seconds)
            .min(MAX_TIMEOUT_SECONDS);
        self.execute_command_with_receipt(command, working_dir, timeout, context)
    }

    fn execute_command_with_receipt(
        &self,
        command: &str,
        working_dir: Option<&str>,
        timeout_seconds: u64,
        context: &ToolCallExecutionContext,
    ) -> Result<ExecToolProcessResult, String> {
        let cwd = self.resolve_working_dir(working_dir)?;
        if let Some(error) = self.guard_command(command, &cwd) {
            return Err(error);
        }
        let sandbox_plan = match self.config.sandbox.as_deref() {
            None => None,
            Some("bwrap") => Some(SandboxPlan {
                backend: SandboxBackend::Bubblewrap,
                fallback: self.config.sandbox_fallback,
                mounts: self.config.sandbox_mounts.clone(),
                network: self.config.sandbox_network,
            }),
            Some(other) => {
                return Err(format!(
                    "Unknown sandbox backend {other:?}. Available: [\"bwrap\"]"
                ))
            }
        };
        let mut env = self.build_env();
        if let Some(path_append) = &self.config.path_append {
            let path = env.get("PATH").cloned().unwrap_or_default();
            let separator = if cfg!(windows) { ";" } else { ":" };
            env.insert(
                "PATH".to_owned(),
                if path.is_empty() {
                    path_append.clone()
                } else {
                    format!("{path}{separator}{path_append}")
                },
            );
        }
        let gate_input = context
            .process_gate_input
            .clone()
            .ok_or_else(|| "missing runtime process context".to_owned())?;
        let mut user_output = None;
        let mut sandbox_fact = None;
        let mut sandbox_warning = None;
        let mut fact_recording_error = None;
        let receipt = ProcessGate::new()
            .evaluate_and_maybe_spawn(gate_input, |authorization| {
                match run_shell(
                    authorization,
                    command,
                    &cwd,
                    &env,
                    Duration::from_secs(timeout_seconds),
                    context.process_abort.as_ref(),
                    sandbox_plan.as_ref(),
                ) {
                    Ok(result) => {
                        if let Some(error) = self.spec030_facts.as_ref().and_then(|facts| {
                            facts
                                .record_controlled_child_receipt(&result.controlled_child_receipt)
                                .err()
                        }) {
                            fact_recording_error = Some(error.to_string());
                        }
                        if let Some(error) = self
                            .spec030_facts
                            .as_ref()
                            .and_then(|facts| facts.record_sandbox_execution(&result.sandbox).err())
                        {
                            fact_recording_error = Some(error.to_string());
                        }
                        user_output = Some(result.output);
                        sandbox_fact = Some(result.sandbox);
                        sandbox_warning = result.sandbox_warning;
                        ProcessSpawnReport {
                            terminal_outcome: result.terminal_outcome,
                            redacted_summary: result.redacted_summary,
                        }
                    }
                    Err(error) => {
                        if let Some(fact) = error.sandbox.as_ref() {
                            sandbox_fact = Some(fact.clone());
                            if let Some(recording_error) = self
                                .spec030_facts
                                .as_ref()
                                .and_then(|facts| facts.record_sandbox_execution(fact).err())
                            {
                                fact_recording_error = Some(recording_error.to_string());
                            }
                        }
                        user_output = Some(format!("Error executing command: {}", error.message));
                        ProcessSpawnReport {
                            terminal_outcome: ProcessTerminalOutcome::Failed,
                            redacted_summary: process_status_summary(
                                "spawn_failed",
                                "shell process failed before terminal output",
                            ),
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        if let Some(error) = fact_recording_error {
            return Err(format!("Spec030 process fact update failed: {error}"));
        }
        let output = if receipt.dispatch_count == 0 {
            format!(
                "Error: Process launch blocked by permission gate ({:?})",
                receipt.terminal_outcome
            )
        } else {
            user_output.ok_or_else(|| "process gate did not return command output".to_owned())?
        };
        let output = output_with_sandbox_warning(output, sandbox_warning.as_deref());
        Ok(ExecToolProcessResult {
            output,
            receipt,
            sandbox: sandbox_fact,
            sandbox_warning,
        })
    }
}

fn output_with_sandbox_warning(output: String, warning: Option<&str>) -> String {
    match warning {
        Some(warning) => format!("Warning: {warning}\n{output}"),
        None => output,
    }
}

#[cfg(test)]
mod tests {
    use super::output_with_sandbox_warning;

    #[test]
    fn normal_tool_output_keeps_native_fallback_warning() {
        // Given
        let warning = "sandbox inactive; trusted native fallback used";

        // When
        let output = output_with_sandbox_warning("Exit code: 0".to_owned(), Some(warning));

        // Then
        assert_eq!(
            output,
            "Warning: sandbox inactive; trusted native fallback used\nExit code: 0"
        );
    }
}
