#[path = "permission_audit_policy_safety.rs"]
mod permission_audit_policy_safety;
#[path = "permission_audit_release.rs"]
mod permission_audit_release;

pub use permission_audit_policy_safety::{
    PermissionPolicySafetySnapshotAuditStatus, PermissionPolicySafetySnapshotAuditSummary,
    PermissionPolicySafetySnapshotDiagnosticsSummary,
};
pub use permission_audit_release::{
    permission_prd005_006_contract_cases, permission_release_evidence_complete,
    required_permission_release_evidence_buckets, PermissionContractCase,
    PermissionReleaseEvidence, PermissionReleaseEvidenceBucket,
};

use crate::runtime::{
    PermissionMode, PermissionPolicyDecision, PermissionPolicyDecisionKind, PermissionPolicyReason,
    PermissionSecretRefStatus, PermissionedAction, PolicySafetySnapshotRef, ProtectedTargetClass,
    RuleDiagnostics, SafetyCapability,
};
use permission_audit_policy_safety::policy_safety_snapshot_audit_summary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionAuditRecord {
    pub action_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_name: String,
    pub capabilities: Vec<SafetyCapability>,
    pub target_summary: Vec<String>,
    pub argument_digest: String,
    pub mode: PermissionMode,
    pub decision: crate::runtime::PermissionPolicyDecisionKind,
    pub decision_reason: PermissionPolicyReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remembered_rule_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_ref_summary: Vec<PermissionSecretRefAuditSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSecretRefAuditSummary {
    pub ref_id: String,
    pub source_kind: String,
    pub safe_summary: String,
    pub redaction_evidence_ref: String,
    pub status: PermissionSecretRefStatus,
    pub requested_consumer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDiagnosticsSummary {
    pub allow_count: u64,
    pub ask_count: u64,
    pub deny_count: u64,
    pub auto_approval_reasons: Vec<PermissionPolicyReason>,
    pub ask_reasons: Vec<PermissionPolicyReason>,
    pub deny_reasons: Vec<PermissionPolicyReason>,
    pub evaluator_failure_count: u64,
    pub evaluator_failure_reasons: Vec<PermissionPolicyReason>,
    pub containment_warning_count: u64,
    pub containment_warnings: Vec<String>,
    pub protected_target_count: u64,
    pub protected_target_reasons: Vec<ProtectedTargetClass>,
    pub secret_refs: PermissionSecretRefDiagnosticsSummary,
    pub policy_safety_refs: PermissionPolicySafetySnapshotDiagnosticsSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PermissionSecretRefDiagnosticsSummary {
    pub resolved_count: u64,
    pub unresolved_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub unsupported_count: u64,
    pub malformed_count: u64,
    pub items: Vec<PermissionSecretRefAuditSummary>,
}

pub fn build_permission_audit_record(
    action: &PermissionedAction,
    decision: &PermissionPolicyDecision,
    created_at_unix_ms: u64,
) -> PermissionAuditRecord {
    PermissionAuditRecord {
        action_id: action.action_id.clone(),
        session_id: action.session_id.clone(),
        turn_id: action.turn_id.clone(),
        tool_name: action.tool_name.clone(),
        capabilities: action.capabilities.clone(),
        target_summary: action
            .target_refs
            .iter()
            .map(|target| format!("{}:{}", target.kind, target.digest))
            .collect(),
        argument_digest: action.argument_digest.clone(),
        mode: action.permission_mode_snapshot.mode,
        decision: decision.kind,
        decision_reason: decision.reason.clone(),
        evaluator_ref: decision.evaluator_ref.clone(),
        approval_ref: decision.approval_ref.clone(),
        remembered_rule_ref: decision.remembered_rule_ref.clone(),
        containment_summary: action
            .containment_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.summary.clone()),
        secret_ref_summary: action
            .secret_ref_evidence
            .iter()
            .map(|evidence| PermissionSecretRefAuditSummary {
                ref_id: evidence.secret_ref.ref_id.as_str().to_owned(),
                source_kind: serde_json::to_value(evidence.secret_ref.source_kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".to_owned()),
                safe_summary: evidence.secret_ref.safe_summary.label.clone(),
                redaction_evidence_ref: serde_json::to_value(
                    &evidence.redaction_evidence.evidence_id,
                )
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
                status: evidence.status,
                requested_consumer: evidence.requested_consumer.clone(),
            })
            .collect(),
        policy_safety_snapshot_ref: action.policy_safety_snapshot_ref.clone(),
        created_at_unix_ms,
    }
}

pub fn build_permission_diagnostics_summary(
    records: &[PermissionAuditRecord],
    rule_diagnostics: &[RuleDiagnostics],
) -> PermissionDiagnosticsSummary {
    let mut summary = PermissionDiagnosticsSummary {
        allow_count: 0,
        ask_count: 0,
        deny_count: 0,
        auto_approval_reasons: Vec::new(),
        ask_reasons: Vec::new(),
        deny_reasons: Vec::new(),
        evaluator_failure_count: 0,
        evaluator_failure_reasons: Vec::new(),
        containment_warning_count: 0,
        containment_warnings: Vec::new(),
        protected_target_count: 0,
        protected_target_reasons: Vec::new(),
        secret_refs: PermissionSecretRefDiagnosticsSummary::default(),
        policy_safety_refs: PermissionPolicySafetySnapshotDiagnosticsSummary::default(),
    };

    for record in records {
        match record.decision {
            PermissionPolicyDecisionKind::Allow => {
                summary.allow_count += 1;
                summary
                    .auto_approval_reasons
                    .push(record.decision_reason.clone());
            }
            PermissionPolicyDecisionKind::Ask => {
                summary.ask_count += 1;
                summary.ask_reasons.push(record.decision_reason.clone());
            }
            PermissionPolicyDecisionKind::Deny => {
                summary.deny_count += 1;
                summary.deny_reasons.push(record.decision_reason.clone());
            }
        }
        if is_evaluator_failure_reason(&record.decision_reason) {
            summary.evaluator_failure_count += 1;
            summary
                .evaluator_failure_reasons
                .push(record.decision_reason.clone());
        }
        for secret_ref in &record.secret_ref_summary {
            match secret_ref.status {
                PermissionSecretRefStatus::Resolved => summary.secret_refs.resolved_count += 1,
                PermissionSecretRefStatus::Unresolved => summary.secret_refs.unresolved_count += 1,
                PermissionSecretRefStatus::Missing => summary.secret_refs.missing_count += 1,
                PermissionSecretRefStatus::Stale => summary.secret_refs.stale_count += 1,
                PermissionSecretRefStatus::Unsupported => {
                    summary.secret_refs.unsupported_count += 1
                }
                PermissionSecretRefStatus::Malformed => summary.secret_refs.malformed_count += 1,
            }
            summary.secret_refs.items.push(secret_ref.clone());
        }
        let policy_safety_ref = policy_safety_snapshot_audit_summary(
            record.policy_safety_snapshot_ref.as_ref(),
            record.created_at_unix_ms,
        );
        match policy_safety_ref.status {
            PermissionPolicySafetySnapshotAuditStatus::Present => {
                summary.policy_safety_refs.present_count += 1
            }
            PermissionPolicySafetySnapshotAuditStatus::Missing => {
                summary.policy_safety_refs.missing_count += 1
            }
            PermissionPolicySafetySnapshotAuditStatus::Stale => {
                summary.policy_safety_refs.stale_count += 1
            }
            PermissionPolicySafetySnapshotAuditStatus::Malformed => {
                summary.policy_safety_refs.malformed_count += 1
            }
        }
        summary.policy_safety_refs.items.push(policy_safety_ref);
    }

    for diagnostics in rule_diagnostics {
        if let Some(warning) = &diagnostics.containment_warning {
            summary.containment_warning_count += 1;
            summary.containment_warnings.push(warning.clone());
        }
        for protected_target in &diagnostics.protected_targets {
            summary.protected_target_count += 1;
            summary
                .protected_target_reasons
                .push(protected_target.clone());
        }
    }

    summary
}

fn is_evaluator_failure_reason(reason: &PermissionPolicyReason) -> bool {
    matches!(
        reason,
        PermissionPolicyReason::EvaluatorUnavailable
            | PermissionPolicyReason::EvaluatorUncertain
            | PermissionPolicyReason::PromptInjectionSignal
    )
}
