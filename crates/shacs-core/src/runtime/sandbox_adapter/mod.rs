mod bwrap;
mod execute;

use crate::controlled_child::{ControlledChildError, ControlledChildReceipt};
use crate::runtime::trusted_runtime::{
    SandboxInactiveFallback, SandboxInactiveStatus, SandboxObservation,
};
use shacs_projection::{ProcessAdapterKind, SandboxFilesystemPolicy, SandboxNetworkPolicy};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

pub use bwrap::sandbox_argv;
pub use execute::execute_bash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Bubblewrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxFallbackPolicy {
    TrustedNativeFallback,
    SandboxRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxNetworkPlan {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxMountPlan {
    pub deny_read: Vec<PathBuf>,
    pub allow_write: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPlan {
    pub backend: SandboxBackend,
    pub fallback: SandboxFallbackPolicy,
    pub mounts: SandboxMountPlan,
    pub network: SandboxNetworkPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxRuntimeStatus {
    Disabled,
    Unsupported,
    Failed,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecutionFact {
    pub status: SandboxRuntimeStatus,
    pub fallback: SandboxFallbackPolicy,
    pub applied_adapter: Option<ProcessAdapterKind>,
    pub filesystem_policy: SandboxFilesystemPolicy,
    pub network_policy: SandboxNetworkPolicy,
    pub wrapped_execution: bool,
    pub reason: Option<String>,
}

impl SandboxExecutionFact {
    pub fn observation(&self) -> SandboxObservation {
        match self.status {
            SandboxRuntimeStatus::Active => SandboxObservation::Active {
                applied_adapters: self.applied_adapter.into_iter().collect(),
                filesystem_policy: self.filesystem_policy,
                network_policy: self.network_policy,
            },
            SandboxRuntimeStatus::Disabled => SandboxObservation::Inactive {
                status: SandboxInactiveStatus::Disabled,
                fallback: inactive_fallback(self.fallback),
            },
            SandboxRuntimeStatus::Unsupported => SandboxObservation::Inactive {
                status: SandboxInactiveStatus::Unsupported,
                fallback: inactive_fallback(self.fallback),
            },
            SandboxRuntimeStatus::Failed => SandboxObservation::Inactive {
                status: SandboxInactiveStatus::Failed,
                fallback: inactive_fallback(self.fallback),
            },
        }
    }

    pub(super) fn failed(fallback: SandboxFallbackPolicy, reason: String) -> Self {
        Self {
            status: SandboxRuntimeStatus::Failed,
            fallback,
            applied_adapter: None,
            filesystem_policy: SandboxFilesystemPolicy::NotApplied,
            network_policy: SandboxNetworkPolicy::NotApplied,
            wrapped_execution: false,
            reason: Some(reason),
        }
    }
}

const fn inactive_fallback(fallback: SandboxFallbackPolicy) -> SandboxInactiveFallback {
    match fallback {
        SandboxFallbackPolicy::TrustedNativeFallback => {
            SandboxInactiveFallback::TrustedNativeFallback
        }
        SandboxFallbackPolicy::SandboxRequired => SandboxInactiveFallback::ExecutionDenied,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecution {
    pub fact: SandboxExecutionFact,
    pub receipt: ControlledChildReceipt,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxExecutionError {
    RequiredUnavailable(SandboxExecutionFact),
    InvalidPlan(SandboxExecutionFact),
    Child(ControlledChildError),
}

impl fmt::Display for SandboxExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiredUnavailable(fact) => match fact.reason.as_deref() {
                Some(reason) => write!(formatter, "required sandbox is unavailable: {reason}"),
                None => formatter.write_str("required sandbox is unavailable"),
            },
            Self::InvalidPlan(fact) => write!(
                formatter,
                "invalid sandbox plan: {}",
                fact.reason.as_deref().unwrap_or("unknown plan error")
            ),
            Self::Child(error) => write!(formatter, "sandbox child failed: {error}"),
        }
    }
}

impl Error for SandboxExecutionError {}

impl SandboxExecutionError {
    pub const fn fact(&self) -> Option<&SandboxExecutionFact> {
        match self {
            Self::RequiredUnavailable(fact) | Self::InvalidPlan(fact) => Some(fact),
            Self::Child(_) => None,
        }
    }
}

impl From<ControlledChildError> for SandboxExecutionError {
    fn from(error: ControlledChildError) -> Self {
        Self::Child(error)
    }
}
