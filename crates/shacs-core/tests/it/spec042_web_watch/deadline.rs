use super::support::*;
use chrono::{DateTime, FixedOffset};
use shacs_core::runtime::*;
use std::error::Error;

const TARGET: &str = "email:user@example.test";
const DAY: u64 = 86_400_000;
const HOUR: u64 = 3_600_000;

/// A watched price with an email target, as confirmed at `T0`.
fn ready() -> Result<(Fixture, WebWatchConfigured), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let configured = fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    Ok((fixture, configured))
}

/// `ms` written as RFC 3339 in +09:00.
fn seoul(ms: u64) -> Result<String, Box<dyn Error>> {
    let offset = FixedOffset::east_opt(9 * 3_600).ok_or("offset")?;
    Ok(i64::try_from(ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .ok_or("time")?
        .with_timezone(&offset)
        .to_rfc3339())
}

/// The worker's deadline step at `now_ms`, then its notification step and the hand-out.
fn step(fixture: &Fixture, now_ms: u64) -> Result<Vec<OutboundMessage>, Box<dyn Error>> {
    fixture.remind(now_ms)?;
    fixture.deliver(now_ms)
}

#[test]
fn a_deadline_reminds_once_at_its_reminder_time_without_a_change() -> TestResult {
    // Given: a deadline three days ahead, written in +09:00, with the default reminder.
    let (fixture, configured) = ready()?;
    let set = fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;
    let remind_at = T0 + 2 * DAY;

    // When: the worker runs just before the reminder time, at it, and again later with new
    // dispatchers, as after a restart.
    let early = step(&fixture, remind_at - 1)?;
    let due = step(&fixture, remind_at)?;
    let again = step(&fixture, remind_at + HOUR)?;

    // Then
    let deadline = set.deadline.ok_or("no deadline")?;
    assert_eq!(set.revision, 1);
    assert_eq!(
        (deadline.at_ms, deadline.remind_at_ms),
        (T0 + 3 * DAY, remind_at)
    );
    assert_eq!(deadline.at_text, seoul(T0 + 3 * DAY)?);
    assert!(deadline.at_text.ends_with("+09:00"));
    assert!(early.is_empty());
    assert_eq!(titles(&due), vec!["Decision deadline reminder"]);
    for expected in [
        "Goal: watch a price",
        &format!("(set as {})", deadline.at_text),
        "Time left: 1d 0h 0m",
        "value <= 11000",
        "Latest value: 12900 (condition met: false)",
        "Tracking: web-watch-notification:",
    ] {
        assert!(
            due[0].content.contains(expected),
            "{expected}\n{}",
            due[0].content
        );
    }
    assert!(again.is_empty());
    let status = fixture.status()?;
    assert_eq!(status.condition_revision, configured.condition_revision);
    assert_eq!(status.baseline, Some(configured.baseline));
    assert_eq!(status.deadline.map(|view| view.revision), Some(1));
    Ok(())
}

#[test]
fn a_changed_deadline_reminds_only_at_its_new_time_and_keeps_the_baseline() -> TestResult {
    // Given: a deadline three days ahead.
    let (fixture, configured) = ready()?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;
    step(&fixture, T0 + HOUR)?;

    // When: before its reminder the deadline moves a day later (a stale revision is refused).
    let stale = fixture.deadline(Some(&seoul(T0 + 5 * DAY)?), None, false, 0, T0 + HOUR);
    let moved = fixture.deadline(Some(&seoul(T0 + 4 * DAY)?), None, false, 1, T0 + HOUR)?;
    let at_old = step(&fixture, T0 + 2 * DAY)?;
    let at_new = step(&fixture, T0 + 3 * DAY)?;

    // Then
    assert!(stale.is_err());
    assert_eq!(moved.revision, 2);
    assert!(at_old.is_empty());
    assert_eq!(titles(&at_new), vec!["Decision deadline reminder"]);
    let status = fixture.status()?;
    assert_eq!(status.condition_revision, configured.condition_revision);
    assert_eq!(status.baseline, Some(configured.baseline));
    assert_eq!(status.notifications.len(), 1);
    Ok(())
}

#[test]
fn a_queued_reminder_of_a_changed_deadline_is_not_sent() -> TestResult {
    // Given: the reminder of the first deadline queued but not sent.
    let (fixture, _) = ready()?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;
    let remind_at = T0 + 2 * DAY;
    fixture.remind(remind_at)?;
    fixture.route(remind_at)?;
    let queued = fixture.due_inbound(remind_at)?;

    // When: the deadline moves a day later before the reminder is consumed.
    fixture.deadline(
        Some(&seoul(T0 + 4 * DAY)?),
        None,
        false,
        1,
        remind_at + HOUR,
    )?;
    for message in queued {
        fixture.consume_at(message, remind_at + HOUR)?;
    }
    let stale = fixture.bus.consume_outbound();
    let at_new = step(&fixture, T0 + 3 * DAY)?;

    // Then: the old reminder is suppressed, the new deadline reminds once.
    assert!(stale.is_none());
    assert_eq!(titles(&at_new), vec!["Decision deadline reminder"]);
    let status = fixture.status()?;
    assert_eq!(status.notifications.len(), 2);
    assert!(status.notifications[0].suppressed_stale);
    Ok(())
}

#[test]
fn pause_resume_done_and_clear_decide_whether_the_reminder_goes_out() -> TestResult {
    use EntrustedWorkLifecycleAction::{Clear, Done, Pause, Resume};
    let remind_at = T0 + 2 * DAY;
    let mut outcomes = Vec::new();
    for case in ["pause", "resume before", "resume after", "done", "clear"] {
        // Given: a deadline three days ahead whose reminder wake is scheduled.
        let (fixture, _) = ready()?;
        fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;
        step(&fixture, T0 + HOUR)?;

        // When
        let mut mails = Vec::new();
        match case {
            "pause" => fixture.transition(Pause, T0 + DAY)?,
            "resume before" => {
                fixture.transition(Pause, T0 + DAY)?;
                fixture.transition(Resume, remind_at - HOUR)?;
            }
            "resume after" => {
                fixture.transition(Pause, T0 + DAY)?;
                mails.extend(step(&fixture, remind_at)?);
                fixture.transition(Resume, remind_at + HOUR)?;
            }
            "done" => fixture.transition(Done, T0 + DAY)?,
            _ => fixture.transition(Clear, T0 + DAY)?,
        }
        mails.extend(step(&fixture, remind_at)?);
        mails.extend(step(&fixture, remind_at + 2 * HOUR)?);
        outcomes.push((case, titles(&mails)));
    }

    // Then: only a resume before the reminder time gets the reminder, once.
    let reminder = vec!["Decision deadline reminder".to_owned()];
    assert_eq!(
        outcomes,
        vec![
            ("pause", Vec::new()),
            ("resume before", reminder),
            ("resume after", Vec::new()),
            ("done", Vec::new()),
            ("clear", Vec::new()),
        ]
    );
    Ok(())
}

#[test]
fn deadline_settings_are_checked_and_an_immediate_reminder_must_be_chosen() -> TestResult {
    // Given: a watch with a target, and one without.
    let (fixture, _) = ready()?;
    let untargeted = Fixture::new()?;
    untargeted.watch_price("12,900", "11000")?;
    let now = T0 + HOUR;
    let soon = seoul(now + 2 * HOUR)?;

    // When
    let refused = [
        fixture.deadline(Some(&seoul(now - 1)?), None, false, 0, now),
        fixture.deadline(Some("2027-01-20T18:00:00"), None, false, 0, now),
        fixture.deadline(Some(&soon), None, false, 0, now),
        fixture.deadline(Some(&seoul(now + 3 * DAY)?), None, true, 0, now),
        fixture.deadline(
            Some(&seoul(now + 3 * DAY)?),
            Some(31 * 86_400),
            false,
            0,
            now,
        ),
        fixture.deadline(Some(&seoul(now + 3 * DAY)?), Some(0), false, 0, now),
        fixture.deadline(Some(&seoul(now + 3 * DAY)?), Some(59), false, 0, now),
        untargeted.deadline(Some(&seoul(now + 3 * DAY)?), None, false, 0, now),
        fixture.deadline(
            Some(&format!("2099-01-01T09:00:00.{}+09:00", "0".repeat(70_000))),
            None,
            false,
            0,
            now,
        ),
    ];
    let immediate = fixture.deadline(Some(&soon), None, true, 0, now)?;
    let mails = step(&fixture, now)?;
    let cleared = fixture.deadline(None, None, false, 1, now + HOUR)?;

    // Then
    for result in refused {
        assert!(
            matches!(result, Err(WebWatchError::InvalidInput(_))),
            "{result:?}"
        );
    }
    assert_eq!(
        immediate.deadline.map(|deadline| deadline.remind_at_ms),
        Some(now)
    );
    assert_eq!(titles(&mails), vec!["Decision deadline reminder"]);
    assert!(mails[0].content.contains("Time left: 0d 2h 0m"));
    assert_eq!((cleared.revision, cleared.deadline), (2, None));
    Ok(())
}

#[test]
fn a_reminder_queued_before_the_deadline_is_not_sent_after_it() -> TestResult {
    // Given: a reminder queued at its reminder time but not handed out yet.
    let (fixture, _) = ready()?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;
    let remind_at = T0 + 2 * DAY;
    fixture.remind(remind_at)?;
    fixture.route(remind_at)?;
    let queued = fixture.due_inbound(remind_at)?;

    // When: it is consumed only after the deadline, as when no channel runtime ran until then.
    for message in queued {
        fixture.consume_at(message, T0 + 3 * DAY)?;
    }
    let late = fixture.bus.consume_outbound();

    // Then
    assert!(late.is_none());
    let status = fixture.status()?;
    assert_eq!(status.notifications.len(), 1);
    assert!(status.notifications[0].suppressed_stale);
    Ok(())
}

#[test]
fn a_long_goal_is_cut_in_the_reminder() -> TestResult {
    // Given: a goal far longer than a mail line, set while creating the work.
    let fixture = Fixture::with_goal(&"watch the price ".repeat(5_000))?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;

    // When
    let mails = step(&fixture, T0 + 2 * DAY)?;

    // Then: one reminder whose goal line is cut to 240 characters.
    assert_eq!(titles(&mails), vec!["Decision deadline reminder"]);
    let goal = mails[0]
        .content
        .lines()
        .find_map(|line| line.strip_prefix("Goal: "))
        .ok_or("no goal line")?;
    assert_eq!(goal.chars().count(), 240);
    assert!(goal.ends_with('…'));
    Ok(())
}

#[test]
fn the_reminder_carries_no_credentials_or_url_query_from_the_goal() -> TestResult {
    // Given: a goal mentioning a URL with userinfo, query and fragment, and Bearer and Basic
    // credentials.
    let fixture = Fixture::with_goal(
        "buy at https://alice:s3cr3t@shop.example/item?coupon=priv4te#checkout-frag before the \
         sale, Authorization: Bearer abc.def.ghi Authorization: Basic dXNlcjpwYXNz",
    )?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some(TARGET), None)?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;

    // When
    let mails = step(&fixture, T0 + 2 * DAY)?;
    let log = std::fs::read_to_string(fixture.data_dir.join("runtime/durable-events/events.log"))?;

    // Then: neither the mail nor the durable deadline and notification facts keep them.
    assert_eq!(titles(&mails), vec!["Decision deadline reminder"]);
    assert!(mails[0]
        .content
        .contains("Goal: buy at https://shop.example/item before the sale"));
    for secret in [
        "alice",
        "s3cr3t",
        "coupon",
        "priv4te",
        "checkout-frag",
        "abc.def.ghi",
        "dXNlcjpwYXNz",
    ] {
        assert!(!mails[0].content.contains(secret), "{secret} in the mail");
        assert!(!log.contains(secret), "{secret} in the durable log");
    }
    Ok(())
}

#[test]
fn a_phrase_with_a_url_shows_no_credentials_or_query_in_any_mail() -> TestResult {
    // Given: a phrase condition whose phrase is a URL with userinfo, query and fragment.
    let fixture = Fixture::new()?;
    let phrase = "https://u:pw9@x.test/a?q=s3cr3t#frag9";
    fixture
        .server
        .set("/item", Page::html(&format!("<p>Promo {phrase} today</p>")))?;
    let url = fixture.server.url("/item");
    let target = WebPreviewTarget::Text {
        phrase: phrase.to_owned(),
        area: Some(WebTextArea {
            label: "Promo".to_owned(),
            until: "today".to_owned(),
        }),
    };
    let preview = fixture.preview(&url, target)?;
    let token = &preview.candidates.first().ok_or("no candidate")?.token;
    fixture.confirm(&url, token, WebCondition::TextPresent, 1, T0)?;
    fixture.notify(Some(TARGET), None)?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;

    // When: the phrase leaves the page (a change), and later the reminder time comes.
    fixture
        .server
        .set("/item", Page::html("<p>Promo none today</p>"))?;
    let due = fixture.next_due()?;
    fixture.tick(due)?;
    let mut mails = fixture.deliver(due)?;
    mails.extend(step(&fixture, T0 + 2 * DAY)?);
    let log = std::fs::read_to_string(fixture.data_dir.join("runtime/durable-events/events.log"))?;

    // Then: both mails show the URL reduced; no mail and no durable fact keeps the rest.
    assert_eq!(
        titles(&mails),
        vec!["Web page change notification", "Decision deadline reminder",]
    );
    for mail in &mails {
        assert!(
            mail.content.contains("phrase \"https://x.test/a\""),
            "{}",
            mail.content
        );
    }
    for secret in ["pw9", "s3cr3t", "frag9"] {
        assert!(
            !mails.iter().any(|mail| mail.content.contains(secret)),
            "{secret} in a mail"
        );
        assert!(!log.contains(secret), "{secret} in the durable log");
    }
    Ok(())
}

#[test]
fn a_reminder_time_that_is_now_needs_no_immediate_reminder() -> TestResult {
    // Given
    let (fixture, _) = ready()?;
    let now = T0 + HOUR;

    // When: the reminder a minute before a deadline a minute away falls exactly on now.
    let set = fixture.deadline(Some(&seoul(now + 60_000)?), Some(60), false, 0, now)?;
    let mails = step(&fixture, now)?;

    // Then
    assert_eq!(
        set.deadline.map(|deadline| deadline.remind_at_ms),
        Some(now)
    );
    assert_eq!(titles(&mails), vec!["Decision deadline reminder"]);
    Ok(())
}

#[test]
fn no_reminder_is_made_once_the_deadline_has_passed() -> TestResult {
    // Given: a deadline two hours ahead with its reminder an hour before.
    let (fixture, _) = ready()?;
    fixture.deadline(Some(&seoul(T0 + 2 * HOUR)?), Some(3_600), false, 0, T0)?;

    // When: the worker first runs after the deadline.
    let mails = step(&fixture, T0 + 3 * HOUR)?;

    // Then
    assert!(mails.is_empty());
    assert!(fixture.status()?.notifications.is_empty());
    Ok(())
}

#[test]
fn a_reminder_is_not_sent_to_a_target_changed_before_it_goes_out() -> TestResult {
    // Given: a reminder queued for the first target.
    let (fixture, _) = ready()?;
    fixture.deadline(Some(&seoul(T0 + 3 * DAY)?), None, false, 0, T0)?;
    let remind_at = T0 + 2 * DAY;
    fixture.remind(remind_at)?;
    fixture.route(remind_at)?;
    let queued = fixture.due_inbound(remind_at)?;

    // When: the target changes before it is sent, and the worker runs again.
    fixture.notify(Some("email:other@example.test"), None)?;
    for message in queued {
        fixture.consume_at(message, remind_at)?;
    }
    let stale = fixture.bus.consume_outbound();
    let mails = step(&fixture, remind_at + HOUR)?;

    // Then: nothing to the first target; the new target gets the reminder once.
    assert!(stale.is_none());
    assert_eq!(titles(&mails), vec!["Decision deadline reminder"]);
    assert_eq!(mails[0].chat_id, "other@example.test");
    Ok(())
}
