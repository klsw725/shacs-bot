use super::{
    ApplyBlock, ApplyReceipt, RollbackCandidate, RollbackReceipt, SelfImprovementProposal,
    VerificationEvidence,
};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone)]
struct ProposalRecord {
    proposal: SelfImprovementProposal,
    apply: Option<ApplyReceipt>,
    verification: Option<VerificationEvidence>,
    rollback_candidate: Option<RollbackCandidate>,
    rollback: Option<RollbackReceipt>,
}

#[derive(Debug, Default)]
pub struct InMemoryImprovementStore {
    records: Mutex<BTreeMap<String, ProposalRecord>>,
}

impl InMemoryImprovementStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, proposal: SelfImprovementProposal) -> Result<(), ApplyBlock> {
        let mut records = recover_lock(&self.records);
        if records.contains_key(proposal.proposal_id()) {
            return Err(ApplyBlock::DuplicateProposal);
        }
        records.insert(
            proposal.proposal_id().to_owned(),
            ProposalRecord {
                proposal,
                apply: None,
                verification: None,
                rollback_candidate: None,
                rollback: None,
            },
        );
        Ok(())
    }

    pub fn proposal(&self, proposal_id: &str) -> Option<SelfImprovementProposal> {
        recover_lock(&self.records)
            .get(proposal_id)
            .map(|record| record.proposal.clone())
    }

    pub fn apply_receipt(&self, proposal_id: &str) -> Option<ApplyReceipt> {
        recover_lock(&self.records)
            .get(proposal_id)
            .and_then(|record| record.apply.clone())
    }

    pub fn verification(&self, proposal_id: &str) -> Option<VerificationEvidence> {
        recover_lock(&self.records)
            .get(proposal_id)
            .and_then(|record| record.verification.clone())
    }

    pub fn rollback_candidate(&self, proposal_id: &str) -> Option<RollbackCandidate> {
        recover_lock(&self.records)
            .get(proposal_id)
            .and_then(|record| record.rollback_candidate.clone())
    }

    pub fn rollback_receipt(&self, proposal_id: &str) -> Option<RollbackReceipt> {
        recover_lock(&self.records)
            .get(proposal_id)
            .and_then(|record| record.rollback.clone())
    }

    pub(crate) fn record_apply(&self, receipt: ApplyReceipt) -> Result<(), ApplyBlock> {
        let mut records = recover_lock(&self.records);
        let record = records
            .get_mut(receipt.proposal_id())
            .ok_or(ApplyBlock::ProposalNotFound)?;
        if record.apply.is_some() {
            return Err(ApplyBlock::AlreadyApplied);
        }
        record.apply = Some(receipt);
        Ok(())
    }

    pub(crate) fn record_verification(
        &self,
        proposal_id: &str,
        evidence: VerificationEvidence,
    ) -> Result<(), ApplyBlock> {
        let mut records = recover_lock(&self.records);
        let record = records
            .get_mut(proposal_id)
            .ok_or(ApplyBlock::ProposalNotFound)?;
        let checkpoint = record
            .apply
            .as_ref()
            .ok_or(ApplyBlock::NotApplied)?
            .checkpoint()
            .clone();
        record.rollback_candidate = (!evidence.passed()).then(|| RollbackCandidate {
            proposal_id: proposal_id.to_owned(),
            checkpoint,
            verify_failure_ref: evidence.evidence_ref().to_owned(),
        });
        record.verification = Some(evidence);
        Ok(())
    }

    pub(crate) fn record_rollback(
        &self,
        proposal_id: &str,
        receipt: RollbackReceipt,
    ) -> Result<(), ApplyBlock> {
        let mut records = recover_lock(&self.records);
        let record = records
            .get_mut(proposal_id)
            .ok_or(ApplyBlock::ProposalNotFound)?;
        if record.rollback.is_some() {
            return Err(ApplyBlock::AlreadyRolledBack);
        }
        record.rollback = Some(receipt);
        Ok(())
    }
}

fn recover_lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
