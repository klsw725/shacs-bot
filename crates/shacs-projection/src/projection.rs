use shacs_eval::evaluator::{
    spec018_approval_item_channel_visible, spec018_channel_event_kind_for_status,
    spec018_evidence_refs_are_redacted, Spec018ApprovalProjectionItem,
    Spec018AutomationDeliveryStatus, Spec018BlockedProjectionItem, Spec018EvaluatorDecisionSummary,
    Spec018GoalSummary, Spec018Projection, Spec018ReplayRegressionSummary,
    Spec018VerificationProjectionItem, SPEC018_PROJECTION_SCHEMA_LABEL,
    SPEC018_PROJECTION_SCHEMA_VERSION,
};
use shacs_redaction::redact_string;

#[derive(Debug, Clone, Copy)]
pub struct RuntimeSpec018ProjectionInput<'a> {
    pub generated_at_ms: u64,
    pub session_id: &'a str,
    pub goal_summaries: &'a [Spec018GoalSummary],
    pub automation_summaries: &'a [Spec018AutomationDeliveryStatus],
    pub approval_summaries: &'a [Spec018ApprovalProjectionItem],
    pub blocked_summaries: &'a [Spec018BlockedProjectionItem],
    pub verification_summaries: &'a [Spec018VerificationProjectionItem],
    pub replay_summaries: &'a [Spec018ReplayRegressionSummary],
    pub recent_evaluator_decision_summaries: &'a [Spec018EvaluatorDecisionSummary],
}

pub fn build_spec018_projection(input: RuntimeSpec018ProjectionInput<'_>) -> Spec018Projection {
    let evidence_refs = input
        .goal_summaries
        .iter()
        .flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        })
        .chain(input.automation_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(input.approval_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(input.blocked_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(std::iter::once(&item.diagnostics_ref))
        }))
        .chain(input.verification_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(input.replay_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(
            input
                .recent_evaluator_decision_summaries
                .iter()
                .flat_map(|item| {
                    item.evidence_refs
                        .iter()
                        .chain(item.status.evidence_refs.iter())
                }),
        )
        .filter(|evidence_ref| {
            spec018_evidence_refs_are_redacted(std::slice::from_ref(*evidence_ref))
        })
        .cloned()
        .collect();

    Spec018Projection {
        schema_label: SPEC018_PROJECTION_SCHEMA_LABEL.to_owned(),
        schema_version: SPEC018_PROJECTION_SCHEMA_VERSION.to_owned(),
        generated_at_ms: input.generated_at_ms,
        session_id: input.session_id.to_owned(),
        goal_summaries: input.goal_summaries.iter().map(sanitize_goal).collect(),
        automation_summaries: input
            .automation_summaries
            .iter()
            .map(sanitize_automation)
            .collect(),
        approval_summaries: input
            .approval_summaries
            .iter()
            .map(sanitize_approval)
            .collect(),
        blocked_summaries: input
            .blocked_summaries
            .iter()
            .map(sanitize_blocked)
            .collect(),
        verification_summaries: input
            .verification_summaries
            .iter()
            .map(sanitize_verification)
            .collect(),
        replay_summaries: input.replay_summaries.iter().map(sanitize_replay).collect(),
        recent_evaluator_decision_summaries: input
            .recent_evaluator_decision_summaries
            .iter()
            .map(sanitize_evaluator_decision)
            .collect(),
        evidence_refs,
    }
}

pub fn runtime_spec018_local_api_projection(projection: &Spec018Projection) -> Spec018Projection {
    let goal_summaries: Vec<_> = projection
        .goal_summaries
        .iter()
        .map(sanitize_goal)
        .collect();
    let automation_summaries: Vec<_> = projection
        .automation_summaries
        .iter()
        .map(sanitize_automation)
        .collect();
    let approval_summaries: Vec<_> = projection
        .approval_summaries
        .iter()
        .map(sanitize_approval)
        .collect();
    let blocked_summaries: Vec<_> = projection
        .blocked_summaries
        .iter()
        .map(sanitize_blocked)
        .collect();
    let verification_summaries: Vec<_> = projection
        .verification_summaries
        .iter()
        .map(sanitize_verification)
        .collect();
    let replay_summaries: Vec<_> = projection
        .replay_summaries
        .iter()
        .map(sanitize_replay)
        .collect();
    let recent_evaluator_decision_summaries: Vec<_> = projection
        .recent_evaluator_decision_summaries
        .iter()
        .map(sanitize_evaluator_decision)
        .collect();
    let evidence_refs = collect_projection_evidence_refs(
        &goal_summaries,
        &automation_summaries,
        &approval_summaries,
        &blocked_summaries,
        &verification_summaries,
        &replay_summaries,
        &recent_evaluator_decision_summaries,
    );

    Spec018Projection {
        schema_label: projection.schema_label.clone(),
        schema_version: projection.schema_version.clone(),
        generated_at_ms: projection.generated_at_ms,
        session_id: projection.session_id.clone(),
        goal_summaries,
        automation_summaries,
        approval_summaries,
        blocked_summaries,
        verification_summaries,
        replay_summaries,
        recent_evaluator_decision_summaries,
        evidence_refs,
    }
}

pub fn runtime_spec018_channel_projection(projection: &Spec018Projection) -> Spec018Projection {
    let goal_summaries: Vec<_> = projection
        .goal_summaries
        .iter()
        .filter(|item| spec018_channel_event_kind_for_status(&item.status).is_some())
        .map(sanitize_goal)
        .collect();
    let automation_summaries: Vec<_> = projection
        .automation_summaries
        .iter()
        .filter(|item| spec018_channel_event_kind_for_status(&item.status).is_some())
        .map(sanitize_automation)
        .collect();
    let approval_summaries: Vec<_> = projection
        .approval_summaries
        .iter()
        .filter(|item| spec018_approval_item_channel_visible(item))
        .map(sanitize_approval)
        .collect();
    let blocked_summaries = projection
        .blocked_summaries
        .iter()
        .map(sanitize_blocked)
        .collect::<Vec<_>>();
    let verification_summaries: Vec<_> = projection
        .verification_summaries
        .iter()
        .filter(|item| spec018_channel_event_kind_for_status(&item.status).is_some())
        .map(sanitize_verification)
        .collect();
    let replay_summaries: Vec<_> = projection
        .replay_summaries
        .iter()
        .filter(|item| spec018_channel_event_kind_for_status(&item.status).is_some())
        .map(sanitize_replay)
        .collect();
    let recent_evaluator_decision_summaries: Vec<_> = projection
        .recent_evaluator_decision_summaries
        .iter()
        .filter(|item| spec018_channel_event_kind_for_status(&item.status).is_some())
        .map(sanitize_evaluator_decision)
        .collect();
    let evidence_refs = collect_projection_evidence_refs(
        &goal_summaries,
        &automation_summaries,
        &approval_summaries,
        &blocked_summaries,
        &verification_summaries,
        &replay_summaries,
        &recent_evaluator_decision_summaries,
    );

    Spec018Projection {
        schema_label: projection.schema_label.clone(),
        schema_version: projection.schema_version.clone(),
        generated_at_ms: projection.generated_at_ms,
        session_id: projection.session_id.clone(),
        goal_summaries,
        automation_summaries,
        approval_summaries,
        blocked_summaries,
        verification_summaries,
        replay_summaries,
        recent_evaluator_decision_summaries,
        evidence_refs,
    }
}

fn sanitize_refs(refs: &mut Vec<shacs_eval::evaluator::EvidenceRef>) {
    refs.retain(|evidence_ref| {
        spec018_evidence_refs_are_redacted(std::slice::from_ref(evidence_ref))
    });
}

fn sanitize_status(status: &mut shacs_eval::evaluator::Spec018ProjectionStatus) {
    if let Some(user_action_hint) = status.user_action_hint.as_mut() {
        *user_action_hint = redact_string(user_action_hint);
    }
    sanitize_refs(&mut status.evidence_refs);
}

fn sanitize_goal(item: &Spec018GoalSummary) -> Spec018GoalSummary {
    let mut item = item.clone();
    item.summary = redact_string(&item.summary);
    sanitize_status(&mut item.status);
    sanitize_refs(&mut item.evidence_refs);
    item
}

fn sanitize_automation(item: &Spec018AutomationDeliveryStatus) -> Spec018AutomationDeliveryStatus {
    let mut item = item.clone();
    if let Some(suppress_reason) = item.suppress_reason.as_mut() {
        *suppress_reason = redact_string(suppress_reason);
    }
    sanitize_status(&mut item.status);
    sanitize_refs(&mut item.evidence_refs);
    item
}

fn sanitize_approval(item: &Spec018ApprovalProjectionItem) -> Spec018ApprovalProjectionItem {
    let mut item = item.clone();
    item.risk_summary = redact_string(&item.risk_summary);
    item.rollback_summary = redact_string(&item.rollback_summary);
    for decision in &mut item.allowed_decisions {
        if let Some(unavailable_reason) = decision.unavailable_reason.as_mut() {
            *unavailable_reason = redact_string(unavailable_reason);
        }
    }
    sanitize_status(&mut item.status);
    sanitize_refs(&mut item.evidence_refs);
    item
}

fn sanitize_blocked(item: &Spec018BlockedProjectionItem) -> Spec018BlockedProjectionItem {
    let mut item = item.clone();
    item.blocked_reason = redact_string(&item.blocked_reason);
    item.user_action_hint = redact_string(&item.user_action_hint);
    sanitize_refs(&mut item.evidence_refs);
    if !spec018_evidence_refs_are_redacted(std::slice::from_ref(&item.diagnostics_ref)) {
        item.diagnostics_ref = shacs_eval::evaluator::EvidenceRef {
            kind: shacs_eval::evaluator::EvidenceKind::DiagnosticRecord,
            id: "redacted-diagnostics-ref".to_owned(),
            digest: "redacted-diagnostics-ref".to_owned(),
            summary: "diagnostics ref redacted from projection".to_owned(),
            redaction_status: shacs_eval::evaluator::RedactionStatus::Redacted,
            owner_spec: Some("018".to_owned()),
            locator: None,
            retention_hint: Some("projection".to_owned()),
        };
    }
    item
}

fn sanitize_verification(
    item: &Spec018VerificationProjectionItem,
) -> Spec018VerificationProjectionItem {
    let mut item = item.clone();
    item.expected_behavior = redact_string(&item.expected_behavior);
    if let Some(failure_reason) = item.failure_reason.as_mut() {
        *failure_reason = redact_string(failure_reason);
    }
    sanitize_status(&mut item.status);
    sanitize_refs(&mut item.evidence_refs);
    item
}

fn sanitize_replay(item: &Spec018ReplayRegressionSummary) -> Spec018ReplayRegressionSummary {
    let mut item = item.clone();
    item.expected_summary = redact_string(&item.expected_summary);
    item.actual_summary = redact_string(&item.actual_summary);
    sanitize_status(&mut item.status);
    sanitize_refs(&mut item.evidence_refs);
    item
}

fn sanitize_evaluator_decision(
    item: &Spec018EvaluatorDecisionSummary,
) -> Spec018EvaluatorDecisionSummary {
    let mut item = item.clone();
    item.summary = redact_string(&item.summary);
    sanitize_status(&mut item.status);
    sanitize_refs(&mut item.evidence_refs);
    item
}

fn collect_projection_evidence_refs(
    goal_summaries: &[Spec018GoalSummary],
    automation_summaries: &[Spec018AutomationDeliveryStatus],
    approval_summaries: &[Spec018ApprovalProjectionItem],
    blocked_summaries: &[Spec018BlockedProjectionItem],
    verification_summaries: &[Spec018VerificationProjectionItem],
    replay_summaries: &[Spec018ReplayRegressionSummary],
    recent_evaluator_decision_summaries: &[Spec018EvaluatorDecisionSummary],
) -> Vec<shacs_eval::evaluator::EvidenceRef> {
    goal_summaries
        .iter()
        .flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        })
        .chain(automation_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(approval_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(blocked_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(std::iter::once(&item.diagnostics_ref))
        }))
        .chain(verification_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(replay_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .chain(recent_evaluator_decision_summaries.iter().flat_map(|item| {
            item.evidence_refs
                .iter()
                .chain(item.status.evidence_refs.iter())
        }))
        .filter(|evidence_ref| {
            spec018_evidence_refs_are_redacted(std::slice::from_ref(*evidence_ref))
        })
        .cloned()
        .collect()
}
