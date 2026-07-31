use super::containment_permission_types::{
    BlockedExternalSurface, BlockedExternalSurfaceReason, RuntimeBoundaryKind,
};

pub(super) fn blocked_external_surface(
    kind: RuntimeBoundaryKind,
) -> Option<BlockedExternalSurface> {
    let owner = match kind {
        RuntimeBoundaryKind::AppProcess => "spec032_app_supervisor_lifecycle",
        RuntimeBoundaryKind::DependencyPreparation => "prd005_active_trust_provenance",
        RuntimeBoundaryKind::VerifiedEntrypoint => "prd005_verified_entrypoint_trust",
        RuntimeBoundaryKind::UserTurn
        | RuntimeBoundaryKind::Subagent
        | RuntimeBoundaryKind::McpStdio
        | RuntimeBoundaryKind::ExecTool
        | RuntimeBoundaryKind::PluginCommand
        | RuntimeBoundaryKind::PluginTool
        | RuntimeBoundaryKind::PluginHook
        | RuntimeBoundaryKind::DeferredBridge => return None,
    };
    Some(BlockedExternalSurface {
        status: "BLOCKED_EXTERNAL_SURFACE".to_owned(),
        owner: owner.to_owned(),
        evidence_reason: "external owner evidence is absent".to_owned(),
        reason: BlockedExternalSurfaceReason::MissingOwnerEvidence,
    })
}
