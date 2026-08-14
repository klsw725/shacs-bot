use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSnapshotRef {
    snapshot_id: String,
    provenance_digest: String,
}

impl ExecutionSnapshotRef {
    pub fn new(snapshot_id: impl Into<String>, provenance_digest: impl Into<String>) -> Self {
        Self {
            snapshot_id: snapshot_id.into(),
            provenance_digest: provenance_digest.into(),
        }
    }

    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    pub fn provenance_digest(&self) -> &str {
        &self.provenance_digest
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfImprovementProposal {
    proposal_id: String,
    target_ref: String,
    expected_target_digest: String,
    candidate_digest: String,
    execution_snapshot: ExecutionSnapshotRef,
    candidate: Value,
}

impl SelfImprovementProposal {
    pub fn new(
        proposal_id: impl Into<String>,
        target_ref: impl Into<String>,
        expected_target_digest: impl Into<String>,
        candidate_digest: impl Into<String>,
        execution_snapshot: ExecutionSnapshotRef,
        candidate: Value,
    ) -> Self {
        Self {
            proposal_id: proposal_id.into(),
            target_ref: target_ref.into(),
            expected_target_digest: expected_target_digest.into(),
            candidate_digest: candidate_digest.into(),
            execution_snapshot,
            candidate,
        }
    }

    pub fn proposal_id(&self) -> &str {
        &self.proposal_id
    }

    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }

    pub fn expected_target_digest(&self) -> &str {
        &self.expected_target_digest
    }

    pub fn candidate_digest(&self) -> &str {
        &self.candidate_digest
    }

    pub fn execution_snapshot(&self) -> &ExecutionSnapshotRef {
        &self.execution_snapshot
    }

    pub fn candidate(&self) -> &Value {
        &self.candidate
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyGateDecision {
    Allowed,
    HookVeto,
    ConfirmationDenied,
    HeadlessConfirmationDenied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyGateReceipt {
    decision: ApplyGateDecision,
    evidence_ref: String,
}

impl ApplyGateReceipt {
    pub fn new(decision: ApplyGateDecision, evidence_ref: impl Into<String>) -> Self {
        Self {
            decision,
            evidence_ref: evidence_ref.into(),
        }
    }

    pub fn decision(&self) -> &ApplyGateDecision {
        &self.decision
    }

    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointReceipt {
    checkpoint_ref: String,
    target_digest_before: String,
    evidence_ref: String,
}

impl CheckpointReceipt {
    pub fn new(
        checkpoint_ref: impl Into<String>,
        target_digest_before: impl Into<String>,
        evidence_ref: impl Into<String>,
    ) -> Self {
        Self {
            checkpoint_ref: checkpoint_ref.into(),
            target_digest_before: target_digest_before.into(),
            evidence_ref: evidence_ref.into(),
        }
    }

    pub fn checkpoint_ref(&self) -> &str {
        &self.checkpoint_ref
    }

    pub fn target_digest_before(&self) -> &str {
        &self.target_digest_before
    }

    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerApplyEvidence {
    primitive_ref: String,
    evidence_ref: String,
}

impl OwnerApplyEvidence {
    pub fn new(primitive_ref: impl Into<String>, evidence_ref: impl Into<String>) -> Self {
        Self {
            primitive_ref: primitive_ref.into(),
            evidence_ref: evidence_ref.into(),
        }
    }

    pub fn primitive_ref(&self) -> &str {
        &self.primitive_ref
    }

    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRollbackEvidence {
    primitive_ref: String,
    evidence_ref: String,
}

impl OwnerRollbackEvidence {
    pub fn new(primitive_ref: impl Into<String>, evidence_ref: impl Into<String>) -> Self {
        Self {
            primitive_ref: primitive_ref.into(),
            evidence_ref: evidence_ref.into(),
        }
    }

    pub fn primitive_ref(&self) -> &str {
        &self.primitive_ref
    }

    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReceipt {
    proposal_id: String,
    gate: ApplyGateReceipt,
    checkpoint: CheckpointReceipt,
    owner_evidence: OwnerApplyEvidence,
}

impl ApplyReceipt {
    pub(crate) fn new(
        proposal_id: String,
        gate: ApplyGateReceipt,
        checkpoint: CheckpointReceipt,
        owner_evidence: OwnerApplyEvidence,
    ) -> Self {
        Self {
            proposal_id,
            gate,
            checkpoint,
            owner_evidence,
        }
    }

    pub fn proposal_id(&self) -> &str {
        &self.proposal_id
    }
    pub fn gate(&self) -> &ApplyGateReceipt {
        &self.gate
    }
    pub fn checkpoint(&self) -> &CheckpointReceipt {
        &self.checkpoint
    }
    pub fn owner_evidence(&self) -> &OwnerApplyEvidence {
        &self.owner_evidence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEvidence {
    passed: bool,
    evidence_ref: String,
}

impl VerificationEvidence {
    pub fn new(passed: bool, evidence_ref: impl Into<String>) -> Self {
        Self {
            passed,
            evidence_ref: evidence_ref.into(),
        }
    }

    pub fn passed(&self) -> bool {
        self.passed
    }
    pub fn evidence_ref(&self) -> &str {
        &self.evidence_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackCandidate {
    pub(crate) proposal_id: String,
    pub(crate) checkpoint: CheckpointReceipt,
    pub(crate) verify_failure_ref: String,
}

impl RollbackCandidate {
    pub fn proposal_id(&self) -> &str {
        &self.proposal_id
    }
    pub fn checkpoint(&self) -> &CheckpointReceipt {
        &self.checkpoint
    }
    pub fn verify_failure_ref(&self) -> &str {
        &self.verify_failure_ref
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackReceipt {
    gate: ApplyGateReceipt,
    owner_evidence: OwnerRollbackEvidence,
}

impl RollbackReceipt {
    pub(crate) fn new(gate: ApplyGateReceipt, owner_evidence: OwnerRollbackEvidence) -> Self {
        Self {
            gate,
            owner_evidence,
        }
    }

    pub fn gate(&self) -> &ApplyGateReceipt {
        &self.gate
    }
    pub fn owner_evidence(&self) -> &OwnerRollbackEvidence {
        &self.owner_evidence
    }
}
