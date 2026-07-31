use crate::runtime::{
    PermissionPolicyDecisionKind, PermissionPolicyReason, PolicySafetySnapshotRef,
    POLICY_SAFETY_SNAPSHOT_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReplayInput {
    pub recorded_snapshot_digest: String,
    pub replay_snapshot_digest: String,
    pub recorded_rule_version: String,
    pub replay_rule_version: String,
    pub recorded_decision: PermissionPolicyDecisionKind,
    pub replay_decision: PermissionPolicyDecisionKind,
    pub replay_reason: PermissionPolicyReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_receipt_policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    #[serde(default)]
    pub replay_dispatch_count: usize,
    #[serde(default)]
    pub now_unix_ms: u64,
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
    MissingPolicySafetySnapshotRef,
    PolicySafetySnapshotRefMismatch,
    PolicySafetySnapshotRefStale,
    PolicySafetySnapshotRefMalformed,
    UnknownPolicySafetySnapshotSchema,
    ReplayAttemptedLiveDispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionReplayPolicySafetySnapshotStatus {
    Matched,
    Missing,
    Mismatch,
    Stale,
    Malformed,
    UnknownSchema,
    DispatchAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReplayOutcome {
    pub accepted: bool,
    pub invariant: Option<PermissionReplayInvariant>,
    pub violation: Option<PermissionReplayViolation>,
    pub policy_safety_snapshot_status: PermissionReplayPolicySafetySnapshotStatus,
    pub dispatch_count: usize,
}

pub fn evaluate_permission_replay_value(value: Value, now_unix_ms: u64) -> PermissionReplayOutcome {
    if has_unknown_policy_safety_schema(&value) {
        return replay_violation(
            PermissionReplayViolation::UnknownPolicySafetySnapshotSchema,
            PermissionReplayPolicySafetySnapshotStatus::UnknownSchema,
        );
    }
    match serde_json::from_value::<PermissionReplayInput>(value) {
        Ok(mut input) => {
            input.now_unix_ms = now_unix_ms;
            evaluate_permission_replay(&input)
        }
        Err(_error) => replay_violation(
            PermissionReplayViolation::PolicySafetySnapshotRefMalformed,
            PermissionReplayPolicySafetySnapshotStatus::Malformed,
        ),
    }
}

pub fn evaluate_permission_replay(input: &PermissionReplayInput) -> PermissionReplayOutcome {
    if input.replay_dispatch_count != 0 {
        return replay_violation(
            PermissionReplayViolation::ReplayAttemptedLiveDispatch,
            PermissionReplayPolicySafetySnapshotStatus::DispatchAttempted,
        );
    }
    if let Some(violation) = policy_ref_violation(input) {
        return violation;
    }
    let same_context = input.recorded_snapshot_digest == input.replay_snapshot_digest
        && input.recorded_rule_version == input.replay_rule_version;
    if same_context && input.recorded_decision != input.replay_decision {
        return replay_violation(
            PermissionReplayViolation::SameSnapshotDecisionDrift,
            PermissionReplayPolicySafetySnapshotStatus::Matched,
        );
    }
    if same_context {
        return replay_invariant(PermissionReplayInvariant::SameSnapshotSameDecision);
    }
    if input.recorded_decision == PermissionPolicyDecisionKind::Deny
        && input.replay_decision == PermissionPolicyDecisionKind::Allow
    {
        return replay_violation(
            PermissionReplayViolation::LooserReplayAllowedRecordedDeny,
            PermissionReplayPolicySafetySnapshotStatus::Matched,
        );
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
        policy_safety_snapshot_status: PermissionReplayPolicySafetySnapshotStatus::Matched,
        dispatch_count: 0,
    }
}

fn replay_violation(
    violation: PermissionReplayViolation,
    policy_safety_snapshot_status: PermissionReplayPolicySafetySnapshotStatus,
) -> PermissionReplayOutcome {
    PermissionReplayOutcome {
        accepted: false,
        invariant: None,
        violation: Some(violation),
        policy_safety_snapshot_status,
        dispatch_count: 0,
    }
}

fn policy_ref_violation(input: &PermissionReplayInput) -> Option<PermissionReplayOutcome> {
    let Some(recorded_ref) = input.recorded_policy_safety_snapshot_ref.as_ref() else {
        return Some(replay_violation(
            PermissionReplayViolation::MissingPolicySafetySnapshotRef,
            PermissionReplayPolicySafetySnapshotStatus::Missing,
        ));
    };
    let Some(replay_ref) = input.replay_policy_safety_snapshot_ref.as_ref() else {
        return Some(replay_violation(
            PermissionReplayViolation::MissingPolicySafetySnapshotRef,
            PermissionReplayPolicySafetySnapshotStatus::Missing,
        ));
    };
    let Some(receipt_ref) = input.process_receipt_policy_safety_snapshot_ref.as_ref() else {
        return Some(replay_violation(
            PermissionReplayViolation::MissingPolicySafetySnapshotRef,
            PermissionReplayPolicySafetySnapshotStatus::Missing,
        ));
    };
    if [recorded_ref, replay_ref, receipt_ref]
        .into_iter()
        .any(|reference| stale(reference, input.now_unix_ms))
    {
        return Some(replay_violation(
            PermissionReplayViolation::PolicySafetySnapshotRefStale,
            PermissionReplayPolicySafetySnapshotStatus::Stale,
        ));
    }
    if [recorded_ref, replay_ref, receipt_ref]
        .into_iter()
        .any(malformed)
    {
        return Some(replay_violation(
            PermissionReplayViolation::PolicySafetySnapshotRefMalformed,
            PermissionReplayPolicySafetySnapshotStatus::Malformed,
        ));
    }
    if recorded_ref != replay_ref || recorded_ref != receipt_ref {
        return Some(replay_violation(
            PermissionReplayViolation::PolicySafetySnapshotRefMismatch,
            PermissionReplayPolicySafetySnapshotStatus::Mismatch,
        ));
    }
    None
}

fn has_unknown_policy_safety_schema(value: &Value) -> bool {
    [
        "recorded_policy_safety_snapshot_ref",
        "replay_policy_safety_snapshot_ref",
        "process_receipt_policy_safety_snapshot_ref",
    ]
    .into_iter()
    .filter_map(|field| value.get(field))
    .filter_map(|reference| reference.get("schema_id"))
    .filter_map(Value::as_str)
    .any(|schema_id| schema_id != POLICY_SAFETY_SNAPSHOT_SCHEMA_V1)
}

fn stale(reference: &PolicySafetySnapshotRef, now_unix_ms: u64) -> bool {
    reference
        .expires_at_unix_ms
        .is_some_and(|expires_at_unix_ms| now_unix_ms > expires_at_unix_ms)
}

fn malformed(reference: &PolicySafetySnapshotRef) -> bool {
    reference.snapshot_id.0.trim().is_empty() || !is_sha256_hex(&reference.policy_safety_digest.0)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn decision_rank(kind: PermissionPolicyDecisionKind) -> u8 {
    match kind {
        PermissionPolicyDecisionKind::Allow => 0,
        PermissionPolicyDecisionKind::Ask => 1,
        PermissionPolicyDecisionKind::Deny => 2,
    }
}
