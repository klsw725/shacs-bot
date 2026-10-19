use super::support::{Fixture, Reader, TestResult};
use shacs_core::runtime::*;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::{evaluate_durable_work_recovery, ReplayWorkState};

#[test]
fn stale_lease_requires_recovery_and_never_invents_observation() -> TestResult {
    let fixture = Fixture::new()?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    let state = replay(&fixture);
    let item = state.items.values().next().ok_or("queued work")?;
    dispatcher.lease_work(item, 1000)?;
    let reader = Reader::new(&fixture.workspace);

    let mut restarted = DurableWorkDispatcher::open(
        fixture.data_dir.join("runtime/durable-events"),
        fixture.data_dir.join("runtime/work-payloads"),
        fixture.bus.clone(),
        "restarted-owner",
        120_000,
    )?;
    let result = restarted.process_entrusted_observations(&fixture.runtime(200_000), &reader);

    assert!(matches!(
        result,
        Err(ObservationError::Blocked(ObservationBlock::RecoveryNeeded))
    ));
    assert_eq!(reader.reads.get(), 0);
    assert!(fixture.read()?.history.is_empty());
    let state = replay(&fixture);
    let admission = evaluate_durable_work_recovery(
        &state,
        fixture.data_dir.join("runtime/work-payloads"),
        200_000,
    );
    dispatcher.requeue_stale(&state, &admission)?;
    dispatcher.process_entrusted_observations(&fixture.runtime(200_001), &reader)?;
    assert_eq!(reader.reads.get(), 1);
    assert_eq!(fixture.read()?.history.len(), 1);
    Ok(())
}

#[test]
fn durable_cancellation_during_read_discards_observation_and_next_wake() -> TestResult {
    let fixture = Fixture::new()?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    let mut reader = Reader::new(&fixture.workspace);
    reader.after_read = Some(Box::new(|| {
        let state = replay(&fixture);
        let item = state
            .items
            .values()
            .find(|item| item.state == ReplayWorkState::Leased)
            .expect("lease");
        fixture
            .dispatcher()
            .expect("dispatcher")
            .request_cancellation(item, "user-stop")
            .expect("cancel");
    }));

    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    let snapshot = fixture.read()?;
    assert!(snapshot.history.is_empty());
    assert_eq!(
        snapshot.next_wake,
        ObservationNextWake::Stopped {
            reason: ObservationBlock::Cancelled
        }
    );
    assert_eq!(fixture.bus.outbound_size(), 0);
    Ok(())
}

#[test]
fn stale_configuration_cannot_replace_plan_and_new_conditions_use_new_lineage() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.revise()?;

    let result =
        configure_entrusted_observation(&fixture.workspace, fixture.configuration(2, "other.txt"));
    fixture
        .dispatcher()?
        .enqueue_entrusted_observations(&fixture.runtime(500))?;

    assert!(matches!(
        result,
        Err(ObservationError::Work(
            EntrustedWorkError::RevisionConflict { .. }
        ))
    ));
    let snapshot = fixture.read()?;
    assert_eq!(snapshot.condition_revision, 3);
    let ObservationNextWake::Queued { work_id, .. } = snapshot.next_wake else {
        return Err("expected queued".into());
    };
    assert!(work_id.starts_with(&format!("observation:{}:3:", fixture.work_id)));
    Ok(())
}

#[test]
fn stopped_work_keeps_previous_history_after_new_goal_replaces_binding() -> TestResult {
    let fixture = Fixture::new()?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    dispatcher
        .process_entrusted_observations(&fixture.runtime(1000), &Reader::new(&fixture.workspace))?;
    fixture.transition(EntrustedWorkLifecycleAction::Done)?;

    apply_goal_surface_action(
        &fixture.workspace,
        "cli:owner",
        GoalSurfaceAction::Set {
            text: "unrelated goal".to_owned(),
            turn_budget: 5,
        },
        "later",
    )?;
    dispatcher
        .process_entrusted_observations(&fixture.runtime(6000), &Reader::new(&fixture.workspace))?;

    let snapshot = fixture.read()?;
    assert_eq!(snapshot.history.len(), 1);
    assert_eq!(
        snapshot.next_wake,
        ObservationNextWake::Stopped {
            reason: ObservationBlock::StaleLineage
        }
    );
    Ok(())
}

fn replay(fixture: &Fixture) -> shacs_session::durable_work::DurableWorkReplayState {
    evaluate_durable_recovery(
        fixture.data_dir.join("runtime/durable-events"),
        fixture.data_dir.join("runtime/durable-checkpoints"),
    )
    .state
    .expect("replay")
    .work
}

#[test]
fn concurrent_condition_writer_fences_the_observation_commit() -> TestResult {
    let fixture = Fixture::new()?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    let (read_done, wait_read) = std::sync::mpsc::sync_channel(0);
    let (write_done, wait_write) = std::sync::mpsc::sync_channel(0);
    let timeout = std::time::Duration::from_secs(5);

    std::thread::scope(|scope| {
        let writer_fixture = &fixture;
        let writer = scope.spawn(move || {
            wait_read
                .recv_timeout(timeout)
                .expect("source read completed");
            writer_fixture
                .revise()
                .expect("concurrent revision commits");
            write_done.send(()).expect("revision signal");
        });
        let mut reader = Reader::new(&fixture.workspace);
        reader.after_read = Some(Box::new(|| {
            read_done.send(()).expect("read signal");
            wait_write
                .recv_timeout(timeout)
                .expect("revision completed");
        }));
        let result = dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader);
        writer.join().expect("writer thread");
        result
    })?;

    let snapshot = fixture.read()?;
    assert_eq!(snapshot.condition_revision, 3);
    assert!(snapshot.current_observation.is_none());
    assert!(snapshot.history.is_empty());
    assert!(replay(&fixture)
        .items
        .values()
        .any(|item| item.state == ReplayWorkState::Cancelled));
    Ok(())
}
