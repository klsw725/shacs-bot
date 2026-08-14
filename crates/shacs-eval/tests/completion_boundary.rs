use shacs_eval::completion_boundary::{
    record_evaluator_boundary, DeliveryOutcome, EvaluatorBoundaryContext,
    EvaluatorBoundaryRecordInput, EvaluatorRoute, EvaluatorRouteStopReason, OwnerResultLocator,
    TaskResultOutcome,
};
use shacs_eval::evaluator::{
    EvaluationTriggerSource, EvaluatorRequestEnvelope, EvaluatorVerdictEnvelope, RedactionStatus,
    SuggestedNextAction, VerdictKind,
};

#[test]
fn every_declared_trigger_records_advisory_boundary_owner_facts() {
    let sources = [
        EvaluationTriggerSource::SessionTurn,
        EvaluationTriggerSource::ScheduledJob,
        EvaluationTriggerSource::Heartbeat,
        EvaluationTriggerSource::Subagent,
        EvaluationTriggerSource::AppTask,
        EvaluationTriggerSource::Channel,
        EvaluationTriggerSource::LocalApi,
        EvaluationTriggerSource::ManualReplay,
    ];

    for source in sources {
        let record = record_evaluator_boundary(EvaluatorBoundaryRecordInput {
            input: request(source.clone()),
            output: verdict(),
            requested_route: EvaluatorRoute::Notify,
            owner_result_locator: OwnerResultLocator::new(format!("owner://{source:?}")),
            task_outcome: TaskResultOutcome::Succeeded,
            delivery_outcome: DeliveryOutcome::Pending,
            context: EvaluatorBoundaryContext {
                user_interrupted: false,
                continuation_budget_remaining: 1,
            },
        });

        assert_eq!(record.input.source, source);
        assert_eq!(record.output.verdict_kind, VerdictKind::Pass);
        assert_eq!(record.route, EvaluatorRoute::Notify);
        assert!(!record.owner_result_locator.as_str().is_empty());
        assert!(!record.grants_execution_authority());
    }
}

#[test]
fn interruption_and_budget_bound_continuation_route() {
    for context in [
        EvaluatorBoundaryContext {
            user_interrupted: true,
            continuation_budget_remaining: 2,
        },
        EvaluatorBoundaryContext {
            user_interrupted: false,
            continuation_budget_remaining: 0,
        },
    ] {
        let expected_stop = if context.user_interrupted {
            EvaluatorRouteStopReason::UserInterrupted
        } else {
            EvaluatorRouteStopReason::ContinuationBudgetExhausted
        };
        let record = record_evaluator_boundary(EvaluatorBoundaryRecordInput {
            input: request(EvaluationTriggerSource::SessionTurn),
            output: verdict(),
            requested_route: EvaluatorRoute::Continue,
            owner_result_locator: OwnerResultLocator::new("owner://turn-1"),
            task_outcome: TaskResultOutcome::Succeeded,
            delivery_outcome: DeliveryOutcome::NotRequested,
            context,
        });

        assert_eq!(record.route, EvaluatorRoute::Suppress);
        assert_eq!(record.route_stop_reason, Some(expected_stop));
    }
}

#[test]
fn task_and_delivery_outcomes_remain_distinct() {
    let record = record_evaluator_boundary(EvaluatorBoundaryRecordInput {
        input: request(EvaluationTriggerSource::Channel),
        output: verdict(),
        requested_route: EvaluatorRoute::Notify,
        owner_result_locator: OwnerResultLocator::new("owner://channel-result-1"),
        task_outcome: TaskResultOutcome::Succeeded,
        delivery_outcome: DeliveryOutcome::Failed,
        context: EvaluatorBoundaryContext {
            user_interrupted: false,
            continuation_budget_remaining: 0,
        },
    });

    assert_eq!(record.task_outcome, TaskResultOutcome::Succeeded);
    assert_eq!(record.delivery_outcome, DeliveryOutcome::Failed);
}

fn request(source: EvaluationTriggerSource) -> EvaluatorRequestEnvelope {
    EvaluatorRequestEnvelope {
        request_id: "request-1".to_owned(),
        evaluator_kind: shacs_eval::evaluator::EvaluatorKind::GoalCompletion,
        correlation_id: "corr-1".to_owned(),
        session_id: Some("session-1".to_owned()),
        turn_id: Some("turn-1".to_owned()),
        source,
        snapshot_digest: "snapshot-1".to_owned(),
        redaction_profile: "default".to_owned(),
        caller_intent: "advisory".to_owned(),
    }
}

fn verdict() -> EvaluatorVerdictEnvelope {
    EvaluatorVerdictEnvelope {
        verdict_kind: VerdictKind::Pass,
        reason: "evidence supports completion".to_owned(),
        confidence: 0.9,
        evidence_refs: Vec::new(),
        suggested_next_action: SuggestedNextAction::None,
        expires_at_ms: None,
        redaction_status: RedactionStatus::AlreadySafe,
        evaluator_version: "eval-v1".to_owned(),
    }
}
