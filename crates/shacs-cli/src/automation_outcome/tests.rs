use super::*;
use shacs_core::runtime::{
    create_persistent_goal, persistent_goal_from_session, store_persistent_goal,
    AutomationOutcomePolicy,
};
use shacs_eval::evaluator::{AutomationRunRequest, AutomationRunTriggerKind};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::{Session, SessionManager};

fn request(policy: AutomationOutcomePolicy) -> AutomationDispatchRequest {
    AutomationDispatchRequest {
        work_id: "automation-work-1".to_owned(),
        session_key: "telegram:chat-1".to_owned(),
        work_kind: "automation.run".to_owned(),
        dedupe_key: "dedupe-1".to_owned(),
        idempotency_key: "idempotency-1".to_owned(),
        run: AutomationRunRequest {
            run_id: "run-1".to_owned(),
            job_id: "job-1".to_owned(),
            trigger_kind: AutomationRunTriggerKind::Heartbeat,
            trigger_ref: shacs_eval::evaluator::AutomationTriggerRef {
                runtime_service_event_id: "event-1".to_owned(),
                source_type: "heartbeat".to_owned(),
                source_owner: "runtime".to_owned(),
                received_at_ms: 1,
                idempotency_key: "trigger-1".to_owned(),
            },
            session_id: Some("telegram:chat-1".to_owned()),
            goal_id: Some("ignored-request-goal".to_owned()),
            execution_mode: shacs_eval::evaluator::AutomationExecutionMode::NoAgentCheck,
            timeout_policy_ref: "timeout".to_owned(),
            retry_policy_ref: "retry".to_owned(),
            delivery_policy_ref: "delivery".to_owned(),
            recursion_guard_token: "guard".to_owned(),
        },
        requirements: shacs_core::runtime::AutomationExecutionRequirements {
            execution_sensitive: false,
            credential_required: false,
            sandbox_required: false,
            confirmation: shacs_core::runtime::AutomationConfirmationFact::NotRequired,
        },
        instruction: Some("work".to_owned()),
        outcome_policy: policy,
        owner_target_ref: None,
    }
}

fn workspace_with_goal(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut manager = SessionManager::new(root)?;
    let mut session = Session::new("telegram:chat-1");
    let goal = create_persistent_goal(&session.key, "finish", "2026-08-13T00:00:00Z", 2);
    let goal_id = goal.id.clone();
    store_persistent_goal(&mut session, &goal)?;
    manager.save(&session)?;
    Ok(goal_id)
}

#[test]
fn continue_reads_goal_under_lock_and_enqueues_one_bounded_follow_up(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let goal_id = workspace_with_goal(root.path())?;
    let request = request(AutomationOutcomePolicy::Continue);
    let result = AutomationJobResult::Succeeded {
        result_ref: "result-1".to_owned(),
    };

    // When
    let first = evaluate_and_route(root.path(), root.path(), &request, &result)?;
    let second = evaluate_and_route(root.path(), root.path(), &request, &result)?;

    // Then
    assert_eq!(first, second);
    let session = SessionManager::open_existing(root.path())?
        .and_then(|manager| manager.load_existing("telegram:chat-1"))
        .ok_or("missing session")?;
    let goal = persistent_goal_from_session(&session).ok_or("missing goal")?;
    assert_eq!(goal.id, goal_id);
    assert_eq!(goal.turns_used, 1);
    let state = evaluate_durable_recovery(
        root.path().join("runtime/durable-events"),
        root.path().join("runtime/durable-checkpoints"),
    )
    .state
    .ok_or("missing durable state")?;
    assert_eq!(
        state
            .work
            .items
            .values()
            .filter(|item| item.work_id.starts_with("automation-route-continue-"))
            .count(),
        1
    );
    assert!(!root.path().join("runtime/automation-routes").exists());
    Ok(())
}

#[test]
fn continue_enqueue_failure_does_not_spend_goal_budget() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    workspace_with_goal(root.path())?;
    let request = request(AutomationOutcomePolicy::Continue);
    std::fs::create_dir_all(root.path().join("runtime"))?;
    std::fs::write(root.path().join("runtime/work-payloads"), b"blocked")?;

    // When
    evaluate_and_route(
        root.path(),
        root.path(),
        &request,
        &AutomationJobResult::Succeeded {
            result_ref: "result-1".to_owned(),
        },
    )
    .expect_err("durable continuation enqueue must fail");

    // Then
    let session = SessionManager::open_existing(root.path())?
        .and_then(|manager| manager.load_existing("telegram:chat-1"))
        .ok_or("missing session")?;
    let goal = persistent_goal_from_session(&session).ok_or("missing goal")?;
    assert_eq!(goal.turns_used, 0);
    Ok(())
}

#[test]
fn unsupported_delivery_target_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    workspace_with_goal(root.path())?;
    let mut request = request(AutomationOutcomePolicy::Notify);
    request.session_key = "unsupported".to_owned();

    // When
    let error = evaluate_and_route(
        root.path(),
        root.path(),
        &request,
        &AutomationJobResult::Succeeded {
            result_ref: "result-1".to_owned(),
        },
    )
    .expect_err("missing target must fail closed");

    // Then
    assert!(error.contains("target surface"));
    Ok(())
}
