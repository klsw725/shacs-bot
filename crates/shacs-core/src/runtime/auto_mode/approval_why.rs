//! The `Why:` line and the answers an approval question accepts (Spec 048
//! PRD 000 scope 15).

use super::block_counts::{AutoModeEscalation, CONSECUTIVE_LIMIT, TOTAL_LIMIT};
use super::permission_materials::PermissionMaterials;
use super::secret_values::MASK_PREFIX;
use crate::runtime::{
    AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, PermissionPolicyDecision,
    PermissionPolicyReason,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovalExplanation {
    pub why: Option<String>,
    /// The question accepts only a one-time approve or deny.
    pub once_only: bool,
}

pub(crate) fn approval_explanation(
    decision: &PermissionPolicyDecision,
    materials: Option<&PermissionMaterials>,
    evaluator: Option<&AutoEvaluatorVerdict>,
) -> ApprovalExplanation {
    let text = |value: Option<&String>| value.cloned().unwrap_or_else(|| "unknown".to_owned());
    let why = match decision.reason {
        PermissionPolicyReason::SecretFileRead => {
            let name = text(materials.and_then(|materials| materials.secret_name.as_ref()));
            let mut why = format!(
                "`{name}` is a secret file. If you approve, its contents go to the AI provider and stay in the session history."
            );
            if materials.is_some_and(|materials| materials.secret_read_by_text) {
                why.push_str(
                    " The command names it inside code or text, so it may also change the file.",
                );
            }
            Some(why)
        }
        PermissionPolicyReason::UserRuleAsk => Some(format!(
            "your ask rule `{}` matches this call.",
            text(materials.and_then(|materials| materials.ask_rule.as_ref()))
        )),
        PermissionPolicyReason::CriticalPathRemoval => {
            let target = text(materials.and_then(|materials| materials.critical_removal.as_ref()));
            // A folder path starts with `/` unless an `.env.json` value masked
            // its start; a reason never starts either way.
            Some(
                if target.starts_with('/') || target.starts_with(MASK_PREFIX) {
                    format!("this command can remove the critical folder `{target}`.")
                } else {
                    format!("the folders this rm can remove could not be checked: {target}.")
                },
            )
        }
        PermissionPolicyReason::ProtectedPathThroughSymlink => Some(format!(
            "the write follows a symlink to the protected path `{}`.",
            text(materials.and_then(|materials| materials.symlinked_protected_write.as_ref()))
        )),
        PermissionPolicyReason::OutsideWorkspaceThroughSymlink => Some(format!(
            "the write follows a symlink to `{}`, outside the workspace.",
            text(materials.and_then(|materials| materials.symlinked_outside_write.as_ref()))
        )),
        PermissionPolicyReason::EvaluatorUncertain
        | PermissionPolicyReason::EvaluatorUnavailable
        | PermissionPolicyReason::PromptInjectionSignal => {
            let why = match evaluator {
                Some(verdict) if verdict.verdict == AutoEvaluatorVerdictKind::DenyCandidate => {
                    format!("Auto mode blocked this: {}", verdict.risk_summary)
                }
                _ => "Auto mode could not check this action".to_owned(),
            };
            Some(
                match materials.and_then(|materials| materials.auto_mode_escalation) {
                    Some(escalation) => format!(
                        "{}. {}",
                        why.trim_end_matches('.'),
                        escalation_note(escalation)
                    ),
                    None => why,
                },
            )
        }
        _ => None,
    };
    let once_only = matches!(
        decision.reason,
        PermissionPolicyReason::SecretFileRead
            | PermissionPolicyReason::UserRuleAsk
            | PermissionPolicyReason::CriticalPathRemoval
            | PermissionPolicyReason::ProtectedPathThroughSymlink
            | PermissionPolicyReason::OutsideWorkspaceThroughSymlink
    );
    ApprovalExplanation { why, once_only }
}

/// Why a call the classifier did not allow became a question (Spec 048 PRD
/// 020 scope 3).
fn escalation_note(escalation: AutoModeEscalation) -> String {
    match escalation {
        AutoModeEscalation::Consecutive => {
            format!("Auto mode stopped {CONSECUTIVE_LIMIT} calls in a row, so it asks you now.")
        }
        AutoModeEscalation::Total => {
            format!("Auto mode stopped {TOTAL_LIMIT} calls in this session, so it asks you now.")
        }
    }
}
