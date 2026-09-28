use shacs_projection::{
    LifecycleBoundaryKind, LifecycleBoundaryStatus, ProcessAdapterKind, ProcessAdapterSupport,
    ProcessControlScope, ResourceLoadStatus, SandboxFallback, SandboxFilesystemPolicy,
    SandboxNetworkPolicy, SandboxStatus, Spec030Availability, Spec030RuntimeProjection,
    Spec031Availability, Spec031ReadinessComponentKind, Spec031ReadinessObservation,
    Spec031ReasonCode, TraceStatus, TrustedCodeDisclosure,
};

pub(super) fn observations(
    owner: &Spec030RuntimeProjection,
) -> Result<[Spec031ReadinessObservation; 2], String> {
    Ok([
        observation(
            Spec031ReadinessComponentKind::RuntimeControls,
            controls(owner),
        )?,
        observation(
            Spec031ReadinessComponentKind::ResourceDisclosure,
            disclosure(owner),
        )?,
    ])
}

fn observation(
    kind: Spec031ReadinessComponentKind,
    (state, summary): (Spec031Availability, &str),
) -> Result<Spec031ReadinessObservation, String> {
    let code = match state {
        Spec031Availability::Ready => Spec031ReasonCode::Included,
        Spec031Availability::Degraded => Spec031ReasonCode::Degraded,
        Spec031Availability::Blocked => Spec031ReasonCode::Blocked,
        Spec031Availability::Unknown | Spec031Availability::Unavailable => {
            Spec031ReasonCode::Missing
        }
    };
    super::readiness_observation::observation(kind, state, code, summary)
}

fn controls(owner: &Spec030RuntimeProjection) -> (Spec031Availability, &'static str) {
    match owner.availability() {
        Spec030Availability::Unavailable => {
            return (
                Spec031Availability::Unavailable,
                "active runtime owner controls are unavailable",
            )
        }
        Spec030Availability::Unknown => {
            return (
                Spec031Availability::Unknown,
                "active runtime owner controls are unknown",
            )
        }
        Spec030Availability::Available | Spec030Availability::Degraded => {}
    }
    match owner.sandbox().fallback {
        SandboxFallback::ExecutionDenied => {
            return (
                Spec031Availability::Blocked,
                "owner sandbox observation denies required execution",
            )
        }
        SandboxFallback::Unknown => {
            return (
                Spec031Availability::Unknown,
                "owner sandbox execution observation is unknown",
            )
        }
        SandboxFallback::NotApplicable | SandboxFallback::TrustedNativeFallback => {}
    }
    match owner.profile().availability {
        Spec030Availability::Unavailable => {
            return (
                Spec031Availability::Unavailable,
                "owner trusted runtime profile is unavailable",
            )
        }
        Spec030Availability::Unknown => {
            return (
                Spec031Availability::Unknown,
                "owner trusted runtime profile is unknown",
            )
        }
        Spec030Availability::Available | Spec030Availability::Degraded => {}
    }
    if owner.sandbox().status == SandboxStatus::Unknown || owner.process_adapters().is_empty() {
        return (
            Spec031Availability::Unknown,
            "owner execution controls are not observed",
        );
    }
    if owner.sandbox().filesystem_policy == SandboxFilesystemPolicy::Unknown
        || owner.sandbox().network_policy == SandboxNetworkPolicy::Unknown
        || owner.sandbox().applied_adapters.iter().any(|kind| {
            !owner.process_adapters().iter().any(|adapter| {
                adapter.adapter == *kind && adapter.support == ProcessAdapterSupport::Supported
            })
        })
    {
        return (
            Spec031Availability::Unknown,
            "owner applied sandbox scope or policy is not observed",
        );
    }
    let mut limited = owner.availability() == Spec030Availability::Degraded
        || owner.profile().availability == Spec030Availability::Degraded;
    for adapter in owner.process_adapters() {
        match adapter.support {
            ProcessAdapterSupport::Unknown | ProcessAdapterSupport::Unsupported => {}
            ProcessAdapterSupport::Supported => {
                let capabilities = adapter.capabilities;
                limited |= adapter.availability == Spec030Availability::Degraded;
                limited |= match adapter.control_scope {
                    ProcessControlScope::ControlledChild => {
                        !(capabilities.timeout
                            && capabilities.abort
                            && capabilities.cwd
                            && capabilities.env
                            && capabilities.bounded_output
                            && capabilities.descendant_cleanup)
                    }
                    ProcessControlScope::LifecycleOnly
                        if adapter.adapter == ProcessAdapterKind::DaemonWorker =>
                    {
                        match owner
                            .lifecycle_boundaries()
                            .iter()
                            .find(|boundary| boundary.kind == LifecycleBoundaryKind::DaemonWorker)
                            .map(|boundary| boundary.status)
                        {
                            Some(LifecycleBoundaryStatus::Active) => {
                                !(capabilities.startup_readiness && capabilities.generation_fencing)
                            }
                            Some(LifecycleBoundaryStatus::Inactive) => true,
                            Some(LifecycleBoundaryStatus::Unavailable) => {
                                return (
                                    Spec031Availability::Unavailable,
                                    "owner daemon lifecycle observation is unavailable",
                                )
                            }
                            None => {
                                return (
                                    Spec031Availability::Unknown,
                                    "owner daemon lifecycle observation is missing",
                                )
                            }
                        }
                    }
                    ProcessControlScope::LifecycleOnly
                    | ProcessControlScope::TransportOnly
                    | ProcessControlScope::Unsupported => true,
                };
            }
        }
    }
    match owner.sandbox().status {
        SandboxStatus::Unknown => (
            Spec031Availability::Unknown,
            "owner sandbox status is unknown",
        ),
        SandboxStatus::Disabled | SandboxStatus::Unsupported | SandboxStatus::Failed => (
            Spec031Availability::Degraded,
            "owner observed usable trusted native execution without active optional sandbox",
        ),
        SandboxStatus::Active if limited => (
            Spec031Availability::Degraded,
            "owner runtime profile or adapter controls have known limitations",
        ),
        SandboxStatus::Active => (
            Spec031Availability::Ready,
            "owner observed adapter scoped execution controls; not universal isolation",
        ),
    }
}

fn disclosure(owner: &Spec030RuntimeProjection) -> (Spec031Availability, &'static str) {
    match owner.availability() {
        Spec030Availability::Unavailable => {
            return (
                Spec031Availability::Unavailable,
                "active runtime owner resource and data disclosure is unavailable",
            )
        }
        Spec030Availability::Unknown => {
            return (
                Spec031Availability::Unknown,
                "active runtime owner resource and data disclosure is unknown",
            )
        }
        Spec030Availability::Available | Spec030Availability::Degraded => {}
    }
    match owner.disclosure().trace.status {
        TraceStatus::Unavailable => {
            return (
                Spec031Availability::Unavailable,
                "owner trace disclosure observation is unavailable",
            )
        }
        TraceStatus::Disabled | TraceStatus::Preview | TraceStatus::Enabled => {}
    }
    let limited = owner.resources().iter().any(|resource| {
        resource.trusted_code_disclosure == TrustedCodeDisclosure::Required
            || match resource.load_status {
                ResourceLoadStatus::Loaded | ResourceLoadStatus::Candidate => false,
                ResourceLoadStatus::Rejected
                | ResourceLoadStatus::ParseFailed
                | ResourceLoadStatus::Unsupported => true,
            }
    });
    if limited {
        (
            Spec031Availability::Degraded,
            "owner resource loading or trusted code disclosure has limitations",
        )
    } else {
        (
            Spec031Availability::Ready,
            "owner resource and data disclosure is available; not complete redaction",
        )
    }
}
