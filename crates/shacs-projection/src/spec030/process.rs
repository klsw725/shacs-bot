use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessControlScope {
    ControlledChild,
    Unsupported,
    LifecycleOnly,
    TransportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessControlReason {
    ControlledChildObservedNoRollback,
    BashNotObserved,
    GenericExecNotObserved,
    CredentialCommandNotUsed,
    PackageCommandNotUsed,
    PythonKernelNotRegistered,
    DaemonLifecycleOnly,
    McpTransportOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessAdapterProjection {
    pub adapter: ProcessAdapterKind,
    pub availability: Spec030Availability,
    pub support: ProcessAdapterSupport,
    pub control_scope: ProcessControlScope,
    pub reason: ProcessControlReason,
    pub capabilities: ProcessAdapterCapabilities,
    pub recent_outcomes: Vec<ProcessOutcomeProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessAdapterCapabilities {
    pub timeout: bool,
    pub abort: bool,
    pub cwd: bool,
    pub env: bool,
    pub bounded_output: bool,
    pub descendant_cleanup: bool,
    pub startup_readiness: bool,
    pub generation_fencing: bool,
}

impl ProcessAdapterCapabilities {
    pub(super) const fn any(self) -> bool {
        self.timeout
            || self.abort
            || self.cwd
            || self.env
            || self.bounded_output
            || self.descendant_cleanup
            || self.startup_readiness
            || self.generation_fencing
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessOutcomeProjection {
    pub outcome: ProcessTerminalOutcome,
    pub output_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}
