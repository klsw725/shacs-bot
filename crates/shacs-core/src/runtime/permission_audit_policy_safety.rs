use crate::runtime::PolicySafetySnapshotRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PermissionPolicySafetySnapshotDiagnosticsSummary {
    pub present_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub malformed_count: u64,
    pub items: Vec<PermissionPolicySafetySnapshotAuditSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPolicySafetySnapshotAuditSummary {
    pub status: PermissionPolicySafetySnapshotAuditStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicySafetySnapshotAuditStatus {
    Present,
    Missing,
    Stale,
    Malformed,
}

pub(super) fn policy_safety_snapshot_audit_summary(
    reference: Option<&PolicySafetySnapshotRef>,
    now_unix_ms: u64,
) -> PermissionPolicySafetySnapshotAuditSummary {
    let Some(reference) = reference else {
        return PermissionPolicySafetySnapshotAuditSummary {
            status: PermissionPolicySafetySnapshotAuditStatus::Missing,
            snapshot_id: None,
            policy_safety_digest: None,
        };
    };
    let status = if reference
        .expires_at_unix_ms
        .is_some_and(|expires_at_unix_ms| now_unix_ms > expires_at_unix_ms)
    {
        PermissionPolicySafetySnapshotAuditStatus::Stale
    } else if reference.snapshot_id.0.trim().is_empty()
        || !is_sha256_hex(&reference.policy_safety_digest.0)
    {
        PermissionPolicySafetySnapshotAuditStatus::Malformed
    } else {
        PermissionPolicySafetySnapshotAuditStatus::Present
    };
    PermissionPolicySafetySnapshotAuditSummary {
        status,
        snapshot_id: Some(reference.snapshot_id.0.clone()),
        policy_safety_digest: Some(reference.policy_safety_digest.0.clone()),
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
