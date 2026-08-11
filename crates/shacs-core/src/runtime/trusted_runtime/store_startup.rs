use super::{
    McpTransportOutcome, ProcessAdapterRegistration, Spec030FactStore, Spec030FactStoreError,
};
use shacs_projection::{
    LifecycleBoundaryStatus, ProcessAdapterCapabilities, ProcessAdapterKind, ProcessControlReason,
    ProcessOutcomeProjection, ProcessTerminalOutcome,
};

impl Spec030FactStore {
    pub fn record_daemon_started(&self) -> Result<(), Spec030FactStoreError> {
        self.with_facts(|facts| {
            facts.lifecycle.daemon_worker = LifecycleBoundaryStatus::Active;
        })?;
        self.register_process_adapter(ProcessAdapterRegistration {
            adapter: ProcessAdapterKind::DaemonWorker,
            capabilities: startup_capabilities(true),
            reason: ProcessControlReason::DaemonLifecycleOnly,
        })
    }

    pub fn record_mcp_transport(
        &self,
        outcome: McpTransportOutcome,
    ) -> Result<(), Spec030FactStoreError> {
        self.register_process_adapter(ProcessAdapterRegistration {
            adapter: ProcessAdapterKind::Mcp,
            capabilities: startup_capabilities(false),
            reason: ProcessControlReason::McpTransportOnly,
        })?;
        self.record_process_outcome(
            ProcessAdapterKind::Mcp,
            ProcessOutcomeProjection {
                outcome: match outcome {
                    McpTransportOutcome::Configured => ProcessTerminalOutcome::Unsupported,
                    McpTransportOutcome::Connected => ProcessTerminalOutcome::Succeeded,
                    McpTransportOutcome::Failed => ProcessTerminalOutcome::Failed,
                },
                output_truncated: false,
                duration_ms: None,
            },
        )
    }
}

const fn startup_capabilities(generation_fencing: bool) -> ProcessAdapterCapabilities {
    ProcessAdapterCapabilities {
        timeout: false,
        abort: false,
        cwd: false,
        env: false,
        bounded_output: false,
        descendant_cleanup: false,
        startup_readiness: true,
        generation_fencing,
    }
}
