use super::support::{Fixture, Reader, TestResult};
use shacs_core::runtime::*;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::ReplayWorkState;
use shacs_session::{SessionManager, SessionMutationGuard};

#[test]
fn inactive_work_cannot_configure_or_enqueue() -> TestResult {
    for action in [
        EntrustedWorkLifecycleAction::Pause,
        EntrustedWorkLifecycleAction::Done,
        EntrustedWorkLifecycleAction::Clear,
    ] {
        let fixture = Fixture::new()?;
        fixture.transition(action)?;

        let configured = configure_entrusted_observation(
            &fixture.workspace,
            fixture.configuration(2, "source.txt"),
        );
        fixture
            .dispatcher()?
            .enqueue_entrusted_observations(&fixture.runtime(500))?;

        assert!(matches!(
            configured,
            Err(ObservationError::Blocked(ObservationBlock::GoalInactive))
        ));
        assert_eq!(
            fixture.read()?.next_wake,
            ObservationNextWake::Stopped {
                reason: ObservationBlock::GoalInactive
            }
        );
        assert!(replay(&fixture).items.is_empty());
    }
    Ok(())
}

#[test]
fn inactive_due_work_is_cancelled_without_reading() -> TestResult {
    for action in [
        EntrustedWorkLifecycleAction::Pause,
        EntrustedWorkLifecycleAction::Done,
        EntrustedWorkLifecycleAction::Clear,
    ] {
        let fixture = Fixture::new()?;
        let mut dispatcher = fixture.dispatcher()?;
        dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
        fixture.transition(action)?;
        let reader = Reader::new(&fixture.workspace);

        dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

        assert_eq!(reader.reads.get(), 0);
        assert!(fixture.read()?.history.is_empty());
        assert!(replay(&fixture)
            .items
            .values()
            .all(|item| item.state == ReplayWorkState::Cancelled));
    }
    Ok(())
}

#[test]
fn condition_change_during_prepare_cancels_old_due_before_read() -> TestResult {
    let fixture = Fixture::new()?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    let mut reader = Reader::new(&fixture.workspace);
    reader.before_read = Some(Box::new(|| fixture.revise().expect("revise")));

    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    assert_eq!(reader.reads.get(), 0);
    assert!(fixture.read()?.current_observation.is_none());
    assert!(replay(&fixture)
        .items
        .values()
        .any(|item| item.state == ReplayWorkState::Cancelled));
    Ok(())
}

#[test]
fn condition_change_after_read_cannot_commit_old_observation() -> TestResult {
    let fixture = Fixture::new()?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    let mut reader = Reader::new(&fixture.workspace);
    reader.after_read = Some(Box::new(|| fixture.revise().expect("revise")));

    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    assert_eq!(reader.reads.get(), 1);
    assert!(fixture.read()?.history.is_empty());
    assert!(replay(&fixture)
        .items
        .values()
        .any(|item| item.state == ReplayWorkState::Cancelled));
    assert_eq!(fixture.bus.outbound_size(), 0);
    Ok(())
}

#[test]
fn pause_done_clear_after_read_prevent_observation_commit_and_next_wake() -> TestResult {
    for action in [
        EntrustedWorkLifecycleAction::Pause,
        EntrustedWorkLifecycleAction::Done,
        EntrustedWorkLifecycleAction::Clear,
    ] {
        let fixture = Fixture::new()?;
        let mut dispatcher = fixture.dispatcher()?;
        dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
        let mut reader = Reader::new(&fixture.workspace);
        reader.after_read = Some(Box::new(|| fixture.transition(action).expect("transition")));

        dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

        assert!(fixture.read()?.history.is_empty());
        assert!(replay(&fixture)
            .items
            .values()
            .all(|item| item.state == ReplayWorkState::Cancelled));
        assert_eq!(fixture.bus.outbound_size(), 0);
    }
    Ok(())
}

#[test]
fn revoked_consent_cannot_enqueue() -> TestResult {
    let fixture = Fixture::new()?;
    {
        let _guard = SessionMutationGuard::acquire(&fixture.workspace, "cli:owner")?;
        let mut manager = SessionManager::new(&fixture.workspace)?;
        let mut session = manager.load_existing("cli:owner").ok_or("session")?;
        session.metadata[ENTRUSTED_WORK_METADATA_KEY]["works"][0]["consent"]["evidence_ref"] =
            serde_json::json!("");
        manager.save_with_fsync(&session)?;
    }

    fixture
        .dispatcher()?
        .enqueue_entrusted_observations(&fixture.runtime(500))?;

    assert!(replay(&fixture).items.is_empty());
    assert_eq!(
        fixture.read()?.next_wake,
        ObservationNextWake::Stopped {
            reason: ObservationBlock::ConsentMissing
        }
    );
    Ok(())
}

#[test]
fn denied_execution_gate_records_blocked_without_read_or_retry() -> TestResult {
    let fixture = Fixture::new()?;
    let mut reader = Reader::new(&fixture.workspace);
    reader.gate = Err(ObservationBlock::HookDenied);
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;

    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    assert_eq!(reader.reads.get(), 0);
    assert_eq!(
        fixture
            .read()?
            .current_observation
            .ok_or("observation")?
            .result,
        ObservationResult::Blocked {
            reason: ObservationBlock::HookDenied
        }
    );
    assert_eq!(
        fixture.read()?.next_wake,
        ObservationNextWake::Stopped {
            reason: ObservationBlock::HookDenied
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
    .expect("durable replay")
    .work
}

#[test]
fn revoked_consent_at_due_and_after_read_never_commits() -> TestResult {
    for revoke_after_read in [false, true] {
        let fixture = Fixture::new()?;
        let mut dispatcher = fixture.dispatcher()?;
        dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
        let revoke = || {
            let _guard =
                SessionMutationGuard::acquire(&fixture.workspace, "cli:owner").expect("guard");
            let mut manager = SessionManager::new(&fixture.workspace).expect("manager");
            let mut session = manager.load_existing("cli:owner").expect("session");
            session.metadata[ENTRUSTED_WORK_METADATA_KEY]["works"][0]["consent"]["evidence_ref"] =
                serde_json::json!("");
            manager.save_with_fsync(&session).expect("revoke");
        };
        let mut reader = Reader::new(&fixture.workspace);
        if revoke_after_read {
            reader.after_read = Some(Box::new(revoke));
        } else {
            revoke();
        }

        dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

        assert_eq!(reader.reads.get(), u32::from(revoke_after_read));
        assert!(fixture.read()?.history.is_empty());
        assert!(replay(&fixture)
            .items
            .values()
            .all(|item| item.state == ReplayWorkState::Cancelled));
    }
    Ok(())
}
