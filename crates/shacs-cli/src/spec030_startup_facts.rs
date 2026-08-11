use shacs_core::runtime::trusted_runtime::{
    McpTransportOutcome, Spec030FactStore, Spec030FactStoreError,
};
use shacs_core::tools::McpServerConnectionReport;

pub(crate) fn publish_mcp_reports(
    facts: &Spec030FactStore,
    reports: &[McpServerConnectionReport],
) -> Result<(), Spec030FactStoreError> {
    for report in reports {
        let outcome = if report.connected {
            McpTransportOutcome::Connected
        } else if report.error.is_some() {
            McpTransportOutcome::Failed
        } else {
            McpTransportOutcome::Configured
        };
        facts.record_mcp_transport(outcome)?;
    }
    Ok(())
}

#[cfg(not(test))]
pub(crate) fn publish_daemon_started(
    facts: &Spec030FactStore,
) -> Result<(), Spec030FactStoreError> {
    facts.record_daemon_started()
}
