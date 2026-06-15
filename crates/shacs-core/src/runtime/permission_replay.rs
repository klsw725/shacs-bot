use crate::runtime::{PermissionPolicyDecisionKind, PermissionPolicyReason};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReplayInput {
    pub recorded_snapshot_digest: String,
    pub replay_snapshot_digest: String,
    pub recorded_rule_version: String,
    pub replay_rule_version: String,
    pub recorded_decision: PermissionPolicyDecisionKind,
    pub replay_decision: PermissionPolicyDecisionKind,
    pub replay_reason: PermissionPolicyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionReplayInvariant {
    SameSnapshotSameDecision,
    StricterReplayDeniedRecordedAllow,
    DecisionChangedUnderChangedContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionReplayViolation {
    SameSnapshotDecisionDrift,
    LooserReplayAllowedRecordedDeny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReplayOutcome {
    pub accepted: bool,
    pub invariant: Option<PermissionReplayInvariant>,
    pub violation: Option<PermissionReplayViolation>,
}

pub fn evaluate_permission_replay(input: &PermissionReplayInput) -> PermissionReplayOutcome {
    let same_context = input.recorded_snapshot_digest == input.replay_snapshot_digest
        && input.recorded_rule_version == input.replay_rule_version;
    if same_context && input.recorded_decision != input.replay_decision {
        return replay_violation(PermissionReplayViolation::SameSnapshotDecisionDrift);
    }
    if same_context {
        return replay_invariant(PermissionReplayInvariant::SameSnapshotSameDecision);
    }
    if input.recorded_decision == PermissionPolicyDecisionKind::Deny
        && input.replay_decision == PermissionPolicyDecisionKind::Allow
    {
        return replay_violation(PermissionReplayViolation::LooserReplayAllowedRecordedDeny);
    }
    if decision_rank(input.replay_decision) > decision_rank(input.recorded_decision) {
        return replay_invariant(PermissionReplayInvariant::StricterReplayDeniedRecordedAllow);
    }
    replay_invariant(PermissionReplayInvariant::DecisionChangedUnderChangedContext)
}

fn replay_invariant(invariant: PermissionReplayInvariant) -> PermissionReplayOutcome {
    PermissionReplayOutcome {
        accepted: true,
        invariant: Some(invariant),
        violation: None,
    }
}

fn replay_violation(violation: PermissionReplayViolation) -> PermissionReplayOutcome {
    PermissionReplayOutcome {
        accepted: false,
        invariant: None,
        violation: Some(violation),
    }
}

fn decision_rank(kind: PermissionPolicyDecisionKind) -> u8 {
    match kind {
        PermissionPolicyDecisionKind::Allow => 0,
        PermissionPolicyDecisionKind::Ask => 1,
        PermissionPolicyDecisionKind::Deny => 2,
    }
}
