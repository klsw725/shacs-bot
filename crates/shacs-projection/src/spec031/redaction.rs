use super::{Spec031Lineage, Spec031Reason};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031ConstructionViolation {
    UnsafeOpaqueRef,
    UnsafeSummary,
    CapabilityFamilyMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031ConstructionError {
    pub(super) field: &'static str,
    pub(super) kind: Spec031ConstructionViolation,
}

impl Spec031ConstructionError {
    pub const fn field(&self) -> &'static str {
        self.field
    }

    pub const fn kind(&self) -> Spec031ConstructionViolation {
        self.kind
    }
}

impl fmt::Display for Spec031ConstructionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsafe Spec031 projection input in {}",
            self.field
        )
    }
}

impl Error for Spec031ConstructionError {}

pub(super) fn construction_error(
    field: &'static str,
    kind: Spec031ConstructionViolation,
) -> Spec031ConstructionError {
    Spec031ConstructionError { field, kind }
}

pub(super) fn sanitize_reason(
    reason: Spec031Reason,
) -> Result<Spec031Reason, Spec031ConstructionError> {
    Ok(reason)
}

pub(super) fn sanitize_lineage(
    lineage: Spec031Lineage,
) -> Result<Spec031Lineage, Spec031ConstructionError> {
    Ok(lineage)
}
