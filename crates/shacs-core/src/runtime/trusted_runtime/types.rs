use crate::controlled_child::ControlledChildAdapter;
use shacs_projection::{
    CredentialStatusProjection, DataDisclosureProjection, HookRuntimeProjection,
    LifecycleBoundaryStatus, ProcessAdapterCapabilities, ProcessAdapterKind, ProcessControlReason,
    ProcessControlScope, ProcessOutcomeProjection, ResourceCandidateProjection,
    SandboxFilesystemPolicy, SandboxNetworkPolicy, Spec030UnavailableReason,
    Spec030ValidationError, Spec030ValidationViolation,
};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedRuntimeInput {
    Available(Box<TrustedRuntimeOwnerFacts>),
    Unavailable(Spec030UnavailableReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedRuntimeOwnerFacts {
    pub workspace_trust: WorkspaceTrustObservation,
    pub lifecycle: LifecycleObservations,
    pub hooks: HookRuntimeProjection,
    pub process_adapters: Vec<ProcessAdapterObservation>,
    pub credential: CredentialStatusProjection,
    pub sandbox: SandboxObservation,
    pub resources: Vec<ResourceCandidateProjection>,
    pub disclosure: DataDisclosureProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustObservation {
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleObservations {
    pub daemon_worker: LifecycleBoundaryStatus,
    pub kernel: LifecycleBoundaryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessAdapterObservation {
    Supported {
        adapter: ProcessAdapterKind,
        capabilities: ProcessAdapterCapabilities,
        recent_outcomes: Vec<ProcessOutcomeProjection>,
        control_scope: ProcessControlScope,
        reason: ProcessControlReason,
    },
    Unsupported {
        adapter: ProcessAdapterKind,
        control_scope: ProcessControlScope,
        reason: ProcessControlReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessAdapterRegistration {
    pub adapter: ProcessAdapterKind,
    pub capabilities: ProcessAdapterCapabilities,
    pub reason: ProcessControlReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceDisclosureUpdate {
    pub raw_content_possible: bool,
    pub surfaces: Vec<shacs_projection::DataSurface>,
    pub trace: shacs_projection::TraceDisclosureProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransportOutcome {
    Configured,
    Connected,
    Failed,
}

impl ProcessAdapterObservation {
    pub fn supported(
        adapter: ProcessAdapterKind,
        capabilities: ProcessAdapterCapabilities,
        recent_outcomes: Vec<ProcessOutcomeProjection>,
        reason: ProcessControlReason,
    ) -> Self {
        let control_scope = match reason {
            ProcessControlReason::ControlledChildObservedNoRollback => {
                ProcessControlScope::ControlledChild
            }
            ProcessControlReason::DaemonLifecycleOnly => ProcessControlScope::LifecycleOnly,
            ProcessControlReason::McpTransportOnly => ProcessControlScope::TransportOnly,
            ProcessControlReason::BashNotObserved
            | ProcessControlReason::GenericExecNotObserved
            | ProcessControlReason::CredentialCommandNotUsed
            | ProcessControlReason::PackageCommandNotUsed
            | ProcessControlReason::PythonKernelNotRegistered => ProcessControlScope::Unsupported,
        };
        Self::Supported {
            adapter,
            capabilities,
            recent_outcomes,
            control_scope,
            reason,
        }
    }

    pub fn unsupported(adapter: ProcessAdapterKind) -> Self {
        let (control_scope, reason) = unsupported_process_fact(adapter);
        Self::Unsupported {
            adapter,
            control_scope,
            reason,
        }
    }
}

const fn unsupported_process_fact(
    adapter: ProcessAdapterKind,
) -> (ProcessControlScope, ProcessControlReason) {
    match adapter {
        ProcessAdapterKind::Bash => (
            ProcessControlScope::Unsupported,
            ProcessControlReason::BashNotObserved,
        ),
        ProcessAdapterKind::GenericExec => (
            ProcessControlScope::Unsupported,
            ProcessControlReason::GenericExecNotObserved,
        ),
        ProcessAdapterKind::CredentialCommand => (
            ProcessControlScope::Unsupported,
            ProcessControlReason::CredentialCommandNotUsed,
        ),
        ProcessAdapterKind::PackageOperation => (
            ProcessControlScope::Unsupported,
            ProcessControlReason::PackageCommandNotUsed,
        ),
        ProcessAdapterKind::PythonKernel => (
            ProcessControlScope::LifecycleOnly,
            ProcessControlReason::PythonKernelNotRegistered,
        ),
        ProcessAdapterKind::DaemonWorker => (
            ProcessControlScope::LifecycleOnly,
            ProcessControlReason::DaemonLifecycleOnly,
        ),
        ProcessAdapterKind::Mcp => (
            ProcessControlScope::TransportOnly,
            ProcessControlReason::McpTransportOnly,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxObservation {
    Unknown,
    Disabled,
    Unsupported,
    Failed,
    Inactive {
        status: SandboxInactiveStatus,
        fallback: SandboxInactiveFallback,
    },
    Active {
        applied_adapters: Vec<ProcessAdapterKind>,
        filesystem_policy: SandboxFilesystemPolicy,
        network_policy: SandboxNetworkPolicy,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxInactiveStatus {
    Disabled,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxInactiveFallback {
    TrustedNativeFallback,
    ExecutionDenied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustedRuntimeBuildError {
    InvalidProjection(Spec030ValidationViolation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030FactStoreError {
    OwnerUnavailable,
    EmptyProcessCapabilities(ProcessAdapterKind),
    UnregisteredProcessAdapter(ProcessAdapterKind),
    UnprojectedControlledChildAdapter(ControlledChildAdapter),
}

impl fmt::Display for Spec030FactStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerUnavailable => formatter.write_str("Spec030 owner facts are unavailable"),
            Self::EmptyProcessCapabilities(adapter) => {
                write!(
                    formatter,
                    "process adapter has no capabilities: {adapter:?}"
                )
            }
            Self::UnregisteredProcessAdapter(adapter) => {
                write!(formatter, "process adapter is not registered: {adapter:?}")
            }
            Self::UnprojectedControlledChildAdapter(adapter) => {
                write!(
                    formatter,
                    "controlled child adapter is not projected: {adapter:?}"
                )
            }
        }
    }
}

impl Error for Spec030FactStoreError {}

impl fmt::Display for TrustedRuntimeBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProjection(_) => {
                formatter.write_str("invalid trusted runtime owner facts")
            }
        }
    }
}

impl Error for TrustedRuntimeBuildError {}

impl From<Spec030ValidationError> for TrustedRuntimeBuildError {
    fn from(error: Spec030ValidationError) -> Self {
        Self::InvalidProjection(error.violation())
    }
}
