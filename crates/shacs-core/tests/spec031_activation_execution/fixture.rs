use shacs_core::runtime::{
    ActivationReason, ActivationRecord, ActivationRecordInput, ActivationSource, ActivationStatus,
    WorkspaceTrustRef,
};
use shacs_projection::{
    ResourceActivation, ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind,
    ResourceLoadStatus, ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};

pub fn activation() -> ActivationRecord {
    ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:skill:formatter:v1".to_owned(),
        source: ActivationSource::TrustedWorkspace,
        workspace_trust_ref: WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        resource_ref: "resource:skill:formatter".to_owned(),
        source_identity: "source:project:.shacs/skills/formatter".to_owned(),
        content_digest: "a".repeat(64),
        dependency_manifest_digest: "sha256:deps-a".to_owned(),
        status: ActivationStatus::Active,
        reason: ActivationReason::Activated,
        recorded_at_unix_ms: 31_004,
    })
}

pub fn eligible_resource(content_digest: &str) -> ResourceCandidateProjection {
    ResourceCandidateProjection {
        resource_ref: "resource:skill:formatter".to_owned(),
        kind: ResourceKind::Skill,
        source: ResourceSource::Project,
        precedence: ResourcePrecedence::TrustedProjectAuto,
        canonical_path: ".shacs/skills/formatter".to_owned(),
        content_sha256: Some(content_digest.to_owned()),
        collision: ResourceCollisionStatus::None,
        load_status: ResourceLoadStatus::Loaded,
        activation: ResourceActivation::TrustedWorkspace,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        diagnostics: Vec::new(),
    }
}
