#[path = "spec036_notification/recovery.rs"]
mod recovery;
#[path = "spec036_notification/stale.rs"]
mod stale;
#[path = "spec036_notification/support.rs"]
mod support;

use shacs_core::runtime::*;
use shacs_session::durable_event::{SESSION_TURN_ACCEPTED, WORK_ENQUEUED};
use support::{Fixture, TestResult};

#[test]
fn only_significant_change_routes_through_durable_session_turn() -> TestResult {
    let fixture = Fixture::new(false)?;
    for (time, value) in [(1000, 120), (6000, 120), (11000, 110)] {
        fixture.observe(time, value)?;
        fixture.route()?;
        assert!(fixture.read()?.notifications.is_empty());
    }
    fixture.observe(16000, 90)?;

    fixture.route()?;
    let result = fixture.consume(fixture.inbound()?)?;

    assert_eq!(result.outbound_count, 1);
    assert!(result.tools_used.is_empty());
    assert_eq!(fixture.count(SESSION_TURN_ACCEPTED)?, 1);
    let outbound = fixture.bus.consume_outbound().ok_or("outbound")?;
    assert!(outbound.content.len() < 1600);
    let decision = fixture.read()?.current_decision.ok_or("decision")?;
    for evidence in [
        decision.previous.ok_or("previous")?,
        decision.current.ok_or("current")?,
    ] {
        assert!(outbound
            .content
            .contains(&evidence.summary.value.to_string()));
        assert!(outbound.content.contains(&evidence.evidence_ref));
    }
    assert_eq!(
        outbound.metadata[ENTRUSTED_NOTIFICATION_CORRELATION],
        fixture.read()?.notifications[0].intent.correlation_id
    );
    assert_eq!(
        fixture.read()?.notifications[0].delivery,
        EntrustedNotificationDeliveryState::Pending
    );
    Ok(())
}

#[test]
fn same_event_retry_and_reopened_dispatcher_never_enqueue_or_consume_twice() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.candidate()?;
    fixture.route()?;
    let inbound = fixture.inbound()?;
    fixture.consume(inbound.clone())?;
    let count = fixture.count(WORK_ENQUEUED)?;

    fixture.route()?;
    let repeated = fixture.consume(inbound)?;

    assert_eq!(fixture.count(WORK_ENQUEUED)?, count);
    assert_eq!(repeated.outbound_count, 0);
    assert_eq!(fixture.count(SESSION_TURN_ACCEPTED)?, 1);
    assert_eq!(fixture.bus.outbound_size(), 1);
    Ok(())
}

#[test]
fn a_new_condition_crossing_has_a_distinct_notification_identity() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.candidate()?;
    fixture.route()?;

    fixture.observe(11000, 130)?;
    fixture.route()?;

    let snapshots = fixture.read()?.notifications;
    assert_eq!(snapshots.len(), 2);
    assert_ne!(
        snapshots[0].intent.correlation_id,
        snapshots[1].intent.correlation_id
    );
    assert!(snapshots
        .iter()
        .all(|value| value.enqueue == EntrustedNotificationEnqueueState::Enqueued));
    Ok(())
}

#[test]
fn requested_summary_keeps_suppression_distinct_from_change_notification() -> TestResult {
    let fixture = Fixture::new(true)?;
    fixture.observe(1000, 120)?;
    fixture.observe(6000, 120)?;

    fixture.route()?;

    let snapshot = fixture.read()?;
    assert_eq!(
        snapshot.current_decision.ok_or("decision")?.verdict,
        ObservationVerdict::Suppress
    );
    assert_eq!(snapshot.notifications.len(), 2);
    assert!(snapshot
        .notifications
        .iter()
        .all(|value| value.intent.kind == EntrustedNotificationKind::RegularSummary));
    Ok(())
}

#[test]
fn transport_failure_remains_separate_from_successful_observation_and_consumption() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.candidate()?;
    fixture.route()?;
    fixture.consume(fixture.inbound()?)?;
    let outbound = fixture.bus.consume_outbound().ok_or("outbound")?;

    record_entrusted_notification_delivery(
        &fixture.events(),
        &outbound,
        EntrustedNotificationTransportFact::Failed,
    )?;

    let snapshot = fixture.read()?;
    assert_eq!(
        snapshot.notifications[0].delivery,
        EntrustedNotificationDeliveryState::Failed
    );
    assert_eq!(
        snapshot.notifications[0].enqueue,
        EntrustedNotificationEnqueueState::Enqueued
    );
    assert!(snapshot.notifications[0].consumption_started);
    assert!(matches!(
        snapshot.current_observation.ok_or("observation")?.result,
        ObservationResult::Observed { .. }
    ));
    Ok(())
}

#[test]
fn competing_production_ticks_do_not_reclaim_a_consumed_observation() -> TestResult {
    struct CompetingTick<'a>(&'a Fixture);
    impl EntrustedObservationExecutor for CompetingTick<'_> {
        fn prepare(&self, _: &ObservationWake) -> Result<String, ObservationBlock> {
            self.0.observe(6000, 90).expect("competing observation");
            Ok("snapshot:outer".to_owned())
        }
        fn observe(&self, _: &ObservationWake) -> ObservationResult {
            panic!("already consumed observation must not execute again")
        }
    }
    let fixture = Fixture::new(false)?;
    fixture.observe(1000, 120)?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &CompetingTick(&fixture))?;

    assert_eq!(fixture.read()?.history.len(), 2);
    Ok(())
}
