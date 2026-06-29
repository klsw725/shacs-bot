use crate::runtime::{
    evaluate_inherited_ceiling, StaticRuleDecision, StaticRuleDecisionKind, StaticRuleReason,
};
use crate::runtime::{
    ApprovalCorrelation, ApprovalCorrelationError, InheritedPermissionContext, PermissionMode,
    PermissionedAction, SafetyCapability,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyDecisionKind {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyReason {
    StaticDeny,
    StaticAskRequired,
    ProtectedTarget,
    ModeBaselineAllow,
    ModeBaselineAsk,
    ModeBaselineDeny,
    EvaluatorAllow,
    EvaluatorUnavailable,
    EvaluatorUncertain,
    PromptInjectionSignal,
    ApprovalAccepted,
    ApprovalRejected,
    CeilingViolation,
    LocalUserInteraction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPolicyDecision {
    pub kind: PermissionPolicyDecisionKind,
    pub reason: PermissionPolicyReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_error: Option<ApprovalCorrelationError>,
    pub can_handoff_to_tool_runtime: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPolicyInput {
    pub action: PermissionedAction,
    pub static_rule_decision: StaticRuleDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator: Option<AutoEvaluatorVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalCorrelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherited_context: Option<InheritedPermissionContext>,
    pub interactive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoEvaluatorVerdict {
    pub verdict: AutoEvaluatorVerdictKind,
    pub confidence: EvaluatorConfidence,
    pub scope_match: EvaluatorScopeMatch,
    pub risk_summary: String,
    pub evidence_refs: Vec<String>,
    pub expires_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_ref: Option<String>,
    #[serde(default)]
    pub prompt_injection_signals: Vec<PromptInjectionSignal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoEvaluatorVerdictKind {
    AllowCandidate,
    AskUser,
    DenyCandidate,
    InsufficientContext,
    Timeout,
    ParseFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorScopeMatch {
    Requested,
    Adjacent,
    Unrelated,
    Hostile,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptInjectionSignal {
    pub source_ref: String,
    pub reason: String,
    pub confidence: EvaluatorConfidence,
}

pub fn decide_permission(input: PermissionPolicyInput) -> PermissionPolicyDecision {
    if let Some(context) = &input.inherited_context {
        let ceiling = evaluate_inherited_ceiling(context);
        if !ceiling.allowed {
            return decision(
                PermissionPolicyDecisionKind::Deny,
                PermissionPolicyReason::CeilingViolation,
                None,
                None,
                None,
            );
        }
    }

    match input.static_rule_decision.kind {
        StaticRuleDecisionKind::Deny => {
            return decision(
                PermissionPolicyDecisionKind::Deny,
                static_deny_reason(&input.static_rule_decision),
                None,
                None,
                None,
            );
        }
        StaticRuleDecisionKind::AskRequired | StaticRuleDecisionKind::AllowCandidate => {}
    }

    if input.action.tool_name == "ask_user" && input.action.capabilities.is_empty() {
        return decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::LocalUserInteraction,
            None,
            None,
            None,
        );
    }
    if input.action.capabilities.is_empty() {
        return decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ModeBaselineDeny,
            None,
            None,
            None,
        );
    }

    if let Some(approval) = input.approval {
        if approval.is_approved() {
            return decision(
                PermissionPolicyDecisionKind::Allow,
                PermissionPolicyReason::ApprovalAccepted,
                None,
                approval.approval_ref,
                None,
            );
        }
        return decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ApprovalRejected,
            None,
            None,
            approval.error,
        );
    }

    if input.static_rule_decision.kind == StaticRuleDecisionKind::AskRequired {
        return ask_or_deny(input.interactive, PermissionPolicyReason::StaticAskRequired);
    }

    match input.action.permission_mode_snapshot.mode {
        PermissionMode::Plan => {
            if !input.action.capabilities.is_empty()
                && input
                    .action
                    .capabilities
                    .iter()
                    .all(|capability| *capability == SafetyCapability::FsRead)
            {
                decision(
                    PermissionPolicyDecisionKind::Allow,
                    PermissionPolicyReason::ModeBaselineAllow,
                    None,
                    None,
                    None,
                )
            } else {
                decision(
                    PermissionPolicyDecisionKind::Deny,
                    PermissionPolicyReason::ModeBaselineDeny,
                    None,
                    None,
                    None,
                )
            }
        }
        PermissionMode::Default => {
            if !input.action.capabilities.is_empty()
                && input
                    .action
                    .capabilities
                    .iter()
                    .all(|capability| *capability == SafetyCapability::FsRead)
            {
                decision(
                    PermissionPolicyDecisionKind::Allow,
                    PermissionPolicyReason::ModeBaselineAllow,
                    None,
                    None,
                    None,
                )
            } else {
                ask_or_deny(input.interactive, PermissionPolicyReason::ModeBaselineAsk)
            }
        }
        PermissionMode::AcceptEdits => {
            if !input.action.capabilities.is_empty()
                && input.action.capabilities.iter().all(|capability| {
                    matches!(
                        capability,
                        SafetyCapability::FsRead | SafetyCapability::FsWrite
                    )
                })
            {
                decision(
                    PermissionPolicyDecisionKind::Allow,
                    PermissionPolicyReason::ModeBaselineAllow,
                    None,
                    None,
                    None,
                )
            } else {
                ask_or_deny(input.interactive, PermissionPolicyReason::ModeBaselineAsk)
            }
        }
        PermissionMode::Auto => decide_auto(input.evaluator, input.interactive),
        PermissionMode::DontAsk => decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ModeBaselineDeny,
            None,
            None,
            None,
        ),
        PermissionMode::BypassPermissions => decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::ModeBaselineAllow,
            None,
            None,
            None,
        ),
    }
}

fn static_deny_reason(decision: &StaticRuleDecision) -> PermissionPolicyReason {
    match decision.reason {
        StaticRuleReason::ProtectedTarget | StaticRuleReason::RawAuthExport => {
            PermissionPolicyReason::ProtectedTarget
        }
        _ => PermissionPolicyReason::StaticDeny,
    }
}

fn decide_auto(
    evaluator: Option<AutoEvaluatorVerdict>,
    interactive: bool,
) -> PermissionPolicyDecision {
    let Some(evaluator) = evaluator else {
        return ask_or_deny(interactive, PermissionPolicyReason::EvaluatorUnavailable);
    };
    let evaluator_ref = evaluator.evaluator_ref.clone();
    if !evaluator.prompt_injection_signals.is_empty() {
        return ask_or_deny_with_evaluator(
            interactive,
            PermissionPolicyReason::PromptInjectionSignal,
            evaluator_ref,
        );
    }
    if evaluator.verdict == AutoEvaluatorVerdictKind::AllowCandidate
        && evaluator.confidence == EvaluatorConfidence::High
        && evaluator.scope_match == EvaluatorScopeMatch::Requested
    {
        return decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::EvaluatorAllow,
            evaluator_ref,
            None,
            None,
        );
    }
    if evaluator.verdict == AutoEvaluatorVerdictKind::DenyCandidate {
        return decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::EvaluatorUncertain,
            evaluator_ref,
            None,
            None,
        );
    }
    ask_or_deny_with_evaluator(
        interactive,
        PermissionPolicyReason::EvaluatorUncertain,
        evaluator_ref,
    )
}

fn ask_or_deny(interactive: bool, reason: PermissionPolicyReason) -> PermissionPolicyDecision {
    if interactive {
        decision(PermissionPolicyDecisionKind::Ask, reason, None, None, None)
    } else {
        decision(PermissionPolicyDecisionKind::Deny, reason, None, None, None)
    }
}

fn ask_or_deny_with_evaluator(
    interactive: bool,
    reason: PermissionPolicyReason,
    evaluator_ref: Option<String>,
) -> PermissionPolicyDecision {
    if interactive {
        decision(
            PermissionPolicyDecisionKind::Ask,
            reason,
            evaluator_ref,
            None,
            None,
        )
    } else {
        decision(
            PermissionPolicyDecisionKind::Deny,
            reason,
            evaluator_ref,
            None,
            None,
        )
    }
}

fn decision(
    kind: PermissionPolicyDecisionKind,
    reason: PermissionPolicyReason,
    evaluator_ref: Option<String>,
    approval_ref: Option<String>,
    approval_error: Option<ApprovalCorrelationError>,
) -> PermissionPolicyDecision {
    PermissionPolicyDecision {
        kind,
        reason,
        evaluator_ref,
        approval_ref,
        approval_error,
        can_handoff_to_tool_runtime: kind == PermissionPolicyDecisionKind::Allow,
    }
}
