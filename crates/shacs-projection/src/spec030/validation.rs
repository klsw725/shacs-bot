use super::*;
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030ValidationViolation {
    FalseActiveClaim,
    FalseSupportedClaim,
    InconsistentStatus,
    MissingEvidence,
    UnsafeResourceActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec030ValidationError {
    violation: Spec030ValidationViolation,
}

impl Spec030ValidationError {
    pub const fn violation(self) -> Spec030ValidationViolation {
        self.violation
    }
}

impl fmt::Display for Spec030ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "inconsistent Spec030 runtime projection")
    }
}

impl Error for Spec030ValidationError {}

pub(super) fn validate_runtime(
    projection: &Spec030RuntimeProjection,
) -> Result<(), Spec030ValidationError> {
    validate_runtime_status(projection)?;
    super::profile::validate_profile(&projection.profile)?;
    validate_hooks(&projection.hooks)?;
    for adapter in &projection.process_adapters {
        validate_process_adapter(adapter)?;
    }
    validate_credential(&projection.credential)?;
    validate_sandbox(&projection.sandbox)?;
    for resource in &projection.resources {
        validate_resource(resource)?;
        validate_workspace_resource(&projection.profile, resource)?;
    }
    super::disclosure::validate_trace(&projection.disclosure.trace)
}

fn validate_runtime_status(
    projection: &Spec030RuntimeProjection,
) -> Result<(), Spec030ValidationError> {
    match projection.status {
        Spec030RuntimeStatus::Active => {
            require(
                projection.availability == Spec030Availability::Available
                    && projection.unavailable_reason.is_none(),
                Spec030ValidationViolation::FalseActiveClaim,
            )?;
        }
        Spec030RuntimeStatus::Degraded => {
            require(
                projection.availability == Spec030Availability::Degraded
                    && projection.unavailable_reason.is_none(),
                Spec030ValidationViolation::InconsistentStatus,
            )?;
        }
        Spec030RuntimeStatus::Unavailable => {
            require(
                projection.availability == Spec030Availability::Unavailable
                    && projection.unavailable_reason.is_some(),
                Spec030ValidationViolation::InconsistentStatus,
            )?;
            require(
                !has_positive_claim(projection),
                Spec030ValidationViolation::FalseActiveClaim,
            )?;
        }
    }
    Ok(())
}

fn has_positive_claim(projection: &Spec030RuntimeProjection) -> bool {
    projection.profile.status == TrustedProfileStatus::Active
        || projection.hooks.status == HookRuntimeStatus::Active
        || projection
            .lifecycle_boundaries
            .iter()
            .any(|boundary| boundary.status == LifecycleBoundaryStatus::Active)
        || projection
            .process_adapters
            .iter()
            .any(|adapter| adapter.support == ProcessAdapterSupport::Supported)
        || matches!(
            projection.credential.status,
            CredentialStatus::Resolved | CredentialStatus::Refreshing
        )
        || projection.sandbox.status == SandboxStatus::Active
        || projection
            .resources
            .iter()
            .any(|resource| resource.load_status == ResourceLoadStatus::Loaded)
}

fn validate_hooks(hooks: &HookRuntimeProjection) -> Result<(), Spec030ValidationError> {
    match hooks.status {
        HookRuntimeStatus::Active => require(
            matches!(
                hooks.availability,
                Spec030Availability::Available | Spec030Availability::Degraded
            ) && hooks.registered_handlers > 0,
            Spec030ValidationViolation::FalseActiveClaim,
        ),
        HookRuntimeStatus::Inactive => Ok(()),
        HookRuntimeStatus::Unavailable => require(
            hooks.availability == Spec030Availability::Unavailable,
            Spec030ValidationViolation::InconsistentStatus,
        ),
    }
}

fn validate_process_adapter(
    adapter: &ProcessAdapterProjection,
) -> Result<(), Spec030ValidationError> {
    match adapter.support {
        ProcessAdapterSupport::Supported => require(
            matches!(
                adapter.availability,
                Spec030Availability::Available | Spec030Availability::Degraded
            ) && adapter.capabilities.any()
                && matches!(
                    (adapter.control_scope, adapter.reason),
                    (
                        ProcessControlScope::ControlledChild,
                        ProcessControlReason::ControlledChildObservedNoRollback
                    ) | (
                        ProcessControlScope::LifecycleOnly,
                        ProcessControlReason::DaemonLifecycleOnly
                    ) | (
                        ProcessControlScope::TransportOnly,
                        ProcessControlReason::McpTransportOnly
                    )
                ),
            Spec030ValidationViolation::FalseSupportedClaim,
        ),
        ProcessAdapterSupport::Unsupported => require(
            adapter.availability == Spec030Availability::Unavailable
                && !adapter.capabilities.any()
                && matches!(
                    adapter.control_scope,
                    ProcessControlScope::Unsupported
                        | ProcessControlScope::LifecycleOnly
                        | ProcessControlScope::TransportOnly
                ),
            Spec030ValidationViolation::FalseSupportedClaim,
        ),
        ProcessAdapterSupport::Unknown => require(
            matches!(
                adapter.availability,
                Spec030Availability::Unknown | Spec030Availability::Unavailable
            ) && !adapter.capabilities.any(),
            Spec030ValidationViolation::FalseSupportedClaim,
        ),
    }
}

fn validate_credential(
    credential: &CredentialStatusProjection,
) -> Result<(), Spec030ValidationError> {
    match credential.status {
        CredentialStatus::Resolved => require(
            credential.availability == Spec030Availability::Available
                && credential.source.is_some(),
            Spec030ValidationViolation::MissingEvidence,
        ),
        CredentialStatus::Unavailable => require(
            credential.availability == Spec030Availability::Unavailable
                && credential.source.is_none(),
            Spec030ValidationViolation::InconsistentStatus,
        ),
        CredentialStatus::Missing
        | CredentialStatus::Expired
        | CredentialStatus::Refreshing
        | CredentialStatus::RefreshFailed => Ok(()),
    }
}

fn validate_sandbox(sandbox: &SandboxStatusProjection) -> Result<(), Spec030ValidationError> {
    match sandbox.status {
        SandboxStatus::Active => require(
            sandbox.availability == Spec030Availability::Available
                && !sandbox.applied_adapters.is_empty()
                && sandbox.fallback == SandboxFallback::NotApplicable
                && sandbox.filesystem_policy != SandboxFilesystemPolicy::NotApplied
                && sandbox.network_policy != SandboxNetworkPolicy::NotApplied,
            Spec030ValidationViolation::FalseActiveClaim,
        ),
        SandboxStatus::Disabled | SandboxStatus::Unsupported | SandboxStatus::Failed => require(
            sandbox.applied_adapters.is_empty()
                && sandbox.fallback != SandboxFallback::NotApplicable,
            Spec030ValidationViolation::InconsistentStatus,
        ),
        SandboxStatus::Unknown => require(
            sandbox.availability == Spec030Availability::Unavailable
                && sandbox.applied_adapters.is_empty(),
            Spec030ValidationViolation::InconsistentStatus,
        ),
    }
}

fn validate_resource(resource: &ResourceCandidateProjection) -> Result<(), Spec030ValidationError> {
    if resource.load_status != ResourceLoadStatus::ParseFailed {
        require(
            resource.content_sha256.as_deref().is_some_and(|digest| {
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }),
            Spec030ValidationViolation::MissingEvidence,
        )?;
    }
    if resource.load_status == ResourceLoadStatus::Loaded && resource.kind.is_executable() {
        return require(
            resource.activation != ResourceActivation::Inactive
                && resource.trusted_code_disclosure == TrustedCodeDisclosure::Shown,
            Spec030ValidationViolation::UnsafeResourceActivation,
        );
    }
    Ok(())
}

fn validate_workspace_resource(
    profile: &TrustedRuntimeProfileProjection,
    resource: &ResourceCandidateProjection,
) -> Result<(), Spec030ValidationError> {
    match profile.workspace_trust {
        WorkspaceTrust::UserAsserted | WorkspaceTrust::Unknown => Ok(()),
        WorkspaceTrust::NotAsserted => match resource.activation {
            ResourceActivation::Explicit | ResourceActivation::Inactive => Ok(()),
            ResourceActivation::TrustedWorkspace => require(
                !resource.kind.is_executable(),
                Spec030ValidationViolation::UnsafeResourceActivation,
            ),
        },
    }
}

pub(super) fn require(
    condition: bool,
    violation: Spec030ValidationViolation,
) -> Result<(), Spec030ValidationError> {
    if condition {
        Ok(())
    } else {
        Err(Spec030ValidationError { violation })
    }
}
