use super::{
    storage, ApplyError, ApplyPending, AuthoringFlowStore, AuthoringProposal, InstallHandoff,
    RecoveryEvidence,
};
use crate::app::{
    AppBundlePath, AppLifecycleState, AppManifest, AppRegistryEntry, AppRegistryStore,
    ValidatedAppBundle,
};
use std::path::Path;

impl AuthoringFlowStore {
    pub(super) fn finish_install(
        &self,
        pending: ApplyPending,
    ) -> Result<InstallHandoff, ApplyError> {
        let validated = AppManifest::load_from_bundle(&AppBundlePath::new(&pending.target_path))?;
        if validated.digest != pending.checkpoint.proposal.candidate_digest {
            return Err(ApplyError::VerificationFailed {
                reason: "applied digest differs from proposal".to_owned(),
            });
        }
        let registry_store = AppRegistryStore::new(&self.data_dir);
        let mut registry = registry_store.load()?;
        let installed_at_unix_ms = registry
            .inspect(&validated.manifest.id)
            .map_or(0, |entry| entry.installed_at_unix_ms);
        let entry = AppRegistryEntry {
            app_id: validated.manifest.id.clone(),
            version: validated.manifest.version,
            digest: validated.digest,
            bundle_path: pending.target_path,
            lifecycle_state: AppLifecycleState::Installed,
            permission_requests: validated.manifest.permissions,
            secret_requests: validated.manifest.secrets,
            resource_summaries: validated.resource_summaries,
            grant_reference: None,
            unavailable_reasons: Vec::new(),
            process_snapshots: Vec::new(),
            installed_at_unix_ms,
        };
        registry.entries.insert(entry.app_id.clone(), entry.clone());
        registry_store.save(&registry)?;
        self.write_recovery(&RecoveryEvidence {
            checkpoint_id: pending.checkpoint.checkpoint_id.clone(),
            recovery_required: false,
            phase: "install-handoff-complete".to_owned(),
        })?;
        let handoff = InstallHandoff {
            checkpoint_id: pending.checkpoint.checkpoint_id,
            app_id: entry.app_id.clone(),
            version: entry.version.clone(),
            digest: entry.digest.clone(),
            registry_entry: entry,
            runtime_authorization_created: false,
            executable_activation_created: false,
            process_started: false,
        };
        storage::write_json(
            &self
                .flow_dir()
                .join("receipts")
                .join(format!("{}.json", handoff.checkpoint_id)),
            &handoff,
        )?;
        Ok(handoff)
    }

    pub(super) fn validate_candidate(
        &self,
        candidate: &Path,
    ) -> Result<ValidatedAppBundle, ApplyError> {
        let manifest: AppManifest = storage::read_json(&candidate.join("manifest.json"))?;
        let staging = self
            .flow_dir()
            .join("validation")
            .join(format!("{}.shacsapp", manifest.id));
        storage::remove_tree(&staging)?;
        storage::copy_tree(candidate, &staging)?;
        let result =
            AppManifest::load_from_bundle(&AppBundlePath::new(&staging)).map_err(ApplyError::from);
        storage::remove_tree(&staging)?;
        result
    }

    pub(super) fn check_installed_digest(
        &self,
        proposal: &AuthoringProposal,
    ) -> Result<(), ApplyError> {
        let actual = AppRegistryStore::new(&self.data_dir)
            .inspect(&proposal.app_id)?
            .map(|entry| entry.digest);
        if actual != proposal.installed_digest {
            return Err(ApplyError::InstalledDigestChanged {
                expected: proposal.installed_digest.clone(),
                actual,
            });
        }
        if let Some(expected) = proposal.installed_digest.as_deref() {
            let actual_bundle = AppManifest::load_from_bundle(&AppBundlePath::new(
                self.data_dir
                    .join("apps")
                    .join(format!("{}.shacsapp", proposal.app_id)),
            ))?;
            if actual_bundle.digest != expected {
                return Err(ApplyError::InstalledDigestChanged {
                    expected: Some(expected.to_owned()),
                    actual: Some(actual_bundle.digest),
                });
            }
        }
        if let Some(expected) = proposal.installed_tree_digest.as_deref() {
            let actual = storage::public_tree_digest(
                &self
                    .data_dir
                    .join("apps")
                    .join(format!("{}.shacsapp", proposal.app_id)),
            )?;
            if actual != expected {
                return Err(ApplyError::InstalledDigestChanged {
                    expected: Some(expected.to_owned()),
                    actual: Some(actual),
                });
            }
        }
        Ok(())
    }
}
