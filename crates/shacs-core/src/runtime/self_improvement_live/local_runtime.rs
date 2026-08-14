use super::local_owner::owner_evidence;
use super::local_transaction::{
    LocalTransactionPhase, ProcessTransaction, TransactionJournal, TransactionReceipt,
};
use super::local_types::digest;
use super::{
    LocalApplyReceipt, LocalArtifactOwner, LocalGateSource, LocalImprovementBlock,
    LocalImprovementProposal, LocalImprovementStatus, LocalImprovementStore,
    LocalImprovementVerifier, LocalRollbackCandidate, LocalRollbackReceipt, VerificationEvidence,
};
use std::sync::Arc;

pub struct LocalImprovementRuntime<G: ?Sized, V: ?Sized> {
    store: Arc<LocalImprovementStore>,
    owner: Arc<LocalArtifactOwner>,
    gates: Arc<G>,
    verifier: Arc<V>,
}

impl<G: LocalGateSource + ?Sized, V: LocalImprovementVerifier + ?Sized>
    LocalImprovementRuntime<G, V>
{
    pub fn new(
        store: Arc<LocalImprovementStore>,
        owner: Arc<LocalArtifactOwner>,
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

    pub fn propose(&self, proposal: LocalImprovementProposal) -> Result<(), LocalImprovementBlock> {
        proposal
            .snapshot()
            .validate_provenance()
            .map_err(|_| LocalImprovementBlock::InvalidSnapshot)?;
        let transaction = ProcessTransaction::acquire(self.owner.root())?;
        self.recover_locked(&transaction)?;
        if self.store_path_exists() {
            self.store.reload()?;
        }
        self.store.insert(proposal)
    }

    pub fn apply(&self, id: &str) -> Result<LocalApplyReceipt, LocalImprovementBlock> {
        let transaction = ProcessTransaction::acquire(self.owner.root())?;
        self.recover_locked(&transaction)?;
        self.store.reload()?;
        let proposal = self
            .store
            .proposal(id)
            .ok_or(LocalImprovementBlock::ProposalNotFound)?;
        if self.store.apply_receipt(id).is_some() {
            return Err(LocalImprovementBlock::AlreadyApplied);
        }
        let current = self.owner.read(&proposal)?;
        let current_digest = digest(&current);
        require_digest(&current_digest, proposal.expected_target_digest())?;
        let gates = self.gates.current_receipts(&proposal, &current_digest)?;
        gates.validate(&proposal.snapshot().provenance_digest, &current_digest)?;
        let receipt = LocalApplyReceipt {
            owner_evidence_id: owner_evidence("apply", &proposal, &current_digest),
            gate_evidence_ids: gates.evidence_ids(),
        };
        let journal = TransactionJournal {
            schema_version: 1,
            proposal_id: id.to_owned(),
            target_ref: proposal.target_ref().to_owned(),
            before_digest: current_digest,
            after_digest: proposal.candidate_digest().to_owned(),
            checkpoint: current,
            replacement: proposal.candidate().to_vec(),
            receipt: TransactionReceipt::Apply {
                receipt: receipt.clone(),
            },
            phase: LocalTransactionPhase::IntentDurable,
        };
        self.mutate(&transaction, &proposal, journal)?;
        Ok(receipt)
    }

    pub fn verify(&self, id: &str) -> Result<VerificationEvidence, LocalImprovementBlock> {
        let transaction = ProcessTransaction::acquire(self.owner.root())?;
        self.recover_locked(&transaction)?;
        self.store.reload()?;
        let record = self
            .store
            .record(id)
            .ok_or(LocalImprovementBlock::ProposalNotFound)?;
        if record.apply.is_none() {
            return Err(LocalImprovementBlock::NotApplied);
        }
        let current = self.owner.read(&record.proposal)?;
        let evidence = self.verifier.verify(&record.proposal, &current);
        self.store
            .record_verification(id, evidence.passed(), evidence.evidence_ref())?;
        Ok(evidence)
    }

    pub fn rollback_candidate(&self, id: &str) -> Option<LocalRollbackCandidate> {
        self.store.rollback_candidate(id)
    }

    pub fn inspect(&self, id: &str) -> Result<LocalImprovementStatus, LocalImprovementBlock> {
        let transaction = ProcessTransaction::acquire(self.owner.root())?;
        self.recover_locked(&transaction)?;
        if self.store_path_exists() {
            self.store.reload()?;
        }
        self.store.status(id)
    }

    pub fn rollback(&self, id: &str) -> Result<LocalRollbackReceipt, LocalImprovementBlock> {
        let transaction = ProcessTransaction::acquire(self.owner.root())?;
        self.recover_locked(&transaction)?;
        self.store.reload()?;
        let record = self
            .store
            .record(id)
            .ok_or(LocalImprovementBlock::ProposalNotFound)?;
        if record.rollback.is_some() {
            return Err(LocalImprovementBlock::AlreadyRolledBack);
        }
        match (
            record.rollback_candidate.as_ref(),
            record.verification_passed,
        ) {
            (None, Some(true)) => return Err(LocalImprovementBlock::VerificationPassed),
            (None, Some(false) | None) => return Err(LocalImprovementBlock::RollbackUnavailable),
            (Some(_), Some(true) | Some(false) | None) => {}
        }
        let current = self.owner.read(&record.proposal)?;
        let current_digest = digest(&current);
        require_digest(&current_digest, record.proposal.candidate_digest())?;
        let gates = self
            .gates
            .current_receipts(&record.proposal, &current_digest)?;
        gates.validate(
            &record.proposal.snapshot().provenance_digest,
            &current_digest,
        )?;
        let apply = record
            .apply
            .as_ref()
            .ok_or(LocalImprovementBlock::NotApplied)?;
        if !gates.differs_from(&apply.gate_evidence_ids) {
            return Err(LocalImprovementBlock::StaleGateEvidence);
        }
        let checkpoint = record
            .checkpoint
            .clone()
            .ok_or(LocalImprovementBlock::RollbackUnavailable)?;
        let receipt = LocalRollbackReceipt {
            owner_evidence_id: owner_evidence("rollback", &record.proposal, &current_digest),
        };
        let journal = TransactionJournal {
            schema_version: 1,
            proposal_id: id.to_owned(),
            target_ref: record.proposal.target_ref().to_owned(),
            before_digest: current_digest,
            after_digest: digest(&checkpoint),
            checkpoint: current,
            replacement: checkpoint,
            receipt: TransactionReceipt::Rollback {
                receipt: receipt.clone(),
            },
            phase: LocalTransactionPhase::IntentDurable,
        };
        self.mutate(&transaction, &record.proposal, journal)?;
        Ok(receipt)
    }

    fn mutate(
        &self,
        transaction: &ProcessTransaction,
        proposal: &LocalImprovementProposal,
        mut journal: TransactionJournal,
    ) -> Result<(), LocalImprovementBlock> {
        let current_digest = digest(&self.owner.read(proposal)?);
        require_digest(&current_digest, &journal.before_digest)?;
        transaction.persist(&journal)?;
        let staged = transaction.stage(&journal.replacement)?;
        journal.phase = LocalTransactionPhase::Staged;
        transaction.persist(&journal)?;
        let target = self.owner.target_path(proposal)?;
        transaction.commit_target(&staged, &target)?;
        self.owner.note_mutation();
        journal.phase = LocalTransactionPhase::TargetReplaced;
        transaction.persist(&journal)?;
        self.commit_receipt(&journal)?;
        transaction.clear()
    }

    fn recover_locked(
        &self,
        transaction: &ProcessTransaction,
    ) -> Result<(), LocalImprovementBlock> {
        let Some(mut journal) = transaction.journal()? else {
            return Ok(());
        };
        self.store.reload()?;
        let proposal = self
            .store
            .proposal(&journal.proposal_id)
            .ok_or(LocalImprovementBlock::RecoveryRequired)?;
        if proposal.target_ref() != journal.target_ref {
            return Err(LocalImprovementBlock::RecoveryRequired);
        }
        let current_digest = digest(&self.owner.read(&proposal)?);
        if current_digest == journal.before_digest {
            return transaction.clear();
        }
        if current_digest != journal.after_digest {
            return Err(LocalImprovementBlock::RecoveryRequired);
        }
        journal.phase = LocalTransactionPhase::TargetReplaced;
        transaction.persist(&journal)?;
        self.commit_receipt(&journal)?;
        transaction.clear()
    }

    fn commit_receipt(&self, journal: &TransactionJournal) -> Result<(), LocalImprovementBlock> {
        match &journal.receipt {
            TransactionReceipt::Apply { receipt } => {
                if self.store.apply_receipt(&journal.proposal_id).is_none() {
                    self.store.record_apply(
                        &journal.proposal_id,
                        receipt.clone(),
                        journal.checkpoint.clone(),
                    )?;
                }
            }
            TransactionReceipt::Rollback { receipt } => {
                let record = self
                    .store
                    .record(&journal.proposal_id)
                    .ok_or(LocalImprovementBlock::ProposalNotFound)?;
                if record.rollback.is_none() {
                    self.store
                        .record_rollback(&journal.proposal_id, receipt.clone())?;
                }
            }
        }
        Ok(())
    }

    fn store_path_exists(&self) -> bool {
        self.store.has_durable_document()
    }
}

fn require_digest(current: &str, expected: &str) -> Result<(), LocalImprovementBlock> {
    if current == expected {
        Ok(())
    } else {
        Err(LocalImprovementBlock::StaleTarget {
            expected: expected.to_owned(),
            current: current.to_owned(),
        })
    }
}
