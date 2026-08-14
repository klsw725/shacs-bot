#[path = "spec033_automation_results.rs"]
mod fixtures;

use fixtures::{requirements, snapshot, source_event};
use shacs_core::runtime::{
    build_spec033_snapshot_from, enqueue_production_automation, route_task_outcome,
    AutomationOutcomePolicy, AutomationOwnerEffect, AutomationProductionJob,
    AutomationRouteEvidence, AutomationRouteOwners, AutomationScheduleKind,
    AutomationSourceEventKind, AutomationTaskOutcomeDecision, AutomationTaskOutcomeInput,
    AutomationWorkEnvelope, DurableWorkDispatcher, MessageBus, SandboxMode,
};
use shacs_eval::completion_boundary::EvaluatorRoute;
use shacs_eval::evaluator::{AutomationExecutionMode, ProjectionSurface};
use shacs_projection::SandboxFallback;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::{Session, SessionManager};
use std::error::Error;

#[test]
fn production_job_scenarios_enqueue_durable_automation() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let mut dispatcher = DurableWorkDispatcher::open(
        &events,
        root.path().join("payloads"),
        MessageBus::new(),
        "owner",
        100,
    )?;
    let scenarios = [
        (
            "heartbeat",
            AutomationSourceEventKind::Heartbeat,
            AutomationScheduleKind::Recurring,
            Some(10),
        ),
        (
            "one-shot",
            cron_source(),
            AutomationScheduleKind::OneShot,
            Some(20),
        ),
        (
            "recurring",
            cron_source(),
            AutomationScheduleKind::Recurring,
            Some(30),
        ),
        (
            "result",
            subagent_source(),
            AutomationScheduleKind::OneShot,
            None,
        ),
    ];
    for (id, source, schedule, wake) in scenarios {
        let mut event = source_event(AutomationExecutionMode::NoAgentCheck);
        event.runtime_service_event_id = id.to_owned();
        event.job_id = id.to_owned();
        event.source = source;
        let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
        enqueue_production_automation(
            &mut dispatcher,
            AutomationProductionJob {
                work_id: format!("work-{id}"),
                envelope: AutomationWorkEnvelope {
                    event,
                    schedule,
                    existing_runs: Vec::new(),
                    expected_current_facts_digest: snapshot.semantic_compatibility_digest()?,
                    enqueue_provenance_snapshot: Some(snapshot),
                    hook_evidence: None,
                    requirements: requirements(),
                    instruction: Some(id.to_owned()),
                    outcome_policy: AutomationOutcomePolicy::Notify,
                },
                next_wake_at_ms: wake,
            },
        )?;
    }
    let state = evaluate_durable_recovery(&events, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), 4);
    Ok(())
}

#[test]
fn identical_production_job_reenqueue_is_a_durable_noop() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let mut dispatcher = DurableWorkDispatcher::open(
        &events,
        root.path().join("payloads"),
        MessageBus::new(),
        "owner",
        100,
    )?;
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let job = AutomationProductionJob {
        work_id: "stable-work".to_owned(),
        envelope: AutomationWorkEnvelope {
            event: {
                let mut event = source_event(AutomationExecutionMode::NoAgentCheck);
                event.source = subagent_source();
                event
            },
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            expected_current_facts_digest: snapshot.semantic_compatibility_digest()?,
            enqueue_provenance_snapshot: Some(snapshot),
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("stable".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Notify,
        },
        next_wake_at_ms: None,
    };

    enqueue_production_automation(&mut dispatcher, job.clone())?;
    enqueue_production_automation(&mut dispatcher, job)?;

    let state = evaluate_durable_recovery(&events, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), 1);
    assert!(state.work.items.contains_key("stable-work"));
    Ok(())
}

#[test]
fn queued_automation_projects_canonical_ids_and_lineage() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let events = root.path().join("runtime/durable-events");
    let mut dispatcher = DurableWorkDispatcher::open(
        &events,
        root.path().join("runtime/work-payloads"),
        MessageBus::new(),
        "owner",
        100,
    )?;
    let mut event = source_event(AutomationExecutionMode::NoAgentCheck);
    event.job_id = "job-queued".to_owned();
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let snapshot_id = snapshot.snapshot_id.clone();
    dispatcher.enqueue_automation(shacs_core::runtime::AutomationWorkEnqueueInput {
        work_id: "work-queued".to_owned(),
        envelope: AutomationWorkEnvelope {
            event,
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            expected_current_facts_digest: snapshot.semantic_compatibility_digest()?,
            enqueue_provenance_snapshot: Some(snapshot),
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("queued".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Notify,
        },
        next_wake_at_ms: None,
    })?;

    // When
    let projection = build_spec033_snapshot_from(root.path(), root.path(), "session-1")?;

    // Then
    let fact = projection
        .automation
        .fact
        .ok_or("missing queued automation")?;
    assert_eq!(fact.job_id, "job-queued");
    assert!(fact.run_id.starts_with("run-"));
    assert_eq!(fact.snapshot_id.as_deref(), Some(snapshot_id.as_str()));
    assert_eq!(
        projection
            .diagnostics
            .execution_snapshot_id
            .value
            .as_deref(),
        Some(snapshot_id.as_str())
    );
    assert_eq!(
        projection.diagnostics.automation_job_id.value.as_deref(),
        Some("job-queued")
    );
    assert_eq!(
        projection
            .diagnostics
            .execution_snapshot_digest
            .value
            .as_deref(),
        fact.snapshot_digest.as_deref()
    );
    assert_eq!(
        fact.job_status,
        shacs_projection::Spec033AutomationJobStatus::Pending
    );
    assert!(projection
        .automation
        .lineage
        .evidence_refs
        .iter()
        .any(|reference| reference == "durable_work:work-queued"));
    Ok(())
}

#[test]
fn malformed_improvement_store_preserves_session_goal_projection() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    manager.save(&Session::new("session-1"))?;
    shacs_core::runtime::apply_goal_surface_action(
        root.path(),
        "session-1",
        shacs_core::runtime::GoalSurfaceAction::Set {
            text: "preserve valid goal".to_owned(),
            turn_budget: 3,
        },
        "1",
    )?;
    std::fs::create_dir_all(root.path().join(".shacs-self-improvement"))?;
    std::fs::write(
        root.path().join(".shacs-self-improvement/store.json"),
        b"not-json",
    )?;

    // When
    let projection = build_spec033_snapshot_from(root.path(), root.path(), "session-1")?;

    // Then
    assert_eq!(
        projection.goal.availability,
        shacs_projection::Spec033Availability::Available
    );
    assert_eq!(
        projection.self_improvement.availability,
        shacs_projection::Spec033Availability::Unavailable
    );
    Ok(())
}

#[test]
fn six_advisory_routes_use_explicit_owner_consumers() -> Result<(), Box<dyn Error>> {
    let mut owners = RecordingOwners::default();
    for route in [
        EvaluatorRoute::Notify,
        EvaluatorRoute::Suppress,
        EvaluatorRoute::Continue,
        EvaluatorRoute::Escalate,
        EvaluatorRoute::Verify,
        EvaluatorRoute::RollbackCandidate,
    ] {
        let record = route_task_outcome(
            AutomationTaskOutcomeDecision {
                route,
                evaluator_evidence_ref: "evaluator:1".to_owned(),
            },
            &outcome_input(),
            &mut owners,
        )?;
        assert!(!record.owner_evidence_ref.is_empty());
    }
    assert_eq!(owners.routes.len(), 6);
    assert_eq!(
        owners.routes.last(),
        Some(&EvaluatorRoute::RollbackCandidate)
    );
    Ok(())
}

#[test]
fn unsupported_owner_effect_fails_closed() {
    let input = outcome_input();
    let error = route_task_outcome(
        AutomationTaskOutcomeDecision {
            route: EvaluatorRoute::Verify,
            evaluator_evidence_ref: "evaluator:1".to_owned(),
        },
        &input,
        &mut WrongEffectOwner,
    )
    .expect_err("mismatched effect must fail closed");
    assert!(error.contains("supported durable effect"));
}

#[test]
fn production_evaluator_selects_all_six_typed_policies() {
    use shacs_core::runtime::{
        AutomationJobResult, AutomationTaskOutcomeEvaluator,
        ConservativeAutomationTaskOutcomeEvaluator,
    };

    let evaluator = ConservativeAutomationTaskOutcomeEvaluator;
    for route in [
        EvaluatorRoute::Notify,
        EvaluatorRoute::Suppress,
        EvaluatorRoute::Continue,
        EvaluatorRoute::Escalate,
        EvaluatorRoute::Verify,
        EvaluatorRoute::RollbackCandidate,
    ] {
        let mut input = outcome_input();
        input.policy = AutomationOutcomePolicy::from(route);
        assert_eq!(
            evaluator
                .evaluate(
                    &input,
                    &AutomationJobResult::Succeeded {
                        result_ref: "result:1".to_owned(),
                    },
                )
                .route,
            route
        );
    }
}

#[test]
fn continue_route_requires_goal_and_remaining_budget() {
    use shacs_core::runtime::{
        AutomationJobResult, AutomationTaskOutcomeEvaluator,
        ConservativeAutomationTaskOutcomeEvaluator,
    };

    let evaluator = ConservativeAutomationTaskOutcomeEvaluator;
    let mut input = outcome_input();
    input.policy = AutomationOutcomePolicy::Continue;
    input.continuation_budget_remaining = 0;
    assert_eq!(
        evaluator
            .evaluate(&input, &AutomationJobResult::Pending)
            .route,
        EvaluatorRoute::Suppress
    );
}

#[test]
fn continue_route_stops_when_user_interrupted() {
    use shacs_core::runtime::{
        AutomationJobResult, AutomationTaskOutcomeEvaluator,
        ConservativeAutomationTaskOutcomeEvaluator,
    };

    let evaluator = ConservativeAutomationTaskOutcomeEvaluator;
    let mut input = outcome_input();
    input.policy = AutomationOutcomePolicy::Continue;
    input.user_interrupted = true;

    assert_eq!(
        evaluator
            .evaluate(&input, &AutomationJobResult::Pending)
            .route,
        EvaluatorRoute::Suppress
    );
}

fn cron_source() -> AutomationSourceEventKind {
    AutomationSourceEventKind::Cron {
        approved_automation_rule_ref: Some("cron-rule:1".to_owned()),
    }
}

fn subagent_source() -> AutomationSourceEventKind {
    AutomationSourceEventKind::SubagentResult {
        merge_state: shacs_core::runtime::SubagentMergeState::Terminal,
        result_ref: "subagent-result:1".to_owned(),
    }
}

fn outcome_input() -> AutomationTaskOutcomeInput {
    AutomationTaskOutcomeInput {
        work_id: "work-1".to_owned(),
        session_key: "session-1".to_owned(),
        result_ref: "result:1".to_owned(),
        source: shacs_eval::evaluator::EvaluationTriggerSource::ScheduledJob,
        target_surface: Some(ProjectionSurface::Channel),
        policy: AutomationOutcomePolicy::Notify,
        user_interrupted: false,
        goal_id: Some("goal-1".to_owned()),
        continuation_budget_remaining: 1,
        owner_target_ref: None,
    }
}

#[derive(Default)]
struct RecordingOwners {
    routes: Vec<EvaluatorRoute>,
}

struct WrongEffectOwner;

impl AutomationRouteOwners for WrongEffectOwner {
    fn notify(
        &mut self,
        _: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        unreachable!()
    }
    fn suppress(
        &mut self,
        _: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        unreachable!()
    }
    fn continue_task(
        &mut self,
        _: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        unreachable!()
    }
    fn escalate(
        &mut self,
        _: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        unreachable!()
    }
    fn verify(
        &mut self,
        _: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        Ok(AutomationRouteEvidence {
            owner_evidence_ref: "owner:wrong".to_owned(),
            effect: AutomationOwnerEffect::NoNotification,
        })
    }
    fn rollback_candidate(
        &mut self,
        _: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        unreachable!()
    }
}

impl AutomationRouteOwners for RecordingOwners {
    fn notify(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record(EvaluatorRoute::Notify, input)
    }
    fn suppress(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record(EvaluatorRoute::Suppress, input)
    }
    fn continue_task(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record(EvaluatorRoute::Continue, input)
    }
    fn escalate(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record(EvaluatorRoute::Escalate, input)
    }
    fn verify(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record(EvaluatorRoute::Verify, input)
    }
    fn rollback_candidate(
        &mut self,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.record(EvaluatorRoute::RollbackCandidate, input)
    }
}

impl RecordingOwners {
    fn record(
        &mut self,
        route: EvaluatorRoute,
        input: &AutomationTaskOutcomeInput,
    ) -> Result<AutomationRouteEvidence, String> {
        self.routes.push(route);
        Ok(AutomationRouteEvidence {
            owner_evidence_ref: format!("owner:{}", input.work_id),
            effect: match route {
                EvaluatorRoute::Notify => AutomationOwnerEffect::DeliveryRequest,
                EvaluatorRoute::Suppress => AutomationOwnerEffect::NoNotification,
                EvaluatorRoute::Continue => AutomationOwnerEffect::ContinuationEnqueue,
                EvaluatorRoute::Escalate => AutomationOwnerEffect::DurableEscalation,
                EvaluatorRoute::Verify => AutomationOwnerEffect::VerificationRequest,
                EvaluatorRoute::RollbackCandidate => AutomationOwnerEffect::RollbackCandidateRecord,
            },
        })
    }
}
