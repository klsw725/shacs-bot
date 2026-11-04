//! A question that takes only a one-time answer (Spec 048 PRD 000 scope 15):
//! any other reply runs nothing and shows the same question again.

use super::{PendingPermissionApproval, PermissionApprovalReply};
use crate::runtime::ApprovalDecisionKind;

const ONE_TIME_ONLY: &str =
    "This question takes only `1` or `approve` to run it once, or `2` or `deny` to cancel. Nothing was run.";

/// The text to send back when the reply is not one the question offered.
pub(super) fn repeated_question(
    approval: &PendingPermissionApproval,
    reply: PermissionApprovalReply,
) -> Option<String> {
    let offered = &approval.approval_request.allowed_decisions;
    if offered.contains(&ApprovalDecisionKind::ApprovedForSession) {
        return None;
    }
    let decision = match reply {
        PermissionApprovalReply::Approve => ApprovalDecisionKind::Approved,
        PermissionApprovalReply::Deny => ApprovalDecisionKind::Denied,
        PermissionApprovalReply::ApproveSession
        | PermissionApprovalReply::ApproveProject
        | PermissionApprovalReply::DenySession
        | PermissionApprovalReply::DenyProject
        | PermissionApprovalReply::Unknown => ApprovalDecisionKind::InspectOnly,
    };
    if offered.contains(&decision) {
        return None;
    }
    Some(if approval.question.is_empty() {
        ONE_TIME_ONLY.to_owned()
    } else {
        format!("{ONE_TIME_ONLY}\n\n{}", approval.question)
    })
}
