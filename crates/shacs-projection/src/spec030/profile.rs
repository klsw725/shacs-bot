use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedRuntimeProfileProjection {
    pub availability: Spec030Availability,
    pub status: TrustedProfileStatus,
    pub profile: TrustedRuntimeProfile,
    pub execution_authority: ExecutionAuthority,
    pub workspace_trust: WorkspaceTrust,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_trust_remediation: Option<WorkspaceTrustRemediation>,
    pub resource_trust: ResourceTrust,
    pub default_containment: DefaultContainment,
    pub optional_sandbox: OptionalSandboxScope,
}

impl TrustedRuntimeProfileProjection {
    pub(super) const fn unavailable() -> Self {
        Self {
            availability: Spec030Availability::Unavailable,
            status: TrustedProfileStatus::Unavailable,
            profile: TrustedRuntimeProfile::Unknown,
            execution_authority: ExecutionAuthority::Unknown,
            workspace_trust: WorkspaceTrust::Unknown,
            workspace_trust_remediation: None,
            resource_trust: ResourceTrust::Unknown,
            default_containment: DefaultContainment::Unknown,
            optional_sandbox: OptionalSandboxScope::Unknown,
        }
    }
}

pub(super) fn validate_profile(
    profile: &TrustedRuntimeProfileProjection,
) -> Result<(), Spec030ValidationError> {
    match profile.status {
        TrustedProfileStatus::Active => {
            super::validation::require(
                profile.profile == TrustedRuntimeProfile::TrustedLocalAgent
                    && profile.execution_authority == ExecutionAuthority::CurrentOsUser
                    && profile.default_containment == DefaultContainment::None
                    && profile.optional_sandbox == OptionalSandboxScope::AdapterScoped,
                Spec030ValidationViolation::FalseActiveClaim,
            )?;
            match profile.workspace_trust {
                WorkspaceTrust::UserAsserted => super::validation::require(
                    profile.availability == Spec030Availability::Available
                        && profile.workspace_trust_remediation.is_none()
                        && profile.resource_trust == ResourceTrust::ExplicitOrTrustedWorkspace,
                    Spec030ValidationViolation::InconsistentStatus,
                ),
                WorkspaceTrust::NotAsserted => super::validation::require(
                    profile.availability == Spec030Availability::Degraded
                        && profile.workspace_trust_remediation
                            == Some(WorkspaceTrustRemediation::ReviewAndAssertTrust)
                        && profile.resource_trust == ResourceTrust::ExplicitOnly,
                    Spec030ValidationViolation::MissingEvidence,
                ),
                WorkspaceTrust::Unknown => {
                    super::validation::require(false, Spec030ValidationViolation::MissingEvidence)
                }
            }
        }
        TrustedProfileStatus::Unavailable => super::validation::require(
            profile.availability == Spec030Availability::Unavailable
                && profile.workspace_trust == WorkspaceTrust::Unknown
                && profile.workspace_trust_remediation.is_none(),
            Spec030ValidationViolation::InconsistentStatus,
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleBoundaryProjection {
    pub kind: LifecycleBoundaryKind,
    pub status: LifecycleBoundaryStatus,
    pub isolation: LifecycleIsolation,
}
