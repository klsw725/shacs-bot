use super::accounting::{
    diagnostic_refs, digest_json, latency_accounting, refresh_evidence_id, token_accounting,
    unavailable,
};
use super::types::*;
use crate::runtime::{
    AutoEvaluatorVerdict, PermissionPolicyDecision, PermissionPolicyDecisionKind,
    PermissionPolicyReason, PermissionedAction,
};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierAttemptStatus {
    Success,
    ProviderError,
    ProviderTimeout,
    ParseFailure,
    MalformedAccountingInput,
}

#[derive(Debug, Clone)]
pub struct ClassifierEvidenceInput<'a> {
    pub created_at_unix_ms: u64,
    pub completed_at_unix_ms: Option<u64>,
    pub model_id: &'a str,
    pub action: &'a PermissionedAction,
    pub initial_decision: &'a PermissionPolicyDecision,
    pub final_decision: &'a PermissionPolicyDecision,
    pub request_payload: &'a Value,
    pub verdict: &'a AutoEvaluatorVerdict,
    pub usage: Option<&'a BTreeMap<String, u64>>,
    pub attempt_status: ClassifierAttemptStatus,
}

pub fn classifier_decision_evidence(
    input: ClassifierEvidenceInput<'_>,
) -> ClassifierDecisionEvidence {
    let diagnostics = diagnostic_refs(input.verdict);
    let mut evidence = ClassifierDecisionEvidence {
        schema_id: ClassifierEvidenceSchemaId::V1,
        evidence_id: ClassifierEvidenceId(String::new()),
        created_at_unix_ms: input.created_at_unix_ms,
        request: ClassifierRequestCorrelation {
            provider_call_id: None,
            classifier_request_digest: digest_json(input.request_payload),
        },
        action: action_correlation(input.action),
        route: ClassifierRouteEvidence {
            route_id: route_id_for_attempt(input.attempt_status).to_owned(),
            kind: route_kind_for_attempt(input.attempt_status),
        },
        model: ClassifierModelEvidence {
            model_id: input.model_id.to_owned(),
            source_ref: None,
        },
        token_accounting: token_accounting(
            input.usage,
            accounting_reason_for_attempt(input.attempt_status),
        ),
        latency: ClassifierLatencyAccounting {
            duration_ms: latency_accounting(input.created_at_unix_ms, input.completed_at_unix_ms),
        },
        cost: ClassifierCostAccounting {
            total: unavailable(AccountingUnavailableReason::PriceUnconfigured),
        },
        verdict: ClassifierVerdictEvidence {
            verdict: input.verdict.verdict,
            confidence: input.verdict.confidence,
            scope_match: input.verdict.scope_match,
            prompt_injection_signal_count: input.verdict.prompt_injection_signals.len(),
            explanation_refs: diagnostics.clone(),
        },
        precedence: precedence_for_decision(input.initial_decision),
        disposition: disposition_for_decision(input.final_decision, input.attempt_status),
        fallback: fallback_for_attempt(input.attempt_status),
        diagnostics,
    };
    refresh_evidence_id(&mut evidence);
    evidence
}

fn action_correlation(action: &PermissionedAction) -> ClassifierActionCorrelation {
    ClassifierActionCorrelation {
        action_id: action.action_id.clone(),
        provider_tool_call_id: action.provider_tool_call_id.clone(),
        tool_name: action.tool_name.clone(),
        action_digest: action.action_digest.clone(),
        argument_digest: action.argument_digest.clone(),
        snapshot_digest: action.snapshot_digest.clone(),
        policy_safety_snapshot_ref: action.policy_safety_snapshot_ref.clone(),
        capabilities: action.capabilities.clone(),
    }
}

fn precedence_for_decision(decision: &PermissionPolicyDecision) -> StaticPolicyPrecedence {
    match decision.reason {
        PermissionPolicyReason::CeilingViolation => StaticPolicyPrecedence::CeilingWins,
        PermissionPolicyReason::StaticDeny | PermissionPolicyReason::ProtectedTarget => {
            StaticPolicyPrecedence::StaticDenyWins
        }
        PermissionPolicyReason::StaticAskRequired => {
            StaticPolicyPrecedence::StaticAskBlocksClassifier
        }
        PermissionPolicyReason::EvaluatorUnavailable => {
            StaticPolicyPrecedence::ClassifierReviewable
        }
        PermissionPolicyReason::ModeBaselineAsk
        | PermissionPolicyReason::ApprovalRejected
        | PermissionPolicyReason::EvaluatorUncertain
        | PermissionPolicyReason::PromptInjectionSignal => StaticPolicyPrecedence::ApprovalRequired,
        PermissionPolicyReason::ModeBaselineAllow
        | PermissionPolicyReason::ModeBaselineDeny
        | PermissionPolicyReason::EvaluatorAllow
        | PermissionPolicyReason::ApprovalAccepted
        | PermissionPolicyReason::LocalUserInteraction => {
            StaticPolicyPrecedence::ClassifierReviewable
        }
    }
}

fn disposition_for_decision(
    decision: &PermissionPolicyDecision,
    attempt_status: ClassifierAttemptStatus,
) -> ClassifierDisposition {
    match attempt_status {
        ClassifierAttemptStatus::ProviderError
        | ClassifierAttemptStatus::ProviderTimeout
        | ClassifierAttemptStatus::ParseFailure
        | ClassifierAttemptStatus::MalformedAccountingInput => ClassifierDisposition::FailedClosed,
        ClassifierAttemptStatus::Success => success_disposition(decision),
    }
}

fn success_disposition(decision: &PermissionPolicyDecision) -> ClassifierDisposition {
    match decision.reason {
        PermissionPolicyReason::EvaluatorAllow
            if decision.kind == PermissionPolicyDecisionKind::Allow =>
        {
            ClassifierDisposition::AllowCandidateConsumed
        }
        PermissionPolicyReason::EvaluatorUncertain => ClassifierDisposition::DenyCandidateRecorded,
        PermissionPolicyReason::PromptInjectionSignal => ClassifierDisposition::AskUser,
        PermissionPolicyReason::CeilingViolation => ClassifierDisposition::NotInvokedCeiling,
        PermissionPolicyReason::StaticDeny | PermissionPolicyReason::ProtectedTarget => {
            ClassifierDisposition::NotInvokedStaticPolicy
        }
        PermissionPolicyReason::StaticAskRequired
        | PermissionPolicyReason::EvaluatorAllow
        | PermissionPolicyReason::EvaluatorUnavailable
        | PermissionPolicyReason::ModeBaselineAllow
        | PermissionPolicyReason::ModeBaselineAsk
        | PermissionPolicyReason::ModeBaselineDeny
        | PermissionPolicyReason::ApprovalAccepted
        | PermissionPolicyReason::ApprovalRejected
        | PermissionPolicyReason::LocalUserInteraction => ClassifierDisposition::AskUser,
    }
}

fn fallback_for_attempt(
    attempt_status: ClassifierAttemptStatus,
) -> Option<ClassifierFallbackEvidence> {
    let fallback_cause = match attempt_status {
        ClassifierAttemptStatus::Success => return None,
        ClassifierAttemptStatus::ProviderError => ClassifierFallbackCause::ProviderError,
        ClassifierAttemptStatus::ProviderTimeout => ClassifierFallbackCause::ProviderTimeout,
        ClassifierAttemptStatus::ParseFailure => ClassifierFallbackCause::ParseFailure,
        ClassifierAttemptStatus::MalformedAccountingInput => {
            ClassifierFallbackCause::AccountingUnavailable
        }
    };
    Some(ClassifierFallbackEvidence {
        fallback_cause,
        previous_route_id: "permission_classifier.primary".to_owned(),
        selected_route_id: "permission_classifier.fallback.local_static".to_owned(),
        provider_call_attempted: true,
    })
}

fn accounting_reason_for_attempt(
    attempt_status: ClassifierAttemptStatus,
) -> Option<AccountingUnavailableReason> {
    match attempt_status {
        ClassifierAttemptStatus::Success => None,
        ClassifierAttemptStatus::ProviderError | ClassifierAttemptStatus::ProviderTimeout => {
            Some(AccountingUnavailableReason::ProviderError)
        }
        ClassifierAttemptStatus::ParseFailure => Some(AccountingUnavailableReason::ParseFailure),
        ClassifierAttemptStatus::MalformedAccountingInput => {
            Some(AccountingUnavailableReason::MalformedAccountingInput)
        }
    }
}

fn route_id_for_attempt(attempt_status: ClassifierAttemptStatus) -> &'static str {
    match attempt_status {
        ClassifierAttemptStatus::Success => "permission_classifier.primary",
        ClassifierAttemptStatus::ProviderError
        | ClassifierAttemptStatus::ProviderTimeout
        | ClassifierAttemptStatus::ParseFailure
        | ClassifierAttemptStatus::MalformedAccountingInput => {
            "permission_classifier.fallback.local_static"
        }
    }
}

fn route_kind_for_attempt(attempt_status: ClassifierAttemptStatus) -> ClassifierRouteKind {
    match attempt_status {
        ClassifierAttemptStatus::Success => ClassifierRouteKind::Primary,
        ClassifierAttemptStatus::ProviderError
        | ClassifierAttemptStatus::ProviderTimeout
        | ClassifierAttemptStatus::ParseFailure
        | ClassifierAttemptStatus::MalformedAccountingInput => ClassifierRouteKind::Fallback,
    }
}
