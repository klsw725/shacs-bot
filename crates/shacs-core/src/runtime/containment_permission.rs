#[path = "containment_permission_eval.rs"]
mod containment_permission_eval;
#[path = "containment_permission_external.rs"]
mod containment_permission_external;
#[path = "containment_permission_types.rs"]
mod containment_permission_types;

pub use containment_permission_eval::{
    containment_permission_proof_for_process_gate, evaluate_containment_permission,
};
pub use containment_permission_types::{
    BlockedExternalSurface, BlockedExternalSurfaceReason, ContainmentBoundaryRef,
    ContainmentComparisonOutcome, ContainmentEvidenceState, ContainmentPermissionError,
    ContainmentPermissionInput, ContainmentPermissionProof,
    ContainmentPermissionProofProjectionInput, ContainmentProofViolation,
    PermissionCeilingComparisonOutcome, PermissionCeilingProofInput, ProcessEnvelopeAdmission,
    RuntimeBoundaryKind, WorkspaceComparisonOutcome, WorkspaceScopeProof,
};
