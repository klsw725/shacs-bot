use super::{
    LifecycleObservations, ProcessAdapterObservation, ProcessAdapterRegistration,
    SandboxObservation, Spec030FactStoreError, TraceDisclosureUpdate, TrustedRuntimeInput,
    TrustedRuntimeOwnerFacts, WorkspaceTrustObservation,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialStatus, CredentialStatusProjection,
    DataDisclosureProjection, DataSurface, HookRuntimeProjection, HookRuntimeStatus,
    LifecycleBoundaryStatus, ProcessAdapterKind, ProcessOutcomeProjection,
    RefreshSerializationStatus, ResourceCandidateProjection, Spec030Availability,
    Spec030UnavailableReason, TraceDisclosureProjection, TraceStatus,
};
use std::sync::{Arc, Mutex, MutexGuard};

const MAX_PROCESS_OUTCOMES: usize = 16;

#[derive(Debug, Clone)]
pub struct Spec030FactStore {
    state: Arc<Mutex<TrustedRuntimeInput>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec030FactSnapshot {
    input: TrustedRuntimeInput,
}

impl Spec030FactSnapshot {
    pub fn into_input(self) -> TrustedRuntimeInput {
        self.input
    }
}

impl Spec030FactStore {
    pub fn new(workspace_trust: WorkspaceTrustObservation) -> Self {
        Self {
            state: Arc::new(Mutex::new(TrustedRuntimeInput::Available(Box::new(
                initial_facts(workspace_trust),
            )))),
        }
    }

    pub fn unavailable(reason: Spec030UnavailableReason) -> Self {
        Self {
            state: Arc::new(Mutex::new(TrustedRuntimeInput::Unavailable(reason))),
        }
    }

    pub fn snapshot(&self) -> Spec030FactSnapshot {
        Spec030FactSnapshot {
            input: recover_lock(&self.state).clone(),
        }
    }

    pub fn update_hooks(&self, hooks: HookRuntimeProjection) -> Result<(), Spec030FactStoreError> {
        self.with_facts(|facts| facts.hooks = hooks)
    }

    pub(crate) fn publish_hooks(&self, hooks: HookRuntimeProjection) {
        let mut state = recover_lock(&self.state);
        match &mut *state {
            TrustedRuntimeInput::Available(facts) => facts.hooks = hooks,
            TrustedRuntimeInput::Unavailable(_) => {}
        }
    }

    pub(crate) fn hook_projection(&self) -> Option<HookRuntimeProjection> {
        match &*recover_lock(&self.state) {
            TrustedRuntimeInput::Available(facts) => Some(facts.hooks.clone()),
            TrustedRuntimeInput::Unavailable(_) => None,
        }
    }

    pub fn register_process_adapter(
        &self,
        registration: ProcessAdapterRegistration,
    ) -> Result<(), Spec030FactStoreError> {
        if !capabilities_present(registration.capabilities) {
            return Err(Spec030FactStoreError::EmptyProcessCapabilities(
                registration.adapter,
            ));
        }
        self.with_facts(|facts| {
            let observation = facts
                .process_adapters
                .iter_mut()
                .find(|observation| adapter_kind(observation) == registration.adapter);
            let recent_outcomes =
                observation.map_or_else(Vec::new, |observation| match observation {
                    ProcessAdapterObservation::Supported {
                        recent_outcomes, ..
                    } => std::mem::take(recent_outcomes),
                    ProcessAdapterObservation::Unsupported { .. } => Vec::new(),
                });
            let registered = ProcessAdapterObservation::supported(
                registration.adapter,
                registration.capabilities,
                recent_outcomes,
                registration.reason,
            );
            if let Some(observation) = facts
                .process_adapters
                .iter_mut()
                .find(|observation| adapter_kind(observation) == registration.adapter)
            {
                *observation = registered;
            } else {
                facts.process_adapters.push(registered);
            }
        })
    }

    pub fn record_process_outcome(
        &self,
        adapter: ProcessAdapterKind,
        outcome: ProcessOutcomeProjection,
    ) -> Result<(), Spec030FactStoreError> {
        let mut state = recover_lock(&self.state);
        let facts = available_facts(&mut state)?;
        let observation = facts
            .process_adapters
            .iter_mut()
            .find(|observation| adapter_kind(observation) == adapter)
            .ok_or(Spec030FactStoreError::UnregisteredProcessAdapter(adapter))?;
        match observation {
            ProcessAdapterObservation::Supported {
                recent_outcomes, ..
            } => {
                if recent_outcomes.len() == MAX_PROCESS_OUTCOMES {
                    recent_outcomes.remove(0);
                }
                recent_outcomes.push(outcome);
                Ok(())
            }
            ProcessAdapterObservation::Unsupported { .. } => {
                Err(Spec030FactStoreError::UnregisteredProcessAdapter(adapter))
            }
        }
    }

    pub fn update_credential(
        &self,
        credential: CredentialStatusProjection,
    ) -> Result<(), Spec030FactStoreError> {
        self.with_facts(|facts| facts.credential = credential)
    }

    pub fn update_sandbox(&self, sandbox: SandboxObservation) -> Result<(), Spec030FactStoreError> {
        self.with_facts(|facts| facts.sandbox = sandbox)
    }

    pub fn update_resources(
        &self,
        resources: Vec<ResourceCandidateProjection>,
    ) -> Result<(), Spec030FactStoreError> {
        self.with_facts(|facts| facts.resources = resources)
    }

    pub fn update_trace(&self, update: TraceDisclosureUpdate) -> Result<(), Spec030FactStoreError> {
        self.with_facts(|facts| {
            facts.disclosure = DataDisclosureProjection {
                raw_content_possible: update.raw_content_possible,
                surfaces: update.surfaces,
                trace: update.trace,
            };
        })
    }

    pub(super) fn with_facts(
        &self,
        update: impl FnOnce(&mut TrustedRuntimeOwnerFacts),
    ) -> Result<(), Spec030FactStoreError> {
        let mut state = recover_lock(&self.state);
        update(available_facts(&mut state)?);
        Ok(())
    }
}

fn available_facts(
    input: &mut TrustedRuntimeInput,
) -> Result<&mut TrustedRuntimeOwnerFacts, Spec030FactStoreError> {
    match input {
        TrustedRuntimeInput::Available(facts) => Ok(facts),
        TrustedRuntimeInput::Unavailable(_) => Err(Spec030FactStoreError::OwnerUnavailable),
    }
}

fn initial_facts(workspace_trust: WorkspaceTrustObservation) -> TrustedRuntimeOwnerFacts {
    TrustedRuntimeOwnerFacts {
        workspace_trust,
        lifecycle: LifecycleObservations {
            daemon_worker: LifecycleBoundaryStatus::Unavailable,
            kernel: LifecycleBoundaryStatus::Unavailable,
        },
        hooks: HookRuntimeProjection {
            availability: Spec030Availability::Unavailable,
            status: HookRuntimeStatus::Unavailable,
            registered_handlers: 0,
            diagnostics: Vec::new(),
            recent_denials: Vec::new(),
        },
        process_adapters: all_adapters()
            .into_iter()
            .map(ProcessAdapterObservation::unsupported)
            .collect(),
        credential: CredentialStatusProjection {
            availability: Spec030Availability::Unavailable,
            status: CredentialStatus::Unavailable,
            source: None,
            fingerprint: CredentialFingerprintStatus::Unavailable,
            refresh_serialization: RefreshSerializationStatus::Unavailable,
        },
        sandbox: SandboxObservation::Unknown,
        resources: Vec::new(),
        disclosure: DataDisclosureProjection {
            raw_content_possible: true,
            surfaces: vec![
                DataSurface::Session,
                DataSurface::Log,
                DataSurface::Trace,
                DataSurface::ToolOutput,
                DataSurface::ExtensionData,
            ],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Unavailable,
                preview: None,
            },
        },
    }
}

fn all_adapters() -> [ProcessAdapterKind; 7] {
    [
        ProcessAdapterKind::Bash,
        ProcessAdapterKind::GenericExec,
        ProcessAdapterKind::CredentialCommand,
        ProcessAdapterKind::PackageOperation,
        ProcessAdapterKind::PythonKernel,
        ProcessAdapterKind::DaemonWorker,
        ProcessAdapterKind::Mcp,
    ]
}

const fn adapter_kind(observation: &ProcessAdapterObservation) -> ProcessAdapterKind {
    match observation {
        ProcessAdapterObservation::Supported { adapter, .. }
        | ProcessAdapterObservation::Unsupported { adapter, .. } => *adapter,
    }
}

const fn capabilities_present(capabilities: shacs_projection::ProcessAdapterCapabilities) -> bool {
    capabilities.timeout
        || capabilities.abort
        || capabilities.cwd
        || capabilities.env
        || capabilities.bounded_output
        || capabilities.descendant_cleanup
        || capabilities.startup_readiness
        || capabilities.generation_fencing
}

fn recover_lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
