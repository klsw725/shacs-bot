use shacs_core::runtime::trusted_runtime::{
    build_trusted_runtime_projection, LifecycleObservations, ProcessAdapterObservation,
    SandboxObservation, TrustedRuntimeInput, TrustedRuntimeOwnerFacts, WorkspaceTrustObservation,
    WorkspaceTrustRemediation,
};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, CredentialStatusProjection,
    DataDisclosureProjection, DataSurface, HookRuntimeProjection, HookRuntimeStatus,
    LifecycleBoundaryKind, LifecycleBoundaryStatus, LifecycleIsolation, ProcessAdapterCapabilities,
    ProcessAdapterKind, RefreshSerializationStatus, ResourceActivation,
    ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind, ResourceLoadStatus,
    ResourcePrecedence, ResourceSource, SandboxFallback, SandboxFilesystemPolicy,
    SandboxNetworkPolicy, SandboxStatus, Spec030Availability, Spec030RuntimeStatus,
    Spec030UnavailableReason, TraceDestination, TraceDisclosureProjection, TracePreviewProjection,
    TraceStatus, TrustedCodeDisclosure, TrustedRuntimeProfile, WorkspaceTrust,
};
use std::error::Error;

fn active_owner_facts() -> TrustedRuntimeOwnerFacts {
    TrustedRuntimeOwnerFacts {
        workspace_trust: WorkspaceTrustObservation::Trusted,
        lifecycle: LifecycleObservations {
            daemon_worker: LifecycleBoundaryStatus::Active,
            kernel: LifecycleBoundaryStatus::Unavailable,
        },
        hooks: HookRuntimeProjection {
            availability: Spec030Availability::Available,
            status: HookRuntimeStatus::Active,
            registered_handlers: 2,
            diagnostics: Vec::new(),
            recent_denials: Vec::new(),
        },
        process_adapters: vec![ProcessAdapterObservation::supported(
            ProcessAdapterKind::Bash,
            ProcessAdapterCapabilities {
                timeout: true,
                abort: true,
                cwd: true,
                env: true,
                bounded_output: true,
                descendant_cleanup: true,
                startup_readiness: false,
                generation_fencing: false,
            },
            Vec::new(),
            shacs_projection::ProcessControlReason::ControlledChildObservedNoRollback,
        )],
        credential: CredentialStatusProjection {
            availability: Spec030Availability::Available,
            status: CredentialStatus::Resolved,
            source: Some(CredentialSource::LocalAuthStore),
            fingerprint: CredentialFingerprintStatus::Current,
            refresh_serialization: RefreshSerializationStatus::Active,
        },
        sandbox: SandboxObservation::Disabled,
        resources: vec![ResourceCandidateProjection {
            resource_ref: "skill:reviewed".to_owned(),
            kind: ResourceKind::Skill,
            source: ResourceSource::Project,
            precedence: ResourcePrecedence::TrustedProjectAuto,
            canonical_path: "/workspace/skills/reviewed/SKILL.md".to_owned(),
            content_sha256: Some("0".repeat(64)),
            collision: ResourceCollisionStatus::None,
            load_status: ResourceLoadStatus::Loaded,
            activation: ResourceActivation::TrustedWorkspace,
            trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            diagnostics: Vec::new(),
        }],
        disclosure: DataDisclosureProjection {
            raw_content_possible: true,
            surfaces: vec![DataSurface::Session, DataSurface::ToolOutput],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Enabled,
                preview: Some(TracePreviewProjection {
                    record_count: 4,
                    approximate_bytes: 512,
                    destination: TraceDestination::LocalOnly,
                    exporter: None,
                    endpoint_summary: None,
                }),
            },
        },
    }
}

#[test]
fn spec030_trusted_runtime_builds_active_projection_from_owner_facts() -> Result<(), Box<dyn Error>>
{
    let projection = build_trusted_runtime_projection(TrustedRuntimeInput::Available(Box::new(
        active_owner_facts(),
    )))?;

    assert_eq!(projection.status(), Spec030RuntimeStatus::Active);
    assert_eq!(
        projection.profile().profile,
        TrustedRuntimeProfile::TrustedLocalAgent
    );
    assert_eq!(
        projection.profile().workspace_trust,
        WorkspaceTrust::UserAsserted
    );
    assert_eq!(projection.hooks().registered_handlers, 2);
    assert_eq!(projection.resources().len(), 1);
    Ok(())
}

#[test]
fn spec030_trusted_runtime_builds_unavailable_projection_when_owner_facts_are_missing(
) -> Result<(), Box<dyn Error>> {
    let projection = build_trusted_runtime_projection(TrustedRuntimeInput::Unavailable(
        Spec030UnavailableReason::OwnerFactsMissing,
    ))?;

    assert_eq!(projection.status(), Spec030RuntimeStatus::Unavailable);
    assert_eq!(
        projection.unavailable_reason(),
        Some(Spec030UnavailableReason::OwnerFactsMissing)
    );
    assert!(projection.process_adapters().is_empty());
    Ok(())
}

#[test]
fn spec030_trusted_runtime_projects_untrusted_workspace_with_remediation(
) -> Result<(), Box<dyn Error>> {
    let mut facts = active_owner_facts();
    facts.workspace_trust = WorkspaceTrustObservation::Untrusted;

    let projection =
        build_trusted_runtime_projection(TrustedRuntimeInput::Available(Box::new(facts)))?;
    let serialized = serde_json::to_string(&projection)?;

    assert_eq!(projection.status(), Spec030RuntimeStatus::Degraded);
    assert_eq!(
        projection.profile().workspace_trust,
        WorkspaceTrust::NotAsserted
    );
    assert_eq!(
        projection.profile().workspace_trust_remediation,
        Some(WorkspaceTrustRemediation::ReviewAndAssertTrust)
    );
    assert_eq!(
        projection.resources()[0].load_status,
        ResourceLoadStatus::Rejected
    );
    assert_eq!(
        projection.resources()[0].activation,
        ResourceActivation::Inactive
    );
    assert!(serialized.contains("reviewAndAssertTrust"));
    assert_eq!(
        shacs_projection::Spec030RuntimeProjection::parse_json(&serialized)?,
        projection
    );
    Ok(())
}

#[test]
fn spec030_trusted_runtime_preserves_unsupported_kernel_and_adapter_specific_capabilities(
) -> Result<(), Box<dyn Error>> {
    let mut facts = active_owner_facts();
    facts
        .process_adapters
        .push(ProcessAdapterObservation::unsupported(
            ProcessAdapterKind::PythonKernel,
        ));

    let projection =
        build_trusted_runtime_projection(TrustedRuntimeInput::Available(Box::new(facts)))?;

    let kernel = &projection.lifecycle_boundaries()[1];
    assert_eq!(kernel.kind, LifecycleBoundaryKind::Kernel);
    assert_eq!(kernel.status, LifecycleBoundaryStatus::Unavailable);
    assert_eq!(kernel.isolation, LifecycleIsolation::LifecycleOnly);
    assert!(
        !projection.process_adapters()[0]
            .capabilities
            .startup_readiness
    );
    assert!(!projection.process_adapters()[1].capabilities.timeout);
    Ok(())
}

#[test]
fn spec030_trusted_runtime_projects_disabled_sandbox_without_applied_policy(
) -> Result<(), Box<dyn Error>> {
    let projection = build_trusted_runtime_projection(TrustedRuntimeInput::Available(Box::new(
        active_owner_facts(),
    )))?;

    assert_eq!(projection.sandbox().status, SandboxStatus::Disabled);
    assert_eq!(
        projection.sandbox().fallback,
        SandboxFallback::TrustedNativeFallback
    );
    assert!(projection.sandbox().applied_adapters.is_empty());
    assert_eq!(
        projection.sandbox().filesystem_policy,
        SandboxFilesystemPolicy::NotApplied
    );
    assert_eq!(
        projection.sandbox().network_policy,
        SandboxNetworkPolicy::NotApplied
    );
    Ok(())
}

#[test]
fn spec030_trusted_runtime_discloses_raw_content_and_trace_metadata_without_payloads(
) -> Result<(), Box<dyn Error>> {
    let projection = build_trusted_runtime_projection(TrustedRuntimeInput::Available(Box::new(
        active_owner_facts(),
    )))?;

    assert!(projection.disclosure().raw_content_possible);
    assert_eq!(
        projection.disclosure().surfaces,
        vec![DataSurface::Session, DataSurface::ToolOutput]
    );
    assert_eq!(
        projection.disclosure().trace.preview.as_ref(),
        Some(&TracePreviewProjection {
            record_count: 4,
            approximate_bytes: 512,
            destination: TraceDestination::LocalOnly,
            exporter: None,
            endpoint_summary: None,
        })
    );
    let serialized = serde_json::to_string(&projection)?;
    for forbidden_field in ["credentialValue", "stdout", "permissionSnapshot"] {
        assert!(!serialized.contains(forbidden_field));
    }
    Ok(())
}
