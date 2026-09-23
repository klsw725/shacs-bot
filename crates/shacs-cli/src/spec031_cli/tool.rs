use super::{reason_summary, render::envelope_line, severity};
use crate::opaque_ref;
use shacs_core::runtime::{
    ExecutionDomain, ExecutionIdentity, ExecutionOutcome, LateResultDecision,
    RuntimeExecutionLedger, ToolOutcomeKind,
};
use shacs_projection::{
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031ConstructionError,
    Spec031Count, Spec031Envelope, Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage,
    Spec031ObservedAtUnixMs, Spec031ParentRef, Spec031ProjectionKind, Spec031Reason,
    Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Source, Spec031SourceOwner,
    Spec031SubjectRef, Spec031ToolCapability,
};
use shacs_session::SessionManager;
use std::collections::BTreeSet;

pub(crate) fn read(manager: &SessionManager, session: &str) -> Vec<String> {
    let value = manager
        .load_existing(session)
        .and_then(|session| session.metadata.get("runtime_execution").cloned());
    let ledger = value
        .map(serde_json::from_value::<RuntimeExecutionLedger>)
        .transpose();
    match ledger {
        Ok(ledger) => lines(ledger.as_ref()),
        Err(_) => vec!["Spec031 tool: state=unavailable reason=unsupported attempts=unknown freshness=unavailable".to_owned()],
    }
}

pub(crate) fn lines(ledger: Option<&RuntimeExecutionLedger>) -> Vec<String> {
    let Some(ledger) = ledger else {
        return vec!["Spec031 tool: state=unavailable reason=missing_external_owner_evidence attempts=unknown freshness=unavailable".to_owned()];
    };
    let outcomes: Vec<_> = ledger
        .outcomes
        .iter()
        .filter_map(|record| match &record.fact.outcome {
            ExecutionOutcome::Tool(outcome) => Some((record, outcome)),
            ExecutionOutcome::Provider(_) | ExecutionOutcome::Subagent(_) => None,
        })
        .collect();
    let pending: Vec<_> = ledger
        .pending
        .iter()
        .filter(|pending| pending.domain == ExecutionDomain::Tool)
        .collect();
    let attempts: BTreeSet<_> = outcomes
        .iter()
        .map(|(record, _)| &record.fact.identity)
        .chain(pending.iter().map(|pending| &pending.identity))
        .map(|identity| {
            (
                &identity.scope.session_id,
                &identity.scope.turn_id,
                &identity.effect_id,
                &identity.correlation_id,
                &identity.attempt_id,
            )
        })
        .collect();
    let mut lines = outcomes
        .iter()
        .rev()
        .take(20)
        .rev()
        .map(|(record, outcome)| {
            let (state, reason, decision) = match record.decision {
                LateResultDecision::Accepted => {
                    let (state, reason) = outcome_state(outcome);
                    (state, reason, "accepted")
                }
                LateResultDecision::DuplicateIgnored { .. } => (
                    Spec031Availability::Degraded,
                    Spec031ReasonCode::Skipped,
                    "duplicate",
                ),
                LateResultDecision::DiscardedLate { .. } => (
                    Spec031Availability::Degraded,
                    Spec031ReasonCode::Skipped,
                    "late",
                ),
                LateResultDecision::DiscardedStale { .. } => (
                    Spec031Availability::Degraded,
                    Spec031ReasonCode::Skipped,
                    "stale",
                ),
            };
            let result_ref = record
                .fact
                .artifact_ref
                .as_ref()
                .map(|reference| opaque_ref("result", &reference.locator));
            format!(
                "{} outcome={} decision={decision} result={}",
                line(
                    &record.fact.identity,
                    (state, reason),
                    record.fact.finished_at_ms
                ),
                outcome_label(outcome),
                result_ref.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>();
    lines.extend(pending.iter().rev().take(20).rev().map(|pending| {
        format!(
            "{} pending=1",
            line(
                &pending.identity,
                (Spec031Availability::Unknown, Spec031ReasonCode::Requested),
                pending.started_at_ms
            )
        )
    }));
    lines.push(format!("Tool owner counts: pending={} outcomes={} retained_rows={} attempts={} detail_capabilities=unavailable",
        pending.len(), outcomes.len(), outcomes.len().min(20), attempts.len()));
    if attempts.is_empty() {
        lines.push(
            "Spec031 tool: state=unavailable reason=missing attempts=0 freshness=current"
                .to_owned(),
        );
    }
    lines
}

fn line(
    identity: &ExecutionIdentity,
    status: (Spec031Availability, Spec031ReasonCode),
    observed_at: u128,
) -> String {
    match envelope(identity, status, observed_at) {
        Ok(envelope) => format!(
            "{} attempt={}",
            envelope_line("tool", &envelope),
            opaque_ref("attempt", &identity.attempt_id)
        ),
        Err(error) => format!("Spec031 tool: state=unavailable reason=unsupported detail={error}"),
    }
}

fn envelope(
    identity: &ExecutionIdentity,
    (state, reason): (Spec031Availability, Spec031ReasonCode),
    observed_at: u128,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Tool,
        state,
        severity: severity(state),
        reason: Spec031Reason {
            code: reason,
            safe_summary: Spec031SafeSummary::try_new(reason_summary(reason))?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(&opaque_ref(
                "subject:tool",
                &identity.effect_id,
            ))?,
            parent_ref: Some(Spec031ParentRef::try_new(&opaque_ref(
                "parent:turn",
                &identity.scope.turn_id,
            ))?),
            action_ref: Some(Spec031ActionRef::try_new(&opaque_ref(
                "action:tool",
                &identity.effect_id,
            ))?),
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Session,
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(
                u64::try_from(observed_at).unwrap_or(u64::MAX),
            )),
            freshness: Spec031Freshness::Current,
        },
        capability: Spec031Capability::Tool(Spec031ToolCapability {
            attempt_count: Some(Spec031Count::new(1)),
        }),
        children: Vec::new(),
    })
}

const fn outcome_state(outcome: &ToolOutcomeKind) -> (Spec031Availability, Spec031ReasonCode) {
    match outcome {
        ToolOutcomeKind::Completed => (Spec031Availability::Ready, Spec031ReasonCode::Completed),
        ToolOutcomeKind::Failed { .. } => {
            (Spec031Availability::Blocked, Spec031ReasonCode::Blocked)
        }
        ToolOutcomeKind::TimedOut
        | ToolOutcomeKind::Cancelled
        | ToolOutcomeKind::Interrupted { .. } => {
            (Spec031Availability::Blocked, Spec031ReasonCode::Interrupted)
        }
        ToolOutcomeKind::Skipped { .. } | ToolOutcomeKind::Stale => {
            (Spec031Availability::Degraded, Spec031ReasonCode::Skipped)
        }
    }
}

const fn outcome_label(outcome: &ToolOutcomeKind) -> &'static str {
    match outcome {
        ToolOutcomeKind::Completed => "completed",
        ToolOutcomeKind::Failed { .. } => "failed",
        ToolOutcomeKind::TimedOut => "timed_out",
        ToolOutcomeKind::Cancelled => "cancelled",
        ToolOutcomeKind::Interrupted { .. } => "interrupted",
        ToolOutcomeKind::Skipped { .. } => "skipped",
        ToolOutcomeKind::Stale => "stale",
    }
}
