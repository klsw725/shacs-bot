use super::support::{Fixture, TestResult};
use shacs_core::runtime::*;
use shacs_session::durable_event::*;

#[test]
fn interrupted_consumption_is_unknown_and_not_republished_after_reopen() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.candidate()?;
    fixture.route()?;
    let inbound = fixture.inbound()?;
    let intent = fixture.read()?.notifications[0].intent.clone();
    let mut store = DurableEventStore::open(fixture.events())?;
    store.append(DurableEventInput::new(
        &intent.owner_session_id,
        ENTRUSTED_NOTIFICATION_FACT,
        DurableEventPayload::inline(
            "entrusted_notification.v1",
            serde_json::to_value(EntrustedNotificationFact {
                intent: intent.clone(),
                stage: EntrustedNotificationStage::Consuming,
                writer_instance_ref: "prior-process".to_owned(),
            })?,
        ),
    ))?;
    store.append(DurableEventInput::new(
        &intent.owner_session_id,
        ENTRUSTED_NOTIFICATION_FACT,
        DurableEventPayload::inline(
            "entrusted_notification.v1",
            serde_json::to_value(EntrustedNotificationFact {
                intent: intent.clone(),
                stage: EntrustedNotificationStage::OutboundPending,
                writer_instance_ref: "prior-process".to_owned(),
            })?,
        ),
    ))?;

    let result = fixture.consume(inbound)?;

    assert_eq!(result.outbound_count, 0);
    assert_eq!(
        fixture.read()?.notifications[0].delivery,
        EntrustedNotificationDeliveryState::Unknown
    );
    assert_eq!(fixture.bus.outbound_size(), 0);
    Ok(())
}

#[test]
fn untrusted_json_cannot_restore_owner_accepted_notification_marker() -> TestResult {
    let inbound = InboundMessage::new("email", "user", "owner", "hello")
        .with_owner_accepted_notification("entrusted-notification:fake".to_owned());

    let restored: InboundMessage = serde_json::from_value(serde_json::to_value(inbound)?)?;

    assert!(restored.owner_accepted_notification().is_none());
    Ok(())
}

#[test]
fn blocked_observation_never_creates_notification_intent() -> TestResult {
    struct Denied;
    impl EntrustedObservationExecutor for Denied {
        fn prepare(&self, _: &ObservationWake) -> Result<String, ObservationBlock> {
            Err(ObservationBlock::HookDenied)
        }
        fn observe(&self, _: &ObservationWake) -> ObservationResult {
            panic!("blocked read")
        }
    }
    let fixture = Fixture::new(true)?;
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &Denied)?;

    fixture.route()?;

    assert_eq!(
        fixture.read()?.current_decision.ok_or("decision")?.verdict,
        ObservationVerdict::Blocked
    );
    assert!(fixture.read()?.notifications.is_empty());
    Ok(())
}
