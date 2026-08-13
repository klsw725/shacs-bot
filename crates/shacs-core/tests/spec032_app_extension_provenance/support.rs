use shacs_app::app::{AppId, AppLifecycleState};
use shacs_app::app_lifecycle::AppProcessState;
use shacs_core::runtime::{
    ActivationReason, ActivationRecord, ActivationRecordInput, ActivationSource, ActivationStatus,
    AppExtensionSourceFacts, WorkspaceTrustRef,
};
use shacs_projection::{
    ResourceActivation, ResourceCandidateProjection, ResourceCollisionStatus, ResourceKind,
    ResourceLoadStatus, ResourcePrecedence, ResourceSource, TrustedCodeDisclosure,
};

pub fn facts(
    lifecycle: AppLifecycleState,
    process_state: AppProcessState,
    content: &str,
) -> AppExtensionSourceFacts {
    AppExtensionSourceFacts {
        source_app_id: AppId::parse("tools.formatter").expect("valid app id"),
        extension_name: "formatter".to_owned(),
        manifest_digest: "manifest-a".to_owned(),
        content_digest: content.to_owned(),
        dependency_manifest_digest: "deps-a".to_owned(),
        lifecycle,
        process_state,
        source_identity: "app:tools.formatter".to_owned(),
    }
}

pub fn activation(
    status: ActivationStatus,
    reason: ActivationReason,
    content: &str,
) -> ActivationRecord {
    ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:app:formatter:v1".to_owned(),
        source: ActivationSource::App,
        workspace_trust_ref: WorkspaceTrustRef::new("workspace:owner"),
        resource_ref: "resource:extension:formatter".to_owned(),
        source_identity: "app:tools.formatter".to_owned(),
        content_digest: content.to_owned(),
        dependency_manifest_digest: "deps-a".to_owned(),
        status,
        reason,
        recorded_at_unix_ms: 32_001,
    })
}

pub fn eligible(content: &str) -> ResourceCandidateProjection {
    resource(
        content,
        ResourceLoadStatus::Loaded,
        ResourceActivation::TrustedWorkspace,
    )
}

pub fn ineligible(content: &str) -> ResourceCandidateProjection {
    resource(
        content,
        ResourceLoadStatus::Rejected,
        ResourceActivation::Inactive,
    )
}

fn resource(
    content: &str,
    load_status: ResourceLoadStatus,
    activation: ResourceActivation,
) -> ResourceCandidateProjection {
    ResourceCandidateProjection {
        resource_ref: "resource:extension:formatter".to_owned(),
        kind: ResourceKind::Extension,
        source: ResourceSource::Project,
        precedence: ResourcePrecedence::TrustedProjectAuto,
        canonical_path: "apps/tools.formatter/extensions/formatter".to_owned(),
        content_sha256: Some(content.to_owned()),
        collision: ResourceCollisionStatus::None,
        load_status,
        activation,
        trusted_code_disclosure: TrustedCodeDisclosure::Shown,
        diagnostics: Vec::new(),
    }
}
