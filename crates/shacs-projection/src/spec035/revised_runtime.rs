use super::{
    Spec035AdapterScope, Spec035ContainmentGuarantee, Spec035RuntimeControlProjection,
    Spec035SandboxFallback, Spec035SandboxRuntimeState,
};
use crate::{SandboxFallback, SandboxStatus, Spec030RuntimeProjection};

pub(super) fn runtime_controls(
    trusted_runtime: &Spec030RuntimeProjection,
) -> Vec<Spec035RuntimeControlProjection> {
    let sandbox = trusted_runtime.sandbox();
    let Some(sandbox_status) = sandbox_status(sandbox.status) else {
        return Vec::new();
    };
    let fallback = sandbox_fallback(sandbox.fallback);
    if sandbox.applied_adapters.is_empty() {
        return vec![Spec035RuntimeControlProjection {
            adapter: None,
            sandbox_status,
            fallback,
            scope: Spec035AdapterScope::Unavailable,
            containment: Spec035ContainmentGuarantee::NotGuaranteed,
        }];
    }
    sandbox
        .applied_adapters
        .iter()
        .map(|adapter| Spec035RuntimeControlProjection {
            adapter: Some(*adapter),
            sandbox_status,
            fallback,
            scope: Spec035AdapterScope::AdapterScoped,
            containment: Spec035ContainmentGuarantee::NotGuaranteed,
        })
        .collect()
}

const fn sandbox_status(status: SandboxStatus) -> Option<Spec035SandboxRuntimeState> {
    match status {
        SandboxStatus::Active => Some(Spec035SandboxRuntimeState::Active),
        SandboxStatus::Disabled => Some(Spec035SandboxRuntimeState::Disabled),
        SandboxStatus::Unsupported => Some(Spec035SandboxRuntimeState::Unsupported),
        SandboxStatus::Failed => Some(Spec035SandboxRuntimeState::Failed),
        SandboxStatus::Unknown => None,
    }
}

const fn sandbox_fallback(fallback: SandboxFallback) -> Spec035SandboxFallback {
    match fallback {
        SandboxFallback::NotApplicable => Spec035SandboxFallback::NotApplicable,
        SandboxFallback::TrustedNativeFallback => Spec035SandboxFallback::NativeFallback,
        SandboxFallback::ExecutionDenied => Spec035SandboxFallback::ExecutionDenied,
        SandboxFallback::Unknown => Spec035SandboxFallback::Unavailable,
    }
}
