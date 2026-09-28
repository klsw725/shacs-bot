use serde_json::json;
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec031ActionRef, Spec031ApprovalState, Spec035AdapterScope, Spec035DecisionProjection,
    Spec035DisclosureState, Spec035DurableApprovalProjection,
    Spec035EphemeralConfirmationProjection, Spec035EphemeralConfirmationState,
    Spec035FinalDeliveryState, Spec035OwnerSurface, Spec035RevisedOwnerFacts,
    Spec035RevisedProjection, Spec035SandboxFallback, Spec035SandboxRuntimeState,
};
use std::error::Error;

fn unavailable_projection(
    surface: Spec035OwnerSurface,
) -> Result<Spec035RevisedProjection, Box<dyn Error>> {
    let trusted_runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    Ok(project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(surface, &trusted_runtime),
    )?)
}

#[test]
fn revised_projection_uses_missing_spec030_owner_evidence_without_fabricating_success(
) -> Result<(), Box<dyn Error>> {
    // Given: the actual Spec030 owner projection reports missing facts.
    let trusted_runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);

    // When: the shared Spec035 mapper projects that owner evidence for a CLI surface.
    let projection = project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
        Spec035OwnerSurface::Cli,
        &trusted_runtime,
    ))?;
    let value = serde_json::to_value(projection)?;

    // Then: no successful decision/control/resource/delivery fact is invented.
    assert_eq!(value["freshness"], "unavailable");
    assert_eq!(value["decisions"], json!([]));
    assert_eq!(value["runtime_controls"], json!([]));
    assert_eq!(value["resources"], json!([]));
    assert_eq!(value["delivery"]["final_delivery"]["state"], "unknown");
    Ok(())
}

#[test]
fn revised_owner_facts_keep_ephemeral_decisions_out_of_durable_approval(
) -> Result<(), Box<dyn Error>> {
    // Given: actual typed durable and ephemeral owner observations.
    let trusted_runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    let facts = Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Cli, &trusted_runtime)
        .with_durable_approval(Spec035DurableApprovalProjection {
            state: Spec031ApprovalState::Pending,
            approval_ref: Spec031ActionRef::try_new("action:approval:owner-request")?,
            action_digest: None,
            expires_at_unix_ms: None,
            retry_count: None,
            remembered_allow: None,
        })
        .with_ephemeral_confirmation(Spec035EphemeralConfirmationProjection {
            state: Spec035EphemeralConfirmationState::ConfirmationAllowed,
            call_ref: Spec031ActionRef::try_new("action:tool:confirmation")?,
        });
    let projection = project_spec035_revised_owner_facts(facts)?;

    // When: the shared projection crosses an adapter serialization boundary.
    let value = serde_json::to_value(&projection)?;

    // Then: only the durable variant carries durable approval fields.
    assert_eq!(value["decisions"][0]["kind"], "durable_approval");
    assert_eq!(value["decisions"][1]["kind"], "ephemeral_confirmation");
    assert_eq!(
        value["decisions"][1]["details"]["state"],
        "confirmation_allowed"
    );
    for decision in value["decisions"].as_array().into_iter().flatten().skip(1) {
        let details = &decision["details"];
        assert!(details.get("approval_ref").is_none());
        assert!(details.get("expires_at_unix_ms").is_none());
        assert!(details.get("retry_count").is_none());
        assert!(details.get("remembered_allow").is_none());
    }
    Ok(())
}

#[test]
fn revised_required_vocabulary_serializes_canonically() -> Result<(), Box<dyn Error>> {
    // Given: every newly required bounded vocabulary family.
    let sandbox = [
        Spec035SandboxRuntimeState::Active,
        Spec035SandboxRuntimeState::Disabled,
        Spec035SandboxRuntimeState::Unsupported,
        Spec035SandboxRuntimeState::Failed,
    ];
    let fallback = [
        Spec035SandboxFallback::NativeFallback,
        Spec035SandboxFallback::NotApplicable,
        Spec035SandboxFallback::ExecutionDenied,
        Spec035SandboxFallback::Unavailable,
    ];
    let scope = [
        Spec035AdapterScope::AdapterScoped,
        Spec035AdapterScope::Unavailable,
    ];
    let disclosure = [
        Spec035DisclosureState::SafeSummary,
        Spec035DisclosureState::RedactedForSurface,
        Spec035DisclosureState::RawContentPossibleElsewhere,
        Spec035DisclosureState::RemoteTraceOptIn,
    ];
    let final_delivery = [
        Spec035FinalDeliveryState::FinalDelivered,
        Spec035FinalDeliveryState::FinalPending,
        Spec035FinalDeliveryState::FinalFailed,
        Spec035FinalDeliveryState::Unknown,
    ];

    // When: each family is serialized.
    let actual = json!({
        "sandbox": sandbox,
        "fallback": fallback,
        "scope": scope,
        "disclosure": disclosure,
        "final_delivery": final_delivery,
    });

    // Then: the PRD 000 identifiers are canonical snake_case values.
    assert_eq!(
        actual["sandbox"],
        json!(["active", "disabled", "unsupported", "failed"])
    );
    assert_eq!(
        actual["fallback"],
        json!([
            "native_fallback",
            "not_applicable",
            "execution_denied",
            "unavailable"
        ])
    );
    assert_eq!(actual["scope"], json!(["adapter_scoped", "unavailable"]));
    assert_eq!(
        actual["disclosure"],
        json!([
            "safe_summary",
            "redacted_for_surface",
            "raw_content_possible_elsewhere",
            "remote_trace_opt_in"
        ])
    );
    assert_eq!(
        actual["final_delivery"],
        json!([
            "final_delivered",
            "final_pending",
            "final_failed",
            "unknown"
        ])
    );
    assert!(matches!(
        project_spec035_revised_owner_facts(
            Spec035RevisedOwnerFacts::new(
                Spec035OwnerSurface::Cli,
                &Spec030RuntimeProjection::unavailable(
                    Spec030UnavailableReason::OwnerFactsMissing,
                ),
            )
            .with_ephemeral_confirmation(Spec035EphemeralConfirmationProjection {
                state: Spec035EphemeralConfirmationState::ConfirmationRequired,
                call_ref: Spec031ActionRef::try_new("action:tool:confirmation")?,
            }),
        )?
        .decisions()[0],
        Spec035DecisionProjection::EphemeralConfirmation(_)
    ));
    Ok(())
}

#[test]
fn revised_parser_preserves_freshness_and_rejects_unknown_or_sensitive_schema(
) -> Result<(), Box<dyn Error>> {
    // Given: owner-backed output plus stale, unknown-version, and sensitive mutations.
    let projection = unavailable_projection(Spec035OwnerSurface::Cli)?;
    let mut stale = serde_json::to_value(&projection)?;
    stale["freshness"] = json!("stale");
    let mut unknown_version = stale.clone();
    unknown_version["schema_version"] = json!(2);
    let mut sensitive = stale.clone();
    sensitive["process_handle"] = json!("process-handle-raw-4242");

    // When: each fixture crosses the bounded parser.
    let stale = Spec035RevisedProjection::parse_json(&serde_json::to_string(&stale)?)?;
    let unknown_error =
        Spec035RevisedProjection::parse_json(&serde_json::to_string(&unknown_version)?)
            .expect_err("unknown schema must fail");
    let sensitive_error = Spec035RevisedProjection::parse_json(&serde_json::to_string(&sensitive)?)
        .expect_err("sensitive unknown field must fail");

    // Then: stale stays visible and errors reflect no raw value.
    let stale = serde_json::to_value(stale)?;
    assert_eq!(stale["freshness"], "stale");
    assert_eq!(stale["surface_summary"], "CLI owner facts projection");
    assert_eq!(unknown_error.to_string(), "invalid Spec035 revised schema");
    assert_eq!(
        sensitive_error.to_string(),
        "invalid Spec035 revised schema"
    );
    Ok(())
}
