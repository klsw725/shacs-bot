use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, McpTransportOutcome, Spec030FactStore,
    WorkspaceTrustObservation,
};
use shacs_projection::{
    LifecycleBoundaryKind, LifecycleBoundaryStatus, ProcessAdapterKind, ProcessAdapterSupport,
    ProcessTerminalOutcome, Spec030ProjectionProvider,
};

#[test]
fn daemon_startup_marks_lifecycle_and_readiness_as_observed() {
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Untrusted);

    facts
        .record_daemon_started()
        .unwrap_or_else(|error| panic!("daemon fact failed: {error}"));
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();

    assert!(projection.lifecycle_boundaries().iter().any(|boundary| {
        boundary.kind == LifecycleBoundaryKind::DaemonWorker
            && boundary.status == LifecycleBoundaryStatus::Active
    }));
    let daemon = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::DaemonWorker)
        .unwrap_or_else(|| panic!("daemon adapter missing"));
    assert_eq!(daemon.support, ProcessAdapterSupport::Supported);
    assert!(daemon.capabilities.startup_readiness);
    assert!(daemon.capabilities.generation_fencing);
}

#[test]
fn mcp_transport_reports_preserve_configured_connected_and_failed_outcomes() {
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Untrusted);

    for outcome in [
        McpTransportOutcome::Configured,
        McpTransportOutcome::Connected,
        McpTransportOutcome::Failed,
    ] {
        facts
            .record_mcp_transport(outcome)
            .unwrap_or_else(|error| panic!("MCP fact failed: {error}"));
    }
    let projection = LocalSpec030ProjectionProvider::new(facts).projection();

    let mcp = projection
        .process_adapters()
        .iter()
        .find(|adapter| adapter.adapter == ProcessAdapterKind::Mcp)
        .unwrap_or_else(|| panic!("MCP adapter missing"));
    assert_eq!(mcp.support, ProcessAdapterSupport::Supported);
    assert!(mcp.capabilities.startup_readiness);
    assert_eq!(
        mcp.recent_outcomes
            .iter()
            .map(|outcome| outcome.outcome)
            .collect::<Vec<_>>(),
        [
            ProcessTerminalOutcome::Unsupported,
            ProcessTerminalOutcome::Succeeded,
            ProcessTerminalOutcome::Failed,
        ]
    );
}
