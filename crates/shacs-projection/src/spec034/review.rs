use super::Spec034EvidenceRef;
use serde::{Deserialize, Serialize};

pub const SPEC034_REVIEW_SCHEMA: &str = "spec034.review_evidence.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034ReviewKind {
    Qa,
    Goal,
    Code,
    Security,
    Docs,
}

impl Spec034ReviewKind {
    pub const fn required() -> [Self; 5] {
        [Self::Qa, Self::Goal, Self::Code, Self::Security, Self::Docs]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034ReviewVerdict {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034ReviewRecord {
    pub kind: Spec034ReviewKind,
    pub verdict: Spec034ReviewVerdict,
    pub final_review: bool,
    pub evidence: Vec<Spec034EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034CargoCommandEvidence {
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub passed: bool,
    pub evidence: Spec034EvidenceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034ReviewEvidence {
    pub schema: String,
    pub reviews: Vec<Spec034ReviewRecord>,
    pub cargo_commands: Vec<Spec034CargoCommandEvidence>,
}
