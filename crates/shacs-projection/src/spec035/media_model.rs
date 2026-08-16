use super::media_adapter::normalize_owner_facts;
use super::media_validation::validate_media_input;
use super::*;
use crate::{Spec031ExternalOwnerRef, Spec031Freshness, Spec031SafeSummary};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaReason {
    pub code: Spec035MediaReasonCode,
    pub safe_summary: Spec031SafeSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaLineage {
    pub artifact_ref: Spec031ExternalOwnerRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analyzer_ref: Option<Spec031ExternalOwnerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_ref: Option<Spec035MediaOpaqueRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<Spec035MediaDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035MediaProjectionInput {
    pub state: Spec035MediaState,
    pub reason: Spec035MediaReason,
    pub lineage: Spec035MediaLineage,
    pub owner_facts: Spec035MediaOwnerFactsInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035MediaProjection {
    schema_version: u32,
    kind: Spec035MediaProjectionKind,
    state: Spec035MediaState,
    reason: Spec035MediaReason,
    lineage: Spec035MediaLineage,
    freshness: Spec031Freshness,
    disclosure: Spec035MediaDisclosure,
    owner_facts: Spec035MediaOwnerFacts,
}

impl Spec035MediaProjection {
    pub fn try_new(
        input: Spec035MediaProjectionInput,
    ) -> Result<Self, Spec035MediaValidationError> {
        validate_media_input(&input)?;
        let freshness = input.owner_facts.freshness;
        let (disclosure, owner_facts) = normalize_owner_facts(input.owner_facts);
        Ok(Self {
            schema_version: SPEC035_MEDIA_SCHEMA_VERSION,
            kind: Spec035MediaProjectionKind::MediaCapability,
            state: input.state,
            reason: input.reason,
            lineage: input.lineage,
            freshness,
            disclosure,
            owner_facts,
        })
    }

    pub const fn state(&self) -> Spec035MediaState {
        self.state
    }

    pub const fn freshness(&self) -> Spec031Freshness {
        self.freshness
    }

    pub const fn disclosure(&self) -> &Spec035MediaDisclosure {
        &self.disclosure
    }

    pub const fn owner_facts(&self) -> &Spec035MediaOwnerFacts {
        &self.owner_facts
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec035MediaParseError> {
        serde_json::from_str::<Spec035MediaProjectionWire>(input)
            .map_err(Spec035MediaParseError::from_serde)
            .and_then(Self::try_from_wire)
    }

    pub fn from_json_value(value: serde_json::Value) -> Result<Self, Spec035MediaParseError> {
        serde_json::from_value::<Spec035MediaProjectionWire>(value)
            .map_err(Spec035MediaParseError::from_serde)
            .and_then(Self::try_from_wire)
    }

    fn try_from_wire(wire: Spec035MediaProjectionWire) -> Result<Self, Spec035MediaParseError> {
        if wire.schema_version != SPEC035_MEDIA_SCHEMA_VERSION
            || wire.kind != Spec035MediaProjectionKind::MediaCapability
        {
            return Err(Spec035MediaParseError::invalid_schema());
        }
        Self::try_new(wire.into_input()).map_err(Spec035MediaParseError::from_validation)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec035MediaProjectionWire {
    schema_version: u32,
    kind: Spec035MediaProjectionKind,
    state: Spec035MediaState,
    reason: Spec035MediaReason,
    lineage: Spec035MediaLineage,
    freshness: Spec031Freshness,
    disclosure: Spec035MediaDisclosure,
    owner_facts: Spec035MediaOwnerFacts,
}

impl Spec035MediaProjectionWire {
    fn into_input(self) -> Spec035MediaProjectionInput {
        let mut facts = Vec::new();
        if let Some(source) = self.owner_facts.analyzer_source {
            facts.push(Spec035MediaOwnerFactInput::AnalyzerSource {
                analyzer_ref: source.analyzer_ref,
                source: source.source,
                activation: source.activation,
                trust: source.trust,
                trusted_code_disclosure: source.trusted_code_disclosure,
            });
        }
        if let Some(sandbox) = self.owner_facts.sandbox {
            facts.push(Spec035MediaOwnerFactInput::Sandbox(sandbox));
        }
        if let Some(credential) = self.owner_facts.credential {
            facts.push(Spec035MediaOwnerFactInput::Credential(credential));
        }
        if let Spec035MediaDisclosure::Recorded(disclosure) = self.disclosure {
            facts.push(Spec035MediaOwnerFactInput::Disclosure(disclosure));
        }
        if let Some(snapshot) = self.owner_facts.snapshot {
            facts.push(Spec035MediaOwnerFactInput::Snapshot {
                snapshot_ref: snapshot.snapshot_ref,
                provenance_digest: snapshot.provenance_digest,
            });
        }
        Spec035MediaProjectionInput {
            state: self.state,
            reason: self.reason,
            lineage: self.lineage,
            owner_facts: Spec035MediaOwnerFactsInput {
                freshness: self.freshness,
                unavailable_reasons: self.owner_facts.unavailable_reasons,
                facts,
            },
        }
    }
}
