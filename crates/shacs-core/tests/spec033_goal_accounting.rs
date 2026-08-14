use shacs_core::runtime::{
    apply_completion_verdict, clear_goal, consume_evaluator_decision, create_persistent_goal,
    mark_goal_done, pause_goal, record_goal_stop, resume_goal, EvaluatorDecisionInput,
    GoalCompletionVerdict, GoalEvidenceAvailability, GoalObservedState, GoalStopReason,
    LedgerConsumptionStatus, RuntimePolicyGateResults, RuntimeSelectedAction,
};
use shacs_eval::evaluator::{
    EvaluationTriggerSource, EvaluatorKind, RedactionStatus, SuggestedNextAction,
};

#[test]
fn goal_helpers_record_canonical_transition_accounting() {
    let goal = create_persistent_goal("session-1", "ship it", "t0", 2);
    let paused = pause_goal(&goal, "t1").expect("active goal pauses");
    let resumed = resume_goal(&paused, "t2").expect("paused goal resumes");
    let continued = apply_completion_verdict(&resumed, GoalCompletionVerdict::Continue, None, "t3")
        .expect("active goal continues");
    let done = mark_goal_done(&continued, "t4").expect("active goal is user-marked done");
    let cleared = clear_goal(&done, "t5").expect("done goal clears");

    let facts = [
        goal.last_transition.as_ref().expect("set transition"),
        paused.last_transition.as_ref().expect("pause transition"),
        resumed.last_transition.as_ref().expect("resume transition"),
        continued
            .last_transition
            .as_ref()
            .expect("continue transition"),
        done.last_transition.as_ref().expect("done transition"),
        cleared.last_transition.as_ref().expect("clear transition"),
    ];

    assert_eq!(facts[0].prior_state, GoalObservedState::Unavailable);
    assert_eq!(facts[1].prior_state, GoalObservedState::Active);
    assert_eq!(facts[1].current_state, GoalObservedState::Paused);
    assert_eq!(facts[1].stop_reason, GoalStopReason::PausedByUser);
    assert_eq!(facts[3].budget.turns_used, 1);
    assert_eq!(facts[3].budget.remaining_turns, 1);
    assert_eq!(facts[3].observed_at, "t3");
    assert_eq!(done.transitions.len(), 5);
    assert_eq!(
        done.transitions[4].stop_reason,
        GoalStopReason::MarkedDoneByUser
    );
    assert_eq!(
        continued.transitions[3].stop_reason,
        GoalStopReason::EvaluatorContinuationAccepted
    );
    assert!(facts.iter().all(
        |fact| fact.goal_id == goal.id && fact.evidence == GoalEvidenceAvailability::Available
    ));
}

#[test]
fn illegal_transition_fails_without_mutating_goal_or_evidence() {
    let goal = create_persistent_goal("session-1", "ship it", "t0", 2);
    let before = goal.clone();

    let error = resume_goal(&goal, "t1").expect_err("active goal cannot resume");

    assert_eq!(goal, before);
    assert_eq!(error.prior_state(), GoalObservedState::Active);
    assert_eq!(goal.transitions.len(), 1);
}

#[test]
fn interruption_and_budget_stops_are_append_only_observable_facts() {
    let goal = create_persistent_goal("session-1", "ship it", "t0", 1);
    let interrupted = record_goal_stop(&goal, GoalStopReason::UserInterrupted, true, "t1")
        .expect("active interruption is recorded");
    let exhausted = record_goal_stop(
        &interrupted,
        GoalStopReason::ContinuationBudgetExhausted,
        false,
        "t2",
    )
    .expect("active budget stop is recorded");

    assert_eq!(exhausted.status, goal.status);
    assert_eq!(exhausted.transitions.len(), 3);
    assert!(exhausted.transitions[1].user_interrupted);
    assert_eq!(
        exhausted.transitions[2].stop_reason,
        GoalStopReason::ContinuationBudgetExhausted
    );
}

#[test]
fn missing_evaluator_evidence_cannot_mark_goal_done() {
    let goal = create_persistent_goal("session-1", "ship it", "t0", 2);
    let input = EvaluatorDecisionInput {
        verdict_id: "verdict-1".to_owned(),
        evaluator_kind: EvaluatorKind::GoalCompletion,
        evaluator_version: "eval-v1".to_owned(),
        source_ledger_ref: String::new(),
        frozen_snapshot_digest: "snapshot".to_owned(),
        current_target_snapshot_digest: "snapshot".to_owned(),
        goal_id: Some(goal.id.clone()),
        turn_id: Some("turn-1".to_owned()),
        expires_at_ms: None,
        suggested_action: SuggestedNextAction::None,
        confidence: 1.0,
        evidence_refs: Vec::new(),
        redaction_status: RedactionStatus::AlreadySafe,
        explicit_goal_completion_verdict: Some(GoalCompletionVerdict::Done),
        blocked_reason: None,
        unblock_hint: None,
        created_at_ms: 10,
        correlation_id: "corr-1".to_owned(),
        superseding_verdict_ref: None,
        task_outcome_class: None,
    };
    let gates = RuntimePolicyGateResults {
        now_ms: 10,
        ..RuntimePolicyGateResults::all_passed()
    };

    let (decision, consumption) = consume_evaluator_decision(&input, Some(&goal), &[], &gates);

    assert_eq!(consumption.status, LedgerConsumptionStatus::FailedToApply);
    assert_eq!(decision.selected_action, RuntimeSelectedAction::None);
    assert!(decision.next_goal_state.is_none());
}

#[test]
fn goal_evaluation_request_preserves_declared_trigger_source(
) -> Result<(), Box<dyn std::error::Error>> {
    let goal = create_persistent_goal("session-1", "ship it", "t0", 2);

    let request = shacs_core::runtime::build_goal_completion_evaluation_request(
        &goal,
        EvaluationTriggerSource::SessionTurn,
        10,
    )?;

    assert_eq!(request.request.source, EvaluationTriggerSource::SessionTurn);
    Ok(())
}

#[test]
fn goal_snapshot_exposes_bounded_usage_derived_from_goal_owner(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut manager = shacs_session::SessionManager::new(root.path())?;
    manager.save(&shacs_session::Session::new("session-1"))?;
    shacs_core::runtime::apply_goal_surface_action(
        root.path(),
        "session-1",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "ship it".to_owned(),
            turn_budget: 2,
        },
        "t0",
    )?;

    let snapshot = shacs_core::runtime::build_spec033_snapshot(root.path(), "session-1")?;
    let fact = snapshot.goal.fact.ok_or("missing goal fact")?;

    assert_eq!(fact.usage.turn_limit, 2);
    assert_eq!(fact.usage.turns_used, 0);
    assert_eq!(fact.usage.remaining_turns, 2);
    assert!(!fact.usage.exhausted);
    assert_eq!(
        snapshot.diagnostics.goal_id.value.as_deref(),
        Some(fact.goal_id.as_str())
    );
    Ok(())
}
