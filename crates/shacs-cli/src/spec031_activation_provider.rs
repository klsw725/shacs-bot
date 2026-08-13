use shacs_config::{load_config, ConfigContext, LoadOptions};
use shacs_core::app::{AppRegistryEntry, AppRegistryStore};
use shacs_core::runtime::{
    ActivationCurrentIdentity, ActivationSnapshotCandidate, ActivationStore, WorkspaceTrustRef,
};
use shacs_projection::ResourceCandidateProjection;
use std::path::{Path, PathBuf};
use std::sync::Arc;

type ActivationProvider =
    Arc<dyn Fn(&ResourceCandidateProjection) -> Option<ActivationSnapshotCandidate> + Send + Sync>;

pub(crate) fn production_activation_provider(
    config_path: PathBuf,
    workspace: PathBuf,
) -> ActivationProvider {
    Arc::new(move |resource| {
        let bundle = load_config(LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: Some(workspace.clone()),
            resolve_env: false,
            write_back_migrations: false,
        })
        .ok()?;
        candidate(&bundle.context, resource)
    })
}

fn candidate(
    context: &ConfigContext,
    resource: &ResourceCandidateProjection,
) -> Option<ActivationSnapshotCandidate> {
    let canonical_source = PathBuf::from(&resource.canonical_path)
        .canonicalize()
        .ok()?;
    let app = owning_app(&context.data_dir, &canonical_source)?;
    let owner = WorkspaceTrustRef::new(context.workspace_permission_id().ok()?.as_str());
    let source_identity = format!("source:app:{}", canonical_source.to_string_lossy());
    let record = ActivationStore::new(activation_store_path(context))
        .find_current(&resource.resource_ref, &owner, &source_identity)
        .ok()??;
    Some(ActivationSnapshotCandidate::new(
        record,
        ActivationCurrentIdentity::new(owner, source_identity, app.digest, app.lifecycle_state),
    ))
}

fn owning_app(data_dir: &Path, source: &Path) -> Option<AppRegistryEntry> {
    AppRegistryStore::new(data_dir)
        .list()
        .ok()?
        .into_iter()
        .find(|entry| {
            entry
                .bundle_path
                .canonicalize()
                .is_ok_and(|root| source.starts_with(root))
        })
}

fn activation_store_path(context: &ConfigContext) -> PathBuf {
    context
        .runtime_subdir("snapshots")
        .join("activation-records.json")
}
