use super::super::coverage::{
    Spec031CoverageRequirementKind, Spec031CoverageStatus, Spec031ExternalAuditStatus,
    Spec031ReleaseCoverageEntry,
};
use super::super::coverage_owner::owner_from_requirement;
use super::super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandStatus, Spec031ReleaseRunArtifacts,
};
use super::find_command;

pub(super) fn validate_external_coverage_status(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if entry.kind != Spec031CoverageRequirementKind::ExternalOwner {
        return Ok(());
    }
    let owner = owner_from_requirement(&entry.requirement_id)?;
    let audit = artifacts
        .external_audits
        .iter()
        .find(|audit| audit.owner == owner)
        .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)?;
    let expected = match audit.status {
        Spec031ExternalAuditStatus::Pass => Spec031CoverageStatus::Pass,
        Spec031ExternalAuditStatus::Blocked => Spec031CoverageStatus::Blocked,
    };
    if entry.status != expected || entry.artifact != audit.artifact {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

pub(super) fn validate_requirement_command_dependency(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if matches!(
        entry.kind,
        Spec031CoverageRequirementKind::RequiredCommand
            | Spec031CoverageRequirementKind::ExternalOwner
    ) {
        return Ok(());
    }
    let Some(command_id) = entry.command_result_id.as_deref() else {
        return Ok(());
    };
    let command = find_command(&artifacts.command_registry, command_id)?;
    if command.status != Spec031ReleaseCommandStatus::Passed {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    Ok(())
}

pub(super) fn validate_blocked_closure_status(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if entry.kind != Spec031CoverageRequirementKind::ClosureEvidence {
        return Ok(());
    }
    let has_blocked_external = artifacts
        .external_audits
        .iter()
        .any(|audit| audit.status == Spec031ExternalAuditStatus::Blocked);
    if has_blocked_external && entry.status == Spec031CoverageStatus::Pass {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    Ok(())
}
