use super::{
    LocalApplyReceipt, LocalArtifactOwner, LocalDigestVerifier, LocalGateSource,
    LocalImprovementBlock, LocalImprovementProposal, LocalImprovementRuntime,
    LocalImprovementStatus, LocalImprovementStore, LocalImprovementVerifier,
    LocalRollbackCandidate, LocalRollbackReceipt, VerificationEvidence,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct LocalImprovementService {
    root: PathBuf,
    runtime: LocalImprovementRuntime<dyn LocalGateSource, dyn LocalImprovementVerifier>,
}

impl LocalImprovementService {
    pub fn open(
        root: impl AsRef<Path>,
        gates: Arc<dyn LocalGateSource>,
    ) -> Result<Self, LocalImprovementBlock> {
        let root = root.as_ref().to_path_buf();
        let owner = Arc::new(LocalArtifactOwner::new(&root)?);
        let store = Arc::new(LocalImprovementStore::open_state(owner.root())?);
        Ok(Self {
            runtime: LocalImprovementRuntime::new(
                store,
                owner,
                gates,
                Arc::new(LocalDigestVerifier),
            ),
            root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn propose(
        &self,
        proposal_id: &str,
        target_ref: &str,
        expected_target_digest: &str,
        candidate_json: &str,
        snapshot_json: &str,
        confirmation_required: bool,
    ) -> Result<LocalImprovementStatus, LocalImprovementBlock> {
        let proposal = LocalImprovementProposal::from_json_artifacts(
            proposal_id,
            target_ref,
            expected_target_digest,
            candidate_json,
            snapshot_json,
        )?;
        let proposal = if confirmation_required {
            proposal.requiring_confirmation()
        } else {
            proposal
        };
        self.runtime.propose(proposal)?;
        self.runtime.inspect(proposal_id)
    }

    pub fn inspect(&self, id: &str) -> Result<LocalImprovementStatus, LocalImprovementBlock> {
        self.runtime.inspect(id)
    }

    pub fn apply(&self, id: &str) -> Result<LocalApplyReceipt, LocalImprovementBlock> {
        self.runtime.apply(id)
    }

    pub fn verify(&self, id: &str) -> Result<VerificationEvidence, LocalImprovementBlock> {
        self.runtime.verify(id)
    }

    pub fn rollback_candidate(&self, id: &str) -> Option<LocalRollbackCandidate> {
        self.runtime.rollback_candidate(id)
    }

    pub fn rollback(&self, id: &str) -> Result<LocalRollbackReceipt, LocalImprovementBlock> {
        self.runtime.rollback(id)
    }
}
