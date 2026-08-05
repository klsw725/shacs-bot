use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031ExternalAuditRow, Spec031ExternalAuditStatus,
    Spec031ExternalOwnerId, Spec031TypedEvidenceClass,
};
use super::external_audit_facts::{external_owner_facts, ExternalOwnerFactDescriptor};
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandStatus, Spec031ReleaseRunArtifacts,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) fn validate_external_audits(
    artifacts: &Spec031ReleaseRunArtifacts,
    repo_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = PathBuf::from(&artifacts.evidence_root);
    let mut owners = HashSet::new();
    for audit in &artifacts.external_audits {
        if !owners.insert(audit.owner) {
            return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
        }
        if audit.source_locator.is_empty()
            || audit.source_status_locator.is_empty()
            || audit.reason.is_empty()
        {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
        validate_external_audit_shape(audit)?;
        if artifact_hash(&root, &audit.artifact)? != audit.artifact_hash {
            return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
        }
        validate_external_audit_content(&root, audit)?;
        validate_owner_facts(artifacts, audit, repo_root)?;
    }
    for owner in required_external_owners() {
        if !owners.contains(&owner) {
            return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
        }
    }
    Ok(())
}

fn validate_external_audit_shape(
    audit: &Spec031ExternalAuditRow,
) -> Result<(), Spec031ReleaseArtifactError> {
    if audit.artifact_media_type != Spec031ArtifactMediaType::Markdown
        || audit.evidence_class != Spec031TypedEvidenceClass::ExternalAuditMarkdown
        || !audit.artifact.starts_with("external/")
        || !audit.artifact.ends_with("-read-audit.md")
    {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    if audit.source_locator != expected_source_locator(audit.owner)
        || audit.source_status_locator != expected_status_locator(audit.owner)
    {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    if audit.status == Spec031ExternalAuditStatus::Pass && audit.command_result_ids.is_empty() {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    let expected = expected_descriptor(audit.owner)?;
    if audit.status == Spec031ExternalAuditStatus::Blocked && !audit.command_result_ids.is_empty() {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    if audit.status == Spec031ExternalAuditStatus::Pass
        && audit.command_result_ids
            != expected
                .command_result_ids
                .iter()
                .map(|id| (*id).to_owned())
                .collect::<Vec<_>>()
    {
        return Err(Spec031ReleaseArtifactError::BlockedAsPass);
    }
    if audit.status == Spec031ExternalAuditStatus::Pass {
        let reason = audit.reason.to_ascii_lowercase();
        if reason.contains("open")
            || reason.contains("block")
            || reason.contains("absent")
            || reason.contains("remain")
        {
            return Err(Spec031ReleaseArtifactError::BlockedAsPass);
        }
    } else if audit.reason.to_ascii_lowercase().contains("complete") {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

fn validate_external_audit_content(
    root: &Path,
    audit: &Spec031ExternalAuditRow,
) -> Result<(), Spec031ReleaseArtifactError> {
    let path = super::validate::require_safe_file(root, &audit.artifact)?;
    let text = std::fs::read_to_string(path)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    if audit.status == Spec031ExternalAuditStatus::Blocked && text.contains("verdict: pass") {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    for required in [
        format!("owner: {}", owner_slug(audit.owner)),
        format!("status: {}", external_status_slug(audit.status)),
        format!("source_status_locator: {}", audit.source_status_locator),
        format!("source_locator: {}", audit.source_locator),
        format!("reason: {}", audit.reason),
    ] {
        if !text.contains(&required) {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
    }
    if audit.status == Spec031ExternalAuditStatus::Pass && text.contains("Status: Open") {
        return Err(Spec031ReleaseArtifactError::BlockedAsPass);
    }
    for artifact in &audit.implementation_artifacts {
        if !text.contains(artifact) {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
    }
    for command_id in &audit.command_result_ids {
        if !text.contains(command_id) {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
    }
    Ok(())
}

fn validate_owner_facts(
    artifacts: &Spec031ReleaseRunArtifacts,
    audit: &Spec031ExternalAuditRow,
    repo_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    if audit.status == Spec031ExternalAuditStatus::Blocked {
        return Ok(());
    }
    if audit.implementation_artifacts.is_empty() {
        return Err(Spec031ReleaseArtifactError::BlockedAsPass);
    }
    let uses_fixture_facts = audit
        .implementation_artifacts
        .iter()
        .all(|artifact| artifact.starts_with("fixtures/"));
    let success_fixture_run =
        artifacts.fixture_registry == ["fixtures/success-fixture/Cargo.toml".to_owned()];
    if uses_fixture_facts != success_fixture_run {
        return Err(Spec031ReleaseArtifactError::BlockedAsPass);
    }
    if uses_fixture_facts {
        if audit.implementation_artifacts != expected_fixture_fact_artifacts(audit.owner) {
            return Err(Spec031ReleaseArtifactError::BlockedAsPass);
        }
    } else if audit.implementation_artifacts != expected_fact_artifacts(audit.owner)? {
        return Err(Spec031ReleaseArtifactError::BlockedAsPass);
    }
    if !uses_fixture_facts && !source_status_is_complete(repo_root, audit)? {
        return Err(Spec031ReleaseArtifactError::BlockedAsPass);
    }
    for artifact in &audit.implementation_artifacts {
        if !fact_artifact_exists(repo_root, &artifacts.evidence_root, artifact) {
            return Err(Spec031ReleaseArtifactError::BlockedAsPass);
        }
    }
    for command_id in &audit.command_result_ids {
        let Some(command) = artifacts
            .command_registry
            .iter()
            .find(|record| record.id == *command_id)
        else {
            return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
        };
        if command.status != Spec031ReleaseCommandStatus::Passed {
            return Err(Spec031ReleaseArtifactError::BlockedAsPass);
        }
    }
    Ok(())
}

fn source_status_is_complete(
    root: &Path,
    audit: &Spec031ExternalAuditRow,
) -> Result<bool, Spec031ReleaseArtifactError> {
    let text = std::fs::read_to_string(root.join(&audit.source_locator))
        .map_err(|_| Spec031ReleaseArtifactError::BlockedAsPass)?;
    Ok(text
        .lines()
        .find(|line| line.trim_start().starts_with("Status:"))
        .is_some_and(|line| line.trim_start().starts_with("Status: Complete")))
}

fn fact_artifact_exists(root: &Path, evidence_root: &str, artifact: &str) -> bool {
    let (path, token) = match artifact.split_once('#') {
        Some(parts) => parts,
        None => (artifact, ""),
    };
    let full_path = if path.starts_with("fixtures/") {
        Path::new(evidence_root).join(path)
    } else {
        root.join(path)
    };
    let Ok(text) = std::fs::read_to_string(full_path) else {
        return false;
    };
    token.is_empty() || text.contains(token)
}

fn expected_descriptor(
    owner: Spec031ExternalOwnerId,
) -> Result<&'static ExternalOwnerFactDescriptor, Spec031ReleaseArtifactError> {
    external_owner_facts()
        .iter()
        .find(|descriptor| descriptor.owner == owner)
        .ok_or(Spec031ReleaseArtifactError::InvalidCoverageEvidence)
}

fn expected_fact_artifacts(
    owner: Spec031ExternalOwnerId,
) -> Result<Vec<String>, Spec031ReleaseArtifactError> {
    Ok(expected_descriptor(owner)?
        .fact_artifacts
        .iter()
        .map(|artifact| (*artifact).to_owned())
        .collect())
}

fn expected_fixture_fact_artifacts(owner: Spec031ExternalOwnerId) -> Vec<String> {
    vec![format!(
        "fixtures/success-fixture/external-owner-facts/{}.json",
        owner_slug(owner)
    )]
}

fn expected_source_locator(owner: Spec031ExternalOwnerId) -> &'static str {
    expected_descriptor(owner).map_or("", |descriptor| descriptor.source_locator)
}

fn expected_status_locator(owner: Spec031ExternalOwnerId) -> &'static str {
    expected_descriptor(owner).map_or("", |descriptor| descriptor.source_status_locator)
}

fn owner_slug(owner: Spec031ExternalOwnerId) -> &'static str {
    match owner {
        Spec031ExternalOwnerId::Spec029 => "spec029",
        Spec031ExternalOwnerId::Spec030 => "spec030",
        Spec031ExternalOwnerId::Spec032 => "spec032",
        Spec031ExternalOwnerId::Spec033 => "spec033",
        Spec031ExternalOwnerId::Spec034 => "spec034",
        Spec031ExternalOwnerId::Spec035 => "spec035",
    }
}

fn external_status_slug(status: Spec031ExternalAuditStatus) -> &'static str {
    match status {
        Spec031ExternalAuditStatus::Pass => "pass",
        Spec031ExternalAuditStatus::Blocked => "blocked",
    }
}

fn required_external_owners() -> [Spec031ExternalOwnerId; 6] {
    [
        Spec031ExternalOwnerId::Spec029,
        Spec031ExternalOwnerId::Spec030,
        Spec031ExternalOwnerId::Spec032,
        Spec031ExternalOwnerId::Spec033,
        Spec031ExternalOwnerId::Spec034,
        Spec031ExternalOwnerId::Spec035,
    ]
}
