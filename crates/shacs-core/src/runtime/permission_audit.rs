use crate::runtime::{
    PermissionMode, PermissionPolicyDecision, PermissionPolicyDecisionKind, PermissionPolicyReason,
    PermissionedAction, ProtectedTargetClass, RuleDiagnostics, SafetyCapability,
};
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
    pub containment_summary: Option<String>,
    pub created_at_unix_ms: u64,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionContractCase {
    pub id: String,
    pub required_bucket: PermissionReleaseEvidenceBucket,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionReleaseEvidenceBucket {
    InheritedBoundaryCases,
    PermissionAuditDiagnostics,
    PermissionReplayInvariants,
    ContractMatrix,
    ReleaseEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReleaseEvidence {
    pub buckets: Vec<PermissionReleaseEvidenceBucket>,
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
        containment_summary: action
            .containment_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.summary.clone()),
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

pub fn permission_prd005_006_contract_cases() -> Vec<PermissionContractCase> {
    vec![
        PermissionContractCase {
            id: "prd005-inherited-boundary".to_owned(),
            required_bucket: PermissionReleaseEvidenceBucket::InheritedBoundaryCases,
            summary: "inherited permission boundaries cannot widen by declaration or deferred gate"
                .to_owned(),
        },
        PermissionContractCase {
            id: "prd005-audit-diagnostics".to_owned(),
            required_bucket: PermissionReleaseEvidenceBucket::PermissionAuditDiagnostics,
            summary: "permission audit diagnostics include decision and failure reason counts"
                .to_owned(),
        },
        PermissionContractCase {
            id: "prd006-replay-invariants".to_owned(),
            required_bucket: PermissionReleaseEvidenceBucket::PermissionReplayInvariants,
            summary: "permission replay preserves same-context decisions and fail-closed denies"
                .to_owned(),
        },
        PermissionContractCase {
            id: "prd006-contract-matrix".to_owned(),
            required_bucket: PermissionReleaseEvidenceBucket::ContractMatrix,
            summary: "permission policy contract matrix records release closure cases".to_owned(),
        },
    ]
}

pub fn required_permission_release_evidence_buckets() -> Vec<PermissionReleaseEvidenceBucket> {
    let mut buckets = permission_prd005_006_contract_cases()
        .into_iter()
        .map(|case| case.required_bucket)
        .collect::<Vec<_>>();
    buckets.push(PermissionReleaseEvidenceBucket::ReleaseEvidence);
    buckets
}

pub fn permission_release_evidence_complete(evidence: &PermissionReleaseEvidence) -> bool {
    required_permission_release_evidence_buckets()
        .into_iter()
        .all(|bucket| evidence.buckets.contains(&bucket))
}

fn is_evaluator_failure_reason(reason: &PermissionPolicyReason) -> bool {
    matches!(
        reason,
        PermissionPolicyReason::EvaluatorUnavailable
            | PermissionPolicyReason::EvaluatorUncertain
            | PermissionPolicyReason::PromptInjectionSignal
    )
}
