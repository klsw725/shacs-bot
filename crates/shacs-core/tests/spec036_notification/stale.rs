use super::support::{Fixture, TestResult, OWNER};
use shacs_core::runtime::*;
use shacs_session::{durable_event::SESSION_TURN_ACCEPTED, SessionManager};

#[test]
fn lifecycle_and_condition_changes_before_enqueue_suppress_notifications() -> TestResult {
    for action in [
        Some(EntrustedWorkLifecycleAction::Pause),
        Some(EntrustedWorkLifecycleAction::Done),
        Some(EntrustedWorkLifecycleAction::Clear),
        None,
    ] {
        let fixture = Fixture::new(false)?;
        fixture.candidate()?;
        fixture.change(action)?;

        fixture.route()?;

        let snapshot = fixture.read()?;
        assert_eq!(
            snapshot.notifications[0].enqueue,
            EntrustedNotificationEnqueueState::Suppressed
        );
        assert_eq!(fixture.bus.outbound_size(), 0);
        assert_eq!(fixture.count(SESSION_TURN_ACCEPTED)?, 0);
    }
    Ok(())
}

#[test]
fn lifecycle_and_condition_changes_before_consumption_leave_history_untouched() -> TestResult {
    for action in [
        Some(EntrustedWorkLifecycleAction::Pause),
        Some(EntrustedWorkLifecycleAction::Done),
        Some(EntrustedWorkLifecycleAction::Clear),
        None,
    ] {
        let fixture = Fixture::new(false)?;
        fixture.candidate()?;
        fixture.route()?;
        let inbound = fixture.inbound()?;
        fixture.change(action)?;
        let mut manager = SessionManager::new(&fixture.workspace)?;
        let before = manager.load_existing(OWNER).ok_or("session")?;

        let result = fixture.consume(inbound)?;

        assert_eq!(result.outbound_count, 0);
        assert_eq!(fixture.bus.outbound_size(), 0);
        assert!(fixture.read()?.notifications[0].suppressed_stale);
        manager.invalidate(OWNER);
        assert_eq!(
            manager.load_existing(OWNER).ok_or("session")?.messages,
            before.messages
        );
        assert_eq!(fixture.count(SESSION_TURN_ACCEPTED)?, 0);
    }
    Ok(())
}

#[test]
fn pending_questions_and_approval_metadata_are_not_consumed_by_synthetic_turn() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.candidate()?;
    fixture.route()?;
    let mut manager = SessionManager::new(&fixture.workspace)?;
    let mut session = manager.load_existing(OWNER).ok_or("session")?;
    session.messages.push(serde_json::json!({"role":"assistant", "tool_calls":[{"id":"question", "type":"function", "function":{"name":"ask_user","arguments":"{\"question\":\"choose\"}"}}]}));
    session.metadata.insert("pending_permission_wizard".to_owned(), serde_json::json!({"session_key":OWNER,"channel":"email","chat_id":"owner@example.test","sender_id":"user","stage":"choose_mode"}));
    session.metadata.insert(
        "pending_permission_approval".to_owned(),
        serde_json::json!({"fixture":"untouched"}),
    );
    manager.save_with_fsync(&session)?;

    let result = fixture.consume(fixture.inbound()?)?;

    assert_eq!(result.outbound_count, 1);
    assert_eq!(result.stop_reason, "entrusted_notification");
    assert!(result.ask_user_options.is_empty());
    manager.invalidate(OWNER);
    let after = manager.load_existing(OWNER).ok_or("session")?;
    assert_eq!(after.messages, session.messages);
    assert_eq!(after.metadata, session.metadata);
    Ok(())
}

#[test]
fn owner_mismatch_cannot_redirect_notification_to_another_session() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.candidate()?;
    fixture.route()?;
    let mut inbound = fixture.inbound()?;
    inbound.session_key_override = Some("email:other@example.test".to_owned());
    inbound.chat_id = "other@example.test".to_owned();

    let result = fixture.consume(inbound)?;

    assert_eq!(result.outbound_count, 0);
    assert_eq!(fixture.bus.outbound_size(), 0);
    assert!(!fixture.read()?.notifications[0].consumption_started);
    Ok(())
}
