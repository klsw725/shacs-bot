use shacs_projection::{
    DefaultContainment, ExecutionAuthority, LifecycleBoundaryKind, LifecycleBoundaryProjection,
    LifecycleIsolation, OptionalSandboxScope, ProcessAdapterCapabilities, ProcessAdapterProjection,
    ProcessAdapterSupport, ResourceActivation, ResourceCandidateProjection, ResourceKind,
    ResourceLoadStatus, ResourceTrust, SandboxFallback, SandboxFilesystemPolicy,
    SandboxNetworkPolicy, SandboxStatus, SandboxStatusProjection, Spec030Availability,
    Spec030RuntimeProjection, Spec030RuntimeProjectionInput, Spec030RuntimeStatus,
    TrustedProfileStatus, TrustedRuntimeProfile, TrustedRuntimeProfileProjection, WorkspaceTrust,
};

mod local_provider;
mod local_resources;
mod store;
mod store_controlled_child;
mod store_credential;
mod store_resources;
mod store_sandbox;
mod store_startup;
mod types;

pub use local_provider::LocalSpec030ProjectionProvider;
pub use local_resources::populate as populate_local_spec030_facts;
pub use shacs_projection::WorkspaceTrustRemediation;
pub use store::{Spec030FactSnapshot, Spec030FactStore};
pub use types::*;

pub fn build_trusted_runtime_projection(
    input: TrustedRuntimeInput,
) -> Result<Spec030RuntimeProjection, TrustedRuntimeBuildError> {
    let facts = match input {
        TrustedRuntimeInput::Unavailable(reason) => {
            return Ok(Spec030RuntimeProjection::unavailable(reason));
        }
        TrustedRuntimeInput::Available(facts) => *facts,
    };
    let (availability, status) = runtime_status(facts.workspace_trust);

    Spec030RuntimeProjection::try_new(Spec030RuntimeProjectionInput {
        availability,
        status,
        unavailable_reason: None,
        profile: profile(facts.workspace_trust),
        lifecycle_boundaries: lifecycle_boundaries(facts.lifecycle),
        hooks: facts.hooks,
        process_adapters: facts
            .process_adapters
            .into_iter()
            .map(process_adapter)
            .collect(),
        credential: facts.credential,
        sandbox: sandbox(facts.sandbox),
        resources: facts
            .resources
            .into_iter()
            .map(|resource| resource_for_workspace(facts.workspace_trust, resource))
            .collect(),
        disclosure: facts.disclosure,
    })
    .map_err(TrustedRuntimeBuildError::from)
}

const fn runtime_status(
    observation: WorkspaceTrustObservation,
) -> (Spec030Availability, Spec030RuntimeStatus) {
    match observation {
        WorkspaceTrustObservation::Trusted => {
            (Spec030Availability::Available, Spec030RuntimeStatus::Active)
        }
        WorkspaceTrustObservation::Untrusted => (
            Spec030Availability::Degraded,
            Spec030RuntimeStatus::Degraded,
        ),
    }
}

const fn profile(observation: WorkspaceTrustObservation) -> TrustedRuntimeProfileProjection {
    let (availability, workspace_trust, workspace_trust_remediation, resource_trust) =
        match observation {
            WorkspaceTrustObservation::Trusted => (
                Spec030Availability::Available,
                WorkspaceTrust::UserAsserted,
                None,
                ResourceTrust::ExplicitOrTrustedWorkspace,
            ),
            WorkspaceTrustObservation::Untrusted => (
                Spec030Availability::Degraded,
                WorkspaceTrust::NotAsserted,
                Some(WorkspaceTrustRemediation::ReviewAndAssertTrust),
                ResourceTrust::ExplicitOnly,
            ),
        };
    TrustedRuntimeProfileProjection {
        availability,
        status: TrustedProfileStatus::Active,
        profile: TrustedRuntimeProfile::TrustedLocalAgent,
        execution_authority: ExecutionAuthority::CurrentOsUser,
        workspace_trust,
        workspace_trust_remediation,
        resource_trust,
        default_containment: DefaultContainment::None,
        optional_sandbox: OptionalSandboxScope::AdapterScoped,
    }
}

fn resource_for_workspace(
    observation: WorkspaceTrustObservation,
    resource: ResourceCandidateProjection,
) -> ResourceCandidateProjection {
    match observation {
        WorkspaceTrustObservation::Trusted => resource,
        WorkspaceTrustObservation::Untrusted => match resource.activation {
            ResourceActivation::Explicit | ResourceActivation::Inactive => resource,
            ResourceActivation::TrustedWorkspace => match resource.kind {
                ResourceKind::Skill | ResourceKind::Extension | ResourceKind::Package => {
                    let mut resource = resource;
                    resource.load_status = ResourceLoadStatus::Rejected;
                    resource.activation = ResourceActivation::Inactive;
                    resource
                }
                ResourceKind::Prompt | ResourceKind::Context => resource,
            },
        },
    }
}

fn lifecycle_boundaries(observations: LifecycleObservations) -> Vec<LifecycleBoundaryProjection> {
    [
        (
            LifecycleBoundaryKind::DaemonWorker,
            observations.daemon_worker,
        ),
        (LifecycleBoundaryKind::Kernel, observations.kernel),
    ]
    .into_iter()
    .map(|(kind, status)| LifecycleBoundaryProjection {
        kind,
        status,
        isolation: LifecycleIsolation::LifecycleOnly,
    })
    .collect()
}

fn process_adapter(observation: ProcessAdapterObservation) -> ProcessAdapterProjection {
    match observation {
        ProcessAdapterObservation::Supported {
            adapter,
            capabilities,
            recent_outcomes,
            control_scope,
            reason,
        } => ProcessAdapterProjection {
            adapter,
            availability: Spec030Availability::Available,
            support: ProcessAdapterSupport::Supported,
            control_scope,
            reason,
            capabilities,
            recent_outcomes,
        },
        ProcessAdapterObservation::Unsupported {
            adapter,
            control_scope,
            reason,
        } => ProcessAdapterProjection {
            adapter,
            availability: Spec030Availability::Unavailable,
            support: ProcessAdapterSupport::Unsupported,
            control_scope,
            reason,
            capabilities: ProcessAdapterCapabilities {
                timeout: false,
                abort: false,
                cwd: false,
                env: false,
                bounded_output: false,
                descendant_cleanup: false,
                startup_readiness: false,
                generation_fencing: false,
            },
            recent_outcomes: Vec::new(),
        },
    }
}

fn sandbox(observation: SandboxObservation) -> SandboxStatusProjection {
    match observation {
        SandboxObservation::Unknown => SandboxStatusProjection {
            availability: Spec030Availability::Unavailable,
            status: SandboxStatus::Unknown,
            fallback: SandboxFallback::Unknown,
            applied_adapters: Vec::new(),
            filesystem_policy: SandboxFilesystemPolicy::Unknown,
            network_policy: SandboxNetworkPolicy::Unknown,
        },
        SandboxObservation::Disabled => inactive_sandbox(
            Spec030Availability::Available,
            SandboxStatus::Disabled,
            SandboxFallback::TrustedNativeFallback,
        ),
        SandboxObservation::Unsupported => inactive_sandbox(
            Spec030Availability::Unavailable,
            SandboxStatus::Unsupported,
            SandboxFallback::TrustedNativeFallback,
        ),
        SandboxObservation::Failed => inactive_sandbox(
            Spec030Availability::Degraded,
            SandboxStatus::Failed,
            SandboxFallback::ExecutionDenied,
        ),
        SandboxObservation::Inactive { status, fallback } => inactive_sandbox(
            match status {
                SandboxInactiveStatus::Disabled => Spec030Availability::Available,
                SandboxInactiveStatus::Unsupported => Spec030Availability::Unavailable,
                SandboxInactiveStatus::Failed => Spec030Availability::Degraded,
            },
            match status {
                SandboxInactiveStatus::Disabled => SandboxStatus::Disabled,
                SandboxInactiveStatus::Unsupported => SandboxStatus::Unsupported,
                SandboxInactiveStatus::Failed => SandboxStatus::Failed,
            },
            match fallback {
                SandboxInactiveFallback::TrustedNativeFallback => {
                    SandboxFallback::TrustedNativeFallback
                }
                SandboxInactiveFallback::ExecutionDenied => SandboxFallback::ExecutionDenied,
            },
        ),
        SandboxObservation::Active {
            applied_adapters,
            filesystem_policy,
            network_policy,
        } => SandboxStatusProjection {
            availability: Spec030Availability::Available,
            status: SandboxStatus::Active,
            fallback: SandboxFallback::NotApplicable,
            applied_adapters,
            filesystem_policy,
            network_policy,
        },
    }
}

const fn inactive_sandbox(
    availability: Spec030Availability,
    status: SandboxStatus,
    fallback: SandboxFallback,
) -> SandboxStatusProjection {
    SandboxStatusProjection {
        availability,
        status,
        fallback,
        applied_adapters: Vec::new(),
        filesystem_policy: SandboxFilesystemPolicy::NotApplied,
        network_policy: SandboxNetworkPolicy::NotApplied,
    }
}
