use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{
    ProcessAdapterKind, ProcessAdapterSupport, ProcessControlReason, ProcessControlScope,
    Spec030ProjectionProvider,
};

#[test]
fn spec030_process_unobserved_adapters_explain_their_actual_control_scope() {
    let projection = LocalSpec030ProjectionProvider::new(Spec030FactStore::new(
        WorkspaceTrustObservation::Trusted,
    ))
    .projection();

    for (adapter, scope, reason) in [
        (
            ProcessAdapterKind::Bash,
            ProcessControlScope::Unsupported,
            ProcessControlReason::BashNotObserved,
        ),
        (
            ProcessAdapterKind::GenericExec,
            ProcessControlScope::Unsupported,
            ProcessControlReason::GenericExecNotObserved,
        ),
        (
            ProcessAdapterKind::CredentialCommand,
            ProcessControlScope::Unsupported,
            ProcessControlReason::CredentialCommandNotUsed,
        ),
        (
            ProcessAdapterKind::PackageOperation,
            ProcessControlScope::Unsupported,
            ProcessControlReason::PackageCommandNotUsed,
        ),
        (
            ProcessAdapterKind::PythonKernel,
            ProcessControlScope::LifecycleOnly,
            ProcessControlReason::PythonKernelNotRegistered,
        ),
        (
            ProcessAdapterKind::DaemonWorker,
            ProcessControlScope::LifecycleOnly,
            ProcessControlReason::DaemonLifecycleOnly,
        ),
        (
            ProcessAdapterKind::Mcp,
            ProcessControlScope::TransportOnly,
            ProcessControlReason::McpTransportOnly,
        ),
    ] {
        let row = projection
            .process_adapters()
            .iter()
            .find(|row| row.adapter == adapter)
            .unwrap_or_else(|| panic!("missing process adapter row: {adapter:?}"));
        assert_eq!(row.support, ProcessAdapterSupport::Unsupported);
        assert_eq!(row.control_scope, scope);
        assert_eq!(row.reason, reason);
    }
}
