use serde_json::json;
use shacs_core::runtime::{
    ApplyBlock, ApplyGateDecision, ApplyGateReceipt, ApplyReceipt, CheckpointReceipt,
    ExecutionSnapshotRef, ImprovementOwner, ImprovementVerifier, InMemoryImprovementStore,
    OwnerApplyEvidence, OwnerRollbackEvidence, RollbackReceipt, SelfImprovementCoordinator,
    SelfImprovementProposal, VerificationEvidence,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

struct Owner {
    digest: Mutex<String>,
    checkpoints: AtomicUsize,
    applies: AtomicUsize,
    rollbacks: AtomicUsize,
    checkpoint_available: AtomicBool,
}

impl Owner {
    fn new(digest: &str) -> Self {
        Self {
            digest: Mutex::new(digest.to_owned()),
            checkpoints: AtomicUsize::new(0),
            applies: AtomicUsize::new(0),
            rollbacks: AtomicUsize::new(0),
            checkpoint_available: AtomicBool::new(true),
        }
    }
}

impl ImprovementOwner for Owner {
    fn current_digest(&self, _target_ref: &str) -> String {
        self.digest.lock().expect("digest lock").clone()
    }

    fn checkpoint(&self, proposal: &SelfImprovementProposal) -> Option<CheckpointReceipt> {
        self.checkpoints.fetch_add(1, Ordering::SeqCst);
        self.checkpoint_available.load(Ordering::SeqCst).then(|| {
            CheckpointReceipt::new(
                "checkpoint:1",
                proposal.expected_target_digest(),
                "evidence:cp",
            )
        })
    }

    fn compare_and_apply(
        &self,
        proposal: &SelfImprovementProposal,
        _checkpoint: &CheckpointReceipt,
    ) -> Result<OwnerApplyEvidence, String> {
        let mut digest = self.digest.lock().expect("digest lock");
        if digest.as_str() != proposal.expected_target_digest() {
            return Err(digest.clone());
        }
        self.applies.fetch_add(1, Ordering::SeqCst);
        *digest = proposal.candidate_digest().to_owned();
        Ok(OwnerApplyEvidence::new("owner:apply:1", "evidence:apply"))
    }

    fn rollback(
        &self,
        _proposal: &SelfImprovementProposal,
        checkpoint: &CheckpointReceipt,
    ) -> Result<OwnerRollbackEvidence, String> {
        self.rollbacks.fetch_add(1, Ordering::SeqCst);
        *self.digest.lock().expect("digest lock") = checkpoint.target_digest_before().to_owned();
        Ok(OwnerRollbackEvidence::new(
            "owner:rollback:1",
            "evidence:rollback",
        ))
    }
}

struct Gates {
    decision: Mutex<ApplyGateDecision>,
    calls: AtomicUsize,
}

impl Gates {
    fn new(decision: ApplyGateDecision) -> Self {
        Self {
            decision: Mutex::new(decision),
            calls: AtomicUsize::new(0),
        }
    }
}

impl shacs_core::runtime::CurrentImprovementGates for Gates {
    fn evaluate(&self, _proposal: &SelfImprovementProposal) -> ApplyGateReceipt {
        self.calls.fetch_add(1, Ordering::SeqCst);
        ApplyGateReceipt::new(
            self.decision.lock().expect("gate lock").clone(),
            "evidence:gate",
        )
    }
}

struct Verifier {
    passes: AtomicBool,
    calls: AtomicUsize,
}

impl ImprovementVerifier for Verifier {
    fn verify(&self, _receipt: &ApplyReceipt) -> VerificationEvidence {
        self.calls.fetch_add(1, Ordering::SeqCst);
        VerificationEvidence::new(self.passes.load(Ordering::SeqCst), "evidence:verify")
    }
}

fn proposal(id: &str, expected: &str) -> SelfImprovementProposal {
    SelfImprovementProposal::new(
        id,
        "skill:formatter",
        expected,
        "digest:new",
        ExecutionSnapshotRef::new("snapshot:1", "snapshot-digest:1"),
        json!({"enabled": true}),
    )
}

fn coordinator(
    owner: Arc<Owner>,
    gates: Arc<Gates>,
    verifier: Arc<Verifier>,
) -> SelfImprovementCoordinator<Owner, Gates, Verifier> {
    SelfImprovementCoordinator::new(
        Arc::new(InMemoryImprovementStore::new()),
        owner,
        gates,
        verifier,
    )
}

#[test]
fn stale_digest_has_zero_gate_checkpoint_and_mutation_calls() {
    let owner = Arc::new(Owner::new("digest:current"));
    let gates = Arc::new(Gates::new(ApplyGateDecision::Allowed));
    let runtime = coordinator(
        owner.clone(),
        gates.clone(),
        Arc::new(Verifier {
            passes: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        }),
    );
    runtime
        .propose(proposal("proposal:stale", "digest:old"))
        .expect("proposal");

    let result = runtime.apply("proposal:stale");

    assert!(matches!(result, Err(ApplyBlock::StaleTarget { .. })));
    assert_eq!(gates.calls.load(Ordering::SeqCst), 0);
    assert_eq!(owner.checkpoints.load(Ordering::SeqCst), 0);
    assert_eq!(owner.applies.load(Ordering::SeqCst), 0);
}

#[test]
fn current_gates_and_checkpoint_block_before_owner_mutation() {
    for decision in [
        ApplyGateDecision::HookVeto,
        ApplyGateDecision::ConfirmationDenied,
        ApplyGateDecision::HeadlessConfirmationDenied,
    ] {
        let owner = Arc::new(Owner::new("digest:old"));
        let runtime = coordinator(
            owner.clone(),
            Arc::new(Gates::new(decision.clone())),
            Arc::new(Verifier {
                passes: AtomicBool::new(true),
                calls: AtomicUsize::new(0),
            }),
        );
        runtime
            .propose(proposal("proposal:gated", "digest:old"))
            .expect("proposal");
        assert!(matches!(
            runtime.apply("proposal:gated"),
            Err(ApplyBlock::Gate(_))
        ));
        assert_eq!(owner.checkpoints.load(Ordering::SeqCst), 0);
        assert_eq!(owner.applies.load(Ordering::SeqCst), 0);
    }

    let owner = Arc::new(Owner::new("digest:old"));
    owner.checkpoint_available.store(false, Ordering::SeqCst);
    let runtime = coordinator(
        owner.clone(),
        Arc::new(Gates::new(ApplyGateDecision::Allowed)),
        Arc::new(Verifier {
            passes: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        }),
    );
    runtime
        .propose(proposal("proposal:no-checkpoint", "digest:old"))
        .expect("proposal");
    assert_eq!(
        runtime.apply("proposal:no-checkpoint"),
        Err(ApplyBlock::MissingCheckpoint)
    );
    assert_eq!(owner.applies.load(Ordering::SeqCst), 0);
}

#[test]
fn concurrent_duplicate_apply_mutates_owner_at_most_once() {
    let owner = Arc::new(Owner::new("digest:old"));
    let runtime = Arc::new(coordinator(
        owner.clone(),
        Arc::new(Gates::new(ApplyGateDecision::Allowed)),
        Arc::new(Verifier {
            passes: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        }),
    ));
    runtime
        .propose(proposal("proposal:race", "digest:old"))
        .expect("proposal");
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let runtime = runtime.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                runtime.apply("proposal:race")
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("apply thread"))
        .collect();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(owner.applies.load(Ordering::SeqCst), 1);
}

#[test]
fn verify_failure_only_creates_candidate_and_rollback_reenters_gates() {
    let owner = Arc::new(Owner::new("digest:old"));
    let gates = Arc::new(Gates::new(ApplyGateDecision::Allowed));
    let verifier = Arc::new(Verifier {
        passes: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    });
    let runtime = coordinator(owner.clone(), gates.clone(), verifier.clone());
    runtime
        .propose(proposal("proposal:verify", "digest:old"))
        .expect("proposal");
    runtime.apply("proposal:verify").expect("apply");

    let verification = runtime.verify("proposal:verify").expect("verify");

    assert!(!verification.passed());
    assert!(runtime.rollback_candidate("proposal:verify").is_some());
    assert_eq!(owner.rollbacks.load(Ordering::SeqCst), 0);
    *gates.decision.lock().expect("gate lock") = ApplyGateDecision::HookVeto;
    assert!(matches!(
        runtime.rollback("proposal:verify"),
        Err(ApplyBlock::Gate(_))
    ));
    assert_eq!(owner.rollbacks.load(Ordering::SeqCst), 0);
    *gates.decision.lock().expect("gate lock") = ApplyGateDecision::Allowed;
    let receipt: RollbackReceipt = runtime.rollback("proposal:verify").expect("rollback");
    assert_eq!(receipt.owner_evidence().evidence_ref(), "evidence:rollback");
    assert_eq!(owner.rollbacks.load(Ordering::SeqCst), 1);
    assert_eq!(gates.calls.load(Ordering::SeqCst), 3);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
}
