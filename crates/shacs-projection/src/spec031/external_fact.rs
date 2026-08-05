use super::external_owner::{
    ExternalOwnerSpec, ExternalOwnerStatus, Spec031ExternalCapability,
    Spec031ExternalOwnerReasonCode,
};
use super::external_refs::{
    Spec031ExternalOwnerReceiptRef, Spec031ExternalOwnerRef, Spec031ExternalReceiptRefKind,
    Spec031ExternalRefKind,
};
use super::redaction::{
    construction_error, Spec031ConstructionError, Spec031ConstructionViolation,
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalOwnerFact {
    owner: ExternalOwnerSpec,
    capability: Spec031ExternalCapability,
    opaque_ref: Spec031ExternalOwnerRef,
    status: ExternalOwnerStatus,
    reason_code: Spec031ExternalOwnerReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    receipt_ref: Option<Spec031ExternalOwnerReceiptRef>,
    #[serde(default)]
    stale: bool,
}

pub struct ExternalOwnerFactInput {
    pub owner: ExternalOwnerSpec,
    pub capability: Spec031ExternalCapability,
    pub opaque_ref: Spec031ExternalOwnerRef,
    pub status: ExternalOwnerStatus,
    pub reason_code: Spec031ExternalOwnerReasonCode,
    pub receipt_ref: Option<Spec031ExternalOwnerReceiptRef>,
    pub stale: bool,
}

impl ExternalOwnerFact {
    pub fn new(input: ExternalOwnerFactInput) -> Result<Self, Spec031ConstructionError> {
        ensure_consistent(
            input.owner,
            input.capability,
            &input.opaque_ref,
            input.receipt_ref.as_ref(),
        )?;
        Ok(Self {
            owner: input.owner,
            capability: input.capability,
            opaque_ref: input.opaque_ref,
            status: input.status,
            reason_code: input.reason_code,
            receipt_ref: input.receipt_ref,
            stale: input.stale,
        })
    }

    pub fn opaque_ref(&self) -> &Spec031ExternalOwnerRef {
        &self.opaque_ref
    }

    pub fn receipt_ref(&self) -> Option<&Spec031ExternalOwnerReceiptRef> {
        self.receipt_ref.as_ref()
    }

    pub(super) const fn owner(&self) -> ExternalOwnerSpec {
        self.owner
    }

    pub(super) const fn capability(&self) -> Spec031ExternalCapability {
        self.capability
    }

    pub(super) const fn status(&self) -> ExternalOwnerStatus {
        self.status
    }

    pub(super) const fn reason_code(&self) -> Spec031ExternalOwnerReasonCode {
        self.reason_code
    }

    pub(super) const fn stale(&self) -> bool {
        self.stale
    }
}

impl<'de> Deserialize<'de> for ExternalOwnerFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawExternalOwnerFact::deserialize(deserializer)?;
        Self::new(ExternalOwnerFactInput {
            owner: raw.owner,
            capability: raw.capability,
            opaque_ref: raw.opaque_ref,
            status: raw.status,
            reason_code: raw.reason_code,
            receipt_ref: raw.receipt_ref,
            stale: raw.stale,
        })
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct RawExternalOwnerFact {
    owner: ExternalOwnerSpec,
    capability: Spec031ExternalCapability,
    opaque_ref: Spec031ExternalOwnerRef,
    status: ExternalOwnerStatus,
    reason_code: Spec031ExternalOwnerReasonCode,
    #[serde(default)]
    receipt_ref: Option<Spec031ExternalOwnerReceiptRef>,
    #[serde(default)]
    stale: bool,
}

fn ensure_consistent(
    owner: ExternalOwnerSpec,
    capability: Spec031ExternalCapability,
    opaque_ref: &Spec031ExternalOwnerRef,
    receipt_ref: Option<&Spec031ExternalOwnerReceiptRef>,
) -> Result<(), Spec031ConstructionError> {
    let expected_ref = match (owner, capability) {
        (ExternalOwnerSpec::Spec032, Spec031ExternalCapability::App) => {
            Spec031ExternalRefKind::Spec032App
        }
        (ExternalOwnerSpec::Spec034, Spec031ExternalCapability::Media) => {
            Spec031ExternalRefKind::Spec034Media
        }
        (ExternalOwnerSpec::Spec032, Spec031ExternalCapability::Media)
        | (ExternalOwnerSpec::Spec034, Spec031ExternalCapability::App) => return mismatch(),
    };
    if opaque_ref.kind() != expected_ref {
        return mismatch();
    }
    let expected_receipt = match owner {
        ExternalOwnerSpec::Spec032 => Spec031ExternalReceiptRefKind::Spec032,
        ExternalOwnerSpec::Spec034 => Spec031ExternalReceiptRefKind::Spec034,
    };
    if receipt_ref.is_some_and(|receipt| receipt.kind() != expected_receipt) {
        return mismatch();
    }
    Ok(())
}

fn mismatch<T>() -> Result<T, Spec031ConstructionError> {
    Err(construction_error(
        "external_owner.fact",
        Spec031ConstructionViolation::CapabilityFamilyMismatch,
    ))
}
