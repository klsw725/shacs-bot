use super::coverage::{
    Spec031ExternalAuditRow, Spec031ExternalAuditStatus, Spec031ExternalOwnerId,
};
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts, SPEC031_RELEASE_RUNNER_SCHEMA,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

pub(super) fn validate_triage_receipts(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<Vec<String>, Spec031ReleaseArtifactError> {
    let root = Path::new(&artifacts.evidence_root);
    let mut codes = Vec::new();
    for path in &artifacts.failure_triage {
        let receipt: TriageReceipt = super::validate::read_json(root, path)?;
        if receipt.schema != SPEC031_RELEASE_RUNNER_SCHEMA
            || receipt.run_id != artifacts.run_id.as_str()
        {
            return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
        }
        if receipt.code.is_empty() || receipt.message.is_empty() {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
        if receipt.code == "blocked_external_evidence" {
            validate_blocked_external_triage(
                artifacts,
                receipt.blocked_external_audits.as_deref(),
            )?;
        }
        codes.push(receipt.code);
    }
    Ok(codes)
}

pub(super) fn validate_reproducibility_observations(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = Path::new(&artifacts.evidence_root);
    for path in &artifacts.reproducibility_observations {
        let observation: ReproducibilityObservation = super::validate::read_json(root, path)?;
        if observation.schema != "spec031.reproducibility_observation.v1"
            || observation.run_id != artifacts.run_id.as_str()
            || observation.kind != "dirty_worktree"
            || observation.semantic_blocker
            || observation.message.is_empty()
        {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    Ok(())
}

fn validate_blocked_external_triage(
    artifacts: &Spec031ReleaseRunArtifacts,
    blockers: Option<&[BlockedExternalAuditReceipt]>,
) -> Result<(), Spec031ReleaseArtifactError> {
    let blocked_audits: Vec<&Spec031ExternalAuditRow> = artifacts
        .external_audits
        .iter()
        .filter(|audit| audit.status == Spec031ExternalAuditStatus::Blocked)
        .collect();
    let blockers = match (blocked_audits.is_empty(), blockers) {
        (true, None) => return Ok(()),
        (_, Some(blockers)) => blockers,
        (false, None) => return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence),
    };
    if blockers.len() != blocked_audits.len() {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    let mut seen = HashSet::new();
    for blocker in blockers {
        if !seen.insert(blocker.owner.as_str()) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
        let audit = blocked_audits
            .iter()
            .find(|audit| owner_slug(audit.owner) == blocker.owner)
            .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
        if blocker.artifact != audit.artifact
            || blocker.source_status_locator != audit.source_status_locator
            || blocker.reason != audit.reason
            || blocker.artifact_hash != audit.artifact_hash
        {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    Ok(())
}

pub(super) fn validate_cleanup_receipts(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = Path::new(&artifacts.evidence_root);
    let mut resources = HashSet::new();
    for fixture in &artifacts.fixture_registry {
        resources.insert(resource_id_from_fixture(fixture));
    }
    let mut cleaned_resources = HashSet::new();
    for receipt_path in &artifacts.cleanup_registry {
        let receipt: CleanupReceipt = super::validate::read_json(root, receipt_path)?;
        if receipt.schema != SPEC031_RELEASE_RUNNER_SCHEMA
            || receipt.run_id != artifacts.run_id.as_str()
            || !receipt.success
            || !matches!(receipt.status.as_str(), "cleaned" | "verified")
        {
            return Err(Spec031ReleaseArtifactError::MissingCleanupReceipt);
        }
        if !resources.contains(receipt.resource_id.as_str()) {
            return Err(Spec031ReleaseArtifactError::MissingCleanupReceipt);
        }
        super::validate::require_safe_file(root, &receipt.check_artifact)?;
        cleaned_resources.insert(receipt.resource_id);
    }
    if cleaned_resources != resources {
        return Err(Spec031ReleaseArtifactError::MissingCleanupReceipt);
    }
    Ok(())
}

fn resource_id_from_fixture(fixture: &str) -> String {
    if fixture == "fixtures/current-worktree.json" {
        return "current-worktree".to_owned();
    }
    fixture
        .strip_suffix("/Cargo.toml")
        .unwrap_or(fixture)
        .to_owned()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanupReceipt {
    schema: String,
    run_id: String,
    status: String,
    success: bool,
    resource_id: String,
    check_artifact: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriageReceipt {
    schema: String,
    run_id: String,
    code: String,
    message: String,
    blocked_external_audits: Option<Vec<BlockedExternalAuditReceipt>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReproducibilityObservation {
    schema: String,
    run_id: String,
    kind: String,
    semantic_blocker: bool,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockedExternalAuditReceipt {
    owner: String,
    artifact: String,
    source_status_locator: String,
    reason: String,
    artifact_hash: String,
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
