use super::*;
use crate::{
    ProcessAdapterKind, Spec031ActionRef, Spec031ApprovalState, Spec031Count, Spec031Freshness,
    Spec031ObservedAtUnixMs, Spec031SafeSummary, Spec031SubjectRef, TrustedCodeDisclosure,
};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub const SPEC035_REVISED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "details", rename_all = "snake_case")]
pub enum Spec035DecisionProjection {
    DurableApproval(Spec035DurableApprovalProjection),
    EphemeralConfirmation(Spec035EphemeralConfirmationProjection),
    HookDenial(Spec035HookDenialProjection),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035DurableApprovalProjection {
    pub state: Spec031ApprovalState,
    pub approval_ref: Spec031ActionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<Spec031Count>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remembered_allow: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035EphemeralConfirmationProjection {
    pub state: Spec035EphemeralConfirmationState,
    pub call_ref: Spec031ActionRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035HookDenialProjection {
    pub state: Spec035HookDenialState,
    pub hook_ref: Spec031SubjectRef,
    pub call_ref: Spec031ActionRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035RuntimeControlProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<ProcessAdapterKind>,
    pub sandbox_status: Spec035SandboxRuntimeState,
    pub fallback: Spec035SandboxFallback,
    pub scope: Spec035AdapterScope,
    pub containment: Spec035ContainmentGuarantee,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035ResourceDisclosureProjection {
    pub resource_ref: Spec031SubjectRef,
    pub trusted_code_disclosure: TrustedCodeDisclosure,
    pub disclosure: Spec035DisclosureState,
    pub boundary: Spec035DisclosureBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", content = "value", rename_all = "snake_case")]
pub enum Spec035ObservedCount {
    Observed(Spec031Count),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035FinalDeliveryProjection {
    pub owner_surface: Spec035OwnerSurface,
    pub state: Spec035FinalDeliveryState,
    pub guarantee: Spec035DeliveryGuarantee,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035DeliveryProjection {
    pub progress: Vec<Spec035ProgressState>,
    pub accepted_count: Spec035ObservedCount,
    pub emitted_count: Spec035ObservedCount,
    pub coalesced_count: Spec035ObservedCount,
    pub dropped_count: Spec035ObservedCount,
    pub final_delivery: Spec035FinalDeliveryProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035RevisedProjection {
    schema_version: u32,
    surface_summary: Spec031SafeSummary,
    observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    freshness: Spec031Freshness,
    decisions: Vec<Spec035DecisionProjection>,
    runtime_controls: Vec<Spec035RuntimeControlProjection>,
    resources: Vec<Spec035ResourceDisclosureProjection>,
    delivery: Spec035DeliveryProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035RevisedProjectionInput {
    pub surface_summary: Spec031SafeSummary,
    pub observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    pub freshness: Spec031Freshness,
    pub decisions: Vec<Spec035DecisionProjection>,
    pub runtime_controls: Vec<Spec035RuntimeControlProjection>,
    pub resources: Vec<Spec035ResourceDisclosureProjection>,
    pub delivery: Spec035DeliveryProjection,
}

impl Spec035RevisedProjection {
    pub fn new(input: Spec035RevisedProjectionInput) -> Self {
        Self {
            schema_version: SPEC035_REVISED_SCHEMA_VERSION,
            surface_summary: input.surface_summary,
            observed_at_unix_ms: input.observed_at_unix_ms,
            freshness: input.freshness,
            decisions: input.decisions,
            runtime_controls: input.runtime_controls,
            resources: input.resources,
            delivery: input.delivery,
        }
    }

    pub fn decisions(&self) -> &[Spec035DecisionProjection] {
        &self.decisions
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec035RevisedParseError> {
        serde_json::from_str::<Spec035RevisedProjectionWire>(input)
            .map_err(Spec035RevisedParseError::from_serde)
            .and_then(Self::try_from_wire)
    }

    fn try_from_wire(wire: Spec035RevisedProjectionWire) -> Result<Self, Spec035RevisedParseError> {
        if wire.schema_version != SPEC035_REVISED_SCHEMA_VERSION {
            return Err(Spec035RevisedParseError::InvalidSchema);
        }
        Ok(Self::new(Spec035RevisedProjectionInput {
            surface_summary: wire.surface_summary,
            observed_at_unix_ms: wire.observed_at_unix_ms,
            freshness: wire.freshness,
            decisions: wire.decisions,
            runtime_controls: wire.runtime_controls,
            resources: wire.resources,
            delivery: wire.delivery,
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec035RevisedProjectionWire {
    schema_version: u32,
    surface_summary: Spec031SafeSummary,
    #[serde(default)]
    observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    freshness: Spec031Freshness,
    decisions: Vec<Spec035DecisionProjection>,
    runtime_controls: Vec<Spec035RuntimeControlProjection>,
    resources: Vec<Spec035ResourceDisclosureProjection>,
    delivery: Spec035DeliveryProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035RevisedParseError {
    InvalidJson,
    InvalidSchema,
}

impl Spec035RevisedParseError {
    fn from_serde(error: serde_json::Error) -> Self {
        if error.is_syntax() || error.is_eof() {
            Self::InvalidJson
        } else {
            Self::InvalidSchema
        }
    }
}

impl fmt::Display for Spec035RevisedParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson => formatter.write_str("invalid Spec035 revised JSON"),
            Self::InvalidSchema => formatter.write_str("invalid Spec035 revised schema"),
        }
    }
}

impl Error for Spec035RevisedParseError {}
