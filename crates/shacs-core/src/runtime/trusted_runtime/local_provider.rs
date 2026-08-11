use super::{
    build_trusted_runtime_projection, local_resources, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_config::{load_config, LoadOptions};
use shacs_projection::{
    Spec030ProjectionProvider, Spec030RuntimeProjection, Spec030UnavailableReason,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LocalSpec030ProjectionProvider {
    facts: Spec030FactStore,
}

impl LocalSpec030ProjectionProvider {
    pub const fn new(facts: Spec030FactStore) -> Self {
        Self { facts }
    }

    pub fn load(config_path: Option<PathBuf>, workspace_override: Option<PathBuf>) -> Self {
        let facts = load_config(LoadOptions {
            config_path,
            workspace_override,
            resolve_env: true,
            write_back_migrations: false,
        })
        .map(|bundle| {
            let workspace_trust = if bundle
                .config
                .plugins
                .trusts_workspace(&bundle.context.workspace)
            {
                WorkspaceTrustObservation::Trusted
            } else {
                WorkspaceTrustObservation::Untrusted
            };
            let facts = Spec030FactStore::new(workspace_trust);
            if local_resources::populate(&facts, &bundle).is_err() {
                Spec030FactStore::unavailable(Spec030UnavailableReason::OwnerUnavailable)
            } else {
                facts
            }
        })
        .unwrap_or_else(|_| {
            Spec030FactStore::unavailable(Spec030UnavailableReason::OwnerUnavailable)
        });
        Self::new(facts)
    }

    pub fn fact_store(&self) -> Spec030FactStore {
        self.facts.clone()
    }
}

impl Spec030ProjectionProvider for LocalSpec030ProjectionProvider {
    fn projection(&self) -> Spec030RuntimeProjection {
        build_trusted_runtime_projection(self.facts.snapshot().into_input()).unwrap_or_else(|_| {
            Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerUnavailable)
        })
    }
}
