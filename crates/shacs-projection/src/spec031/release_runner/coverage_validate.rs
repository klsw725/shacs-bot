use super::coverage::{
    Spec031CoverageEvidenceKind, Spec031CoverageRequirementKind, Spec031CoverageStatus,
    Spec031ExternalAuditStatus, Spec031ReleaseCoverageEntry,
};
use super::coverage_ids::required_command_ids;
use super::coverage_owner::owner_from_requirement;
use super::coverage_validate_artifact::validate_coverage_artifact;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
    Spec031ReleaseRunArtifacts,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(super) fn validate_coverage_matrix(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    if artifacts.command_registry.is_empty() {
        return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
    }
    let mut seen = HashSet::new();
    let expected_entries = super::coverage_matrix::coverage_entries(
        &PathBuf::from(&artifacts.evidence_root),
        "results.json",
        Spec031CoverageStatus::Blocked,
        &artifacts.command_registry,
        &artifacts.external_audits,
    )?;
    let required_ids: HashSet<String> = expected_entries
        .iter()
        .map(|entry| entry.requirement_id.clone())
        .collect();
    let expected_by_id: HashMap<String, Spec031ReleaseCoverageEntry> = expected_entries
        .into_iter()
        .map(|entry| (entry.requirement_id.clone(), entry))
        .collect();
    for entry in &artifacts.coverage_matrix {
        if !required_ids.contains(&entry.requirement_id) {
            return Err(Spec031ReleaseArtifactError::UnknownCoverageRequirement);
        }
        if !seen.insert(entry.requirement_id.clone()) {
            return Err(Spec031ReleaseArtifactError::DuplicateCoverageRequirement);
        }
        validate_coverage_entry(artifacts, entry)?;
        let expected = expected_by_id
            .get(&entry.requirement_id)
            .ok_or(Spec031ReleaseArtifactError::UnknownCoverageRequirement)?;
        if entry != expected {
            return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
        }
    }
    if !required_ids.is_subset(&seen) {
        return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
    }
    Ok(())
}

fn validate_coverage_entry(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if entry.source_locator.is_empty() || entry.owner.is_empty() || entry.reason.is_empty() {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    match entry.evidence_kind {
        Spec031CoverageEvidenceKind::ImplementedArtifact
        | Spec031CoverageEvidenceKind::CommandTranscript
        | Spec031CoverageEvidenceKind::CleanupReceipt
        | Spec031CoverageEvidenceKind::ExternalAudit => {}
        Spec031CoverageEvidenceKind::PlannedProse | Spec031CoverageEvidenceKind::Screenshot => {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
    }
    if entry.kind == Spec031CoverageRequirementKind::ExternalOwner
        && entry.evidence_kind != Spec031CoverageEvidenceKind::ExternalAudit
    {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    validate_coverage_artifact(artifacts, entry)?;
    validate_source_locator(&entry.source_locator, &entry.requirement_id)?;
    validate_command_coverage(artifacts, entry)?;
    validate_external_coverage_status(artifacts, entry)?;
    validate_requirement_command_dependency(artifacts, entry)?;
    validate_blocked_closure_status(artifacts, entry)?;
    Ok(())
}

fn validate_source_locator(
    locator: &str,
    requirement_id: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    let Some((relative, line)) = locator.rsplit_once(':') else {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    };
    let line = line
        .parse::<usize>()
        .map_err(|_| Spec031ReleaseArtifactError::InvalidCoverageEvidence)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or(Spec031ReleaseArtifactError::InvalidCoverageEvidence)?;
    let text = std::fs::read_to_string(root.join(relative))
        .map_err(|_| Spec031ReleaseArtifactError::InvalidCoverageEvidence)?;
    let Some(source_line) = text.lines().nth(line.saturating_sub(1)) else {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    };
    if source_line.trim().is_empty() || !source_line_matches(source_line, requirement_id) {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    Ok(())
}

fn source_line_matches(line: &str, requirement_id: &str) -> bool {
    if let Some(number) = requirement_id.strip_prefix("spec031:must:") {
        return line
            .trim_start()
            .starts_with(&format!("{}.", number_as_usize(number)));
    }
    if let Some(number) = requirement_id.strip_prefix("spec031:acceptance:") {
        return line
            .trim_start()
            .starts_with(&format!("{}.", number_as_usize(number)));
    }
    if let Some(number) = requirement_id.strip_prefix("spec031:closure:") {
        return line
            .trim_start()
            .starts_with(&format!("{}.", number_as_usize(number)));
    }
    if let Some(number) = requirement_id.strip_prefix("spec031:prd:") {
        return line.contains(&format!("PRD 00{}", number_as_usize(number) - 1));
    }
    if let Some(name) = requirement_id.strip_prefix("spec031:artifact:") {
        return line.contains("machine-readable")
            || line.contains("human-readable")
            || line.contains(name);
    }
    let lower = line.to_ascii_lowercase();
    if requirement_id.starts_with("spec031:command:") {
        return lower.contains("cargo")
            || lower.contains("smoke")
            || lower.contains("lifecycle")
            || lower.contains("api")
            || lower.contains("cli")
            || lower.contains("hung command");
    }
    requirement_id.starts_with("spec031:external:") && line.contains("Status:")
}

fn number_as_usize(value: &str) -> usize {
    value.parse::<usize>().map_or(0, |number| number)
}

fn validate_command_coverage(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if entry.kind != Spec031CoverageRequirementKind::RequiredCommand {
        return Ok(());
    }
    let expected = required_command_ids()
        .iter()
        .find(|(name, _)| entry.requirement_id == format!("spec031:command:{name}"))
        .ok_or(Spec031ReleaseArtifactError::UnknownCoverageRequirement)?;
    let Some(command_id) = &entry.command_result_id else {
        if entry.status == Spec031CoverageStatus::Blocked {
            return Ok(());
        }
        return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
    };
    if command_id != expected.1 {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    let command = find_command(&artifacts.command_registry, command_id)?;
    if entry.artifact != command.stdout_path {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    match (entry.status, command.status) {
        (Spec031CoverageStatus::Pass, Spec031ReleaseCommandStatus::Passed) => Ok(()),
        (Spec031CoverageStatus::Pass, _) => {
            Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence)
        }
        (Spec031CoverageStatus::Blocked, _) => Ok(()),
    }
}

fn validate_external_coverage_status(
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

fn validate_requirement_command_dependency(
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

fn validate_blocked_closure_status(
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

fn find_command<'a>(
    commands: &'a [Spec031ReleaseCommandRecord],
    id: &str,
) -> Result<&'a Spec031ReleaseCommandRecord, Spec031ReleaseArtifactError> {
    commands
        .iter()
        .find(|command| command.id == id)
        .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)
}
