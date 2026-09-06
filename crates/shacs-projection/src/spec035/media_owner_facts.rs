use super::{Spec035MediaDigest, Spec035MediaOpaqueRef, Spec035MediaOwnerUnavailableReason};
use crate::{
    CredentialStatusProjection, DataSurface, ResourceActivation, ResourceSource, ResourceTrust,
    SandboxStatusProjection, Spec031ExternalOwnerRef, Spec031Freshness, TraceStatus,
    TrustedCodeDisclosure,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035MediaOwnerFactsInput {
    pub freshness: Spec031Freshness,
    pub unavailable_reasons: Vec<Spec035MediaOwnerUnavailableReason>,
    pub facts: Vec<Spec035MediaOwnerFactInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec035MediaOwnerFactInput {
    AnalyzerSource {
        analyzer_ref: Spec031ExternalOwnerRef,
        source: ResourceSource,
        activation: ResourceActivation,
        trust: ResourceTrust,
        trusted_code_disclosure: TrustedCodeDisclosure,
    },
    Sandbox(SandboxStatusProjection),
    Credential(CredentialStatusProjection),
    Disclosure(Spec035MediaDisclosureFact),
    Snapshot {
        snapshot_ref: Spec035MediaOpaqueRef,
        provenance_digest: Spec035MediaDigest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaDisclosureFact {
    pub raw_content_possible: bool,
    pub surfaces: Vec<DataSurface>,
    pub trace_status: TraceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spec035MediaDisclosure {
    Recorded(Spec035MediaDisclosureFact),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaAnalyzerSourceFact {
    pub analyzer_ref: Spec031ExternalOwnerRef,
    pub source: ResourceSource,
    pub activation: ResourceActivation,
    pub trust: ResourceTrust,
    pub trusted_code_disclosure: TrustedCodeDisclosure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaSnapshotFact {
    pub snapshot_ref: Spec035MediaOpaqueRef,
    pub provenance_digest: Spec035MediaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaOwnerFacts {
    pub unavailable_reasons: Vec<Spec035MediaOwnerUnavailableReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analyzer_source: Option<Spec035MediaAnalyzerSourceFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxStatusProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialStatusProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Spec035MediaSnapshotFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Spec035MediaOwnerFactKind {
    AnalyzerSource,
    Sandbox,
    Credential,
    Disclosure,
    Snapshot,
}

impl Spec035MediaOwnerFactInput {
    pub(super) const fn kind(&self) -> Spec035MediaOwnerFactKind {
        match self {
            Self::AnalyzerSource { .. } => Spec035MediaOwnerFactKind::AnalyzerSource,
            Self::Sandbox(_) => Spec035MediaOwnerFactKind::Sandbox,
            Self::Credential(_) => Spec035MediaOwnerFactKind::Credential,
            Self::Disclosure(_) => Spec035MediaOwnerFactKind::Disclosure,
            Self::Snapshot { .. } => Spec035MediaOwnerFactKind::Snapshot,
        }
    }
}

impl Spec035MediaOwnerFactKind {
    pub(super) const REQUIRED: [Self; 5] = [
        Self::AnalyzerSource,
        Self::Sandbox,
        Self::Credential,
        Self::Disclosure,
        Self::Snapshot,
    ];
}
