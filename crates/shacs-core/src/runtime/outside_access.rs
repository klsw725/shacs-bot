//! Workspace-outside access in the permission decision (Spec 053 PRD 010
//! scopes 4, 6 and 7).

use super::auto_mode::auto_decision::ask_or_deny_when_possible;
use super::{
    AutomationRunSource, PermissionMode, PermissionPolicyDecision, PermissionPolicyDecisionKind,
    PermissionPolicyInput, PermissionPolicyReason,
};

/// What the tool decision and the process gate both read, computed once per
/// call so the two decide the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutsideAccess {
    /// The paths the call names outside the workspace and media folder, `..`
    /// for a `../` in a command. Empty when the tool's own guard refuses them.
    pub targets: Vec<String>,
    pub automation_source: Option<AutomationRunSource>,
}

/// Applied after every other rule. Refusals and questions stand; only an
/// allowed call can change.
pub(crate) fn decide(
    input: &PermissionPolicyInput,
    decision: PermissionPolicyDecision,
) -> PermissionPolicyDecision {
    let access = &input.outside_access;
    if access.targets.is_empty() || decision.kind == PermissionPolicyDecisionKind::Deny {
        return decision;
    }
    let mode = input.action.permission_mode_snapshot.mode;
    if !input.interactive {
        let cron = access
            .automation_source
            .as_ref()
            .is_some_and(AutomationRunSource::is_cron);
        let allowed = if cron {
            cron_list_allows(input)
        } else {
            mode == PermissionMode::BypassPermissions
        };
        return if allowed {
            decision
        } else {
            refused(PermissionPolicyReason::OutsideWorkspaceUnattended)
        };
    }
    let granted_by_user = matches!(
        decision.reason,
        PermissionPolicyReason::ApprovalAccepted
            | PermissionPolicyReason::UserRuleAllow
            | PermissionPolicyReason::RememberedAllow
    );
    if decision.kind == PermissionPolicyDecisionKind::Allow
        && mode != PermissionMode::Auto
        && mode != PermissionMode::BypassPermissions
        && !granted_by_user
    {
        return ask_or_deny_when_possible(input, PermissionPolicyReason::OutsideWorkspaceAccess);
    }
    decision
}

/// A cron job's approved outside-access list. None exists until Spec 053
/// PRD 030, so cron runs refuse workspace-outside access in every mode.
const fn cron_list_allows(_input: &PermissionPolicyInput) -> bool {
    false
}

fn refused(reason: PermissionPolicyReason) -> PermissionPolicyDecision {
    PermissionPolicyDecision {
        kind: PermissionPolicyDecisionKind::Deny,
        reason,
        evaluator_ref: None,
        approval_ref: None,
        approval_error: None,
        remembered_rule_ref: None,
        can_handoff_to_tool_runtime: false,
    }
}
