use super::accounting::{refresh_evidence_id, unavailable, unavailable_token_accounting};
use super::builder::{
    classifier_decision_evidence, ClassifierAttemptStatus, ClassifierEvidenceInput,
};
use super::types::*;
use crate::runtime::{
    AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, EvaluatorConfidence, EvaluatorScopeMatch,
    PermissionPolicyDecision, PermissionPolicyReason, PermissionedAction,
};
use serde_json::json;

pub fn skipped_classifier_evidence(
    created_at_unix_ms: u64,
    model_id: &str,
    action: &PermissionedAction,
    decision: &PermissionPolicyDecision,
    cause: ClassifierFallbackCause,
) -> ClassifierDecisionEvidence {
    let verdict = AutoEvaluatorVerdict {
        verdict: AutoEvaluatorVerdictKind::InsufficientContext,
        confidence: EvaluatorConfidence::Unknown,
        scope_match: EvaluatorScopeMatch::Unknown,
        risk_summary: "classifier not invoked".to_owned(),
        evidence_refs: Vec::new(),
        expires_at_unix_ms: created_at_unix_ms,
        evaluator_ref: Some("auto-mode-classifier".to_owned()),
        prompt_injection_signals: Vec::new(),
    };
    let request_payload = json!({
        "action_digest": action.action_digest,
        "argument_digest": action.argument_digest,
        "snapshot_digest": action.snapshot_digest,
        "skip_cause": cause,
    });
    let mut evidence = classifier_decision_evidence(ClassifierEvidenceInput {
        created_at_unix_ms,
        completed_at_unix_ms: Some(created_at_unix_ms),
        model_id,
        action,
        initial_decision: decision,
        final_decision: decision,
        request_payload: &request_payload,
        verdict: &verdict,
        usage: None,
        attempt_status: ClassifierAttemptStatus::Success,
    });
    evidence.route = ClassifierRouteEvidence {
        route_id: "permission_classifier.skipped".to_owned(),
        kind: ClassifierRouteKind::Skipped,
    };
    let unavailable_reason = unavailable_reason_for_skip(cause);
    evidence.token_accounting = unavailable_token_accounting(unavailable_reason);
    evidence.latency = ClassifierLatencyAccounting {
        duration_ms: unavailable(unavailable_reason),
    };
    evidence.cost = ClassifierCostAccounting {
        total: unavailable(unavailable_reason),
    };
    evidence.fallback = Some(ClassifierFallbackEvidence {
        fallback_cause: cause,
        previous_route_id: "permission_classifier.primary".to_owned(),
        selected_route_id: "permission_classifier.skipped".to_owned(),
        provider_call_attempted: false,
    });
    evidence.disposition = skipped_disposition(decision, cause);
    refresh_evidence_id(&mut evidence);
    evidence
}

fn unavailable_reason_for_skip(cause: ClassifierFallbackCause) -> AccountingUnavailableReason {
    match cause {
        ClassifierFallbackCause::ConfigUnavailable => {
            AccountingUnavailableReason::ConfigUnavailable
        }
        ClassifierFallbackCause::ProviderError | ClassifierFallbackCause::ProviderTimeout => {
            AccountingUnavailableReason::ProviderError
        }
        ClassifierFallbackCause::ParseFailure => AccountingUnavailableReason::ParseFailure,
        ClassifierFallbackCause::AccountingUnavailable => {
            AccountingUnavailableReason::MalformedAccountingInput
        }
        ClassifierFallbackCause::PrimaryUnavailable
        | ClassifierFallbackCause::MissingUserRequest
        | ClassifierFallbackCause::IneligibleCapability
        | ClassifierFallbackCause::StaticPolicyNotReviewable => {
            AccountingUnavailableReason::StaticPolicyNotReviewable
        }
    }
}

fn skipped_disposition(
    decision: &PermissionPolicyDecision,
    cause: ClassifierFallbackCause,
) -> ClassifierDisposition {
    match cause {
        ClassifierFallbackCause::StaticPolicyNotReviewable => {
            if decision.reason == PermissionPolicyReason::CeilingViolation {
                ClassifierDisposition::NotInvokedCeiling
            } else {
                ClassifierDisposition::NotInvokedStaticPolicy
            }
        }
        ClassifierFallbackCause::MissingUserRequest
        | ClassifierFallbackCause::IneligibleCapability => {
            ClassifierDisposition::NotInvokedIneligible
        }
        ClassifierFallbackCause::PrimaryUnavailable
        | ClassifierFallbackCause::ProviderError
        | ClassifierFallbackCause::ProviderTimeout
        | ClassifierFallbackCause::ParseFailure
        | ClassifierFallbackCause::ConfigUnavailable
        | ClassifierFallbackCause::AccountingUnavailable => ClassifierDisposition::FailedClosed,
    }
}
