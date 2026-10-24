use shacs_projection::spec030::*;
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec035OwnerSurface, Spec035RevisedOwnerFacts,
};
use std::error::Error;

fn owner_input() -> Spec030RuntimeProjectionInput {
    Spec030RuntimeProjectionInput {
        availability: Spec030Availability::Available,
        status: Spec030RuntimeStatus::Active,
        unavailable_reason: None,
        profile: TrustedRuntimeProfileProjection {
            availability: Spec030Availability::Available,
            status: TrustedProfileStatus::Active,
            profile: TrustedRuntimeProfile::TrustedLocalAgent,
            execution_authority: ExecutionAuthority::CurrentOsUser,
            workspace_trust: WorkspaceTrust::UserAsserted,
            workspace_trust_remediation: None,
            resource_trust: ResourceTrust::ExplicitOrTrustedWorkspace,
            default_containment: DefaultContainment::None,
            optional_sandbox: OptionalSandboxScope::AdapterScoped,
        },
        lifecycle_boundaries: vec![LifecycleBoundaryProjection {
            kind: LifecycleBoundaryKind::DaemonWorker,
            status: LifecycleBoundaryStatus::Active,
            isolation: LifecycleIsolation::LifecycleOnly,
        }],
        hooks: HookRuntimeProjection {
            availability: Spec030Availability::Available,
            status: HookRuntimeStatus::Active,
            registered_handlers: 1,
            diagnostics: Vec::new(),
            recent_denials: vec![HookDenialProjection {
                hook_ref: "hook:guard".to_owned(),
                call_ref: "call:1".to_owned(),
                reason: HookDenialReason::ExtensionBlocked,
            }],
        },
        process_adapters: Vec::new(),
        credential: CredentialStatusProjection {
            availability: Spec030Availability::Available,
            status: CredentialStatus::Resolved,
            source: Some(CredentialSource::Environment),
            fingerprint: CredentialFingerprintStatus::Current,
            refresh_serialization: RefreshSerializationStatus::Active,
        },
        sandbox: SandboxStatusProjection {
            availability: Spec030Availability::Available,
            status: SandboxStatus::Active,
            fallback: SandboxFallback::NotApplicable,
            applied_adapters: vec![ProcessAdapterKind::Bash],
            filesystem_policy: SandboxFilesystemPolicy::Applied,
            network_policy: SandboxNetworkPolicy::Applied,
        },
        resources: vec![ResourceCandidateProjection {
            resource_ref: "resource:guard".to_owned(),
            kind: ResourceKind::Extension,
            source: ResourceSource::Project,
            precedence: ResourcePrecedence::TrustedProjectAuto,
            canonical_path: "/workspace/.shacs/extensions/guard.js".to_owned(),
            content_sha256: Some("0".repeat(64)),
            collision: ResourceCollisionStatus::Winner,
            load_status: ResourceLoadStatus::Loaded,
            activation: ResourceActivation::TrustedWorkspace,
            trusted_code_disclosure: TrustedCodeDisclosure::Shown,
            diagnostics: Vec::new(),
        }],
        disclosure: DataDisclosureProjection {
            raw_content_possible: true,
            surfaces: vec![DataSurface::Session, DataSurface::ToolOutput],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Disabled,
                preview: None,
            },
        },
    }
}

#[test]
fn revised_mapper_reads_spec030_owner_facts() -> Result<(), Box<dyn Error>> {
    // Given: validated Spec030 hook, sandbox, and resource owner facts.
    let trusted_runtime = Spec030RuntimeProjection::try_new(owner_input())?;

    // When: the shared mapper projects those facts.
    let projection = project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
        Spec035OwnerSurface::Cli,
        &trusted_runtime,
    ))?;
    let value = serde_json::to_value(projection)?;

    // Then: owner meaning is retained without raw path material.
    assert_eq!(value["decisions"][0]["kind"], "hook_denial");
    assert_eq!(value["runtime_controls"][0]["adapter"], "bash");
    assert_eq!(value["resources"][0]["resource_ref"], "resource:guard");
    assert!(!serde_json::to_string(&value)?.contains("/workspace/"));
    Ok(())
}

#[test]
fn revised_mapper_opaques_absolute_resource_owner_refs() -> Result<(), Box<dyn Error>> {
    // Given: a validated owner identity containing an absolute path.
    let mut input = owner_input();
    input.resources[0].resource_ref = "context:/Users/alice/.shacs/context.md".to_owned();
    let trusted_runtime = Spec030RuntimeProjection::try_new(input)?;

    // When: the shared mapper projects the resource.
    let projection = project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
        Spec035OwnerSurface::Cli,
        &trusted_runtime,
    ))?;
    let value = serde_json::to_value(projection)?;

    // Then: lineage is stable but the owner path is absent.
    let resource_ref = value["resources"][0]["resource_ref"]
        .as_str()
        .ok_or("missing resource ref")?;
    assert!(resource_ref.starts_with("resource:sha256:"));
    assert!(!resource_ref.contains("/Users/alice"));
    Ok(())
}

#[test]
fn revised_mapper_preserves_confirmation_denial_reasons() -> Result<(), Box<dyn Error>> {
    // Given: distinct user and headless confirmation denials from the production gate.
    let mut input = owner_input();
    input.hooks.recent_denials = vec![
        HookDenialProjection {
            hook_ref: "hook:user".to_owned(),
            call_ref: "call:user".to_owned(),
            reason: HookDenialReason::UserDenied,
        },
        HookDenialProjection {
            hook_ref: "hook:headless".to_owned(),
            call_ref: "call:headless".to_owned(),
            reason: HookDenialReason::HeadlessConfirmationDenied,
        },
    ];
    let trusted_runtime = Spec030RuntimeProjection::try_new(input)?;

    // When: the shared mapper projects both owner denials.
    let projection = project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
        Spec035OwnerSurface::Cli,
        &trusted_runtime,
    ))?;
    let value = serde_json::to_value(projection)?;

    // Then: neither confirmation denial is collapsed into a hook veto.
    assert_eq!(value["decisions"][0]["kind"], "ephemeral_confirmation");
    assert_eq!(
        value["decisions"][0]["details"]["state"],
        "confirmation_denied"
    );
    assert_eq!(value["decisions"][1]["kind"], "ephemeral_confirmation");
    assert_eq!(
        value["decisions"][1]["details"]["state"],
        "headless_confirmation_denied"
    );
    Ok(())
}

#[test]
fn revised_mapper_keeps_non_active_sandbox_states_without_adapters() -> Result<(), Box<dyn Error>> {
    // Given: disabled, unsupported, and execution-denied sandbox owner facts without adapters.
    let cases = [
        (
            SandboxStatus::Disabled,
            SandboxFallback::TrustedNativeFallback,
            "disabled",
            "native_fallback",
        ),
        (
            SandboxStatus::Unsupported,
            SandboxFallback::TrustedNativeFallback,
            "unsupported",
            "native_fallback",
        ),
        (
            SandboxStatus::Failed,
            SandboxFallback::ExecutionDenied,
            "failed",
            "execution_denied",
        ),
    ];

    for (status, fallback, expected_status, expected_fallback) in cases {
        // When: each owner state crosses the shared projection boundary.
        let mut input = owner_input();
        input.sandbox.status = status;
        input.sandbox.fallback = fallback;
        input.sandbox.applied_adapters.clear();
        let trusted_runtime = Spec030RuntimeProjection::try_new(input)?;
        let projection = project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
            Spec035OwnerSurface::Cli,
            &trusted_runtime,
        ))?;
        let value = serde_json::to_value(projection)?;

        // Then: state and fallback remain visible with explicit unavailable adapter scope.
        assert_eq!(
            value["runtime_controls"][0]["sandbox_status"],
            expected_status
        );
        assert_eq!(value["runtime_controls"][0]["fallback"], expected_fallback);
        assert_eq!(value["runtime_controls"][0]["scope"], "unavailable");
        assert!(value["runtime_controls"][0].get("adapter").is_none());
    }
    Ok(())
}
