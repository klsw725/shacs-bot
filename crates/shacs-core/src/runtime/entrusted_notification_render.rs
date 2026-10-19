use super::{
    EntrustedNotificationKind, ObservationCriterion, ObservationDecision, ObservationError,
};

pub(super) fn render(
    decision: &ObservationDecision,
    kind: EntrustedNotificationKind,
) -> Result<String, ObservationError> {
    let current = decision
        .current
        .as_ref()
        .ok_or(ObservationError::InvalidInput(
            "notification current evidence missing",
        ))?;
    let policy = decision
        .policy
        .as_ref()
        .ok_or(ObservationError::InvalidInput(
            "notification criterion missing",
        ))?;
    let ObservationCriterion::IntegerAtMost { value: threshold } = policy.criterion;
    let previous = decision.previous.as_ref().map_or_else(
        || "not available (baseline)".to_owned(),
        |evidence| {
            format!(
                "{} (condition_met={})",
                evidence.summary.value, evidence.condition_matches
            )
        },
    );
    let (title, reason) = match kind {
        EntrustedNotificationKind::Change => (
            "Change notification",
            "The explicit condition outcome changed.",
        ),
        EntrustedNotificationKind::RegularSummary => (
            "Requested regular summary (not a change alert)",
            "You explicitly requested a summary for each comparable observation.",
        ),
    };
    let safe_ref = |value: &str| -> String {
        value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || *character == ':')
            .take(96)
            .collect()
    };
    let previous_evidence = decision.previous.as_ref().map_or_else(
        || "none".to_owned(),
        |evidence| safe_ref(&evidence.evidence_ref),
    );
    Ok(format!(
        "{title}\nCondition: integer <= {threshold} (revision {})\nPrevious: {previous}\nNew: {} (condition_met={})\nReason: {reason}\nEvidence previous: {previous_evidence}\nEvidence new: {}\nObserved at: {} ms; source as-of: {} ms\nLimitations: user-owned local integer source only; source-asserted timestamp; no external verification. No external action performed. Transport send is not a remote receipt.",
        decision.condition_revision, current.summary.value, current.condition_matches,
        safe_ref(&current.evidence_ref), current.observed_at_ms, current.summary.as_of_ms,
    ))
}
