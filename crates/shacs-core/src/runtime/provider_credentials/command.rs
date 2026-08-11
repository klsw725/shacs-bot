use super::ProviderCredentialRuntime;
use crate::controlled_child::{
    run_configured_credential_command, ControlledChildAbort, ControlledChildCommand,
    ControlledChildOutcome,
};
use shacs_config::{CommandCredentialInput, CommandCredentialOutcome};

impl ProviderCredentialRuntime {
    pub(super) fn command_input(
        &self,
        provider_id: &str,
        command: &str,
        abort: &ControlledChildAbort,
    ) -> Result<CommandCredentialInput, shacs_providers::ProviderError> {
        if let Some(cached) = self
            .command_cache
            .lock()
            .map_err(|_| runtime_error("credential command cache lock failed"))?
            .get(provider_id)
            .cloned()
        {
            return Ok(CommandCredentialInput::cached(cached));
        }
        let argv = if cfg!(windows) {
            vec!["cmd.exe", "/c", command]
        } else {
            vec!["/bin/bash", "-l", "-c", command]
        };
        let child = ControlledChildCommand::new(argv, &self.cwd, self.command_timeout);
        let receipt = run_configured_credential_command(&child, abort)
            .map_err(|_| runtime_error("credential command execution failed"))?;
        self.facts
            .record_controlled_child_receipt(&receipt)
            .map_err(|_| runtime_error("credential command fact update failed"))?;
        let outcome = match receipt.outcome {
            ControlledChildOutcome::Succeeded { .. } if !receipt.stdout.truncated => {
                CommandCredentialOutcome::Succeeded
            }
            ControlledChildOutcome::TimedOut => CommandCredentialOutcome::TimedOut,
            ControlledChildOutcome::Aborted => CommandCredentialOutcome::Aborted,
            ControlledChildOutcome::Succeeded { .. }
            | ControlledChildOutcome::Failed { .. }
            | ControlledChildOutcome::InvalidCwd => CommandCredentialOutcome::NonZero,
        };
        Ok(CommandCredentialInput::result(
            outcome,
            String::from_utf8_lossy(&receipt.stdout.captured),
        ))
    }
}

pub(super) fn runtime_error(message: &str) -> shacs_providers::ProviderError {
    shacs_providers::ProviderError::Api {
        status: None,
        message: message.to_owned(),
        retryable: false,
        headers: std::collections::BTreeMap::new(),
        body: None,
    }
}
