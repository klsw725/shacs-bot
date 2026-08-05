use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use shacs_projection::*;
use std::{error::Error, fmt::Debug};

fn assert_round_trips<T>(value: T) -> Result<(), Box<dyn Error>>
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let serialized = serde_json::to_string(&value)?;
    let parsed: T = serde_json::from_str(&serialized)?;
    assert_eq!(parsed, value);
    Ok(())
}

fn envelope_fixture(capability: Spec031Capability) -> Spec031Envelope {
    envelope_fixture_with_children(capability, Vec::new())
}

fn envelope_fixture_with_children(
    capability: Spec031Capability,
    children: Vec<Spec031Envelope>,
) -> Spec031Envelope {
    Spec031Envelope::try_new(envelope_input(capability, children))
        .expect("test fixture uses safe Spec031 values")
}

fn envelope_input(
    capability: Spec031Capability,
    children: Vec<Spec031Envelope>,
) -> Spec031EnvelopeInput {
    Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: kind_for_capability(&capability),
        state: Spec031Availability::Blocked,
        severity: Spec031Severity::Warning,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Blocked,
            safe_summary: Spec031SafeSummary::try_new("safe blocked summary")
                .expect("safe summary fixture"),
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new("subject:approval:1")
                .expect("safe subject fixture"),
            parent_ref: Some(
                Spec031ParentRef::try_new("subject:session:1").expect("safe parent fixture"),
            ),
            action_ref: Some(
                Spec031ActionRef::try_new("action:approval:1").expect("safe action fixture"),
            ),
            digest: Some(Spec031Digest::try_new("sha256:abc123").expect("safe digest fixture")),
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Spec030,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
            freshness: Spec031Freshness::Current,
        },
        capability,
        children,
    }
}

fn kind_for_capability(capability: &Spec031Capability) -> Spec031ProjectionKind {
    match capability {
        Spec031Capability::Session(_) => Spec031ProjectionKind::Session,
        Spec031Capability::Turn(_) => Spec031ProjectionKind::Turn,
        Spec031Capability::Subagent(_) => Spec031ProjectionKind::Subagent,
        Spec031Capability::Approval(_) => Spec031ProjectionKind::Approval,
        Spec031Capability::Tool(_) => Spec031ProjectionKind::Tool,
        Spec031Capability::Context(_) => Spec031ProjectionKind::Context,
        Spec031Capability::Plugin(_) => Spec031ProjectionKind::Plugin,
        Spec031Capability::App(_) => Spec031ProjectionKind::App,
        Spec031Capability::Media(_) => Spec031ProjectionKind::Media,
        Spec031Capability::Diagnostics(_) => Spec031ProjectionKind::Diagnostics,
        Spec031Capability::ReleaseEvidence(_) => Spec031ProjectionKind::ReleaseEvidence,
        Spec031Capability::Readiness(_) => Spec031ProjectionKind::Readiness,
        Spec031Capability::Progress(_) => Spec031ProjectionKind::Progress,
    }
}

#[test]
fn spec031_model_round_trips_required_vocabularies() -> Result<(), Box<dyn Error>> {
    for value in Spec031Availability::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031ApprovalState::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031InclusionReason::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031ReasonCode::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031ProgressDelivery::ALL {
        assert_round_trips(value)?;
    }
    Ok(())
}

#[test]
fn spec031_model_round_trips_kind_source_severity_freshness_and_capability_variants(
) -> Result<(), Box<dyn Error>> {
    for value in Spec031ProjectionKind::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031SourceOwner::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031Severity::ALL {
        assert_round_trips(value)?;
    }
    for value in Spec031Freshness::ALL {
        assert_round_trips(value)?;
    }
    for value in capability_variants() {
        assert_round_trips(value)?;
    }
    Ok(())
}

fn capability_variants() -> [Spec031Capability; 13] {
    use shacs_projection::Spec031Capability as Capability;
    use shacs_projection::Spec031InclusionReason as Inclusion;

    fn count(value: u64) -> Spec031Count {
        Spec031Count::new(value)
    }

    [
        Capability::Session(Spec031SessionCapability {
            active_turn_count: Some(count(0)),
        }),
        Capability::Turn(Spec031TurnCapability {
            turn_index: Some(count(1)),
        }),
        Capability::Subagent(Spec031SubagentCapability {
            child_count: Some(count(2)),
        }),
        Capability::Approval(Spec031ApprovalCapability {
            state: Spec031ApprovalState::Pending,
        }),
        Capability::Tool(Spec031ToolCapability {
            attempt_count: Some(count(3)),
        }),
        Capability::Context(Spec031ContextCapability {
            reason: Inclusion::Included,
        }),
        Capability::Plugin(Spec031PluginCapability {
            availability: Spec031Availability::Degraded,
        }),
        Capability::App(Spec031AppCapability {
            availability: Spec031Availability::Unavailable,
        }),
        Capability::Media(Spec031MediaCapability {
            reason: Inclusion::ExtractionFailed,
        }),
        Capability::Diagnostics(Spec031DiagnosticsCapability {
            component_count: Some(count(4)),
        }),
        Capability::ReleaseEvidence(Spec031ReleaseEvidenceCapability {
            blocker_count: Some(count(5)),
        }),
        Capability::Readiness(Spec031ReadinessCapability {
            availability: Spec031Availability::Ready,
            component_count: None,
            queue_depth: None,
            queue_capacity: None,
            remediation: None,
        }),
        Capability::Progress(Spec031ProgressCapability::delivery(
            Spec031ProgressDelivery::Live,
        )),
    ]
}

#[test]
fn spec031_model_round_trips_envelope_lineage_source_and_children() -> Result<(), Box<dyn Error>> {
    let child = envelope_fixture(Spec031Capability::Progress(
        Spec031ProgressCapability::delivery(Spec031ProgressDelivery::FinalPending),
    ));
    let envelope = envelope_fixture_with_children(
        Spec031Capability::Approval(Spec031ApprovalCapability {
            state: Spec031ApprovalState::Pending,
        }),
        vec![child],
    );
    let serialized = serde_json::to_string(&envelope)?;
    assert_eq!(Spec031Envelope::parse_json(&serialized)?, envelope);
    Ok(())
}

#[test]
fn spec031_model_rejects_unknown_field_version_and_state() -> Result<(), Box<dyn Error>> {
    let envelope = envelope_fixture(Spec031Capability::Approval(Spec031ApprovalCapability {
        state: Spec031ApprovalState::Allowed,
    }));
    let mut with_unknown_field = serde_json::to_value(&envelope)?;
    with_unknown_field["unexpected"] = json!(true);
    assert!(Spec031Envelope::from_json_value(with_unknown_field).is_err());

    let mut with_unknown_capability_field = serde_json::to_value(&envelope)?;
    with_unknown_capability_field["capability"]["unexpected"] = json!(true);
    assert!(Spec031Envelope::from_json_value(with_unknown_capability_field).is_err());

    let mut with_unknown_version = serde_json::to_value(&envelope)?;
    with_unknown_version["schema_version"] = json!(2);
    assert!(Spec031Envelope::from_json_value(with_unknown_version).is_err());
    assert_eq!(
        Spec031SchemaVersion::try_from_raw(2),
        Err(Spec031VersionError::Unsupported { found: 2 })
    );

    let mut with_unknown_state = serde_json::to_value(envelope)?;
    with_unknown_state["state"] = json!("not_ready");
    assert!(Spec031Envelope::from_json_value(with_unknown_state).is_err());

    let mut with_wrong_type = serde_json::to_value(envelope_fixture(Spec031Capability::Approval(
        Spec031ApprovalCapability {
            state: Spec031ApprovalState::Allowed,
        },
    )))?;
    with_wrong_type["source"]["observed_at_unix_ms"] = json!("31");
    assert!(Spec031Envelope::from_json_value(with_wrong_type).is_err());
    Ok(())
}

#[test]
fn spec031_model_rejects_kind_capability_family_mismatch() -> Result<(), Box<dyn Error>> {
    let capability = Spec031Capability::Session(Spec031SessionCapability {
        active_turn_count: None,
    });
    let mut input = envelope_input(capability, Vec::new());
    input.kind = Spec031ProjectionKind::Approval;
    let error = Spec031Envelope::try_new(input).expect_err("mismatched constructor must fail");
    assert_eq!(
        error.kind(),
        Spec031ConstructionViolation::CapabilityFamilyMismatch
    );

    let envelope = envelope_fixture(Spec031Capability::Approval(Spec031ApprovalCapability {
        state: Spec031ApprovalState::Pending,
    }));
    let mut mismatched_json = serde_json::to_value(envelope)?;
    mismatched_json["capability"] =
        serde_json::to_value(Spec031Capability::Session(Spec031SessionCapability {
            active_turn_count: None,
        }))?;
    let error =
        Spec031Envelope::from_json_value(mismatched_json).expect_err("mismatched JSON must fail");
    assert_eq!(error.kind(), Spec031ParseErrorKind::InvalidSchema);
    Ok(())
}

#[test]
fn spec031_model_preserves_missing_and_explicit_zero() -> Result<(), Box<dyn Error>> {
    let missing = envelope_fixture(Spec031Capability::Session(Spec031SessionCapability {
        active_turn_count: None,
    }));
    let zero = envelope_fixture(Spec031Capability::Session(Spec031SessionCapability {
        active_turn_count: Some(Spec031Count::new(0)),
    }));
    let missing_json = serde_json::to_value(&missing)?;
    let zero_json = serde_json::to_value(&zero)?;

    assert!(missing_json["capability"]["details"]
        .get("active_turn_count")
        .is_none());
    assert_eq!(
        zero_json["capability"]["details"]["active_turn_count"],
        json!(0)
    );
    assert_eq!(Spec031Envelope::from_json_value(missing_json)?, missing);
    assert_eq!(Spec031Envelope::from_json_value(zero_json)?, zero);
    Ok(())
}
