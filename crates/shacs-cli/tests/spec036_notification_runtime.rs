#[path = "spec036_notification_runtime/smtp.rs"]
mod smtp;
#[path = "spec036_notification_runtime/stale.rs"]
mod stale;
#[path = "spec036_notification_runtime/support.rs"]
mod support;

use shacs_core::runtime::{EntrustedNotificationDeliveryState as Delivery, ObservationVerdict};
use std::{fs, sync::atomic::Ordering, time::Duration};
use support::{Runtime, TestResult};

#[test]
#[ignore = "starts the production runtime and a loopback SMTP receiver; retains isolated QA evidence"]
fn production_worker_notification_delivery_and_restart() -> TestResult {
    let sink = smtp::SmtpSink::start()?;
    let mut runtime = Runtime::new(sink.address.port())?;
    runtime.start()?;
    let baseline = runtime.wait("01-baseline", |snapshot| snapshot.history.len() == 1)?;
    assert_eq!(
        baseline.current_decision.ok_or("decision")?.verdict,
        ObservationVerdict::Baseline
    );
    assert!(sink.messages.try_recv().is_err());
    runtime.stop()?;
    runtime.start()?;
    let repeated = runtime.wait("02-repeat-after-restart", |snapshot| {
        snapshot.history.len() >= 2
    })?;
    assert_eq!(
        repeated.current_decision.ok_or("decision")?.verdict,
        ObservationVerdict::Suppress
    );
    assert!(repeated.notifications.is_empty());
    runtime.source(110)?;
    let irrelevant = runtime.wait("03-irrelevant", |snapshot| {
        snapshot
            .current_decision
            .as_ref()
            .and_then(|decision| decision.current.as_ref())
            .is_some_and(|current| current.summary.value == 110)
    })?;
    assert!(irrelevant.notifications.is_empty());
    assert!(sink.messages.try_recv().is_err());
    runtime.source(90)?;
    let sent = runtime.wait("04-significant-sent", |snapshot| {
        snapshot
            .notifications
            .iter()
            .any(|notification| notification.delivery == Delivery::SentHint)
    })?;
    assert_eq!(sent.notifications.len(), 1);
    let raw = sink.messages.recv_timeout(Duration::from_secs(3))?;
    let mail = mailparse::parse_mail(raw.as_bytes())?;
    let body = mail.get_body()?;
    assert!(body.len() < 1600);
    assert!(body.contains("110"));
    assert!(body.contains("90"));
    fs::write(runtime.root.join("notification.eml"), &raw)?;
    println!("SMTP_LOCAL_MESSAGE:\n{body}");
    runtime.stop()?;
    runtime.start()?;
    let prior_count = sent.history.len();
    let after = runtime.wait("05-duplicate-restart", |snapshot| {
        snapshot.history.len() > prior_count
    })?;
    assert_eq!(after.notifications.len(), 1);
    assert!(sink.messages.try_recv().is_err());
    sink.reject.store(true, Ordering::SeqCst);
    runtime.source(130)?;
    let failed = runtime.wait("06-new-change-send-failed", |snapshot| {
        snapshot
            .notifications
            .iter()
            .any(|notification| notification.delivery == Delivery::Failed)
    })?;
    assert_eq!(failed.notifications.len(), 2);
    assert_eq!(
        failed.current_decision.ok_or("decision")?.verdict,
        ObservationVerdict::NotifyCandidate
    );
    fs::write(
        runtime.workspace.join("source.next"),
        b"unsupported observation; never quote this payload",
    )?;
    fs::rename(
        runtime.workspace.join("source.next"),
        runtime.workspace.join("source.json"),
    )?;
    let unknown = runtime.wait("07-unknown-silent", |snapshot| {
        snapshot
            .current_decision
            .as_ref()
            .is_some_and(|decision| decision.verdict == ObservationVerdict::Unknown)
    })?;
    assert_eq!(unknown.notifications.len(), 2);
    assert!(sink.messages.try_recv().is_err());
    runtime.work(&["pause", &runtime.work_id])?;
    runtime.stop()?;
    runtime.start()?;
    runtime.wait("08-paused-restart-blocked", |snapshot| {
        snapshot
            .decisions
            .last()
            .is_some_and(|decision| decision.verdict == ObservationVerdict::Blocked)
    })?;
    runtime.stop()?;
    assert!(sink.messages.try_recv().is_err());
    let mut facts = Vec::new();
    shacs_session::durable_event::DurableEventStore::open(
        runtime.data.join("runtime/durable-events"),
    )?
    .visit_from_sequence(0, |event| {
        let correlated = event.correlation_id.as_deref().is_some_and(|id| id.starts_with("entrusted-notification:"))
            || event.turn_id.as_deref().is_some_and(|id| id.starts_with("turn:entrusted-notification:"))
            || matches!(&event.payload, shacs_session::durable_event::DurableEventPayload::Inline { data, .. }
                if data.get("work_id").and_then(serde_json::Value::as_str).is_some_and(|id| id.starts_with("entrusted-notification:")));
        if correlated { facts.push(event.clone()); }
    })?;
    for kind in [
        "work.enqueued",
        "session.turn_accepted",
        "session.turn_completed",
        "work.terminal",
    ] {
        let count = facts.iter().filter(|event| event.kind == kind).count();
        assert_eq!(count, 2, "{kind}");
        println!("DURABLE_FACT {kind}={count}");
    }
    fs::write(
        runtime.root.join("notification-events.json"),
        serde_json::to_vec_pretty(&facts)?,
    )?;
    println!("QA_PASS: production worker -> durable inbound -> session turn -> MessageBus/ChannelManager -> local SMTP; remote receipt not asserted");
    Ok(())
}
