use shacs_core::runtime::{
    apply_goal_surface_action, build_spec033_snapshot, create_entrusted_work,
    create_persistent_goal, read_entrusted_work, EntrustedWorkConditions, EntrustedWorkConsentKind,
    EntrustedWorkConsentProof, EntrustedWorkCreateRequest, EntrustedWorkError,
    EntrustedWorkReadRequest, GoalSurfaceAction, GoalTransitionFact, ENTRUSTED_WORK_METADATA_KEY,
    PERSISTENT_GOAL_METADATA_KEY,
};
use shacs_session::{Session, SessionManager};

fn create_request() -> EntrustedWorkCreateRequest {
    EntrustedWorkCreateRequest {
        owner_session_id: "session-owner".to_owned(),
        goal_text: "track release readiness".to_owned(),
        turn_budget: 4,
        consent: EntrustedWorkConsentProof {
            kind: EntrustedWorkConsentKind::ExplicitRequest,
            evidence_ref: "message:42".to_owned(),
            recorded_at: "t0".to_owned(),
        },
        conditions: EntrustedWorkConditions {
            instructions: "report verified changes".to_owned(),
            budget: None,
            prior_decisions: Vec::new(),
            remaining_questions: Vec::new(),
        },
        linked_targets: Vec::new(),
        observed_at: "t0".to_owned(),
    }
}

#[test]
fn legacy_goal_remains_readable_but_is_not_auto_entrusted() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    let mut session = Session::new("session-owner");
    let legacy_goal = create_persistent_goal("session-owner", "legacy goal", "t0", 2);
    let mut legacy_value = serde_json::to_value(&legacy_goal)?;
    legacy_value["last_transition"]
        .as_object_mut()
        .ok_or("missing last transition")?
        .remove("transition_id");
    for transition in legacy_value["transitions"]
        .as_array_mut()
        .ok_or("missing transitions")?
    {
        transition
            .as_object_mut()
            .ok_or("invalid transition")?
            .remove("transition_id");
    }
    session
        .metadata
        .insert(PERSISTENT_GOAL_METADATA_KEY.to_owned(), legacy_value);
    manager.save_with_fsync(&session)?;

    // When
    let before = build_spec033_snapshot(root.path(), "session-owner")?;
    let missing = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: "work-does-not-exist".to_owned(),
        },
    );
    let after =
        apply_goal_surface_action(root.path(), "session-owner", GoalSurfaceAction::Pause, "t1")?;

    // Then
    assert!(before.goal.fact.is_some());
    assert!(matches!(missing, Err(EntrustedWorkError::WorkMissing)));
    let transition = after
        .goal
        .fact
        .and_then(|fact| fact.latest_transition)
        .ok_or("missing transition")?;
    assert!(transition
        .transition_id
        .as_deref()
        .is_some_and(|id| id.ends_with(":2")));
    Ok(())
}

#[test]
fn malformed_work_metadata_fails_closed_without_creating_goal(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    let mut session = Session::new("session-owner");
    session.metadata.insert(
        ENTRUSTED_WORK_METADATA_KEY.to_owned(),
        serde_json::json!({"unsupported": true}),
    );
    manager.save_with_fsync(&session)?;

    // When
    let result = create_entrusted_work(root.path(), create_request());

    // Then
    assert!(matches!(result, Err(EntrustedWorkError::Metadata(_))));
    let reloaded = SessionManager::open_existing(root.path())?
        .and_then(|manager| manager.load_existing("session-owner"))
        .ok_or("missing session")?;
    assert!(reloaded
        .metadata
        .get(PERSISTENT_GOAL_METADATA_KEY)
        .is_none());
    Ok(())
}

#[test]
fn missing_consent_evidence_fails_closed_without_creating_goal(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    manager.save_with_fsync(&Session::new("session-owner"))?;
    let mut request = create_request();
    request.consent.evidence_ref.clear();

    // When
    let result = create_entrusted_work(root.path(), request);

    // Then
    assert!(matches!(
        result,
        Err(EntrustedWorkError::InvalidInput("consent evidence"))
    ));
    let reloaded = manager
        .load_existing("session-owner")
        .ok_or("missing session")?;
    assert!(reloaded
        .metadata
        .get(PERSISTENT_GOAL_METADATA_KEY)
        .is_none());
    Ok(())
}

#[test]
fn malformed_goal_metadata_fails_closed_without_creating_work(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    let mut session = Session::new("session-owner");
    session.metadata.insert(
        PERSISTENT_GOAL_METADATA_KEY.to_owned(),
        serde_json::json!({"unsupported": true}),
    );
    manager.save_with_fsync(&session)?;

    // When
    let result = create_entrusted_work(root.path(), create_request());

    // Then
    assert!(matches!(result, Err(EntrustedWorkError::GoalMetadata(_))));
    let reloaded = manager
        .load_existing("session-owner")
        .ok_or("missing session")?;
    assert!(reloaded.metadata.get(ENTRUSTED_WORK_METADATA_KEY).is_none());
    Ok(())
}

#[test]
fn legacy_transition_json_does_not_receive_invented_id() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let raw = serde_json::json!({
        "goal_id": "goal-legacy",
        "prior_state": "unavailable",
        "current_state": "active",
        "stop_reason": "goal_set",
        "budget": {"turn_budget": 2, "turns_used": 0, "remaining_turns": 2},
        "evidence": "available",
        "user_interrupted": false,
        "observed_at": "t0"
    });

    // When
    let fact: GoalTransitionFact = serde_json::from_value(raw)?;

    // Then
    assert!(fact.transition_id.is_none());
    assert!(serde_json::to_value(fact)?.get("transition_id").is_none());
    Ok(())
}
