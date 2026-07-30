use crate::runtime::{
    evaluate_inherited_ceiling, StaticRuleDecision, StaticRuleDecisionKind, StaticRuleReason,
};
use crate::runtime::{
    ApprovalCorrelation, ApprovalCorrelationError, InheritedPermissionContext, PermissionMode,
    PermissionedAction, SafetyCapability,
};
use serde::{Deserialize, Serialize};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher, WorkspacePathScope};

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
    RememberedAllow,
    RememberedDeny,
    RememberedStoreUnavailable,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remembered_rule_ref: Option<String>,
    pub can_handoff_to_tool_runtime: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedPermissionPolicyMatch {
    pub effect: RememberedPermissionEffect,
    pub rule_ref: String,
    pub matcher: RememberedPermissionMatcher,
    #[serde(default)]
    pub session_scoped: bool,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remembered_rules: Vec<RememberedPermissionPolicyMatch>,
    #[serde(default)]
    pub remembered_store_unavailable: bool,
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
                DecisionRefs::default(),
            );
        }
    }

    if input.static_rule_decision.kind == StaticRuleDecisionKind::Deny {
        return static_deny_decision(&input);
    }

    if input.action.tool_name == "ask_user" && input.action.capabilities.is_empty() {
        return decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::LocalUserInteraction,
            DecisionRefs::default(),
        );
    }
    if input.action.capabilities.is_empty() {
        return decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ModeBaselineDeny,
            DecisionRefs::default(),
        );
    }

    match input.action.permission_mode_snapshot.mode {
        PermissionMode::Plan => return decide_plan(&input),
        PermissionMode::BypassPermissions if input.remembered_store_unavailable => {
            return ask_or_deny(
                input.interactive,
                PermissionPolicyReason::RememberedStoreUnavailable,
            );
        }
        PermissionMode::BypassPermissions => {
            return decision(
                PermissionPolicyDecisionKind::Allow,
                PermissionPolicyReason::ModeBaselineAllow,
                DecisionRefs::default(),
            );
        }
        PermissionMode::Default
        | PermissionMode::AcceptEdits
        | PermissionMode::Auto
        | PermissionMode::DontAsk => {}
    }

    if let Some(rule) =
        strongest_remembered_match(&input.remembered_rules, RememberedPermissionEffect::Deny)
    {
        return remembered_decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::RememberedDeny,
            rule,
        );
    }

    if input.remembered_store_unavailable {
        return ask_or_deny(
            input.interactive,
            PermissionPolicyReason::RememberedStoreUnavailable,
        );
    }

    if static_ask_blocks_approval(&input) {
        return ask_or_deny(input.interactive, PermissionPolicyReason::StaticAskRequired);
    }

    if let Some(approval) = &input.approval {
        if approval.is_approved() {
            return decision(
                PermissionPolicyDecisionKind::Allow,
                PermissionPolicyReason::ApprovalAccepted,
                DecisionRefs {
                    approval_ref: approval.approval_ref.clone(),
                    ..DecisionRefs::default()
                },
            );
        }
        if !approval.is_approved() {
            return decision(
                if input.interactive {
                    PermissionPolicyDecisionKind::Ask
                } else {
                    PermissionPolicyDecisionKind::Deny
                },
                PermissionPolicyReason::ApprovalRejected,
                DecisionRefs {
                    approval_error: approval.error,
                    ..DecisionRefs::default()
                },
            );
        }
    }

    if input.static_rule_decision.kind == StaticRuleDecisionKind::AskRequired {
        return ask_or_deny(input.interactive, PermissionPolicyReason::StaticAskRequired);
    }

    if let Some(rule) =
        strongest_remembered_match(&input.remembered_rules, RememberedPermissionEffect::Allow)
    {
        return remembered_decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::RememberedAllow,
            rule,
        );
    }

    match input.action.permission_mode_snapshot.mode {
        PermissionMode::Plan => decide_plan(&input),
        PermissionMode::Default => decide_default(&input),
        PermissionMode::AcceptEdits => decide_accept_edits(&input),
        PermissionMode::Auto => decide_auto(input.evaluator, input.interactive),
        PermissionMode::DontAsk => decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ModeBaselineDeny,
            DecisionRefs::default(),
        ),
        PermissionMode::BypassPermissions => decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::ModeBaselineAllow,
            DecisionRefs::default(),
        ),
    }
}

fn static_ask_blocks_approval(input: &PermissionPolicyInput) -> bool {
    !input.interactive
        && input.static_rule_decision.kind == StaticRuleDecisionKind::AskRequired
        && matches!(
            input.static_rule_decision.reason,
            StaticRuleReason::ProcExecSummaryUnavailable | StaticRuleReason::ContainmentUnknown
        )
}

fn decide_plan(input: &PermissionPolicyInput) -> PermissionPolicyDecision {
    if all_capabilities(input, &[SafetyCapability::FsRead]) {
        decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::ModeBaselineAllow,
            DecisionRefs::default(),
        )
    } else {
        decision(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ModeBaselineDeny,
            DecisionRefs::default(),
        )
    }
}

fn decide_default(input: &PermissionPolicyInput) -> PermissionPolicyDecision {
    if all_capabilities(input, &[SafetyCapability::FsRead]) {
        decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::ModeBaselineAllow,
            DecisionRefs::default(),
        )
    } else {
        ask_or_deny(input.interactive, PermissionPolicyReason::ModeBaselineAsk)
    }
}

fn decide_accept_edits(input: &PermissionPolicyInput) -> PermissionPolicyDecision {
    if all_capabilities(
        input,
        &[SafetyCapability::FsRead, SafetyCapability::FsWrite],
    ) {
        decision(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::ModeBaselineAllow,
            DecisionRefs::default(),
        )
    } else {
        ask_or_deny(input.interactive, PermissionPolicyReason::ModeBaselineAsk)
    }
}

fn all_capabilities(input: &PermissionPolicyInput, allowed: &[SafetyCapability]) -> bool {
    !input.action.capabilities.is_empty()
        && input
            .action
            .capabilities
            .iter()
            .all(|capability| allowed.contains(capability))
}

fn static_deny_decision(input: &PermissionPolicyInput) -> PermissionPolicyDecision {
    let reason = static_deny_reason(&input.static_rule_decision);
    if input.action.permission_mode_snapshot.mode == PermissionMode::Auto
        && input.interactive
        && input.approval.is_none()
        && input.static_rule_decision.reason == StaticRuleReason::ProtectedTarget
    {
        return decision(
            PermissionPolicyDecisionKind::Ask,
            reason,
            DecisionRefs::default(),
        );
    }
    decision(
        PermissionPolicyDecisionKind::Deny,
        reason,
        DecisionRefs::default(),
    )
}

fn static_deny_reason(decision: &StaticRuleDecision) -> PermissionPolicyReason {
    match decision.reason {
        StaticRuleReason::ProtectedTarget | StaticRuleReason::RawAuthExport => {
            PermissionPolicyReason::ProtectedTarget
        }
        StaticRuleReason::NormalizationError
        | StaticRuleReason::UnknownTargetClassification
        | StaticRuleReason::SecretRead
        | StaticRuleReason::ProcExecSummaryUnavailable
        | StaticRuleReason::DangerousProcExec
        | StaticRuleReason::ContainmentUnknown
        | StaticRuleReason::BypassContainmentNotConfirmed
        | StaticRuleReason::NoStaticMatch => PermissionPolicyReason::StaticDeny,
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
            DecisionRefs {
                evaluator_ref,
                ..DecisionRefs::default()
            },
        );
    }
    if evaluator.verdict == AutoEvaluatorVerdictKind::DenyCandidate {
        return ask_or_deny_with_evaluator(
            interactive,
            PermissionPolicyReason::EvaluatorUncertain,
            evaluator_ref,
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
        decision(
            PermissionPolicyDecisionKind::Ask,
            reason,
            DecisionRefs::default(),
        )
    } else {
        decision(
            PermissionPolicyDecisionKind::Deny,
            reason,
            DecisionRefs::default(),
        )
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
            DecisionRefs {
                evaluator_ref,
                ..DecisionRefs::default()
            },
        )
    } else {
        decision(
            PermissionPolicyDecisionKind::Deny,
            reason,
            DecisionRefs {
                evaluator_ref,
                ..DecisionRefs::default()
            },
        )
    }
}

fn strongest_remembered_match(
    rules: &[RememberedPermissionPolicyMatch],
    effect: RememberedPermissionEffect,
) -> Option<&RememberedPermissionPolicyMatch> {
    rules
        .iter()
        .filter(|rule| rule.effect == effect)
        .max_by_key(|rule| matcher_specificity(&rule.matcher))
}

fn matcher_specificity(matcher: &RememberedPermissionMatcher) -> u8 {
    match matcher {
        RememberedPermissionMatcher::ExactAction { .. } => 50,
        RememberedPermissionMatcher::WorkspacePath { scope, .. } => match scope {
            WorkspacePathScope::Exact => 40,
            WorkspacePathScope::Subtree => 30,
        },
        RememberedPermissionMatcher::WebOrigin { .. }
        | RememberedPermissionMatcher::McpTool { .. } => 20,
        RememberedPermissionMatcher::ExecPrefix { tokens } => {
            10_u8.saturating_add(tokens.len().try_into().unwrap_or(u8::MAX))
        }
    }
}

fn remembered_decision(
    kind: PermissionPolicyDecisionKind,
    reason: PermissionPolicyReason,
    rule: &RememberedPermissionPolicyMatch,
) -> PermissionPolicyDecision {
    decision(
        kind,
        reason,
        DecisionRefs {
            remembered_rule_ref: Some(rule.rule_ref.clone()),
            ..DecisionRefs::default()
        },
    )
}

#[derive(Default)]
struct DecisionRefs {
    evaluator_ref: Option<String>,
    approval_ref: Option<String>,
    approval_error: Option<ApprovalCorrelationError>,
    remembered_rule_ref: Option<String>,
}

fn decision(
    kind: PermissionPolicyDecisionKind,
    reason: PermissionPolicyReason,
    refs: DecisionRefs,
) -> PermissionPolicyDecision {
    PermissionPolicyDecision {
        kind,
        reason,
        evaluator_ref: refs.evaluator_ref,
        approval_ref: refs.approval_ref,
        approval_error: refs.approval_error,
        remembered_rule_ref: refs.remembered_rule_ref,
        can_handoff_to_tool_runtime: kind == PermissionPolicyDecisionKind::Allow,
    }
}
