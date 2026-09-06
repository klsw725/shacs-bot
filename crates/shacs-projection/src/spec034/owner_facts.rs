use super::Spec034EvidenceRef;
use serde::{Deserialize, Serialize};

pub const SPEC034_OWNER_FACTS_SCHEMA: &str = "spec034.owner_facts.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034Availability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034UnavailableReason {
    NotRecorded,
    OwnerUnavailable,
    EvidenceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec034OwnerFactKind {
    CurrentOsAuthority,
    NetworkGuard,
    CredentialStatus,
    SandboxScope,
    TrustedAnalyzerSource,
    RawContentDisclosure,
    ExecutionSnapshot,
    ProjectionParity,
}

impl Spec034OwnerFactKind {
    pub const fn required() -> [Self; 8] {
        [
            Self::CurrentOsAuthority,
            Self::NetworkGuard,
            Self::CredentialStatus,
            Self::SandboxScope,
            Self::TrustedAnalyzerSource,
            Self::RawContentDisclosure,
            Self::ExecutionSnapshot,
            Self::ProjectionParity,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034OwnerFactRecord {
    pub kind: Spec034OwnerFactKind,
    pub availability: Spec034Availability,
    pub evidence: Vec<Spec034EvidenceRef>,
    pub unavailable_reason: Option<Spec034UnavailableReason>,
}

impl Spec034OwnerFactRecord {
    pub fn available(kind: Spec034OwnerFactKind, evidence: Vec<Spec034EvidenceRef>) -> Self {
        Self {
            kind,
            availability: Spec034Availability::Available,
            evidence,
            unavailable_reason: None,
        }
    }

    pub const fn unavailable(kind: Spec034OwnerFactKind, reason: Spec034UnavailableReason) -> Self {
        Self {
            kind,
            availability: Spec034Availability::Unavailable,
            evidence: Vec::new(),
            unavailable_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec034OwnerFacts {
    pub schema: String,
    pub facts: Vec<Spec034OwnerFactRecord>,
}
