use super::support::*;
use shacs_core::runtime::*;

const TARGET: &str = "email:user@example.test";

/// One check at its due time followed by the worker's notification step.
fn check(
    fixture: &Fixture,
    page: Page,
) -> Result<Vec<OutboundMessage>, Box<dyn std::error::Error>> {
    fixture.server.set("/item", page)?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.deliver(due)
}

/// Puts a file where the work payload directory was, so every enqueue fails until unblocked.
fn block_queue(fixture: &Fixture) -> std::io::Result<()> {
    let payloads = fixture.data_dir.join("runtime/work-payloads");
    std::fs::rename(&payloads, payloads.with_extension("parked"))?;
    std::fs::write(&payloads, b"")
}

fn unblock_queue(fixture: &Fixture) -> std::io::Result<()> {
    let payloads = fixture.data_dir.join("runtime/work-payloads");
    std::fs::remove_file(&payloads)?;
    std::fs::rename(payloads.with_extension("parked"), &payloads)
}

/// The next check with `page` and the notifications it queues, nothing handed out yet.
fn queue_check(fixture: &Fixture, page: Page) -> Result<u64, Box<dyn std::error::Error>> {
    fixture.server.set("/item", page)?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.route(due)?;
    Ok(due)
}

/// Three failed checks, which pass a 20-minute threshold and queue the failure mail.
fn queue_failure_mail(fixture: &Fixture) -> TestResult {
    for _ in 0..2 {
        check(fixture, Page::status(404))?;
    }
    queue_check(fixture, Page::status(404)).map(|_| ())
}

#[test]
fn only_the_condition_change_is_mailed_once() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;

    // When: A, A, an unrelated change, then B that meets the condition.
    let mut mails = Vec::new();
    for price in ["12,900", "12,900", "12,500", "10,900"] {
        mails.push(check(&fixture, Page::html(&price_page(price)))?);
    }
    let again = fixture.deliver(fixture.next_due()?)?;

    // Then
    assert_eq!(
        mails.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![0, 0, 0, 1]
    );
    let mail = &mails[3][0];
    assert_eq!(
        (mail.channel.as_str(), mail.chat_id.as_str()),
        ("email", "user@example.test")
    );
    for expected in [
        "Web page change notification",
        "Previous: 12500 (condition met: false)",
        "New: 10900 (condition met: true)",
        "value <= 11000",
        &fixture.server.url("/item"),
        "Tracking: web-watch-notification:",
    ] {
        assert!(
            mail.content.contains(expected),
            "{expected}\n{}",
            mail.content
        );
    }
    assert!(again.is_empty());
    let status = fixture.status()?;
    assert_eq!(status.notifications.len(), 1);
    assert_eq!(status.notifications[0].kind, WebNotifyKind::Change);
    assert_eq!(status.notifications[0].enqueue, WebNotifyEnqueue::Enqueued);
    assert!(status.notifications[0].consumption_started);
    Ok(())
}

#[test]
fn a_failure_beyond_the_threshold_is_mailed_once_and_its_recovery_once() -> TestResult {
    // Given: a 20-minute threshold, the shortest a 10-minute cadence allows.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;

    // When: a short failure, then one past the threshold that recovers with the same outcome,
    // then another past the threshold that recovers with the condition met.
    let mut mails = Vec::new();
    for page in [
        Page::status(404),
        Page::html(&price_page("12,900")),
        Page::status(404),
        Page::status(404),
        Page::status(404),
        Page::status(404),
        Page::html(&price_page("12,900")),
        Page::status(503),
        Page::status(404),
        Page::status(404),
        Page::html(&price_page("10,900")),
    ] {
        mails.extend(check(&fixture, page)?);
    }

    // Then
    assert_eq!(
        titles(&mails),
        vec![
            "Web page check failure notification",
            "Web page check recovered",
            "Web page check failure notification",
            "Web page change notification",
        ]
    );
    assert!(mails[0].content.contains("Last failure: http_error"));
    assert!(mails[0].content.contains("Next check:"));
    assert!(mails[3]
        .content
        .contains("Recovered: checks had failed since"));
    Ok(())
}

#[test]
fn an_interval_is_measured_up_to_the_success_that_closes_it_without_downtime() -> TestResult {
    // Given: a 20-minute threshold with a 10-minute cadence.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    let ok = || Page::html(&price_page("12,900"));

    // When: a failure seen again as a success 10 minutes later; failures whose success comes
    // 20 minutes after the first; and a failure whose success comes after 30 minutes of downtime.
    let mut short = Vec::new();
    for page in [Page::status(404), ok()] {
        short.extend(check(&fixture, page)?);
    }
    let mut reached = Vec::new();
    for page in [Page::status(404), Page::status(404), ok()] {
        reached.extend(check(&fixture, page)?);
    }
    let mut downtime = check(&fixture, Page::status(404))?;
    fixture.server.set("/item", ok())?;
    let late = fixture.next_due()? + 3 * CADENCE_MS;
    fixture.tick(late)?;
    downtime.extend(fixture.deliver(late)?);

    // Then
    assert!(short.is_empty());
    assert_eq!(
        titles(&reached),
        vec![
            "Web page check failure notification",
            "Web page check recovered",
        ]
    );
    assert_eq!(titles(&downtime), vec!["Assistant downtime notification"]);
    Ok(())
}

#[test]
fn downtime_before_a_blocked_check_is_left_out_of_the_failure_interval() -> TestResult {
    // Given: a 30-minute threshold and a failing page.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1800))?;
    let mut mails = check(&fixture, Page::status(404))?;

    // When: the runtime is off for 30 minutes, the hook blocks the first check after the
    // restart, and the next check succeeds.
    let late = fixture.next_due()? + 3 * CADENCE_MS;
    fixture.tick_with(late, &BlockingExecutor)?;
    mails.extend(fixture.deliver(late)?);
    mails.extend(check(&fixture, Page::html(&price_page("12,900")))?);

    // Then: only the downtime mail; the closed interval leaves the downtime out.
    assert_eq!(titles(&mails), vec!["Assistant downtime notification"]);
    let interval = fixture
        .status()?
        .failure_intervals
        .last()
        .cloned()
        .ok_or("no interval")?;
    assert!(interval.closed.is_some());
    assert_eq!(interval.excluded_gap_ms, 3 * CADENCE_MS);
    Ok(())
}

#[test]
fn a_recovery_is_mailed_only_after_its_failure_mail_went_out() -> TestResult {
    // Given: a failure past the threshold that recovered while notifications were off, then
    // one whose queued failure mail went stale when the target changed.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    let mut off = Vec::new();
    for _ in 0..3 {
        off.extend(check(&fixture, Page::status(404))?);
    }
    off.extend(check(&fixture, Page::html(&price_page("12,900")))?);
    fixture.notify(Some(TARGET), Some(1200))?;
    off.extend(fixture.deliver(fixture.next_due()? - 1)?);
    for _ in 0..2 {
        off.extend(check(&fixture, Page::status(404))?);
    }
    fixture.server.set("/item", Page::status(404))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.route(due)?;
    let queued = fixture.due_inbound(due)?;
    fixture.notify(Some("email:other@example.test"), Some(1200))?;
    for message in queued {
        fixture.consume(message)?;
    }
    let stale = fixture.bus.consume_outbound();

    // When: the second interval recovers with the condition met before its failure mail is
    // made again.
    let mails = check(&fixture, Page::html(&price_page("10,900")))?;

    // Then: nothing for the first interval; for the second, the failure mail goes to the new
    // target before the change mail that carries the recovery.
    assert!(off.is_empty());
    assert!(stale.is_none());
    assert_eq!(
        titles(&mails),
        vec![
            "Web page check failure notification",
            "Web page change notification",
        ]
    );
    assert!(mails
        .iter()
        .all(|mail| mail.chat_id == "other@example.test"));
    assert!(mails[1]
        .content
        .contains("Recovered: checks had failed since"));
    Ok(())
}

#[test]
fn a_recovery_is_not_sent_when_its_failure_mail_never_left_the_queue() -> TestResult {
    // Given: a page failing past the threshold whose failure mail gets queued.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    let ok = || Page::html(&price_page("12,900"));

    // When: the failure mail's hand-out ends unsent before the page recovers; then, for a
    // second interval, after the recovery was queued behind it.
    queue_failure_mail(&fixture)?;
    let due = fixture.next_due()?;
    for message in fixture.due_inbound(due)? {
        fixture.exhaust(&message)?;
    }
    let mut mails = fixture.deliver(queue_check(&fixture, ok())?)?;
    queue_failure_mail(&fixture)?;
    let due = queue_check(&fixture, ok())?;
    for message in fixture.due_inbound(due)? {
        fixture.exhaust(&message)?;
    }
    mails.extend(fixture.deliver(due)?);

    // Then: no mail at all; both failure mails show a failed delivery, the first interval has
    // no recovery and the queued one is suppressed.
    assert!(mails.is_empty());
    let status = fixture.status()?;
    let of_kind = |kind| {
        status
            .notifications
            .iter()
            .filter(move |notification| notification.kind == kind)
    };
    assert!(of_kind(WebNotifyKind::Failure).all(|failure| {
        failure.delivery == WebNotifyDelivery::Failed && !failure.consumption_started
    }));
    assert_eq!(of_kind(WebNotifyKind::Failure).count(), 2);
    let recoveries = of_kind(WebNotifyKind::Recovery).collect::<Vec<_>>();
    assert_eq!(recoveries.len(), 1);
    assert!(recoveries[0].suppressed_stale && !recoveries[0].consumption_started);
    Ok(())
}

#[test]
fn a_settings_change_before_recovery_replaces_the_still_queued_failure_mail() -> TestResult {
    // Given: a failure mail queued for the first target and not sent yet.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    queue_failure_mail(&fixture)?;

    // When: the target changes, then the next check recovers with the condition met.
    fixture.notify(Some("email:other@example.test"), Some(1200))?;
    let due = queue_check(&fixture, Page::html(&price_page("10,900")))?;
    let mails = fixture.deliver(due)?;

    // Then: the new target gets the failure mail, then the change carrying the recovery.
    assert_eq!(
        titles(&mails),
        vec![
            "Web page check failure notification",
            "Web page change notification",
        ]
    );
    assert!(mails
        .iter()
        .all(|mail| mail.chat_id == "other@example.test"));
    assert!(mails[1]
        .content
        .contains("Recovered: checks had failed since"));
    Ok(())
}

#[test]
fn a_failed_enqueue_keeps_the_failure_mail_before_its_recovery() -> TestResult {
    // Given: a 20-minute threshold, failures at 0 and 10 minutes, and the worker's dispatcher
    // recording the recovering check at 20 minutes.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    for _ in 0..2 {
        check(&fixture, Page::status(404))?;
    }
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;
    let mut worker = fixture.dispatcher()?;
    let due = fixture.next_due()?;
    for _ in 0..2 {
        worker.process_web_watches(&fixture.runtime(due), &Executor)?;
    }

    // When: the failure and recovery mails fail to enter the queue, then are retried later.
    block_queue(&fixture)?;
    worker.enqueue_web_watch_notifications(&fixture.runtime(due))?;
    unblock_queue(&fixture)?;
    let retry_at = due + WEB_NOTIFY_ENQUEUE_RETRY_MS;
    worker.enqueue_web_watch_notifications(&fixture.runtime(retry_at))?;
    let mails = fixture.deliver(retry_at)?;

    // Then
    assert_eq!(
        titles(&mails),
        vec![
            "Web page check failure notification",
            "Web page check recovered",
        ]
    );
    Ok(())
}

#[test]
fn a_change_after_a_failure_mail_that_never_left_the_queue_has_no_recovery_line() -> TestResult {
    // Given: a queued failure mail, then a recovering check that meets the condition, whose
    // change carrying the recovery is queued behind it.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    queue_failure_mail(&fixture)?;
    let due = queue_check(&fixture, Page::html(&price_page("10,900")))?;

    // When: the failure mail's hand-out ends unsent, then the change is handed out.
    for message in fixture.due_inbound(due)? {
        fixture.exhaust(&message)?;
    }
    let mails = fixture.deliver(due)?;

    // Then: the change still goes out, without the recovery line.
    assert_eq!(titles(&mails), vec!["Web page change notification"]);
    assert!(
        !mails[0].content.contains("Recovered"),
        "{}",
        mails[0].content
    );
    Ok(())
}

#[test]
fn new_settings_right_before_a_recovering_check_mail_the_failure_then_the_recovery() -> TestResult {
    for initial in [Some(3600), None] {
        // Given: a page failing for 30 minutes under a one-hour threshold, or with
        // notifications off.
        let fixture = Fixture::new()?;
        fixture.watch_price("12,900", "11000")?;
        if let Some(threshold) = initial {
            fixture.notify(Some(TARGET), Some(threshold))?;
        }
        let mut before = Vec::new();
        for _ in 0..4 {
            before.extend(check(&fixture, Page::status(404))?);
        }

        // When: a 20-minute threshold is set just before the next check, which succeeds and
        // runs ahead of the worker's notification step; then the step runs again.
        fixture.notify(Some(TARGET), Some(1200))?;
        let mails = check(&fixture, Page::html(&price_page("12,900")))?;
        let again = fixture.deliver(fixture.next_due()? - 1)?;

        // Then
        assert!(before.is_empty());
        assert_eq!(
            titles(&mails),
            vec![
                "Web page check failure notification",
                "Web page check recovered",
            ],
            "{initial:?}"
        );
        assert!(again.is_empty());
    }
    Ok(())
}

#[test]
fn a_failure_mail_made_stale_by_a_new_target_goes_to_the_new_target_while_failing() -> TestResult {
    // Given: a failure mail queued for the first target.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    for _ in 0..2 {
        check(&fixture, Page::status(404))?;
    }
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.route(due)?;
    let queued = fixture.due_inbound(due)?;

    // When: the target changes before it is sent, and the page keeps failing.
    fixture.notify(Some("email:other@example.test"), Some(1200))?;
    for message in queued {
        fixture.consume(message)?;
    }
    let stale = fixture.bus.consume_outbound();
    let next = check(&fixture, Page::status(404))?;

    // Then
    assert!(stale.is_none());
    assert_eq!(titles(&next), vec!["Web page check failure notification"]);
    assert_eq!(next[0].chat_id, "other@example.test");
    Ok(())
}

#[test]
fn a_failure_mail_names_the_last_success_of_the_current_plan() -> TestResult {
    // Given: a check of the first plan, then a plan confirmed again later.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    check(&fixture, Page::html(&price_page("12,900")))?;
    let url = fixture.server.url("/item");
    let preview = fixture.preview(&url, number("판매가"))?;
    let token = &preview.candidates.first().ok_or("no candidate")?.token;
    let reconfirmed_at = fixture.next_due()? - 1;
    fixture.confirm(&url, token, at_most("12000"), 2, reconfirmed_at)?;
    fixture.notify(Some(TARGET), Some(1200))?;

    // When
    let mut mails = Vec::new();
    for _ in 0..3 {
        mails.extend(check(&fixture, Page::status(404))?);
    }

    // Then: the confirmation read of the current plan is the last success.
    assert_eq!(titles(&mails), vec!["Web page check failure notification"]);
    let line = mails[0]
        .content
        .lines()
        .find(|line| line.starts_with("Last successful check:"))
        .ok_or("no last success line")?;
    assert!(line.ends_with(&format!("({reconfirmed_at} ms)")), "{line}");
    Ok(())
}

#[test]
fn a_wait_that_holds_the_next_check_past_the_threshold_is_mailed_at_once() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;

    // When: the site asks to wait two hours, longer than the 20-minute threshold.
    let mails = check(&fixture, Page::status(429).header("Retry-After", "7200"))?;

    // Then
    assert_eq!(titles(&mails), vec!["Web page check failure notification"]);
    assert!(mails[0].content.contains("Last failure: rate_limited"));
    assert!(mails[0]
        .content
        .contains("Sent now: the site asked to wait"));
    Ok(())
}

#[test]
fn a_threshold_change_keeps_the_open_interval_and_its_sent_failure_mail() -> TestResult {
    // Given: a failure mail already sent under settings revision 1.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    let mut mails = Vec::new();
    for _ in 0..3 {
        mails.extend(check(&fixture, Page::status(404))?);
    }

    // When: the threshold changes while the page still fails, then the page recovers.
    fixture.notify(Some(TARGET), Some(1800))?;
    mails.extend(check(&fixture, Page::status(404))?);
    mails.extend(check(&fixture, Page::status(404))?);
    mails.extend(check(&fixture, Page::html(&price_page("12,900")))?);

    // Then
    assert_eq!(
        titles(&mails),
        vec![
            "Web page check failure notification",
            "Web page check recovered",
        ]
    );
    assert_eq!(fixture.status()?.failure_intervals.len(), 1);
    Ok(())
}

#[test]
fn a_lowered_threshold_judges_the_open_interval_again_without_a_new_check() -> TestResult {
    // Given: a one-hour threshold and a page failing for 30 minutes.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(3600))?;
    let mut before = Vec::new();
    for _ in 0..4 {
        before.extend(check(&fixture, Page::status(404))?);
    }
    let now = fixture.latest()?.observed_at_ms + 1;

    // When: the threshold drops to 20 minutes and the worker runs before the next check, then
    // again with new dispatchers as after a restart, then the page recovers.
    fixture.notify(Some(TARGET), Some(1200))?;
    let lowered = fixture.deliver(now)?;
    let again = fixture.deliver(now + 1)?;
    let recovered = check(&fixture, Page::html(&price_page("12,900")))?;

    // Then
    assert!(before.is_empty());
    assert_eq!(
        titles(&lowered),
        vec!["Web page check failure notification"]
    );
    assert!(again.is_empty());
    assert_eq!(titles(&recovered), vec!["Web page check recovered"]);
    Ok(())
}

#[test]
fn a_failed_mail_send_is_recorded_apart_from_the_successful_check() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    let mails = check(&fixture, Page::html(&price_page("10,900")))?;

    // When: the SMTP transport reports a failure for the sent mail.
    for mail in &mails {
        record_entrusted_notification_delivery(
            &fixture.data_dir.join("runtime/durable-events"),
            mail,
            EntrustedNotificationTransportFact::Failed,
        )?;
    }

    // Then
    let status = fixture.status()?;
    assert_eq!(mails.len(), 1);
    assert_eq!(status.notifications[0].delivery, WebNotifyDelivery::Failed);
    let latest = status.history.last().ok_or("no check")?;
    assert!(matches!(latest.result, WebCheckResult::Observed { .. }));
    assert_eq!(latest.decision.verdict, ObservationVerdict::NotifyCandidate);
    Ok(())
}

#[test]
fn a_failed_enqueue_is_retried_later_and_the_checks_go_on() -> TestResult {
    // Given: the worker's dispatcher, and a check three cadences late that meets the condition,
    // so a downtime mail and a change mail are due together.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    fixture
        .server
        .set("/item", Page::html(&price_page("10,900")))?;
    let mut worker = fixture.dispatcher()?;
    let late = fixture.next_due()? + 3 * CADENCE_MS;
    for _ in 0..2 {
        worker.process_web_watches(&fixture.runtime(late), &Executor)?;
    }

    // When: the work queue fails the first attempt, then works again.
    block_queue(&fixture)?;
    worker.enqueue_web_watch_notifications(&fixture.runtime(late))?;
    let failed = fixture.status()?.notifications;
    unblock_queue(&fixture)?;
    worker.enqueue_web_watch_notifications(&fixture.runtime(late + 1))?;
    let too_soon = fixture.status()?.notifications;
    let retry_at = late + WEB_NOTIFY_ENQUEUE_RETRY_MS;
    worker.enqueue_web_watch_notifications(&fixture.runtime(retry_at))?;
    let mut mails = titles(&fixture.deliver(retry_at)?);
    let next = check(&fixture, Page::html(&price_page("12,900")))?;

    // Then
    assert_eq!(failed.len(), 2);
    for notification in &failed {
        assert_eq!(
            (notification.enqueue, notification.enqueue_failures),
            (WebNotifyEnqueue::Failed, 1)
        );
    }
    assert_eq!(too_soon, failed);
    mails.sort();
    assert_eq!(
        mails,
        vec![
            "Assistant downtime notification",
            "Web page change notification",
        ]
    );
    assert_eq!(titles(&next), vec!["Web page change notification"]);
    Ok(())
}

#[test]
fn an_enqueue_that_keeps_failing_stops_after_five_attempts() -> TestResult {
    // Given: a change notification while the work queue cannot take it.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    fixture
        .server
        .set("/item", Page::html(&price_page("10,900")))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    let mut worker = fixture.dispatcher()?;
    block_queue(&fixture)?;

    // When: the worker runs every minute for ten minutes, then the queue works again.
    for minute in 0..10 {
        let now_ms = due + minute * WEB_NOTIFY_ENQUEUE_RETRY_MS;
        worker.enqueue_web_watch_notifications(&fixture.runtime(now_ms))?;
    }
    unblock_queue(&fixture)?;
    let mails = fixture.deliver(due + 10 * WEB_NOTIFY_ENQUEUE_RETRY_MS)?;

    // Then
    let status = fixture.status()?;
    assert_eq!(status.notifications.len(), 1);
    assert_eq!(status.notifications[0].enqueue, WebNotifyEnqueue::Failed);
    assert_eq!(
        status.notifications[0].enqueue_failures,
        WEB_NOTIFY_ENQUEUE_ATTEMPTS
    );
    assert!(mails.is_empty());
    Ok(())
}

#[test]
fn downtime_beyond_the_threshold_is_mailed_once_without_a_failure_mail() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), Some(1200))?;
    fixture
        .server
        .set("/item", Page::html(&price_page("12,900")))?;

    // When: one cadence late (below the threshold), then three cadences late with a failing
    // page, then one more failure on time.
    let due = fixture.next_due()?;
    fixture.tick(due + CADENCE_MS)?;
    let short_gap = fixture.deliver(due + CADENCE_MS)?;
    fixture.server.set("/item", Page::status(404))?;
    let due = fixture.next_due()?;
    fixture.tick(due + 3 * CADENCE_MS)?;
    let long_gap = fixture.deliver(due + 3 * CADENCE_MS)?;
    let after_gap = check(&fixture, Page::status(404))?;

    // Then
    assert!(short_gap.is_empty());
    assert_eq!(titles(&long_gap), vec!["Assistant downtime notification"]);
    assert!(after_gap.is_empty());
    assert_eq!(fixture.status()?.runtime_gaps.len(), 2);
    Ok(())
}

#[test]
fn notifications_made_stale_by_pause_or_new_settings_are_not_sent() -> TestResult {
    // Given: a change notification enqueued but not yet consumed.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    fixture
        .server
        .set("/item", Page::html(&price_page("10,900")))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.route(due)?;
    fixture.route(due)?;
    let inbound = fixture.due_inbound(due)?;

    // When: the work is paused before the mail is consumed.
    fixture.transition(EntrustedWorkLifecycleAction::Pause, due + 1)?;
    for message in inbound.clone() {
        fixture.consume(message)?;
    }
    let after_pause = fixture.bus.consume_outbound();

    // Then: nothing is sent, the duplicate route enqueued once, and a replay does nothing.
    assert!(after_pause.is_none());
    assert_eq!(inbound.len(), 1);
    let status = fixture.status()?;
    assert_eq!(status.notifications.len(), 1);
    assert!(status.notifications[0].suppressed_stale);
    assert!(!status.notifications[0].consumption_started);
    for message in inbound {
        fixture.consume(message)?;
    }
    assert!(fixture.bus.consume_outbound().is_none());
    Ok(())
}

#[test]
fn a_settings_change_suppresses_queued_mail_and_a_claimed_mail_is_not_resent() -> TestResult {
    // Given: a change notification enqueued under settings revision 1.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    fixture
        .server
        .set("/item", Page::html(&price_page("10,900")))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.route(due)?;
    let queued = fixture.due_inbound(due)?;

    // When: the target changes before consumption; later a fresh change is consumed twice.
    let revised = fixture.notify(Some("email:other@example.test"), None)?;
    for message in queued {
        fixture.consume(message)?;
    }
    let stale = fixture.bus.consume_outbound();
    let fresh = check(&fixture, Page::html(&price_page("12,900")))?;
    fixture
        .server
        .set("/item", Page::html(&price_page("10,900")))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    fixture.route(due)?;
    let claimed = fixture.due_inbound(due)?;
    let mut sent = Vec::new();
    for message in claimed.iter().chain(claimed.iter()).cloned() {
        fixture.consume(message)?;
    }
    while let Some(mail) = fixture.bus.consume_outbound() {
        sent.push(mail);
    }

    // Then
    assert_eq!(revised.revision, 2);
    assert!(stale.is_none());
    assert_eq!(titles(&fresh), vec!["Web page change notification"]);
    assert_eq!(fresh[0].chat_id, "other@example.test");
    assert_eq!(sent.len(), 1);
    let status = fixture.status()?;
    assert_eq!(status.condition_revision, 2);
    assert_eq!(status.notifications.len(), 3);
    assert!(status.notifications[0].suppressed_stale);
    Ok(())
}

#[test]
fn notification_settings_are_checked_and_keep_the_baseline() -> TestResult {
    // Given
    let fixture = Fixture::new()?;
    let before_watch = fixture.notify(Some(TARGET), None);
    let configured = fixture.watch_price("12,900", "11000")?;
    let not_ready = configure_web_watch_notifications(
        &fixture.workspace,
        ConfigureWebNotifyRequest {
            work: fixture.request(),
            target: Some(TARGET.to_owned()),
            failure_threshold_seconds: None,
            email_ready: false,
            now_ms: T0,
        },
    );

    // When
    let invalid = [
        fixture.notify(Some("slack:C123"), None),
        fixture.notify(Some("email:not-an-address"), None),
        fixture.notify(Some(TARGET), Some(1199)),
        fixture.notify(Some(TARGET), Some(7 * 86_400 + 1)),
    ];
    let set = fixture.notify(Some(TARGET), None)?;
    let off = fixture.notify(None, Some(1200))?;

    // Then
    assert!(matches!(before_watch, Err(WebWatchError::InvalidInput(_))));
    assert!(matches!(not_ready, Err(WebWatchError::InvalidInput(_))));
    for result in invalid {
        assert!(matches!(result, Err(WebWatchError::InvalidInput(_))));
    }
    assert_eq!(set.revision, 1);
    assert_eq!(set.effective_threshold_seconds, 6 * 3600);
    assert_eq!((off.revision, off.target), (2, None));
    assert_eq!(off.effective_threshold_seconds, 1200);
    let status = fixture.status()?;
    assert_eq!(status.condition_revision, configured.condition_revision);
    assert_eq!(status.baseline, Some(configured.baseline));
    assert_eq!(status.notify.map(|notify| notify.revision), Some(2));
    Ok(())
}
