use super::ApplyGateDecision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyBlock {
    ProposalNotFound,
    DuplicateProposal,
    AlreadyApplied,
    StaleTarget { expected: String, current: String },
    Gate(ApplyGateDecision),
    MissingCheckpoint,
    OwnerRejected { current: String },
    NotApplied,
    VerificationPassed,
    RollbackUnavailable,
    AlreadyRolledBack,
}
