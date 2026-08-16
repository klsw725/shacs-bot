use super::{Spec034EvidenceRef, Spec034OwnerFacts, Spec034PrimaryPrd, Spec034ReviewEvidence};
use serde::{Deserialize, Serialize};

pub const SPEC034_RELEASE_SCHEMA: &str = "spec034.release_evidence.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034SourceEvidence {
    pub head_oid: String,
    pub source_digest: String,
    pub worktree_clean: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034RequirementCoverage {
    pub requirement_id: String,
    pub primary_prd: Spec034PrimaryPrd,
    pub evidence: Vec<Spec034EvidenceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034BlockerKind {
    MissingOwnerFact,
    MissingRequirementEvidence,
    FailedReview,
    FailedCargoCommand,
    DirtyWorktree,
    SourceMismatch,
    ArtifactMismatch,
    CleanupIncomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034BlockerDisposition {
    Open,
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034BlockerRecord {
    pub kind: Spec034BlockerKind,
    pub disposition: Spec034BlockerDisposition,
    pub requirement_id: Option<String>,
    pub evidence: Vec<Spec034EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034ReleaseEvidence {
    pub schema: String,
    pub run_id: String,
    pub source: Spec034SourceEvidence,
    pub owner_facts: Spec034OwnerFacts,
    pub review_evidence: Spec034ReviewEvidence,
    pub requirements: Vec<Spec034RequirementCoverage>,
    pub blockers: Vec<Spec034BlockerRecord>,
    pub cleanup_complete: bool,
}
