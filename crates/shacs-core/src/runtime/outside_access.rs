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
    /// This cron run's approved list, valid now, covers every target (Spec
    /// 053 PRD 030 scope 6).
    pub cron_list_covers: bool,
    /// A `cron add` that carries a permission list (PRD 030 scope 2).
    pub cron_list_request: Option<CronListRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronListRequest {
    /// The list can be approved; the question shows this text.
    Ask(String),
    /// The list names something no list may grant.
    Refused(String),
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

/// A cron run's outside access holds only while the job's approved list,
/// read for this call, covers every target (Spec 053 PRD 030 scope 6).
const fn cron_list_allows(input: &PermissionPolicyInput) -> bool {
    input.outside_access.cron_list_covers
}

/// A `cron add` with a permission list: refusals stand, except
/// `dont_ask`'s, and everything else asks once unless the user approved this
/// very call. Nothing else stands in for the user (Spec 053 PRD 030 scope 2).
pub(crate) fn decide_cron_list(
    input: &PermissionPolicyInput,
    decision: PermissionPolicyDecision,
) -> PermissionPolicyDecision {
    let Some(request) = &input.outside_access.cron_list_request else {
        return decision;
    };
    if matches!(request, CronListRequest::Refused(_)) {
        return refused(PermissionPolicyReason::CronPermissionListRefused);
    }
    let mode = input.action.permission_mode_snapshot.mode;
    let dont_ask_baseline = mode == PermissionMode::DontAsk
        && decision.reason == PermissionPolicyReason::ModeBaselineDeny;
    if decision.kind == PermissionPolicyDecisionKind::Deny && !dont_ask_baseline {
        return decision;
    }
    if !input.interactive {
        return refused(PermissionPolicyReason::CronPermissionListRefused);
    }
    // The user's answer to this very call, whatever the mode baseline said
    // before it was read.
    if input
        .materials
        .as_ref()
        .is_some_and(|materials| materials.this_call_approved)
    {
        return decision;
    }
    PermissionPolicyDecision {
        kind: PermissionPolicyDecisionKind::Ask,
        ..refused(PermissionPolicyReason::CronPermissionList)
    }
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
