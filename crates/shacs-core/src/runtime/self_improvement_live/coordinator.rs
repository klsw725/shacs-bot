use super::{
    ApplyBlock, ApplyGateDecision, ApplyGateReceipt, ApplyReceipt, CheckpointReceipt,
    InMemoryImprovementStore, OwnerApplyEvidence, OwnerRollbackEvidence, RollbackCandidate,
    RollbackReceipt, SelfImprovementProposal, VerificationEvidence,
};
use std::sync::Arc;

pub trait ImprovementOwner: Send + Sync {
    fn current_digest(&self, target_ref: &str) -> String;
    fn checkpoint(&self, proposal: &SelfImprovementProposal) -> Option<CheckpointReceipt>;
    fn compare_and_apply(
        &self,
        proposal: &SelfImprovementProposal,
        checkpoint: &CheckpointReceipt,
    ) -> Result<OwnerApplyEvidence, String>;
    fn rollback(
        &self,
        proposal: &SelfImprovementProposal,
        checkpoint: &CheckpointReceipt,
    ) -> Result<OwnerRollbackEvidence, String>;
}

pub trait CurrentImprovementGates: Send + Sync {
    fn evaluate(&self, proposal: &SelfImprovementProposal) -> ApplyGateReceipt;
}

pub trait ImprovementVerifier: Send + Sync {
    fn verify(&self, receipt: &ApplyReceipt) -> VerificationEvidence;
}

pub struct SelfImprovementCoordinator<O, G, V> {
    store: Arc<InMemoryImprovementStore>,
    owner: Arc<O>,
    gates: Arc<G>,
    verifier: Arc<V>,
}

impl<O, G, V> SelfImprovementCoordinator<O, G, V>
where
    O: ImprovementOwner,
    G: CurrentImprovementGates,
    V: ImprovementVerifier,
{
    pub fn new(
        store: Arc<InMemoryImprovementStore>,
        owner: Arc<O>,
        gates: Arc<G>,
        verifier: Arc<V>,
    ) -> Self {
        Self {
            store,
            owner,
            gates,
            verifier,
        }
    }

    pub fn propose(&self, proposal: SelfImprovementProposal) -> Result<(), ApplyBlock> {
        self.store.insert(proposal)
    }

    pub fn apply(&self, proposal_id: &str) -> Result<ApplyReceipt, ApplyBlock> {
        let proposal = self
            .store
            .proposal(proposal_id)
            .ok_or(ApplyBlock::ProposalNotFound)?;
        if self.store.apply_receipt(proposal_id).is_some() {
            return Err(ApplyBlock::AlreadyApplied);
        }
        let current = self.owner.current_digest(proposal.target_ref());
        if current != proposal.expected_target_digest() {
            return Err(ApplyBlock::StaleTarget {
                expected: proposal.expected_target_digest().to_owned(),
                current,
            });
        }
        let gate = self.gates.evaluate(&proposal);
        match gate.decision() {
            ApplyGateDecision::Allowed => {}
            ApplyGateDecision::HookVeto => {
                return Err(ApplyBlock::Gate(ApplyGateDecision::HookVeto))
            }
            ApplyGateDecision::ConfirmationDenied => {
                return Err(ApplyBlock::Gate(ApplyGateDecision::ConfirmationDenied))
            }
            ApplyGateDecision::HeadlessConfirmationDenied => {
                return Err(ApplyBlock::Gate(
                    ApplyGateDecision::HeadlessConfirmationDenied,
                ))
            }
        }
        let checkpoint = self
            .owner
            .checkpoint(&proposal)
            .ok_or(ApplyBlock::MissingCheckpoint)?;
        let owner_evidence = self
            .owner
            .compare_and_apply(&proposal, &checkpoint)
            .map_err(|current| ApplyBlock::OwnerRejected { current })?;
        let receipt = ApplyReceipt::new(
            proposal.proposal_id().to_owned(),
            gate,
            checkpoint,
            owner_evidence,
        );
        self.store.record_apply(receipt.clone())?;
        Ok(receipt)
    }

    pub fn verify(&self, proposal_id: &str) -> Result<VerificationEvidence, ApplyBlock> {
        let receipt = self
            .store
            .apply_receipt(proposal_id)
            .ok_or(ApplyBlock::NotApplied)?;
        let evidence = self.verifier.verify(&receipt);
        self.store
            .record_verification(proposal_id, evidence.clone())?;
        Ok(evidence)
    }

    pub fn rollback_candidate(&self, proposal_id: &str) -> Option<RollbackCandidate> {
        self.store.rollback_candidate(proposal_id)
    }

    pub fn rollback(&self, proposal_id: &str) -> Result<RollbackReceipt, ApplyBlock> {
        if self.store.rollback_receipt(proposal_id).is_some() {
            return Err(ApplyBlock::AlreadyRolledBack);
        }
        let proposal = self
            .store
            .proposal(proposal_id)
            .ok_or(ApplyBlock::ProposalNotFound)?;
        let candidate = self.store.rollback_candidate(proposal_id).ok_or_else(|| {
            match self.store.verification(proposal_id) {
                Some(evidence) if evidence.passed() => ApplyBlock::VerificationPassed,
                Some(_) | None => ApplyBlock::RollbackUnavailable,
            }
        })?;
        let gate = self.gates.evaluate(&proposal);
        match gate.decision() {
            ApplyGateDecision::Allowed => {}
            ApplyGateDecision::HookVeto => {
                return Err(ApplyBlock::Gate(ApplyGateDecision::HookVeto))
            }
            ApplyGateDecision::ConfirmationDenied => {
                return Err(ApplyBlock::Gate(ApplyGateDecision::ConfirmationDenied))
            }
            ApplyGateDecision::HeadlessConfirmationDenied => {
                return Err(ApplyBlock::Gate(
                    ApplyGateDecision::HeadlessConfirmationDenied,
                ))
            }
        }
        let owner_evidence = self
            .owner
            .rollback(&proposal, candidate.checkpoint())
            .map_err(|current| ApplyBlock::OwnerRejected { current })?;
        let receipt = RollbackReceipt::new(gate, owner_evidence);
        self.store.record_rollback(proposal_id, receipt.clone())?;
        Ok(receipt)
    }
}
