use super::{ProcessAdapterRegistration, Spec030FactStore, Spec030FactStoreError};
use crate::controlled_child::{
    ControlledChildAdapter, ControlledChildOutcome, ControlledChildReceipt,
    DescendantCleanupCapability,
};
use shacs_projection::{
    ProcessAdapterCapabilities, ProcessAdapterKind, ProcessControlReason, ProcessOutcomeProjection,
    ProcessTerminalOutcome,
};

impl Spec030FactStore {
    pub fn record_controlled_child_receipt(
        &self,
        receipt: &ControlledChildReceipt,
    ) -> Result<(), Spec030FactStoreError> {
        let adapter = projection_adapter(receipt.adapter)?;
        self.register_process_adapter(ProcessAdapterRegistration {
            adapter,
            capabilities: ProcessAdapterCapabilities {
                timeout: true,
                abort: matches!(
                    receipt.adapter,
                    ControlledChildAdapter::Bash
                        | ControlledChildAdapter::GenericArgv
                        | ControlledChildAdapter::PackageCommand
                ) || receipt.abort_capable,
                cwd: true,
                env: true,
                bounded_output: true,
                descendant_cleanup: matches!(
                    receipt.descendant_cleanup,
                    DescendantCleanupCapability::Supported
                ),
                startup_readiness: false,
                generation_fencing: false,
            },
            reason: ProcessControlReason::ControlledChildObservedNoRollback,
        })?;
        self.record_process_outcome(
            adapter,
            ProcessOutcomeProjection {
                outcome: terminal_outcome(receipt.outcome),
                output_truncated: receipt.stdout.truncated || receipt.stderr.truncated,
                duration_ms: Some(receipt.duration_ms),
            },
        )
    }
}

const fn projection_adapter(
    adapter: ControlledChildAdapter,
) -> Result<ProcessAdapterKind, Spec030FactStoreError> {
    match adapter {
        ControlledChildAdapter::Bash => Ok(ProcessAdapterKind::Bash),
        ControlledChildAdapter::GenericArgv => Ok(ProcessAdapterKind::GenericExec),
        ControlledChildAdapter::CredentialCommand => Ok(ProcessAdapterKind::CredentialCommand),
        ControlledChildAdapter::PackageCommand => Ok(ProcessAdapterKind::PackageOperation),
        ControlledChildAdapter::LoadCheck => Err(
            Spec030FactStoreError::UnprojectedControlledChildAdapter(adapter),
        ),
    }
}

const fn terminal_outcome(outcome: ControlledChildOutcome) -> ProcessTerminalOutcome {
    match outcome {
        ControlledChildOutcome::Succeeded { .. } => ProcessTerminalOutcome::Succeeded,
        ControlledChildOutcome::Failed { .. } => ProcessTerminalOutcome::Failed,
        ControlledChildOutcome::TimedOut => ProcessTerminalOutcome::TimedOut,
        ControlledChildOutcome::Aborted => ProcessTerminalOutcome::Aborted,
        ControlledChildOutcome::InvalidCwd => ProcessTerminalOutcome::InvalidCwd,
    }
}
