use sha2::{Digest, Sha256};

use super::web_watch_fetch::{display_url, sha256_ref};
use super::web_watch_number::compare_canonical;
use super::web_watch_store::WebWatchLedger;
use super::{
    ObservationVerdict as Verdict, WebCheckFailure, WebCheckResult, WebCondition, WebDecision,
    WebDecisionReason as Reason, WebEvidence, WebObservationRecord, WebObservedValue,
    WebRuntimeGap, WebWatchEntry, WebWatchWake, WEB_TEXT_VERSION,
};

const DAY_MS: u64 = 86_400_000;

pub(super) fn condition_matches(condition: &WebCondition, value: &WebObservedValue) -> bool {
    match (condition, value) {
        (WebCondition::AtMost { value: limit }, WebObservedValue::Number { value }) => {
            compare_canonical(value, limit).is_le()
        }
        (WebCondition::AtLeast { value: limit }, WebObservedValue::Number { value }) => {
            compare_canonical(value, limit).is_ge()
        }
        (WebCondition::TextPresent, WebObservedValue::Text { present }) => *present,
        (WebCondition::TextAbsent, WebObservedValue::Text { present }) => !*present,
        _ => false,
    }
}

/// The durable fact for one check: result, judgment, runtime gap and next wake.
pub(super) fn build_record(
    wake: WebWatchWake,
    entry: &WebWatchEntry,
    result: WebCheckResult,
    ledger: &WebWatchLedger,
    now_ms: u64,
    execution_snapshot_ref: Option<String>,
) -> WebObservationRecord {
    let cadence_ms = entry.plan.cadence_seconds.saturating_mul(1000);
    let runtime_gap =
        (now_ms.saturating_sub(wake.due_at_ms) >= cadence_ms).then_some(WebRuntimeGap {
            from_ms: wake.due_at_ms,
            to_ms: now_ms,
        });
    let next_wake_at_ms =
        Some(now_ms.saturating_add(next_delay(cadence_ms, &result, &wake, ledger)));
    let decision = decide(&wake, entry, &result, ledger, now_ms);
    WebObservationRecord {
        request_url: display_url(&entry.plan.url),
        request_url_digest: sha256_ref(&entry.plan.url),
        text_version: WEB_TEXT_VERSION.to_owned(),
        observed_at_ms: now_ms,
        wake,
        result,
        runtime_gap,
        next_wake_at_ms,
        execution_snapshot_ref,
        decision,
    }
}

/// Retry-After and the 429/503 backoff win over the cadence; other results keep the cadence.
fn next_delay(
    cadence_ms: u64,
    result: &WebCheckResult,
    wake: &WebWatchWake,
    ledger: &WebWatchLedger,
) -> u64 {
    let WebCheckResult::Failed { retry_after_ms, .. } = result else {
        return cadence_ms;
    };
    if !throttled(result) {
        return cadence_ms;
    }
    let previous = ledger
        .history
        .iter()
        .rev()
        .take_while(|record| {
            record.wake.lineage_prefix() == wake.lineage_prefix() && throttled(&record.result)
        })
        .count();
    let exponent = u32::try_from(previous).unwrap_or(u32::MAX).min(16);
    let backoff = cadence_ms
        .saturating_mul(1u64 << exponent)
        .min(cadence_ms.max(DAY_MS));
    backoff.max(retry_after_ms.unwrap_or_default())
}

fn throttled(result: &WebCheckResult) -> bool {
    match result {
        WebCheckResult::Failed {
            failure, response, ..
        } => {
            *failure == WebCheckFailure::RateLimited
                || response
                    .as_ref()
                    .is_some_and(|response| response.http_status == 503)
        }
        WebCheckResult::Observed { .. } | WebCheckResult::Blocked { .. } => false,
    }
}

fn decide(
    wake: &WebWatchWake,
    entry: &WebWatchEntry,
    result: &WebCheckResult,
    ledger: &WebWatchLedger,
    now_ms: u64,
) -> WebDecision {
    let decision_id = format!(
        "web-decision:v1:{:x}",
        Sha256::digest(wake.durable_work_id().as_bytes())
    );
    let (verdict, reason, previous, current) = match result {
        WebCheckResult::Failed { failure, .. } => (
            Verdict::Unknown,
            Reason::CheckFailed { failure: *failure },
            None,
            None,
        ),
        WebCheckResult::Blocked { reason } => (
            Verdict::Blocked,
            Reason::ObservationBlocked { reason: *reason },
            None,
            None,
        ),
        WebCheckResult::Observed { response, value } => {
            let current = WebEvidence {
                observation_ref: wake.durable_work_id(),
                observed_at_ms: now_ms,
                value: value.clone(),
                condition_matches: condition_matches(&entry.plan.condition, value),
                response: response.clone(),
            };
            match comparable_before(wake, entry, ledger) {
                None => (Verdict::Baseline, Reason::LineageReset, None, Some(current)),
                Some(previous) => {
                    let (verdict, reason) =
                        if previous.condition_matches != current.condition_matches {
                            (Verdict::NotifyCandidate, Reason::ConditionOutcomeChanged)
                        } else if same_value(&previous.value, &current.value) {
                            (Verdict::Suppress, Reason::EquivalentValue)
                        } else {
                            (Verdict::Suppress, Reason::SameConditionOutcome)
                        };
                    (verdict, reason, Some(previous), Some(current))
                }
            }
        }
    };
    WebDecision {
        decision_id,
        verdict,
        reason,
        previous,
        current,
    }
}

/// The last comparable value of this lineage, or the confirmation read that started it.
fn comparable_before(
    wake: &WebWatchWake,
    entry: &WebWatchEntry,
    ledger: &WebWatchLedger,
) -> Option<WebEvidence> {
    ledger
        .comparable
        .as_ref()
        .filter(|record| {
            record.wake.lineage_prefix() == wake.lineage_prefix()
                && record.wake.plan_digest == wake.plan_digest
        })
        .and_then(|record| record.decision.current.clone())
        .or_else(|| {
            (entry.baseline_revision == wake.condition_revision
                && entry.baseline_transition_id == wake.goal_transition_id
                && entry.plan.digest() == wake.plan_digest)
                .then(|| entry.baseline.clone())
        })
}

fn same_value(left: &WebObservedValue, right: &WebObservedValue) -> bool {
    match (left, right) {
        (WebObservedValue::Number { value: left }, WebObservedValue::Number { value: right }) => {
            compare_canonical(left, right).is_eq()
        }
        _ => left == right,
    }
}
