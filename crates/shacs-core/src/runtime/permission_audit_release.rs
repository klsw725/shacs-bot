use serde::{Deserialize, Serialize};

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
