use super::VerificationEvidence;
use crate::runtime::ExecutionSnapshot;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalImprovementProposal {
    proposal_id: String,
    target_ref: String,
    expected_target_digest: String,
    candidate_digest: String,
    candidate: Vec<u8>,
    snapshot: ExecutionSnapshot,
    #[serde(default)]
    confirmation_required: bool,
}

impl LocalImprovementProposal {
    pub fn from_json_artifacts(
        proposal_id: &str,
        target_ref: &str,
        expected_target_digest: &str,
        candidate_json: &str,
        snapshot_json: &str,
    ) -> Result<Self, LocalImprovementBlock> {
        if proposal_id.trim().is_empty() || !valid_digest(expected_target_digest) {
            return Err(LocalImprovementBlock::InvalidProposal);
        }
        let candidate_value: serde_json::Value = serde_json::from_str(candidate_json)
            .map_err(|_| LocalImprovementBlock::InvalidProposal)?;
        let candidate = serde_json::to_vec(&candidate_value)
            .map_err(|_| LocalImprovementBlock::InvalidProposal)?;
        let snapshot = ExecutionSnapshot::parse_json(snapshot_json)
            .map_err(|_| LocalImprovementBlock::InvalidSnapshot)?;
        Ok(Self {
            proposal_id: proposal_id.to_owned(),
            target_ref: target_ref.to_owned(),
            expected_target_digest: expected_target_digest.to_owned(),
            candidate_digest: digest(&candidate),
            candidate,
            snapshot,
            confirmation_required: false,
        })
    }

    pub fn requiring_confirmation(mut self) -> Self {
        self.confirmation_required = true;
        self
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
    pub fn candidate(&self) -> &[u8] {
        &self.candidate
    }
    pub const fn snapshot(&self) -> &ExecutionSnapshot {
        &self.snapshot
    }
    pub const fn confirmation_required(&self) -> bool {
        self.confirmation_required
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentGateEvidence {
    evidence_id: String,
    snapshot_digest: String,
    target_digest: String,
}

impl CurrentGateEvidence {
    pub fn new(evidence_id: &str, snapshot_digest: &str, target_digest: &str) -> Self {
        Self {
            evidence_id: evidence_id.to_owned(),
            snapshot_digest: snapshot_digest.to_owned(),
            target_digest: target_digest.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentSpec030Receipts {
    hook: CurrentGateEvidence,
    confirmation: CurrentGateEvidence,
    process: CurrentGateEvidence,
    sandbox: CurrentGateEvidence,
    credential: CurrentGateEvidence,
}

impl CurrentSpec030Receipts {
    pub fn try_new(
        hook: CurrentGateEvidence,
        confirmation: CurrentGateEvidence,
        process: CurrentGateEvidence,
        sandbox: CurrentGateEvidence,
        credential: Option<CurrentGateEvidence>,
    ) -> Result<Self, LocalImprovementBlock> {
        Ok(Self {
            hook,
            confirmation,
            process,
            sandbox,
            credential: credential.ok_or(LocalImprovementBlock::MissingGateEvidence)?,
        })
    }

    pub(crate) fn validate(
        &self,
        snapshot_digest: &str,
        target_digest: &str,
    ) -> Result<(), LocalImprovementBlock> {
        for evidence in [
            &self.hook,
            &self.confirmation,
            &self.process,
            &self.sandbox,
            &self.credential,
        ] {
            if evidence.evidence_id.trim().is_empty() {
                return Err(LocalImprovementBlock::MissingGateEvidence);
            }
            if evidence.snapshot_digest != snapshot_digest
                || evidence.target_digest != target_digest
            {
                return Err(LocalImprovementBlock::StaleGateEvidence);
            }
        }
        Ok(())
    }

    pub(crate) fn evidence_ids(&self) -> Vec<String> {
        [
            &self.hook,
            &self.confirmation,
            &self.process,
            &self.sandbox,
            &self.credential,
        ]
        .into_iter()
        .map(|evidence| evidence.evidence_id.clone())
        .collect()
    }

    pub(crate) fn differs_from(&self, previous: &[String]) -> bool {
        self.evidence_ids().iter().all(|id| !previous.contains(id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalApplyReceipt {
    pub(crate) owner_evidence_id: String,
    pub(crate) gate_evidence_ids: Vec<String>,
}

impl LocalApplyReceipt {
    pub fn owner_evidence_id(&self) -> &str {
        &self.owner_evidence_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRollbackReceipt {
    pub(crate) owner_evidence_id: String,
}

impl LocalRollbackReceipt {
    pub fn owner_evidence_id(&self) -> &str {
        &self.owner_evidence_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRollbackCandidate {
    pub(crate) verify_failure_id: String,
}

impl LocalRollbackCandidate {
    pub fn verify_failure_ref(&self) -> &str {
        &self.verify_failure_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalImprovementStatus {
    pub proposal: LocalImprovementProposal,
    pub applied: bool,
    pub verification_passed: Option<bool>,
    pub verification_evidence_id: Option<String>,
    pub rollback_candidate: Option<LocalRollbackCandidate>,
    pub rolled_back: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalImprovementBlock {
    Io,
    InvalidProposal,
    InvalidSnapshot,
    UnsafeTarget,
    ProposalNotFound,
    DuplicateProposal,
    AlreadyApplied,
    NotApplied,
    StaleTarget { expected: String, current: String },
    MissingGateEvidence,
    StaleGateEvidence,
    HookVeto,
    ConfirmationDenied,
    HeadlessConfirmationDenied,
    VerificationPassed,
    RollbackUnavailable,
    AlreadyRolledBack,
    TransactionInProgress,
    RecoveryRequired,
}

impl fmt::Display for LocalImprovementBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl Error for LocalImprovementBlock {}

pub trait LocalGateSource: Send + Sync {
    fn current_receipts(
        &self,
        proposal: &LocalImprovementProposal,
        target_digest: &str,
    ) -> Result<CurrentSpec030Receipts, LocalImprovementBlock>;
}

pub trait LocalImprovementVerifier: Send + Sync {
    fn verify(
        &self,
        proposal: &LocalImprovementProposal,
        current_target: &[u8],
    ) -> VerificationEvidence;
}

#[derive(Debug, Default)]
pub struct LocalDigestVerifier;

impl LocalImprovementVerifier for LocalDigestVerifier {
    fn verify(
        &self,
        proposal: &LocalImprovementProposal,
        current_target: &[u8],
    ) -> VerificationEvidence {
        let current = digest(current_target);
        VerificationEvidence::new(
            current == proposal.candidate_digest,
            digest(format!("local-verify:{}:{current}", proposal.proposal_id).as_bytes()),
        )
    }
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
