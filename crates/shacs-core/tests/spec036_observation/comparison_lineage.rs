use super::comparison::{configured, source};
use super::support::{Fixture, Reader, TestResult};
use shacs_core::runtime::*;

#[test]
fn older_or_conflicting_source_timestamp_cannot_replace_baseline() -> TestResult {
    for (as_of, reason) in [
        (500, ObservationComparisonReason::SourceOutOfOrder),
        (1000, ObservationComparisonReason::SourceTimestampConflict),
    ] {
        let fixture = configured(false)?;
        source(&fixture, 120, 1000)?;
        let reader = Reader::new(&fixture.workspace);
        fixture
            .dispatcher()?
            .process_entrusted_observations(&fixture.runtime(1000), &reader)?;
        let baseline = fixture.read()?.comparison_baseline;
        source(&fixture, 90, as_of)?;

        fixture
            .dispatcher()?
            .process_entrusted_observations(&fixture.runtime(6000), &reader)?;

        let snapshot = fixture.read()?;
        assert_eq!(snapshot.current_decision.ok_or("decision")?.reason, reason);
        assert_eq!(snapshot.comparison_baseline, baseline);
    }
    Ok(())
}

#[test]
fn no_user_criterion_never_makes_digest_change_significant() -> TestResult {
    let fixture = Fixture::new()?;
    fixture
        .dispatcher()?
        .enqueue_entrusted_observations(&fixture.runtime(500))?;
    let reader = Reader::new(&fixture.workspace);
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;
    source(&fixture, 90, 6000)?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &reader)?;

    let decision = fixture.read()?.current_decision.ok_or("decision")?;
    assert_eq!(
        decision.reason,
        ObservationComparisonReason::CriteriaNotConfigured
    );
    assert_eq!(decision.verdict, ObservationVerdict::Unknown);
    assert!(decision.current.is_none());
    Ok(())
}

#[test]
fn condition_cas_during_read_blocks_stale_candidate_and_resets_baseline() -> TestResult {
    let fixture = configured(false)?;
    source(&fixture, 120, 1000)?;
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &Reader::new(&fixture.workspace))?;
    source(&fixture, 90, 6000)?;
    let mut reader = Reader::new(&fixture.workspace);
    reader.after_read = Some(Box::new(|| {
        let mut request = fixture.configuration(3, "source.txt");
        request.plan.comparison = Some(ObservationComparisonPolicy {
            criterion: ObservationCriterion::IntegerAtMost { value: 80 },
            max_age_seconds: 60,
            regular_summary: false,
        });
        configure_entrusted_observation(&fixture.workspace, request).expect("new criterion");
    }));

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &reader)?;

    let snapshot = fixture.read()?;
    assert!(snapshot.current_decision.is_none());
    assert!(snapshot.comparison_baseline.is_none());
    assert_eq!(snapshot.history.len(), 1);
    let blocked = snapshot.decisions.last().ok_or("cancelled decision")?;
    assert_eq!(blocked.verdict, ObservationVerdict::Blocked);
    assert_eq!(
        blocked.reason,
        ObservationComparisonReason::ObservationBlocked {
            reason: ObservationBlock::StaleLineage
        }
    );
    assert_eq!(blocked.condition_revision, 3);
    assert!(matches!(
        blocked.basis,
        ObservationDecisionBasis::CancellationFact { .. }
    ));
    assert_eq!(snapshot.decisions, fixture.read()?.decisions);

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(7000), &Reader::new(&fixture.workspace))?;
    let decision = fixture.read()?.current_decision.ok_or("new decision")?;
    assert_eq!(decision.verdict, ObservationVerdict::Baseline);
    assert_eq!(decision.reason, ObservationComparisonReason::LineageReset);
    assert!(!decision.current.ok_or("summary")?.condition_matches);
    Ok(())
}

#[test]
fn stale_policy_write_does_not_change_criterion_or_summary_opt_in() -> TestResult {
    let fixture = configured(false)?;
    let mut request = fixture.configuration(2, "source.txt");
    request.plan.comparison = Some(ObservationComparisonPolicy {
        criterion: ObservationCriterion::IntegerAtMost { value: 999 },
        max_age_seconds: 60,
        regular_summary: true,
    });

    let result = configure_entrusted_observation(&fixture.workspace, request);

    assert!(matches!(
        result,
        Err(ObservationError::Work(
            EntrustedWorkError::RevisionConflict { .. }
        ))
    ));
    let policy = fixture
        .read()?
        .plan
        .ok_or("plan")?
        .comparison
        .ok_or("policy")?;
    assert_eq!(
        policy.criterion,
        ObservationCriterion::IntegerAtMost { value: 100 }
    );
    assert!(!policy.regular_summary);
    Ok(())
}

#[test]
fn goal_transition_hides_prior_current_decision_until_new_baseline() -> TestResult {
    let fixture = configured(false)?;
    source(&fixture, 120, 1000)?;
    let reader = Reader::new(&fixture.workspace);
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;
    assert!(fixture.read()?.current_decision.is_some());

    fixture.transition(EntrustedWorkLifecycleAction::Pause)?;
    let paused = fixture.read()?;
    assert!(paused.current_observation.is_none());
    assert!(paused.current_decision.is_none());
    assert!(paused.comparison_baseline.is_none());
    assert_eq!(paused.history.len(), 1);

    fixture.transition(EntrustedWorkLifecycleAction::Resume)?;
    assert!(fixture.read()?.current_decision.is_none());
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &reader)?;
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(7000), &reader)?;
    let current = fixture.read()?.current_decision.ok_or("new baseline")?;
    assert_eq!(current.verdict, ObservationVerdict::Baseline);
    assert_eq!(current.reason, ObservationComparisonReason::LineageReset);
    Ok(())
}

#[test]
fn legacy_digest_terminal_is_unknown_and_next_valid_read_sets_baseline() -> TestResult {
    use shacs_session::durable_event::{
        DurableEventInput, DurableEventPayload, DurableEventStore, WORK_TERMINAL,
    };
    use shacs_session::durable_work::{WorkTerminal, WorkTerminalKind};
    let fixture = configured(false)?;
    let events = fixture.data_dir.join("runtime/durable-events");
    let state = shacs_session::durable_replay::evaluate_durable_recovery(
        &events,
        fixture.data_dir.join("runtime/checkpoints"),
    )
    .state
    .ok_or("state")?;
    let item = state.work.items.values().next().ok_or("item")?;
    let mut dispatcher = fixture.dispatcher()?;
    let wake: ObservationWake = serde_json::from_value(dispatcher.read_payload_json(item)?)?;
    dispatcher.lease_work(item, 1000)?;
    let legacy = serde_json::json!({
        "wake":wake, "observed_at_ms":1000,
        "result":{"status":"observed", "evidence_ref":"sha256:legacy", "byte_count":20},
        "execution_snapshot_ref":"snapshot:legacy", "next_wake_at_ms":6000
    });
    DurableEventStore::open(&events)?.append(DurableEventInput::new(
        "cli:owner",
        WORK_TERMINAL,
        DurableEventPayload::inline(
            "durable_work",
            serde_json::to_value(WorkTerminal {
                work_id: item.work_id.clone(),
                terminal_kind: WorkTerminalKind::Succeeded,
                outcome_ref: ENTRUSTED_OBSERVATION_OUTCOME.to_owned(),
                facts: Some(legacy),
            })?,
        ),
    ))?;
    source(&fixture, 90, 6000)?;
    fixture
        .dispatcher()?
        .enqueue_entrusted_observations(&fixture.runtime(1001))?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &Reader::new(&fixture.workspace))?;

    let snapshot = fixture.read()?;
    assert_eq!(snapshot.decisions[0].verdict, ObservationVerdict::Unknown);
    assert_eq!(
        snapshot.decisions[0].reason,
        ObservationComparisonReason::SummaryUnavailable
    );
    assert_eq!(snapshot.decisions[1].verdict, ObservationVerdict::Baseline);
    assert_eq!(
        snapshot.decisions[1].reason,
        ObservationComparisonReason::FirstComparableObservation
    );
    assert!(snapshot.decisions[1].previous.is_none());
    assert_eq!(
        snapshot
            .comparison_baseline
            .ok_or("baseline")?
            .summary
            .value,
        90
    );
    Ok(())
}
