use super::{
    ActivationRecord, ActivationStatus, AppExtensionBlocker, AppExtensionProvenanceProjection,
    AppExtensionReplayDispatchCounters, AppExtensionReplayInput, AppExtensionSourceFacts,
    AppExtensionStatus,
};
use shacs_app::app::AppLifecycleState;
use shacs_app::app_lifecycle::AppProcessState;
use shacs_projection::{ResourceActivation, ResourceLoadStatus};

pub fn resolve_app_extension_provenance(
    input: &AppExtensionReplayInput<'_>,
) -> Option<AppExtensionProvenanceProjection> {
    let activation = input.activation?;
    let (status, blockers) = resolve_status(input, activation);
    Some(AppExtensionProvenanceProjection {
        source_app_id: input
            .source
            .map(|source| source.source_app_id.as_str().to_owned()),
        extension_name: input.source.map_or_else(
            || extension_name(activation.resource_ref()).to_owned(),
            |source| source.extension_name.clone(),
        ),
        manifest_digest: input.source.map(|source| source.manifest_digest.clone()),
        current_content_digest: input.source.map(|source| source.content_digest.clone()),
        current_dependency_manifest_digest: input
            .source
            .map(|source| source.dependency_manifest_digest.clone()),
        activation_ref: activation.activation_ref().to_owned(),
        activated_content_digest: activation.content_digest().to_owned(),
        activated_dependency_manifest_digest: activation.dependency_manifest_digest().to_owned(),
        activation_status: activation.status(),
        activation_reason: activation.reason(),
        status,
        blockers,
        replay_dispatch_counters: AppExtensionReplayDispatchCounters::default(),
    })
}

fn resolve_status(
    input: &AppExtensionReplayInput<'_>,
    activation: &ActivationRecord,
) -> (AppExtensionStatus, Vec<AppExtensionBlocker>) {
    match activation.status() {
        ActivationStatus::Revoked => (
            AppExtensionStatus::Revoked,
            vec![AppExtensionBlocker::ActivationRevoked],
        ),
        ActivationStatus::Removed => (
            AppExtensionStatus::Removed,
            vec![AppExtensionBlocker::SourceRemoved],
        ),
        ActivationStatus::Disabled => (
            AppExtensionStatus::Disabled,
            vec![AppExtensionBlocker::AppDisabled],
        ),
        ActivationStatus::Stale => (
            AppExtensionStatus::Stale,
            vec![AppExtensionBlocker::ActivationStale],
        ),
        ActivationStatus::Active => resolve_active(input, activation),
    }
}

fn resolve_active(
    input: &AppExtensionReplayInput<'_>,
    activation: &ActivationRecord,
) -> (AppExtensionStatus, Vec<AppExtensionBlocker>) {
    let Some(source) = input.source else {
        return (
            AppExtensionStatus::Removed,
            vec![AppExtensionBlocker::SourceRemoved],
        );
    };
    if source.lifecycle != AppLifecycleState::Enabled {
        return (
            AppExtensionStatus::Disabled,
            vec![AppExtensionBlocker::AppDisabled],
        );
    }
    if source.process_state != AppProcessState::Running {
        return (
            AppExtensionStatus::Disabled,
            vec![AppExtensionBlocker::ProcessNotRunning],
        );
    }
    resolve_current_facts(input, source, activation)
}

fn resolve_current_facts(
    input: &AppExtensionReplayInput<'_>,
    source: &AppExtensionSourceFacts,
    activation: &ActivationRecord,
) -> (AppExtensionStatus, Vec<AppExtensionBlocker>) {
    let Some(resource) = input.resource else {
        return (
            AppExtensionStatus::Untrusted,
            vec![AppExtensionBlocker::Spec030Untrusted],
        );
    };
    if resource.resource_ref != activation.resource_ref() {
        return (
            AppExtensionStatus::Untrusted,
            vec![AppExtensionBlocker::ResourceIdentityMismatch],
        );
    }
    if source.source_identity != activation.source_identity() {
        return (
            AppExtensionStatus::Untrusted,
            vec![AppExtensionBlocker::ResourceIdentityMismatch],
        );
    }
    if resource.load_status != ResourceLoadStatus::Loaded
        || resource.activation == ResourceActivation::Inactive
    {
        return (
            AppExtensionStatus::Untrusted,
            vec![AppExtensionBlocker::Spec030Untrusted],
        );
    }
    let mut blockers = Vec::new();
    if resource.content_sha256.as_deref() != Some(activation.content_digest())
        || source.content_digest != activation.content_digest()
    {
        blockers.push(AppExtensionBlocker::ContentDigestMismatch);
    }
    if source.dependency_manifest_digest != activation.dependency_manifest_digest() {
        blockers.push(AppExtensionBlocker::DependencyManifestDigestMismatch);
    }
    if blockers.is_empty() {
        (AppExtensionStatus::Active, blockers)
    } else {
        (AppExtensionStatus::Stale, blockers)
    }
}

fn extension_name(resource_ref: &str) -> &str {
    resource_ref.rsplit(':').next().unwrap_or(resource_ref)
}
