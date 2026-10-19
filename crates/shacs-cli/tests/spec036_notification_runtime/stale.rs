use super::{
    smtp,
    support::{Runtime, TestResult},
};
use shacs_core::runtime::{EntrustedNotificationDeliveryState, EntrustedNotificationEnqueueState};

#[test]
#[ignore = "starts production API/SMTP runtimes to fence pending notification across restart"]
fn paused_pending_notification_is_suppressed_by_restarted_channel_runtime() -> TestResult {
    let sink = smtp::SmtpSink::start()?;
    let mut runtime = Runtime::new(sink.address.port())?;
    runtime.start_api()?;
    runtime.wait("stale-01-baseline", |snapshot| snapshot.history.len() == 1)?;
    runtime.source(90)?;
    let pending = runtime.wait("stale-02-pending", |snapshot| {
        snapshot
            .notifications
            .iter()
            .any(|notification| notification.enqueue == EntrustedNotificationEnqueueState::Enqueued)
    })?;
    assert!(!pending.notifications[0].consumption_started);
    assert_eq!(
        pending.notifications[0].delivery,
        EntrustedNotificationDeliveryState::Pending
    );
    runtime.work(&["pause", &runtime.work_id])?;
    runtime.stop()?;

    runtime.start()?;
    let suppressed = runtime.wait("stale-03-suppressed", |snapshot| {
        snapshot
            .notifications
            .iter()
            .any(|notification| notification.suppressed_stale)
    })?;

    assert_eq!(suppressed.notifications.len(), 1);
    assert!(!suppressed.notifications[0].consumption_started);
    assert_eq!(
        suppressed.notifications[0].delivery,
        EntrustedNotificationDeliveryState::NotRequested
    );
    assert!(sink.messages.try_recv().is_err());
    let session = shacs_session::SessionManager::new(&runtime.workspace)?
        .load_existing(super::support::OWNER)
        .ok_or("session")?;
    assert!(session.messages.is_empty());
    runtime.stop()?;
    println!("STALE_QA_PASS: production serve enqueue -> pause -> runtime start consumption; no SMTP or assistant history");
    Ok(())
}

#[test]
#[ignore = "runs explicit summary opt-in through the production SMTP surface"]
fn explicit_regular_summary_does_not_turn_suppress_into_change_alert() -> TestResult {
    let sink = smtp::SmtpSink::start()?;
    let mut runtime = Runtime::new(sink.address.port())?;
    runtime.work(&[
        "observe",
        &runtime.work_id,
        "--source",
        "source.json",
        "--cadence-seconds",
        "5",
        "--expected-revision",
        "2",
        "--at-most",
        "100",
        "--max-age-seconds",
        "3600",
        "--regular-summary",
        "true",
    ])?;

    runtime.start()?;
    let summaries = runtime.wait("summary-01-suppressed-with-summary", |snapshot| {
        snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.delivery == EntrustedNotificationDeliveryState::SentHint
            })
            .count()
            >= 2
    })?;

    assert_eq!(
        summaries.current_decision.ok_or("decision")?.verdict,
        shacs_core::runtime::ObservationVerdict::Suppress
    );
    assert!(summaries
        .notifications
        .iter()
        .all(|notification| notification.intent.kind
            == shacs_core::runtime::EntrustedNotificationKind::RegularSummary));
    for index in 0..2 {
        let mail = sink
            .messages
            .recv_timeout(std::time::Duration::from_secs(2))?;
        std::fs::write(runtime.root.join(format!("summary-{index}.eml")), mail)?;
    }
    runtime.work(&["pause", &runtime.work_id])?;
    runtime.stop()?;
    println!("SUMMARY_QA_PASS: comparable baseline/suppress produce opted-in summary kind, not change alert");
    Ok(())
}
