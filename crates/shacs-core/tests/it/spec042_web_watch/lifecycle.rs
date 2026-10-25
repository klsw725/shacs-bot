use super::support::*;
use shacs_core::runtime::*;

#[test]
fn restart_keeps_state_and_a_late_check_records_one_runtime_gap() -> TestResult {
    // Given: one failed check opened an interval.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.server.set("/item", Page::status(500))?;
    let failed_at = fixture.next_due()?;
    fixture.tick(failed_at)?;
    let before = fixture.status()?;
    let due = fixture.next_due()?;

    // When: a new dispatcher runs three cadences after the due time.
    let late = due + 3 * CADENCE_MS;
    fixture.tick(late)?;

    // Then: one late check, a recorded gap and the same open interval.
    let after = fixture.status()?;
    assert_eq!(after.history.len(), before.history.len() + 1);
    assert_eq!(
        fixture.latest()?.runtime_gap,
        Some(WebRuntimeGap {
            from_ms: due,
            to_ms: late
        })
    );
    assert_eq!(after.runtime_gaps.len(), 1);
    let interval = after.failure_interval.ok_or("no open interval")?;
    assert_eq!(
        (interval.started_at_ms, interval.failure_count),
        (failed_at, 2)
    );
    assert_eq!((after.plan, after.baseline), (before.plan, before.baseline));
    assert_eq!(fixture.next_due()?, late + CADENCE_MS);
    Ok(())
}

#[test]
fn pause_stops_checks_closes_the_interval_and_resume_starts_a_new_baseline() -> TestResult {
    // Given: a web watch with an open interval; 036 local observation is refused for it.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.server.set("/item", Page::status(500))?;
    fixture.tick(fixture.next_due()?)?;
    let local = configure_entrusted_observation(
        &fixture.workspace,
        ConfigureObservationRequest {
            work: fixture.request(),
            expected_revision: 2,
            plan: EntrustedObservationPlan {
                source: ObservationSource::LocalFile {
                    relative_path: "source.json".to_owned(),
                },
                cadence_seconds: 60,
                first_wake_at_ms: T0,
                comparison: None,
            },
            changed_at: T0.to_string(),
        },
    );
    let observations = fixture.status()?.history.len();
    let next = fixture.next_due()?;

    // When: paused before the next check is due.
    fixture.transition(EntrustedWorkLifecycleAction::Pause, next - 1)?;
    fixture.tick(next)?;
    let paused = fixture.status()?;

    // Then
    assert!(matches!(local, Err(ObservationError::InvalidInput(_))));
    assert_eq!(paused.history.len(), observations);
    assert_eq!(
        paused.next_wake,
        ObservationNextWake::Stopped {
            reason: ObservationBlock::GoalInactive
        }
    );
    assert!(paused.failure_interval.is_none());
    assert_eq!(
        paused
            .failure_intervals
            .last()
            .and_then(|interval| interval.closed)
            .map(|closed| closed.reason),
        Some(WebIntervalCloseReason::GoalInactive)
    );

    // When: resumed later; the new lineage checks at once and sets a new baseline.
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;
    let resumed = next + 10 * CADENCE_MS;
    fixture.transition(EntrustedWorkLifecycleAction::Resume, resumed)?;
    fixture.tick(resumed)?;

    // Then
    let record = fixture.latest()?;
    assert_eq!(record.observed_at_ms, resumed);
    assert_eq!(record.runtime_gap, None);
    assert_eq!(
        (record.decision.verdict, record.decision.reason),
        (
            ObservationVerdict::Baseline,
            WebDecisionReason::LineageReset
        )
    );
    Ok(())
}
