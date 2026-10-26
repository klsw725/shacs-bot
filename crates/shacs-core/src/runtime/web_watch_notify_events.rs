use std::collections::BTreeMap;

use super::web_watch_notify_store::WebNotifyEntry;
use super::web_watch_store::{WebNotifyPoint, WebWatchLedger};
use super::{
    ObservationVerdict, WebCheckResult, WebFailureInterval, WebIntervalCloseReason,
    WebNotifyContent, WebNotifyDelivery, WebNotifyEnqueue, WebNotifyIntent, WebNotifyKind,
    WebNotifySettings, WebRecoveredInterval, WebValueKind, WebWatchEntry,
};

/// Notifications the recorded checks call for under the current settings revision, and a
/// failure notification for the open interval judged with the current settings. Each comes
/// only from facts: verdicts, failure intervals without runtime gaps, and recorded gaps.
pub(super) fn derive_intents(
    ledger: &WebWatchLedger,
    entry: &WebWatchEntry,
    settings: &WebNotifySettings,
    facts: &BTreeMap<String, WebNotifyEntry>,
) -> Vec<WebNotifyIntent> {
    let Some(target) = settings.target.as_deref() else {
        return Vec::new();
    };
    let threshold_ms = settings
        .threshold_seconds(entry.plan.cadence_seconds)
        .saturating_mul(1000);
    let queued = |kind: WebNotifyKind, event_id: &str| {
        facts.values().any(|fact| {
            fact.intent.kind() == kind
                && fact.intent.event_id == event_id
                && fact.snapshot.enqueue == WebNotifyEnqueue::Enqueued
                && !fact.snapshot.suppressed_stale
        })
    };
    let cadence_ms = entry.plan.cadence_seconds.saturating_mul(1000);
    let judge = Judge {
        threshold_ms,
        failing: &|interval: &WebFailureInterval| {
            interval.held_failing_ms(cadence_ms) >= threshold_ms
        },
        failure_sent: &|interval: &WebFailureInterval| {
            facts.values().any(|fact| {
                fact.intent.kind() == WebNotifyKind::Failure
                    && fact.intent.event_id == interval.interval_id
                    && !fact.snapshot.suppressed_stale
                    && (fact.snapshot.consumption_started
                        || (fact.snapshot.enqueue == WebNotifyEnqueue::Enqueued
                            && fact.snapshot.delivery != WebNotifyDelivery::Failed
                            && fact.intent.settings_revision == settings.revision))
            })
        },
        failure_unrecorded: &|interval: &WebFailureInterval| {
            !queued(WebNotifyKind::Failure, &interval.interval_id)
                && !facts.contains_key(&WebNotifyIntent::correlation(
                    WebNotifyKind::Failure,
                    &interval.interval_id,
                    settings.revision,
                ))
        },
    };
    let mut intents: Vec<WebNotifyIntent> = Vec::new();
    let mut add = |point: &WebNotifyPoint, event_id: String, content: WebNotifyContent| {
        let kind = content.kind();
        let correlation_id = WebNotifyIntent::correlation(kind, &event_id, settings.revision);
        if queued(kind, &event_id)
            || intents
                .iter()
                .any(|intent| intent.correlation_id == correlation_id)
        {
            return;
        }
        intents.push(WebNotifyIntent {
            correlation_id,
            owner_session_id: point.record.wake.owner_session_id.clone(),
            entrusted_work_id: point.record.wake.entrusted_work_id.clone(),
            target: target.to_owned(),
            settings_revision: settings.revision,
            event_id,
            wake: point.record.wake.clone(),
            url: point.record.request_url.clone(),
            url_digest: point.record.request_url_digest.clone(),
            content,
        });
    };
    for point in &ledger.notify_points {
        if point.record.notify_revision != Some(settings.revision) {
            continue;
        }
        for (event_id, content) in events(point, entry, &judge) {
            add(point, event_id, content);
        }
    }
    if let Some(point) = &ledger.open_failure {
        if let Some((event_id, content)) = failure_event(point, entry, &judge) {
            add(point, event_id, content);
        }
    }
    intents
}

struct Judge<'a> {
    threshold_ms: u64,
    /// The interval has failed for the threshold, held time included.
    failing: &'a dyn Fn(&WebFailureInterval) -> bool,
    /// A failure mail for the interval was queued under these settings or is being sent.
    failure_sent: &'a dyn Fn(&WebFailureInterval) -> bool,
    /// No failure mail for the interval is queued, nor recorded under these settings.
    failure_unrecorded: &'a dyn Fn(&WebFailureInterval) -> bool,
}

fn events(
    point: &WebNotifyPoint,
    entry: &WebWatchEntry,
    judge: &Judge<'_>,
) -> Vec<(String, WebNotifyContent)> {
    let record = &point.record;
    let mut events = Vec::new();
    let closed = point.interval.as_ref().filter(|interval| {
        interval
            .closed
            .is_some_and(|closure| closure.reason == WebIntervalCloseReason::Recovered)
    });
    match &record.result {
        WebCheckResult::Observed { .. } => {
            // A closed interval past the threshold of the settings this check was recorded with
            // gets its failure mail first when none was made yet, then its recovery.
            let late_failure = closed.filter(|interval| {
                (judge.failure_unrecorded)(interval) && (judge.failing)(interval)
            });
            if let Some(interval) = late_failure {
                events.push(failure_content(interval, point, entry, judge));
            }
            let recovered =
                closed.filter(|interval| late_failure.is_some() || (judge.failure_sent)(interval));
            let decision = &record.decision;
            match (&decision.verdict, &decision.previous, &decision.current) {
                (ObservationVerdict::NotifyCandidate, Some(previous), Some(current))
                    if record.wake.plan_digest == entry.plan.digest() =>
                {
                    events.push((
                        decision.decision_id.clone(),
                        WebNotifyContent::Change {
                            condition: entry.plan.condition.clone(),
                            phrase: match &entry.plan.extraction.value {
                                WebValueKind::Text { phrase } => Some(phrase.clone()),
                                WebValueKind::Number { .. } => None,
                            },
                            previous: Box::new(previous.clone()),
                            current: Box::new(current.clone()),
                            recovered: recovered.map(|interval| WebRecoveredInterval {
                                interval_id: interval.interval_id.clone(),
                                started_at_ms: interval.started_at_ms,
                                failure_count: interval.failure_count,
                            }),
                        },
                    ));
                }
                _ => {
                    if let Some(interval) = recovered {
                        events.push((
                            interval.interval_id.clone(),
                            WebNotifyContent::Recovery {
                                started_at_ms: interval.started_at_ms,
                                failure_count: interval.failure_count,
                                recovered_at_ms: record.observed_at_ms,
                            },
                        ));
                    }
                }
            }
        }
        WebCheckResult::Failed { .. } => events.extend(failure_event(point, entry, judge)),
        WebCheckResult::Blocked { .. } => {}
    }
    if let Some(gap) = record
        .runtime_gap
        .filter(|gap| gap.to_ms.saturating_sub(gap.from_ms) >= judge.threshold_ms)
    {
        events.push((
            record.wake.durable_work_id(),
            WebNotifyContent::Gap {
                from_ms: gap.from_ms,
                to_ms: gap.to_ms,
                next_check_at_ms: record.next_wake_at_ms,
            },
        ));
    }
    events
}

fn failure_event(
    point: &WebNotifyPoint,
    entry: &WebWatchEntry,
    judge: &Judge<'_>,
) -> Option<(String, WebNotifyContent)> {
    let interval = point
        .interval
        .as_ref()
        .filter(|interval| interval.closed.is_none() && (judge.failing)(interval))?;
    Some(failure_content(interval, point, entry, judge))
}

fn failure_content(
    interval: &WebFailureInterval,
    point: &WebNotifyPoint,
    entry: &WebWatchEntry,
    judge: &Judge<'_>,
) -> (String, WebNotifyContent) {
    let plan = &point.record.wake.plan_digest;
    let last_success_at_ms = point
        .last_success
        .as_ref()
        .filter(|(_, digest)| digest == plan)
        .map(|(at_ms, _)| *at_ms)
        .into_iter()
        .chain((entry.plan.digest() == *plan).then_some(entry.baseline.observed_at_ms))
        .max();
    (
        interval.interval_id.clone(),
        WebNotifyContent::Failure {
            failure: interval.last_failure,
            started_at_ms: interval.started_at_ms,
            failure_count: interval.failure_count,
            failing_for_ms: interval.failing_for_ms(),
            last_success_at_ms,
            next_check_at_ms: interval.next_check_at_ms,
            held_until_next_check: interval.failing_for_ms() < judge.threshold_ms,
        },
    )
}
