use serde_json::json;
use shacs_projection::spec030::*;
use std::error::Error;

fn active_input() -> Spec030RuntimeProjectionInput {
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
            diagnostics: vec![HookDiagnosticProjection {
                hook_ref: "hook:guard".to_owned(),
                kind: HookDiagnosticKind::Timeout,
                behavior: HookFailureBehavior::ContinuedFailOpen,
            }],
            recent_denials: vec![HookDenialProjection {
                hook_ref: "hook:guard".to_owned(),
                call_ref: "call:1".to_owned(),
                reason: HookDenialReason::ExtensionBlocked,
            }],
        },
        process_adapters: vec![ProcessAdapterProjection {
            adapter: ProcessAdapterKind::Bash,
            availability: Spec030Availability::Available,
            support: ProcessAdapterSupport::Supported,
            control_scope: ProcessControlScope::ControlledChild,
            reason: ProcessControlReason::ControlledChildObservedNoRollback,
            capabilities: ProcessAdapterCapabilities {
                timeout: true,
                abort: true,
                cwd: true,
                env: false,
                bounded_output: true,
                descendant_cleanup: true,
                startup_readiness: false,
                generation_fencing: false,
            },
            recent_outcomes: vec![ProcessOutcomeProjection {
                outcome: ProcessTerminalOutcome::Succeeded,
                output_truncated: false,
                duration_ms: Some(12),
            }],
        }],
        credential: CredentialStatusProjection {
            availability: Spec030Availability::Available,
            status: CredentialStatus::Resolved,
            source: Some(CredentialSource::Environment),
            fingerprint: CredentialFingerprintStatus::Current,
            refresh_serialization: RefreshSerializationStatus::Active,
        },
        sandbox: SandboxStatusProjection {
            availability: Spec030Availability::Degraded,
            status: SandboxStatus::Disabled,
            fallback: SandboxFallback::TrustedNativeFallback,
            applied_adapters: Vec::new(),
            filesystem_policy: SandboxFilesystemPolicy::NotApplied,
            network_policy: SandboxNetworkPolicy::NotApplied,
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
            diagnostics: vec![ResourceDiagnosticProjection {
                code: "loadFailed".to_owned(),
                path: Some("/workspace/.shacs/extensions/guard.js".to_owned()),
                reason: "configured runtime rejected module".to_owned(),
            }],
        }],
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
                status: TraceStatus::Preview,
                preview: Some(TracePreviewProjection {
                    record_count: 2,
                    approximate_bytes: 128,
                    destination: TraceDestination::ConfiguredRemote,
                    exporter: None,
                    endpoint_summary: None,
                }),
            },
        },
    }
}

#[test]
fn spec030_validation_rejects_remote_enabled_trace_without_exporter_endpoint_evidence() {
    // Given
    let mut input = active_input();
    input.disclosure.trace.status = TraceStatus::Enabled;

    // When
    let error = Spec030RuntimeProjection::try_new(input)
        .expect_err("remote enabled trace requires exporter and endpoint evidence");

    // Then
    assert_eq!(
        error.violation(),
        Spec030ValidationViolation::MissingEvidence
    );
}

#[test]
fn spec030_untrusted_profile_is_inspectable_and_round_trips() -> Result<(), Box<dyn Error>> {
    let mut input = active_input();
    input.availability = Spec030Availability::Degraded;
    input.status = Spec030RuntimeStatus::Degraded;
    input.profile.availability = Spec030Availability::Degraded;
    input.profile.workspace_trust = WorkspaceTrust::NotAsserted;
    input.profile.workspace_trust_remediation =
        Some(WorkspaceTrustRemediation::ReviewAndAssertTrust);
    input.profile.resource_trust = ResourceTrust::ExplicitOnly;
    input.resources[0].load_status = ResourceLoadStatus::Rejected;
    input.resources[0].activation = ResourceActivation::Inactive;

    let projection = Spec030RuntimeProjection::try_new(input)?;
    let value = serde_json::to_value(&projection)?;

    assert_eq!(value["profile"]["workspaceTrust"], json!("notAsserted"));
    assert_eq!(
        value["profile"]["workspaceTrustRemediation"],
        json!("reviewAndAssertTrust")
    );
    assert_eq!(
        Spec030RuntimeProjection::from_json_value(value)?,
        projection
    );
    Ok(())
}

#[test]
fn spec030_v1_requires_resource_content_digest_field() -> Result<(), Box<dyn Error>> {
    // Given
    let projection = Spec030RuntimeProjection::try_new(active_input())?;
    let mut value = serde_json::to_value(projection)?;
    value["resources"][0]
        .as_object_mut()
        .ok_or("resource object missing")?
        .remove("contentSha256");

    // When
    let parsed = Spec030RuntimeProjection::from_json_value(value);

    // Then
    assert!(parsed.is_err());
    Ok(())
}

#[test]
fn spec030_shared_human_render_discloses_runtime_boundaries_when_workspace_is_untrusted(
) -> Result<(), Box<dyn Error>> {
    let mut input = active_input();
    input.availability = Spec030Availability::Degraded;
    input.status = Spec030RuntimeStatus::Degraded;
    input.profile.availability = Spec030Availability::Degraded;
    input.profile.workspace_trust = WorkspaceTrust::NotAsserted;
    input.profile.workspace_trust_remediation =
        Some(WorkspaceTrustRemediation::ReviewAndAssertTrust);
    input.profile.resource_trust = ResourceTrust::ExplicitOnly;
    input.resources[0].load_status = ResourceLoadStatus::Rejected;
    input.resources[0].activation = ResourceActivation::Inactive;
    let projection = Spec030RuntimeProjection::try_new(input)?;

    let rendered = render_spec030_runtime(&projection);

    for required in [
        "Current OS user authority",
        "reviewAndAssertTrust",
        "lifecycleOnly",
        "adapterScoped",
        "sandbox: availability=degraded status=disabled",
        "collision=winner",
        "source=project",
        "precedence=trustedProjectAuto",
        "path=/workspace/.shacs/extensions/guard.js",
        "resource digest: ref=resource:guard sha256=0000000000000000000000000000000000000000000000000000000000000000",
        "hook denial: ref=hook:guard call=call:1 reason=extensionBlocked",
        "resource diagnostic: ref=resource:guard code=loadFailed",
        "reason=configured runtime rejected module",
        "rawContentPossible=true",
        "trace: status=preview",
    ] {
        assert!(
            rendered.contains(required),
            "missing {required}: {rendered}"
        );
    }
    Ok(())
}

#[test]
fn spec030_default_provider_reports_explicit_unavailable_without_material_fields(
) -> Result<(), Box<dyn Error>> {
    let projection = UnavailableSpec030ProjectionProvider.projection();

    let serialized = serialize_spec030_runtime(&projection)?;

    assert!(serialized.contains("ownerFactsMissing"));
    for forbidden in ["credentialValue", "apiKey", "token", "stdout", "rawPayload"] {
        assert!(!serialized.contains(forbidden), "{forbidden}: {serialized}");
    }
    let rendered = render_spec030_runtime(&projection);
    for required in [
        "Unavailable: reason=ownerFactsMissing",
        "lifecycle: Unavailable",
        "hooks: Unavailable",
        "process: Unavailable",
        "credential: Unavailable",
        "sandbox: Unavailable",
        "resource: Unavailable",
    ] {
        assert!(
            rendered.contains(required),
            "missing {required}: {rendered}"
        );
    }
    assert!(
        rendered.lines().all(|line| line.chars().count() <= 116),
        "human renderer must not lose fields in the 120-column TUI pane: {rendered}"
    );
    Ok(())
}

#[test]
fn spec030_validation_requires_untrusted_workspace_remediation() {
    let mut input = active_input();
    input.availability = Spec030Availability::Degraded;
    input.status = Spec030RuntimeStatus::Degraded;
    input.profile.availability = Spec030Availability::Degraded;
    input.profile.workspace_trust = WorkspaceTrust::NotAsserted;
    input.profile.resource_trust = ResourceTrust::ExplicitOnly;
    input.resources[0].load_status = ResourceLoadStatus::Rejected;
    input.resources[0].activation = ResourceActivation::Inactive;

    let error = Spec030RuntimeProjection::try_new(input)
        .expect_err("not-asserted workspace trust requires remediation");

    assert_eq!(
        error.violation(),
        Spec030ValidationViolation::MissingEvidence
    );
}

#[test]
fn spec030_validation_rejects_trusted_workspace_activation_when_trust_is_not_asserted() {
    let mut input = active_input();
    input.availability = Spec030Availability::Degraded;
    input.status = Spec030RuntimeStatus::Degraded;
    input.profile.availability = Spec030Availability::Degraded;
    input.profile.workspace_trust = WorkspaceTrust::NotAsserted;
    input.profile.workspace_trust_remediation =
        Some(WorkspaceTrustRemediation::ReviewAndAssertTrust);
    input.profile.resource_trust = ResourceTrust::ExplicitOnly;

    let error = Spec030RuntimeProjection::try_new(input)
        .expect_err("untrusted workspace cannot activate trusted-workspace executable resources");

    assert_eq!(
        error.violation(),
        Spec030ValidationViolation::UnsafeResourceActivation
    );
}

#[test]
fn spec030_unavailable_projection_is_explicit_strict_and_round_trips() -> Result<(), Box<dyn Error>>
{
    let projection =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    let value = serde_json::to_value(&projection)?;

    assert_eq!(value["schemaVersion"], json!(1));
    assert_eq!(value["availability"], json!("unavailable"));
    assert_eq!(value["status"], json!("unavailable"));
    assert_eq!(
        Spec030RuntimeProjection::from_json_value(value.clone())?,
        projection
    );

    let mut unknown = value;
    unknown["hooks"]["unexpected"] = json!(true);
    assert!(Spec030RuntimeProjection::from_json_value(unknown).is_err());
    Ok(())
}

#[test]
fn spec030_active_projection_covers_owner_fact_vocabulary_without_material(
) -> Result<(), Box<dyn Error>> {
    let projection = Spec030RuntimeProjection::try_new(active_input())?;
    let serialized = serde_json::to_string(&projection)?;
    let parsed = Spec030RuntimeProjection::parse_json(&serialized)?;

    assert_eq!(parsed, projection);
    assert!(serialized.contains("currentOsUser"));
    assert!(serialized.contains("trustedNativeFallback"));
    for forbidden in [
        "rawInput",
        "stdout",
        "apiKey",
        "accessToken",
        "refreshToken",
    ] {
        assert!(!serialized.contains(forbidden));
    }
    Ok(())
}

#[test]
fn spec030_validation_rejects_false_active_and_supported_claims() -> Result<(), Box<dyn Error>> {
    let mut unsupported_input = active_input();
    unsupported_input.process_adapters[0].availability = Spec030Availability::Unavailable;
    let error = Spec030RuntimeProjection::try_new(unsupported_input)
        .expect_err("unavailable adapter cannot claim supported");
    assert_eq!(
        error.violation(),
        Spec030ValidationViolation::FalseSupportedClaim
    );

    let projection = Spec030RuntimeProjection::try_new(active_input())?;
    let mut false_active = serde_json::to_value(projection)?;
    false_active["availability"] = json!("unavailable");
    false_active["unavailableReason"] = json!("ownerFactsMissing");
    let error = Spec030RuntimeProjection::from_json_value(false_active)
        .expect_err("unavailable runtime cannot retain active owner facts");
    assert_eq!(error.kind(), Spec030ParseErrorKind::InvalidSchema);
    Ok(())
}
