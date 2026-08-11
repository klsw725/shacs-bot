use shacs_config::{
    CredentialFingerprintStatus as ConfigCredentialFingerprintStatus,
    CredentialSource as ConfigCredentialSource, CredentialStatus as ConfigCredentialStatus,
    CredentialStatusSnapshot, RefreshSerializationStatus as ConfigRefreshSerializationStatus,
};
use shacs_core::runtime::trusted_resources::{
    ResourceDiagnostic, ResourceDiagnosticKind, ResourceEvidence, ResourceFact,
    TrustedResourceInspection,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, ProcessAdapterRegistration, SandboxInactiveFallback,
    SandboxInactiveStatus, SandboxObservation, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{
    CredentialStatus, DataSurface, HookDenialProjection, HookDenialReason, HookRuntimeProjection,
    HookRuntimeStatus, ProcessAdapterCapabilities, ProcessAdapterKind, ProcessAdapterSupport,
    ProcessOutcomeProjection, ProcessTerminalOutcome, ResourceActivation,
    ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind, ResourceLoadStatus,
    ResourcePrecedence, ResourceSource, SandboxFallback, SandboxStatus, Spec030Availability,
    Spec030ProjectionProvider, TraceDestination, TraceDisclosureProjection, TracePreviewProjection,
    TraceStatus, TrustedCodeDisclosure,
};
use std::error::Error;

fn store() -> Spec030FactStore {
    Spec030FactStore::new(WorkspaceTrustObservation::Trusted)
}

fn capabilities() -> ProcessAdapterCapabilities {
    ProcessAdapterCapabilities {
        timeout: true,
        abort: true,
        cwd: true,
        env: false,
        bounded_output: true,
        descendant_cleanup: true,
        startup_readiness: false,
        generation_fencing: false,
    }
}

#[test]
fn spec030_fact_store_defaults_unregistered_facts_to_non_positive_states() {
    let projection = LocalSpec030ProjectionProvider::new(store()).projection();

    assert!(projection
        .process_adapters()
        .iter()
        .all(|adapter| adapter.support == ProcessAdapterSupport::Unsupported));
    assert_eq!(
        projection.credential().status,
        CredentialStatus::Unavailable
    );
    assert_eq!(projection.sandbox().status, SandboxStatus::Unknown);
    assert_eq!(projection.hooks().status, HookRuntimeStatus::Unavailable);
    assert_eq!(
        projection.disclosure().trace.status,
        TraceStatus::Unavailable
    );
}

#[test]
fn spec030_fact_store_preserves_hook_denial_and_process_terminal_outcomes(
) -> Result<(), Box<dyn Error>> {
    let store = store();
    store.update_hooks(HookRuntimeProjection {
        availability: Spec030Availability::Available,
        status: HookRuntimeStatus::Active,
        registered_handlers: 1,
        diagnostics: Vec::new(),
        recent_denials: vec![HookDenialProjection {
            hook_ref: "hook:guard".to_owned(),
            call_ref: "call:7".to_owned(),
            reason: HookDenialReason::UserDenied,
        }],
    })?;
    store.register_process_adapter(ProcessAdapterRegistration {
        adapter: ProcessAdapterKind::Bash,
        capabilities: capabilities(),
        reason: shacs_projection::ProcessControlReason::ControlledChildObservedNoRollback,
    })?;
    for outcome in [
        ProcessTerminalOutcome::TimedOut,
        ProcessTerminalOutcome::Aborted,
    ] {
        store.record_process_outcome(
            ProcessAdapterKind::Bash,
            ProcessOutcomeProjection {
                outcome,
                output_truncated: true,
                duration_ms: Some(25),
            },
        )?;
    }

    let projection = LocalSpec030ProjectionProvider::new(store).projection();

    assert_eq!(
        projection.hooks().recent_denials[0].reason,
        HookDenialReason::UserDenied
    );
    assert_eq!(projection.process_adapters()[0].recent_outcomes.len(), 2);
    assert_eq!(
        projection.process_adapters()[0].recent_outcomes[1].outcome,
        ProcessTerminalOutcome::Aborted
    );
    Ok(())
}

#[test]
fn spec030_fact_store_tracks_credential_missing_then_resolved() -> Result<(), Box<dyn Error>> {
    let store = store();
    store.record_credential_status(CredentialStatusSnapshot {
        status: ConfigCredentialStatus::Missing,
        source: None,
        fingerprint: ConfigCredentialFingerprintStatus::Unavailable,
        fingerprint_stale: false,
        refresh_serialization: ConfigRefreshSerializationStatus::Inactive,
    })?;
    assert_eq!(
        LocalSpec030ProjectionProvider::new(store.clone())
            .projection()
            .credential()
            .status,
        CredentialStatus::Missing
    );

    store.record_credential_status(CredentialStatusSnapshot {
        status: ConfigCredentialStatus::Resolved,
        source: Some(ConfigCredentialSource::Environment),
        fingerprint: ConfigCredentialFingerprintStatus::Current,
        fingerprint_stale: false,
        refresh_serialization: ConfigRefreshSerializationStatus::Active,
    })?;

    assert_eq!(
        LocalSpec030ProjectionProvider::new(store)
            .projection()
            .credential()
            .status,
        CredentialStatus::Resolved
    );
    Ok(())
}

#[test]
fn spec030_fact_store_tracks_sandbox_resource_collision_and_trace_preview(
) -> Result<(), Box<dyn Error>> {
    let store = store();
    store.update_sandbox(SandboxObservation::Inactive {
        status: SandboxInactiveStatus::Failed,
        fallback: SandboxInactiveFallback::TrustedNativeFallback,
    })?;
    store.record_resource_inspection(&TrustedResourceInspection {
        resources: vec![
            resource_fact(ResourceCollisionStatus::Winner),
            resource_fact(ResourceCollisionStatus::Loser),
        ],
        diagnostics: vec![ResourceDiagnostic {
            resource_ref: "extension:collision".to_owned(),
            kind: ResourceDiagnosticKind::CollisionLoser,
            path: Some("/raw/diagnostic/path".to_owned()),
            reason: "RAW_RESOURCE_REASON".to_owned(),
        }],
    })?;
    store.update_trace(
        shacs_core::runtime::trusted_runtime::TraceDisclosureUpdate {
            raw_content_possible: true,
            surfaces: vec![DataSurface::Session, DataSurface::Trace],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Preview,
                preview: Some(TracePreviewProjection {
                    record_count: 3,
                    approximate_bytes: 144,
                    destination: TraceDestination::LocalOnly,
                    exporter: None,
                    endpoint_summary: None,
                }),
            },
        },
    )?;

    let projection = LocalSpec030ProjectionProvider::new(store).projection();

    assert_eq!(projection.sandbox().status, SandboxStatus::Failed);
    assert_eq!(
        projection.sandbox().fallback,
        SandboxFallback::TrustedNativeFallback
    );
    assert_eq!(
        projection.resources()[1].collision,
        ResourceCollisionStatus::Loser
    );
    assert_eq!(projection.disclosure().trace.status, TraceStatus::Preview);
    let serialized = serde_json::to_string(&projection)?;
    assert!(serialized.contains("RAW_RESOURCE_REASON"));
    Ok(())
}

fn resource_fact(collision: ResourceCollisionStatus) -> ResourceFact {
    ResourceFact {
        projection: ResourceCandidateProjection {
            resource_ref: "extension:collision".to_owned(),
            kind: ResourceKind::Extension,
            source: ResourceSource::Explicit,
            precedence: ResourcePrecedence::Explicit,
            canonical_path: "/workspace/extension".to_owned(),
            content_sha256: Some("0".repeat(64)),
            collision,
            load_status: match collision {
                ResourceCollisionStatus::None | ResourceCollisionStatus::Winner => {
                    ResourceLoadStatus::Loaded
                }
                ResourceCollisionStatus::Loser => ResourceLoadStatus::Rejected,
            },
            activation: match collision {
                ResourceCollisionStatus::None | ResourceCollisionStatus::Winner => {
                    ResourceActivation::Explicit
                }
                ResourceCollisionStatus::Loser => ResourceActivation::Inactive,
            },
            trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            diagnostics: Vec::new(),
        },
        receipt: None,
        authorization: ResourceEvidence::NotProvided,
        sandbox: ResourceEvidence::NotProvided,
    }
}
