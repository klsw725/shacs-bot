use super::{BlockedExternalSurface, BlockedExternalSurfaceReason, ProcessEnvelopeAdmission};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrustPermissionSchemaId {
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrustActionKind {
    DependencyPreparation,
    VerifiedEntrypoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLifecycleStatus {
    Active,
    Stale,
    Revoked,
    Removed,
    Pending,
    Malformed,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTrustDigestPair {
    pub approved: String,
    pub current: String,
    pub envelope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTrustPermissionInput {
    pub schema_id: SkillTrustPermissionSchemaId,
    pub input_id: String,
    pub action_kind: SkillTrustActionKind,
    pub trust_record_ref: String,
    pub trust_owner_ref: String,
    pub lifecycle_status: TrustLifecycleStatus,
    pub lifecycle_status_digest: SkillTrustDigestPair,
    pub staleness_token: String,
    pub skill_descriptor_digest: SkillTrustDigestPair,
    pub source_digest: SkillTrustDigestPair,
    pub content_digest: SkillTrustDigestPair,
    pub dependency_manifest_digest: SkillTrustDigestPair,
    pub package_set_digest: SkillTrustDigestPair,
    pub capability_scope_digest: SkillTrustDigestPair,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint_digest: Option<SkillTrustDigestPair>,
    pub policy_safety_snapshot_ref: String,
    pub process_envelope_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_proof_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_snapshot_ref: Option<String>,
    pub declared_capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_ref: Option<String>,
    pub canonical_input_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTrustGuardInput {
    pub static_policy_admits: bool,
    pub ceiling_admits: bool,
    pub containment_admission: ProcessEnvelopeAdmission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrustPermissionDecisionKind {
    Validated,
    Rejected,
    BlockedExternalSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrustRejectionReason {
    MalformedInput,
    MissingTrustProvenance,
    LifecycleStatus,
    DigestMismatch,
    ManifestOutsideDependency,
    EntrypointDigestRequired,
    StaticPolicy,
    PermissionCeiling,
    ContainmentProof,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTrustPermissionDecision {
    pub kind: SkillTrustPermissionDecisionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SkillTrustRejectionReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_external_surface: Option<BlockedExternalSurface>,
    pub dispatch_count: usize,
}

pub fn validate_skill_trust_permission(
    input: &SkillTrustPermissionInput,
    guards: &SkillTrustGuardInput,
) -> SkillTrustPermissionDecision {
    if malformed(input) {
        return rejected(SkillTrustRejectionReason::MalformedInput);
    }
    if input.trust_record_ref.is_empty() || input.trust_owner_ref.is_empty() {
        return rejected(SkillTrustRejectionReason::MissingTrustProvenance);
    }
    if input.cancellation_ref.is_some() {
        return rejected(SkillTrustRejectionReason::Cancelled);
    }
    if input.lifecycle_status != TrustLifecycleStatus::Active {
        return rejected(SkillTrustRejectionReason::LifecycleStatus);
    }
    if entrypoint_missing(input) {
        return rejected(SkillTrustRejectionReason::EntrypointDigestRequired);
    }
    if manifest_outside(input) {
        return rejected(SkillTrustRejectionReason::ManifestOutsideDependency);
    }
    if digests_mismatch(input) {
        return rejected(SkillTrustRejectionReason::DigestMismatch);
    }
    if !guards.static_policy_admits {
        return rejected(SkillTrustRejectionReason::StaticPolicy);
    }
    if !guards.ceiling_admits {
        return rejected(SkillTrustRejectionReason::PermissionCeiling);
    }
    if guards.containment_admission != ProcessEnvelopeAdmission::Admit {
        return rejected(SkillTrustRejectionReason::ContainmentProof);
    }
    SkillTrustPermissionDecision {
        kind: SkillTrustPermissionDecisionKind::Validated,
        reason: None,
        blocked_external_surface: None,
        dispatch_count: 0,
    }
}

pub fn blocked_skill_trust_external_surface(
    action_kind: SkillTrustActionKind,
) -> SkillTrustPermissionDecision {
    let owner = match action_kind {
        SkillTrustActionKind::DependencyPreparation => {
            "spec032_skill_trust_lifecycle+spec035_trust_persistence"
        }
        SkillTrustActionKind::VerifiedEntrypoint => {
            "spec032_verified_entrypoint_lifecycle+spec035_execution_snapshot"
        }
    };
    SkillTrustPermissionDecision {
        kind: SkillTrustPermissionDecisionKind::BlockedExternalSurface,
        reason: None,
        blocked_external_surface: Some(BlockedExternalSurface {
            status: "BLOCKED_EXTERNAL_SURFACE".to_owned(),
            owner: owner.to_owned(),
            evidence_reason: "Spec032/Spec035 producer evidence is absent".to_owned(),
            reason: BlockedExternalSurfaceReason::MissingOwnerEvidence,
        }),
        dispatch_count: 0,
    }
}

fn malformed(input: &SkillTrustPermissionInput) -> bool {
    input.input_id.is_empty()
        || input.policy_safety_snapshot_ref.is_empty()
        || input.process_envelope_id.is_empty()
        || input.staleness_token.is_empty()
        || input.canonical_input_digest.is_empty()
        || input.declared_capabilities.is_empty()
        || !is_digest(&input.canonical_input_digest)
        || refs_malformed(input)
        || digest_pair_malformed(&input.lifecycle_status_digest)
        || digest_pair_malformed(&input.skill_descriptor_digest)
        || digest_pair_malformed(&input.source_digest)
        || digest_pair_malformed(&input.content_digest)
        || digest_pair_malformed(&input.dependency_manifest_digest)
        || digest_pair_malformed(&input.package_set_digest)
        || digest_pair_malformed(&input.capability_scope_digest)
        || input
            .entrypoint_digest
            .as_ref()
            .is_some_and(digest_pair_malformed)
}

fn refs_malformed(input: &SkillTrustPermissionInput) -> bool {
    [
        input.input_id.as_str(),
        input.trust_record_ref.as_str(),
        input.trust_owner_ref.as_str(),
        input.policy_safety_snapshot_ref.as_str(),
        input.process_envelope_id.as_str(),
        input.staleness_token.as_str(),
    ]
    .into_iter()
    .any(has_control)
}

fn entrypoint_missing(input: &SkillTrustPermissionInput) -> bool {
    match input.action_kind {
        SkillTrustActionKind::DependencyPreparation => input.entrypoint_digest.is_some(),
        SkillTrustActionKind::VerifiedEntrypoint => input.entrypoint_digest.is_none(),
    }
}

fn manifest_outside(input: &SkillTrustPermissionInput) -> bool {
    digest_pair_mismatch(&input.dependency_manifest_digest)
        || digest_pair_mismatch(&input.package_set_digest)
}

fn digests_mismatch(input: &SkillTrustPermissionInput) -> bool {
    digest_pair_mismatch(&input.lifecycle_status_digest)
        || digest_pair_mismatch(&input.skill_descriptor_digest)
        || digest_pair_mismatch(&input.source_digest)
        || digest_pair_mismatch(&input.content_digest)
        || digest_pair_mismatch(&input.capability_scope_digest)
        || input
            .entrypoint_digest
            .as_ref()
            .is_some_and(digest_pair_mismatch)
}

fn digest_pair_malformed(pair: &SkillTrustDigestPair) -> bool {
    !is_digest(&pair.approved) || !is_digest(&pair.current) || !is_digest(&pair.envelope)
}

fn digest_pair_mismatch(pair: &SkillTrustDigestPair) -> bool {
    pair.approved != pair.current || pair.approved != pair.envelope
}

fn is_digest(value: &str) -> bool {
    value.starts_with("sha256:") && value.len() > "sha256:".len() && !has_control(value)
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn rejected(reason: SkillTrustRejectionReason) -> SkillTrustPermissionDecision {
    SkillTrustPermissionDecision {
        kind: SkillTrustPermissionDecisionKind::Rejected,
        reason: Some(reason),
        blocked_external_surface: None,
        dispatch_count: 0,
    }
}
