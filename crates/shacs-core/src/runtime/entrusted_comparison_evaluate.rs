use super::entrusted_comparison::decision_id;
use super::{
    ObservationComparisonEvidence, ObservationComparisonLimit,
    ObservationComparisonReason as Reason, ObservationDecision, ObservationRecord,
    ObservationResult, ObservationVerdict as Verdict,
};

struct ComparisonHistory<'a> {
    baseline: Option<&'a ObservationRecord>,
    previous_observation: Option<&'a ObservationRecord>,
}

pub(super) fn compare(
    record: &ObservationRecord,
    baseline: Option<&ObservationRecord>,
    previous_observation: Option<&ObservationRecord>,
) -> ObservationDecision {
    let wake = &record.wake;
    let mut decision = ObservationDecision {
        basis: super::ObservationDecisionBasis::ObservationTerminal,
        decision_id: decision_id(&wake.durable_work_id()),
        observation_work_id: wake.durable_work_id(),
        entrusted_work_id: wake.entrusted_work_id.clone(),
        condition_revision: wake.condition_revision,
        goal_transition_id: wake.goal_transition_id.clone(),
        source: wake.plan.source.clone(),
        policy: wake.plan.comparison.clone(),
        evaluated_at_ms: record.observed_at_ms,
        previous: None,
        current: None,
        verdict: Verdict::Unknown,
        reason: Reason::CriteriaNotConfigured,
        regular_summary_eligible: false,
        limitations: vec![
            ObservationComparisonLimit::UserOwnedLocalSourceOnly,
            ObservationComparisonLimit::SourceAssertedTimestamp,
            ObservationComparisonLimit::IntegerConditionOnly,
            ObservationComparisonLimit::NoExternalVerification,
            ObservationComparisonLimit::NoDeliveryOrAction,
        ],
    };
    let (verdict, reason) = evaluate(
        &mut decision,
        record,
        ComparisonHistory {
            baseline,
            previous_observation,
        },
    );
    decision.verdict = verdict;
    decision.reason = reason;
    decision.regular_summary_eligible = verdict.establishes_baseline()
        && wake
            .plan
            .comparison
            .as_ref()
            .is_some_and(|policy| policy.regular_summary);
    decision
}

fn evaluate(
    decision: &mut ObservationDecision,
    record: &ObservationRecord,
    history: ComparisonHistory<'_>,
) -> (Verdict, Reason) {
    let (evidence_ref, byte_count, summary) = match &record.result {
        ObservationResult::Observed {
            evidence_ref,
            byte_count,
            summary,
        } => (evidence_ref, byte_count, summary),
        ObservationResult::Unavailable { reason } => {
            return (
                Verdict::Unknown,
                Reason::ObservationUnavailable { reason: *reason },
            );
        }
        ObservationResult::Blocked { reason } => {
            return (
                Verdict::Blocked,
                Reason::ObservationBlocked { reason: *reason },
            );
        }
    };
    let Some(policy) = &record.wake.plan.comparison else {
        return (Verdict::Unknown, Reason::CriteriaNotConfigured);
    };
    let Some(summary) = summary else {
        return (Verdict::Unknown, Reason::SummaryUnavailable);
    };
    decision.current = Some(ObservationComparisonEvidence {
        observation_work_id: record.wake.durable_work_id(),
        evidence_ref: evidence_ref.clone(),
        byte_count: *byte_count,
        observed_at_ms: record.observed_at_ms,
        summary: summary.clone(),
        condition_matches: policy.criterion.matches(summary.value),
        execution_snapshot_ref: record.execution_snapshot_ref.clone(),
    });
    let Some(age) = record.observed_at_ms.checked_sub(summary.as_of_ms) else {
        return (Verdict::Unknown, Reason::SourceFromFuture);
    };
    let max_age_ms = policy.max_age_seconds.saturating_mul(1000);
    if age > max_age_ms {
        return (Verdict::Unknown, Reason::SourceStale);
    }
    let same_lineage = |prior: &&ObservationRecord| {
        prior.wake.lineage_prefix() == record.wake.lineage_prefix()
            && prior.wake.plan == record.wake.plan
    };
    let Some(prior) = history.baseline.filter(same_lineage) else {
        return if history
            .previous_observation
            .filter(same_lineage)
            .and_then(|record| record.decision.as_ref())
            .is_some_and(|prior| prior.verdict.establishes_baseline())
        {
            (Verdict::Unknown, Reason::BaselineUnavailable)
        } else if history.baseline.is_some()
            || history
                .previous_observation
                .is_some_and(|prior| !same_lineage(&prior))
        {
            (Verdict::Baseline, Reason::LineageReset)
        } else {
            (Verdict::Baseline, Reason::FirstComparableObservation)
        };
    };
    let Some(previous) = prior
        .decision
        .as_ref()
        .and_then(|value| value.current.clone())
    else {
        return (Verdict::Unknown, Reason::BaselineUnavailable);
    };
    decision.previous = Some(previous.clone());
    if summary.as_of_ms < previous.summary.as_of_ms {
        return (Verdict::Unknown, Reason::SourceOutOfOrder);
    }
    if summary.as_of_ms == previous.summary.as_of_ms && summary.value != previous.summary.value {
        return (Verdict::Unknown, Reason::SourceTimestampConflict);
    }
    if record
        .observed_at_ms
        .saturating_sub(previous.summary.as_of_ms)
        > max_age_ms
    {
        return (Verdict::Unknown, Reason::BaselineStale);
    }
    if previous.condition_matches != policy.criterion.matches(summary.value) {
        (Verdict::NotifyCandidate, Reason::ConditionOutcomeChanged)
    } else if previous.summary.value == summary.value {
        (Verdict::Suppress, Reason::EquivalentValue)
    } else {
        (Verdict::Suppress, Reason::SameConditionOutcome)
    }
}
