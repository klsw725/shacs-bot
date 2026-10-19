use shacs_core::runtime::{
    apply_goal_surface_action, create_entrusted_work, read_entrusted_work, revise_entrusted_work,
    transition_entrusted_work, EntrustedWorkConditions, EntrustedWorkConsentKind,
    EntrustedWorkConsentProof, EntrustedWorkCreateRequest, EntrustedWorkError,
    EntrustedWorkLifecycleAction, EntrustedWorkLifecycleRequest, EntrustedWorkReadRequest,
    EntrustedWorkReviseRequest, GoalSurfaceAction, PersistentGoalStatus,
    ENTRUSTED_WORK_METADATA_KEY, PERSISTENT_GOAL_METADATA_KEY,
};
use shacs_session::{Session, SessionManager};

fn conditions(instructions: &str) -> EntrustedWorkConditions {
    EntrustedWorkConditions {
        instructions: instructions.to_owned(),
        budget: Some("100 USD".to_owned()),
        prior_decisions: vec!["use local evidence".to_owned()],
        remaining_questions: vec!["which format?".to_owned()],
    }
}

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
        conditions: conditions("report only verified changes"),
        linked_targets: vec!["channel:alerts".to_owned()],
        observed_at: "t0".to_owned(),
    }
}

fn prepare_session(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut manager = SessionManager::new(root)?;
    manager.save_with_fsync(&Session::new("session-owner"))?;
    Ok(())
}

#[test]
fn creation_persists_consent_goal_and_conditions_without_work_state(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    prepare_session(root.path())?;

    // When
    let created = create_entrusted_work(root.path(), create_request())?;
    let restarted = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "channel:alerts".to_owned(),
            work_id: created.work.work_id.clone(),
        },
    )?;
    let unlinked = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "channel:other".to_owned(),
            work_id: created.work.work_id.clone(),
        },
    );

    // Then
    assert_eq!(restarted, created);
    assert_eq!(created.work.current_revision, 1);
    assert_eq!(created.work.consent.evidence_ref, "message:42");
    assert_eq!(created.work.goal_transition_ids.len(), 1);
    assert!(matches!(unlinked, Err(EntrustedWorkError::AccessDenied)));
    assert_eq!(
        created.latest_goal_transition_id,
        created.work.goal_transition_ids[0]
    );
    let manager = SessionManager::open_existing(root.path())?.ok_or("missing manager")?;
    let raw = manager
        .read_session_file("session-owner")
        .ok_or("missing session")?;
    assert!(raw["metadata"].get(PERSISTENT_GOAL_METADATA_KEY).is_some());
    let work = &raw["metadata"][ENTRUSTED_WORK_METADATA_KEY]["works"][0];
    assert!(work.get("status").is_none());
    assert!(work.get("goal_status").is_none());
    Ok(())
}

#[test]
fn lifecycle_uses_only_goal_transitions_and_preserves_exact_ids(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    prepare_session(root.path())?;
    let created = create_entrusted_work(root.path(), create_request())?;
    let mut expected_ids = created.work.goal_transition_ids.clone();

    // When
    let mut latest = created;
    for (action, expected_status, observed_at) in [
        (
            EntrustedWorkLifecycleAction::Pause,
            PersistentGoalStatus::Paused,
            "t1",
        ),
        (
            EntrustedWorkLifecycleAction::Resume,
            PersistentGoalStatus::Active,
            "t2",
        ),
        (
            EntrustedWorkLifecycleAction::Done,
            PersistentGoalStatus::Done,
            "t3",
        ),
        (
            EntrustedWorkLifecycleAction::Clear,
            PersistentGoalStatus::Cleared,
            "t4",
        ),
    ] {
        latest = transition_entrusted_work(
            root.path(),
            EntrustedWorkLifecycleRequest {
                owner_session_id: "session-owner".to_owned(),
                requester_target: "session-owner".to_owned(),
                work_id: latest.work.work_id.clone(),
                action,
                observed_at: observed_at.to_owned(),
            },
        )?;
        expected_ids.push(latest.latest_goal_transition_id.clone());
        assert_eq!(latest.goal_status, expected_status);
    }

    // Then
    assert_eq!(latest.work.goal_transition_ids, expected_ids);
    for (index, transition_id) in expected_ids.iter().enumerate() {
        assert!(transition_id.ends_with(&format!(":{}", index + 1)));
    }
    let restarted = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: latest.work.work_id.clone(),
        },
    )?;
    assert_eq!(restarted, latest);
    Ok(())
}

#[test]
fn stale_revision_cannot_overwrite_current_conditions() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    prepare_session(root.path())?;
    let created = create_entrusted_work(root.path(), create_request())?;
    let revised = revise_entrusted_work(
        root.path(),
        EntrustedWorkReviseRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: created.work.work_id.clone(),
            expected_revision: 1,
            conditions: conditions("notify only after verification"),
            linked_targets: None,
            changed_at: "t1".to_owned(),
        },
    )?;

    // When
    let conflict = revise_entrusted_work(
        root.path(),
        EntrustedWorkReviseRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: created.work.work_id.clone(),
            expected_revision: 1,
            conditions: conditions("stale overwrite"),
            linked_targets: None,
            changed_at: "t2".to_owned(),
        },
    );

    // Then
    assert!(matches!(
        conflict,
        Err(EntrustedWorkError::RevisionConflict {
            expected: 1,
            actual: 2
        })
    ));
    assert_eq!(
        revised
            .work
            .current_conditions()
            .ok_or("missing current conditions")?
            .instructions,
        "notify only after verification"
    );
    Ok(())
}

#[test]
fn terminal_work_remains_readable_after_next_work_is_created(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    prepare_session(root.path())?;
    let first = create_entrusted_work(root.path(), create_request())?;
    let terminal = transition_entrusted_work(
        root.path(),
        EntrustedWorkLifecycleRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: first.work.work_id.clone(),
            action: EntrustedWorkLifecycleAction::Done,
            observed_at: "t1".to_owned(),
        },
    )?;

    // When
    let mut next_request = create_request();
    next_request.goal_text = "track deployment readiness".to_owned();
    next_request.observed_at = "t2".to_owned();
    let second = create_entrusted_work(root.path(), next_request)?;
    let historical = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: first.work.work_id.clone(),
        },
    )?;

    // Then
    assert_ne!(first.work.work_id, second.work.work_id);
    assert_eq!(historical.goal_status, PersistentGoalStatus::Done);
    assert_eq!(
        historical.latest_goal_transition_id,
        terminal.latest_goal_transition_id
    );
    assert_eq!(second.goal_status, PersistentGoalStatus::Active);
    Ok(())
}

#[test]
fn unlinked_goal_transition_blocks_work_snapshot_and_mutations(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    prepare_session(root.path())?;
    let created = create_entrusted_work(root.path(), create_request())?;

    // When
    apply_goal_surface_action(root.path(), "session-owner", GoalSurfaceAction::Pause, "t1")?;
    let read = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: created.work.work_id.clone(),
        },
    );
    let transition = transition_entrusted_work(
        root.path(),
        EntrustedWorkLifecycleRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: created.work.work_id,
            action: EntrustedWorkLifecycleAction::Resume,
            observed_at: "t2".to_owned(),
        },
    );

    // Then
    assert!(matches!(read, Err(EntrustedWorkError::GoalBindingMismatch)));
    assert!(matches!(
        transition,
        Err(EntrustedWorkError::GoalBindingMismatch)
    ));
    Ok(())
}

#[test]
fn linked_target_changes_follow_revision_and_revoke_old_access(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    prepare_session(root.path())?;
    let created = create_entrusted_work(root.path(), create_request())?;

    // When
    let revised = revise_entrusted_work(
        root.path(),
        EntrustedWorkReviseRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "session-owner".to_owned(),
            work_id: created.work.work_id.clone(),
            expected_revision: 1,
            conditions: conditions("only on verified changes"),
            linked_targets: Some(vec!["channel:new".to_owned()]),
            changed_at: "t1".to_owned(),
        },
    )?;
    let revoked = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "channel:alerts".to_owned(),
            work_id: created.work.work_id.clone(),
        },
    );
    let granted = read_entrusted_work(
        root.path(),
        &EntrustedWorkReadRequest {
            owner_session_id: "session-owner".to_owned(),
            requester_target: "channel:new".to_owned(),
            work_id: created.work.work_id,
        },
    )?;

    // Then
    assert!(matches!(revoked, Err(EntrustedWorkError::AccessDenied)));
    assert_eq!(granted, revised);
    assert_eq!(
        revised.work.revisions[0].linked_targets,
        vec!["channel:alerts"]
    );
    assert_eq!(
        revised.work.revisions[1].linked_targets,
        vec!["channel:new"]
    );
    Ok(())
}
