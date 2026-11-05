//! Computes the decision materials for one action in a tool execution
//! context (Spec 048 PRD 000 scopes 1, 4 and 9).

use super::exec_pattern::ExecPattern;
use super::permission_environment::PermissionEnvironment;
use super::permission_materials::{self, AutoAllow, MaterialsInput, PermissionMaterials};
use crate::runtime::{
    PermissionMode, PermissionedAction, RememberedPermissionPolicyMatch, ToolExecutionContext,
};
use crate::tools::ToolRegistry;
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};

/// The approvals that may stand in for an allow rule.
pub(crate) struct StandingApprovals<'a> {
    pub(crate) this_call_approved: bool,
    pub(crate) remembered: &'a [RememberedPermissionPolicyMatch],
    /// A session approval cache entry that matches, with its command pattern.
    pub(crate) session: Option<(String, Option<String>)>,
}

pub(crate) fn action_materials(
    registry: &ToolRegistry,
    action: &PermissionedAction,
    context: &ToolExecutionContext,
    process_refusal: impl FnOnce() -> Option<String>,
    approvals: StandingApprovals<'_>,
) -> Option<PermissionMaterials> {
    let environment = context.permission_environment.as_deref()?;
    let auto_mode = action.permission_mode_snapshot.mode == PermissionMode::Auto;
    let tool_refusal = registry.get(&action.tool_name).and_then(|tool| {
        action
            .decision_arguments()
            .as_object()
            .and_then(|params| tool.preflight_refusal(params))
    });
    let mut materials = permission_materials::permission_materials(MaterialsInput {
        action,
        environment,
        auto_mode,
        allow_workspace_edits: context.permission_auto_approval.allow_workspace_edits,
        preflight_refusal: tool_refusal.or_else(process_refusal),
    });
    materials.this_call_approved = approvals.this_call_approved;
    materials.auto_mode_escalation = context
        .permission_auto_mode_escalation
        .filter(|_| auto_mode);
    mask_shown_text(&mut materials, environment);
    if auto_mode && !materials.auto_allow_blocked {
        materials.auto_allow = materials
            .allow_rule
            .clone()
            .map(|rule| AutoAllow::Rule { rule })
            .or_else(|| remembered_allow(approvals.remembered))
            .or_else(|| {
                let (rule_ref, pattern) = approvals.session?;
                (!pattern.as_deref().is_some_and(is_broad))
                    .then_some(AutoAllow::Remembered { rule_ref })
            });
    }
    Some(materials)
}

/// Text from the materials reaches approval questions and tool results, so
/// `.env.json` values in it are masked (Spec 048 PRD 010 scope 9). Decisions
/// only test whether it is there.
fn mask_shown_text(materials: &mut PermissionMaterials, environment: &PermissionEnvironment) {
    let texts = [
        &mut materials.deny_rule,
        &mut materials.ask_rule,
        &mut materials.allow_rule,
        &mut materials.secret_name,
        &mut materials.critical_removal,
        &mut materials.symlinked_protected_write,
        &mut materials.symlinked_outside_write,
        &mut materials.preflight_refusal,
    ];
    for text in texts.into_iter().flatten() {
        *text = environment.secret_values.mask(text);
    }
}

fn remembered_allow(rules: &[RememberedPermissionPolicyMatch]) -> Option<AutoAllow> {
    rules
        .iter()
        .filter(|rule| rule.effect == RememberedPermissionEffect::Allow)
        .find(|rule| match &rule.matcher {
            RememberedPermissionMatcher::ExecPrefix { tokens } => {
                !is_broad(&format!("{} *", tokens.join(" ")))
            }
            _ => true,
        })
        .map(|rule| AutoAllow::Remembered {
            rule_ref: rule.rule_ref.clone(),
        })
}

fn is_broad(pattern: &str) -> bool {
    ExecPattern::parse(pattern).is_some_and(|pattern| pattern.is_broad())
}
