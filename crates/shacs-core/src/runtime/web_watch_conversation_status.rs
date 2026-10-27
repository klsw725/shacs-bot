use serde::Serialize;

use super::web_watch_notify_render::{condition_text, duration_text, failure_text, time};
use super::web_watch_redact::{safe_text, MAX_PHRASE_CHARS};
use super::{
    ObservationNextWake, WebDecisionReason, WebObservedValue, WebValueKind, WebWatchSnapshot,
};

/// The stored state of a watch in words, so a conversational answer is exactly the facts that
/// `web-status` shows.
pub(super) fn render_status(snapshot: &WebWatchSnapshot) -> String {
    let mut lines = vec![format!(
        "Web watch `{}` ({}), condition revision {}:",
        snapshot.work_id,
        words(&snapshot.goal_status),
        snapshot.condition_revision
    )];
    if let Some(plan) = &snapshot.plan {
        let phrase = match &plan.extraction.value {
            WebValueKind::Text { phrase } => Some(phrase.as_str()),
            WebValueKind::Number { .. } => None,
        };
        lines.push(format!(
            "- Page: {}; notify when {}; checked every {}",
            plan.url,
            condition_text(&plan.condition, phrase),
            duration_text(plan.cadence_seconds.saturating_mul(1000))
        ));
    }
    lines.push(match snapshot.last_success_at_ms {
        Some(at_ms) => format!("- Last successful read: {}", time(at_ms)),
        None => "- Last successful read: none yet".to_owned(),
    });
    if let Some(read) = &snapshot.last_read {
        lines.push(format!(
            "- Latest value: {} (condition met: {}), read at {}",
            observed(&read.value),
            if read.condition_matches { "yes" } else { "no" },
            time(read.observed_at_ms)
        ));
    }
    if let Some(decision) = &snapshot.current_decision {
        lines.push(format!(
            "- Latest decision: {} ({})",
            words(&decision.verdict),
            reason(&decision.reason)
        ));
    }
    lines.push(match &snapshot.failure_interval {
        Some(interval) => format!(
            "- Check failure in progress since {}: {} ({} failed checks)",
            time(interval.started_at_ms),
            failure_text(interval.last_failure),
            interval.failure_count
        ),
        None => "- Check failure in progress: none".to_owned(),
    });
    lines.push(format!("- Next check: {}", next_check(&snapshot.next_wake)));
    lines.push(match &snapshot.notify {
        Some(notify) => format!(
            "- Notification: {}; a check failure is mailed once it lasts {} (settings revision {})",
            notify
                .target
                .as_deref()
                .map_or("none", |target| target.trim_start_matches("email:")),
            duration_text(notify.effective_threshold_seconds.saturating_mul(1000)),
            notify.revision
        ),
        None => "- Notification: none".to_owned(),
    });
    lines.push(
        match snapshot
            .deadline
            .as_ref()
            .and_then(|view| view.deadline.as_ref().map(|deadline| (view, deadline)))
        {
            Some((view, deadline)) => format!(
                "- Decision deadline: {} (set as {}), reminder planned for {} (deadline revision {})",
                time(deadline.at_ms),
                safe_text(&deadline.at_text, MAX_PHRASE_CHARS),
                time(deadline.remind_at_ms),
                view.revision
            ),
            None => "- Decision deadline: none".to_owned(),
        },
    );
    lines.join("\n")
}

fn next_check(next: &ObservationNextWake) -> String {
    match next {
        ObservationNextWake::NotConfigured => "not scheduled".to_owned(),
        ObservationNextWake::AwaitingEnqueue { due_at_ms, .. }
        | ObservationNextWake::Queued { due_at_ms, .. } => time(*due_at_ms),
        ObservationNextWake::Leased { .. } => "in progress".to_owned(),
        ObservationNextWake::Stopped { reason } => format!("stopped ({})", words(reason)),
    }
}

fn reason(reason: &WebDecisionReason) -> String {
    match reason {
        WebDecisionReason::ConfirmedBaseline => "confirmed_baseline".to_owned(),
        WebDecisionReason::LineageReset => "lineage_reset".to_owned(),
        WebDecisionReason::EquivalentValue => "equivalent_value".to_owned(),
        WebDecisionReason::SameConditionOutcome => "same_condition_outcome".to_owned(),
        WebDecisionReason::ConditionOutcomeChanged => "condition_outcome_changed".to_owned(),
        WebDecisionReason::CheckFailed { failure } => {
            format!("check_failed: {}", failure_text(*failure))
        }
        WebDecisionReason::ObservationBlocked { reason } => {
            format!("observation_blocked: {}", words(reason))
        }
    }
}

fn observed(value: &WebObservedValue) -> String {
    match value {
        WebObservedValue::Number { value } => value.clone(),
        WebObservedValue::Text { present: true } => "phrase shown".to_owned(),
        WebObservedValue::Text { present: false } => "phrase not shown".to_owned(),
    }
}

fn words(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(|text| text.replace('_', " ")))
        .unwrap_or_else(|| "unknown".to_owned())
}
