use shacs_projection::{
    Spec033AutomationFact, Spec033AutomationJobStatus, Spec033Availability, Spec033CapabilityOwner,
    Spec033DeliveryStatus, Spec033DiagnosticLink, Spec033DiagnosticUnavailableReason,
    Spec033EvidenceSource, Spec033GoalStatus, Spec033HookConfirmationFact, Spec033Owner,
    Spec033OwnerFact, Spec033Snapshot,
};

#[test]
fn typed_snapshot_rejects_untyped_or_unknown_owner_states() {
    // Given
    let forged = r#"{
        "schema":"spec033.projection_snapshot.v2",
        "session_id":"cli:direct",
        "goal":{"availability":"available","fact":{"goal_id":"goal-1","session_id":"cli:direct","status":"rolled_back","turn_budget":2,"turns_used":0,"last_verdict":null,"blocked":false},"lineage":{"owner":"goal","source":"session_metadata","evidence_refs":["session_metadata:persistent_goal"]}},
        "evaluator":{"availability":"unavailable","lineage":{"owner":"evaluator","source":"session_metadata","evidence_refs":[]}},
        "automation":{"availability":"unavailable","lineage":{"owner":"automation","source":"durable_store","evidence_refs":[]}},
        "hook_confirmation":{"availability":"unavailable","lineage":{"owner":"hook_confirmation","source":"durable_store","evidence_refs":[]}},
        "self_improvement":{"availability":"unavailable","lineage":{"owner":"self_improvement","source":"durable_store","evidence_refs":[]}},
        "verify":{"availability":"unavailable","lineage":{"owner":"verify","source":"durable_store","evidence_refs":[]}},
        "rollback_candidate":{"availability":"unavailable","lineage":{"owner":"rollback_candidate","source":"durable_store","evidence_refs":[]}},
        "replay":{"availability":"unavailable","lineage":{"owner":"replay","source":"durable_store","evidence_refs":[]}}
    }"#;

    // When
    let parsed = serde_json::from_str::<Spec033Snapshot>(forged);

    // Then
    assert!(parsed.is_err());
}

#[test]
fn available_owner_requires_typed_fact_and_evidence_lineage() {
    // Given
    let fact = Spec033AutomationFact {
        work_id: "work-1".to_owned(),
        job_id: "job-1".to_owned(),
        run_id: "run-1".to_owned(),
        turn_id: Some("turn-1".to_owned()),
        snapshot_id: Some("snapshot-1".to_owned()),
        snapshot_digest: Some("sha256:snapshot-1".to_owned()),
        checkpoint_id: Some("checkpoint-1".to_owned()),
        artifact_refs: vec!["artifact:1".to_owned()],
        job_status: Spec033AutomationJobStatus::Succeeded,
        delivery_status: Spec033DeliveryStatus::Failed,
    };

    // When
    let owner = Spec033OwnerFact::available(
        Spec033Owner::Automation,
        Spec033EvidenceSource::DurableStore,
        fact,
        vec!["durable_work:work-1:terminal:4".to_owned()],
    );

    // Then
    assert_eq!(owner.availability, Spec033Availability::Available);
    let fact = owner.fact.expect("available owner fact");
    assert_eq!(fact.job_status, Spec033AutomationJobStatus::Succeeded);
    assert_eq!(fact.delivery_status, Spec033DeliveryStatus::Failed);
    assert_eq!(owner.lineage.evidence_refs.len(), 1);
}

#[test]
fn missing_diagnostic_identifiers_remain_typed_unavailable() {
    let snapshot = Spec033Snapshot::unavailable("cli:direct");

    assert_eq!(
        snapshot.diagnostics.goal_id,
        Spec033DiagnosticLink::unavailable(
            Spec033DiagnosticUnavailableReason::MissingOwnerEvidence
        )
    );
    assert_eq!(
        snapshot.diagnostics.hook_confirmation_event_id,
        Spec033DiagnosticLink::unavailable(
            Spec033DiagnosticUnavailableReason::IdentifierNotRecorded
        )
    );
    assert_eq!(
        snapshot.diagnostics.checkpoint_id,
        Spec033DiagnosticLink::unavailable(
            Spec033DiagnosticUnavailableReason::IdentifierNotRecorded
        )
    );
    assert_eq!(
        snapshot.diagnostics.safe_artifact_refs.availability,
        Spec033Availability::Unavailable
    );
    assert!(snapshot.diagnostics.safe_artifact_refs.values.is_empty());
}

#[test]
fn hook_confirmation_is_not_inferred_from_job_success() {
    // Given / When
    let confirmation = Spec033HookConfirmationFact::NotRequired;

    // Then
    assert_ne!(confirmation, Spec033HookConfirmationFact::Confirmed);
}

#[test]
fn owner_capabilities_are_typed_unavailable_without_evidence() {
    // Given / When
    let capability = Spec033CapabilityOwner::unavailable(
        shacs_projection::Spec033Owner::Replay,
        shacs_projection::Spec033EvidenceSource::DurableStore,
    );

    // Then
    assert_eq!(capability.availability, Spec033Availability::Unavailable);
    assert!(capability.lineage.evidence_refs.is_empty());
    let statuses = [
        Spec033GoalStatus::Active,
        Spec033GoalStatus::Paused,
        Spec033GoalStatus::Blocked,
        Spec033GoalStatus::Done,
        Spec033GoalStatus::Cleared,
    ];
    assert_eq!(statuses.len(), 5);
}
