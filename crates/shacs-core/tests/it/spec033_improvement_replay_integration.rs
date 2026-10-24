use serde_json::json;
use shacs_core::runtime::{
    ApplyGateDecision, ApplyGateReceipt, ApplyReceipt, CheckpointReceipt, CurrentImprovementGates,
    ExecutionSnapshotRef, ImprovementOwner, ImprovementVerifier, InMemoryImprovementStore,
    OwnerApplyEvidence, OwnerRollbackEvidence, SelfImprovementCoordinator, SelfImprovementProposal,
    VerificationEvidence,
};
use std::sync::{Arc, Mutex};

struct Owner {
    digest: Mutex<String>,
}

impl ImprovementOwner for Owner {
    fn current_digest(&self, _target_ref: &str) -> String {
        self.digest.lock().expect("digest lock").clone()
    }

    fn checkpoint(&self, proposal: &SelfImprovementProposal) -> Option<CheckpointReceipt> {
        Some(CheckpointReceipt::new(
            "checkpoint-1",
            proposal.expected_target_digest(),
            "checkpoint-evidence-1",
        ))
    }

    fn compare_and_apply(
        &self,
        proposal: &SelfImprovementProposal,
        _checkpoint: &CheckpointReceipt,
    ) -> Result<OwnerApplyEvidence, String> {
        *self.digest.lock().expect("digest lock") = proposal.candidate_digest().to_owned();
        Ok(OwnerApplyEvidence::new("owner-apply-1", "apply-evidence-1"))
    }

    fn rollback(
        &self,
        _proposal: &SelfImprovementProposal,
        checkpoint: &CheckpointReceipt,
    ) -> Result<OwnerRollbackEvidence, String> {
        *self.digest.lock().expect("digest lock") = checkpoint.target_digest_before().to_owned();
        Ok(OwnerRollbackEvidence::new(
            "owner-rollback-1",
            "rollback-evidence-1",
        ))
    }
}

struct Gates;

impl CurrentImprovementGates for Gates {
    fn evaluate(&self, _proposal: &SelfImprovementProposal) -> ApplyGateReceipt {
        ApplyGateReceipt::new(ApplyGateDecision::Allowed, "gate-evidence-1")
    }
}

struct Verifier;

impl ImprovementVerifier for Verifier {
    fn verify(&self, _receipt: &ApplyReceipt) -> VerificationEvidence {
        VerificationEvidence::new(false, "verify-evidence-1")
    }
}

#[test]
fn self_improvement_failure_becomes_candidate_before_read_only_replay() {
    // Given
    let owner = Arc::new(Owner {
        digest: Mutex::new("digest-old".to_owned()),
    });
    let coordinator = SelfImprovementCoordinator::new(
        Arc::new(InMemoryImprovementStore::new()),
        owner,
        Arc::new(Gates),
        Arc::new(Verifier),
    );
    let proposal = SelfImprovementProposal::new(
        "proposal-1",
        "skill:formatter",
        "digest-old",
        "digest-new",
        ExecutionSnapshotRef::new("snapshot-1", "snapshot-digest-1"),
        json!({"enabled":true}),
    );
    coordinator.propose(proposal).expect("proposal");
    coordinator.apply("proposal-1").expect("owner apply");

    // When
    let verification = coordinator.verify("proposal-1").expect("verification");
    let candidate = coordinator
        .rollback_candidate("proposal-1")
        .expect("rollback candidate");
    // Then
    assert!(!verification.passed());
    assert_eq!(candidate.verify_failure_ref(), "verify-evidence-1");
}
