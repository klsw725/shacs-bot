mod finalize;
mod storage;
mod types;

pub use types::{
    ApplyError, ApplyPending, AuthoringCheckpoint, AuthoringProposal, InstallHandoff, ProposalKind,
    RecoveryEvidence, RedactedIntent, VerificationOutcome,
};

use crate::app::{AppId, AppRegistryStore};
use std::path::{Path, PathBuf};

pub struct AuthoringFlowStore {
    pub(super) data_dir: PathBuf,
}

impl AuthoringFlowStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn propose_new(
        &self,
        draft_path: &Path,
        intent: &str,
    ) -> Result<AuthoringProposal, ApplyError> {
        let candidate = if draft_path.join("candidates").is_dir() {
            draft_path.join("candidates")
        } else {
            draft_path.to_path_buf()
        };
        self.propose(candidate, intent, ProposalKind::Install)
    }

    pub fn propose_update(
        &self,
        candidate: &Path,
        intent: &str,
    ) -> Result<AuthoringProposal, ApplyError> {
        self.propose(candidate.to_path_buf(), intent, ProposalKind::Update)
    }

    pub fn checkpoint(
        &self,
        proposal: &AuthoringProposal,
    ) -> Result<AuthoringCheckpoint, ApplyError> {
        let actual_revision = storage::tree_digest(&proposal.candidate_path)?;
        if actual_revision != proposal.revision_digest {
            return Err(ApplyError::StaleRevision {
                expected: proposal.revision_digest.clone(),
                actual: actual_revision,
            });
        }
        self.check_installed_digest(proposal)?;
        let checkpoint_id = format!(
            "checkpoint-{}",
            storage::short_digest(&proposal.proposal_id)
        );
        let snapshot_path = match proposal.kind {
            ProposalKind::Install => None,
            ProposalKind::Update => {
                let source = self.installed_path(&proposal.app_id);
                let snapshot = self.flow_dir().join("snapshots").join(&checkpoint_id);
                storage::copy_tree(&source, &snapshot)?;
                Some(snapshot)
            }
        };
        let checkpoint = AuthoringCheckpoint {
            checkpoint_id,
            proposal: proposal.clone(),
            snapshot_path,
            original_registry_entry: AppRegistryStore::new(&self.data_dir)
                .inspect(&proposal.app_id)?,
        };
        storage::write_json(
            &self
                .flow_dir()
                .join("checkpoints")
                .join(format!("{}.json", checkpoint.checkpoint_id)),
            &checkpoint,
        )?;
        Ok(checkpoint)
    }

    pub fn apply(&self, checkpoint: &AuthoringCheckpoint) -> Result<ApplyPending, ApplyError> {
        let actual_revision = storage::tree_digest(&checkpoint.proposal.candidate_path)?;
        if actual_revision != checkpoint.proposal.revision_digest {
            return Err(ApplyError::StaleRevision {
                expected: checkpoint.proposal.revision_digest.clone(),
                actual: actual_revision,
            });
        }
        self.check_installed_digest(&checkpoint.proposal)?;
        let evidence = RecoveryEvidence {
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            recovery_required: true,
            phase: "verification-pending".to_owned(),
        };
        self.write_recovery(&evidence)?;
        let target = self.installed_path(&checkpoint.proposal.app_id);
        storage::replace_tree(&checkpoint.proposal.candidate_path, &target)?;
        Ok(ApplyPending {
            checkpoint: checkpoint.clone(),
            target_path: target,
        })
    }

    pub fn verify(
        &self,
        pending: ApplyPending,
        outcome: VerificationOutcome,
    ) -> Result<InstallHandoff, ApplyError> {
        match outcome {
            VerificationOutcome::Failed { reason } => {
                self.write_recovery(&RecoveryEvidence {
                    checkpoint_id: pending.checkpoint.checkpoint_id.clone(),
                    recovery_required: true,
                    phase: "verification-failed".to_owned(),
                })?;
                Err(ApplyError::VerificationFailed { reason })
            }
            VerificationOutcome::Passed => self.finish_install(pending),
        }
    }

    pub fn inspect_recovery(&self, checkpoint_id: &str) -> Result<RecoveryEvidence, ApplyError> {
        storage::read_json(&self.recovery_path(checkpoint_id))
    }

    pub fn recover(&self, checkpoint_id: &str) -> Result<RecoveryEvidence, ApplyError> {
        let current = self.inspect_recovery(checkpoint_id)?;
        if !current.recovery_required {
            return Err(ApplyError::RecoveryNotRequired(checkpoint_id.to_owned()));
        }
        let checkpoint: AuthoringCheckpoint = storage::read_json(
            &self
                .flow_dir()
                .join("checkpoints")
                .join(format!("{checkpoint_id}.json")),
        )?;
        match checkpoint.proposal.kind {
            ProposalKind::Update => storage::replace_tree(
                &self.flow_dir().join("snapshots").join(checkpoint_id),
                &self.installed_path(&checkpoint.proposal.app_id),
            )?,
            ProposalKind::Install => {
                storage::remove_tree(&self.installed_path(&checkpoint.proposal.app_id))?
            }
        }
        let registry_store = AppRegistryStore::new(&self.data_dir);
        let mut registry = registry_store.load()?;
        match checkpoint.original_registry_entry {
            Some(entry) => {
                registry.entries.insert(entry.app_id.clone(), entry);
            }
            None => {
                registry.entries.remove(&checkpoint.proposal.app_id);
            }
        }
        registry_store.save(&registry)?;
        let evidence = RecoveryEvidence {
            checkpoint_id: checkpoint_id.to_owned(),
            recovery_required: false,
            phase: "recovered".to_owned(),
        };
        self.write_recovery(&evidence)?;
        Ok(evidence)
    }

    fn propose(
        &self,
        candidate: PathBuf,
        intent: &str,
        kind: ProposalKind,
    ) -> Result<AuthoringProposal, ApplyError> {
        let validated = self.validate_candidate(&candidate)?;
        let existing = AppRegistryStore::new(&self.data_dir).inspect(&validated.manifest.id)?;
        match (&kind, &existing) {
            (ProposalKind::Install, Some(_)) => {
                return Err(ApplyError::AlreadyInstalled(validated.manifest.id))
            }
            (ProposalKind::Update, None) => {
                return Err(ApplyError::NotInstalled(validated.manifest.id))
            }
            (ProposalKind::Install, None) | (ProposalKind::Update, Some(_)) => {}
        }
        let revision_digest = storage::tree_digest(&candidate)?;
        let proposal_id = format!("proposal-{}", storage::short_digest(&revision_digest));
        let installed_digest = existing.as_ref().map(|entry| entry.digest.clone());
        let installed_tree_digest = existing
            .as_ref()
            .map(|entry| storage::public_tree_digest(&entry.bundle_path))
            .transpose()?;
        let diff = existing.map_or_else(Vec::new, |entry| {
            vec![format!("{} -> {}", entry.digest, validated.digest)]
        });
        let proposal = AuthoringProposal {
            proposal_id,
            app_id: validated.manifest.id,
            kind,
            user_intent: RedactedIntent::new(intent),
            revision_digest,
            candidate_digest: validated.digest,
            installed_digest,
            installed_tree_digest,
            validation_summary: "static manifest and local resource validation passed".to_owned(),
            risk_summary: "authoring mutation only; no runtime authorization or activation"
                .to_owned(),
            diff,
            candidate_path: candidate,
        };
        storage::write_json(
            &self
                .flow_dir()
                .join("proposals")
                .join(format!("{}.json", proposal.proposal_id)),
            &proposal,
        )?;
        Ok(proposal)
    }

    pub(super) fn flow_dir(&self) -> PathBuf {
        self.data_dir.join("authoring").join("flow")
    }
    fn installed_path(&self, app_id: &AppId) -> PathBuf {
        self.data_dir
            .join("apps")
            .join(format!("{app_id}.shacsapp"))
    }
    fn recovery_path(&self, checkpoint_id: &str) -> PathBuf {
        self.flow_dir()
            .join("recovery")
            .join(format!("{checkpoint_id}.json"))
    }
    pub(super) fn write_recovery(&self, evidence: &RecoveryEvidence) -> Result<(), ApplyError> {
        storage::write_json(&self.recovery_path(&evidence.checkpoint_id), evidence)
    }
}
