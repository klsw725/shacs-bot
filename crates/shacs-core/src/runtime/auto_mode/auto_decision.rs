//! The auto mode decision order and the rules every other mode gains (Spec
//! 048 PRD 000 scopes 2 and 3).

use super::permission_materials::{AutoAllow, PermissionMaterials};
use crate::runtime::permission_policy::{
    ask_or_deny, decide_auto, remembered_decision, strongest_remembered_match,
};
use crate::runtime::{
    PermissionMode, PermissionPolicyDecision, PermissionPolicyDecisionKind, PermissionPolicyInput,
    PermissionPolicyReason, SafetyCapability, StaticRuleDecisionKind, StaticRuleReason,
};
use shacs_config::RememberedPermissionEffect;

pub(crate) fn decide_auto_mode(
    input: &PermissionPolicyInput,
    materials: &PermissionMaterials,
) -> PermissionPolicyDecision {
    let interactive = input.interactive;
    let capabilities = &input.action.capabilities;
    if blocked_by_normalization(input) {
        return decided(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::StaticDeny,
        );
    }
    if capabilities.is_empty() {
        return if input.action.tool_name == "ask_user" {
            decided(
                PermissionPolicyDecisionKind::Allow,
                PermissionPolicyReason::LocalUserInteraction,
            )
        } else {
            decided(
                PermissionPolicyDecisionKind::Deny,
                PermissionPolicyReason::ModeBaselineDeny,
            )
        };
    }
    if let Some(denied) = denied_by_user(input, materials) {
        return denied;
    }
    if materials.preflight_refusal.is_some() {
        return decided(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::ToolRefusal,
        );
    }
    if input.remembered_store_unavailable {
        return ask_or_deny(
            interactive,
            PermissionPolicyReason::RememberedStoreUnavailable,
        );
    }
    if let Some(approval) = input
        .approval
        .as_ref()
        .filter(|approval| approval.is_approved())
    {
        return approved(approval.approval_ref.clone());
    }
    let ask_reason = if materials.secret_read {
        Some(PermissionPolicyReason::SecretFileRead)
    } else if materials.ask_rule.is_some() {
        Some(PermissionPolicyReason::UserRuleAsk)
    } else if materials.critical_removal.is_some() {
        Some(PermissionPolicyReason::CriticalPathRemoval)
    } else if materials.symlinked_protected_write.is_some() {
        Some(PermissionPolicyReason::ProtectedPathThroughSymlink)
    } else {
        None
    };
    if let Some(reason) = ask_reason {
        return ask_or_deny(interactive, reason);
    }
    match &materials.auto_allow {
        Some(AutoAllow::Rule { .. }) => {
            return decided(
                PermissionPolicyDecisionKind::Allow,
                PermissionPolicyReason::UserRuleAllow,
            );
        }
        Some(AutoAllow::Remembered { rule_ref }) => {
            let mut allowed = decided(
                PermissionPolicyDecisionKind::Allow,
                PermissionPolicyReason::RememberedAllow,
            );
            allowed.remembered_rule_ref = Some(rule_ref.clone());
            return allowed;
        }
        None => {}
    }
    if materials.symlinked_outside_write.is_some() {
        return ask_or_deny(
            interactive,
            PermissionPolicyReason::OutsideWorkspaceThroughSymlink,
        );
    }
    if capabilities
        .iter()
        .all(|capability| *capability == SafetyCapability::FsRead)
    {
        return decided(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::WorkspaceRead,
        );
    }
    let file_capabilities = capabilities.iter().all(|capability| {
        matches!(
            capability,
            SafetyCapability::FsRead | SafetyCapability::FsWrite
        )
    });
    if file_capabilities && materials.workspace_edit {
        return decided(
            PermissionPolicyDecisionKind::Allow,
            PermissionPolicyReason::WorkspaceEdit,
        );
    }
    decide_auto(input.evaluator.clone(), interactive)
}

/// The additions that come before the mode's own rules outside auto mode:
/// secret writes and deny rules refuse, and secret reads and critical-path
/// removals need this call's approval.
pub(crate) fn before_mode_rules(
    input: &PermissionPolicyInput,
    materials: &PermissionMaterials,
) -> Option<PermissionPolicyDecision> {
    if blocked_by_normalization(input) {
        return None;
    }
    if let Some(denied) = denied_by_user(input, materials) {
        return Some(denied);
    }
    if materials.preflight_refusal.is_some() {
        return None;
    }
    let reason = if materials.secret_read {
        PermissionPolicyReason::SecretFileRead
    } else if materials.critical_removal.is_some() {
        PermissionPolicyReason::CriticalPathRemoval
    } else {
        return None;
    };
    Some(if materials.this_call_approved {
        approved(
            input
                .approval
                .as_ref()
                .and_then(|approval| approval.approval_ref.clone()),
        )
    } else {
        ask_or_deny(can_ask(input), reason)
    })
}

/// An ask rule turns an allowed call without this call's approval into a
/// question, in every mode other than auto.
pub(crate) fn after_mode_rules(
    input: &PermissionPolicyInput,
    materials: &PermissionMaterials,
    decision: PermissionPolicyDecision,
) -> PermissionPolicyDecision {
    if decision.kind == PermissionPolicyDecisionKind::Allow
        && materials.ask_rule.is_some()
        && !materials.this_call_approved
    {
        return ask_or_deny(can_ask(input), PermissionPolicyReason::UserRuleAsk);
    }
    decision
}

fn blocked_by_normalization(input: &PermissionPolicyInput) -> bool {
    input.static_rule_decision.kind == StaticRuleDecisionKind::Deny
        && matches!(
            input.static_rule_decision.reason,
            StaticRuleReason::NormalizationError | StaticRuleReason::UnknownTargetClassification
        )
}

fn denied_by_user(
    input: &PermissionPolicyInput,
    materials: &PermissionMaterials,
) -> Option<PermissionPolicyDecision> {
    if materials.secret_write {
        return Some(decided(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::SecretFileWrite,
        ));
    }
    if materials.deny_rule.is_some() {
        return Some(decided(
            PermissionPolicyDecisionKind::Deny,
            PermissionPolicyReason::UserRuleDeny,
        ));
    }
    strongest_remembered_match(&input.remembered_rules, RememberedPermissionEffect::Deny).map(
        |rule| {
            remembered_decision(
                PermissionPolicyDecisionKind::Deny,
                PermissionPolicyReason::RememberedDeny,
                rule,
            )
        },
    )
}

fn can_ask(input: &PermissionPolicyInput) -> bool {
    input.interactive && input.action.permission_mode_snapshot.mode != PermissionMode::DontAsk
}

fn approved(approval_ref: Option<String>) -> PermissionPolicyDecision {
    let mut allowed = decided(
        PermissionPolicyDecisionKind::Allow,
        PermissionPolicyReason::ApprovalAccepted,
    );
    allowed.approval_ref = approval_ref;
    allowed
}

fn decided(
    kind: PermissionPolicyDecisionKind,
    reason: PermissionPolicyReason,
) -> PermissionPolicyDecision {
    PermissionPolicyDecision {
        kind,
        reason,
        evaluator_ref: None,
        approval_ref: None,
        approval_error: None,
        remembered_rule_ref: None,
        can_handoff_to_tool_runtime: kind == PermissionPolicyDecisionKind::Allow,
    }
}

#[cfg(test)]
#[path = "auto_decision_tests.rs"]
mod tests;
