use super::model::Spec031ReleaseArtifactError;
use super::spec035_catalog::{prd_closure_ids, PRDS, SPEC_ROOT};
use super::spec035_classification_model::{Classification, Owners, Status};
use super::spec035_evidence_io::{evidence_error, read_json, read_text, sha256};
use super::validate::require_safe_file as safe_file;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) const ROOT: &str = ".omo/evidence/spec035/prd000-009";

pub(super) fn validate_classification(
    repo: &Path,
    manifest: &Classification,
) -> Result<(), Spec031ReleaseArtifactError> {
    validate_catalog(manifest)?;
    if manifest.final_committed_seal
        || manifest.all_requirements_approved
        || manifest.provenance.observations_are_execution_binding
        || manifest.provenance.historical_source_binding_status != "STALE_NOT_REBOUND"
        || manifest.summary.commands_executed_for_this_correction != 0
    {
        return Err(evidence_error("classification is not an execution receipt"));
    }
    if manifest.provenance.classification_artifact_inventory
        != "../closure/f1-correction-artifacts.sha256"
    {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    let inventory = validate_inventory(repo)?;
    let owners_path = format!("{ROOT}/{}", manifest.registries.external_owners);
    for path in [format!("{ROOT}/manifest.json"), owners_path.clone()] {
        if !inventory.contains_key(&path) {
            return Err(evidence_error(format!(
                "classification inventory is missing {path}"
            )));
        }
    }
    let owners: Owners = read_json(repo, &owners_path)?;
    validate_owners(&owners, manifest)?;
    for paths in manifest.evidence_sets.values() {
        for path in paths {
            safe_file(repo, path)?;
        }
    }
    Ok(())
}

pub(super) fn validate_catalog(
    manifest: &Classification,
) -> Result<(), Spec031ReleaseArtifactError> {
    if manifest.authority_root != format!("{SPEC_ROOT}/prds/")
        || manifest.evidence_path_base != "repository root"
        || manifest.authorities.len() != PRDS.len()
        || manifest.requirements.len() != 45
    {
        return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
    }
    let mut ids = HashSet::new();
    for row in &manifest.requirements {
        if !ids.insert(row.id.as_str()) {
            return Err(Spec031ReleaseArtifactError::DuplicateCoverageRequirement);
        }
        for set in &row.evidence_sets {
            if !manifest.evidence_sets.contains_key(set) {
                return Err(Spec031ReleaseArtifactError::MissingRequiredArtifact);
            }
        }
    }
    for (prd, expected) in PRDS.iter().enumerate() {
        let authority_id = format!("PRD{prd:03}");
        let authority = manifest
            .authorities
            .get(&authority_id)
            .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)?;
        if authority.file != expected.file
            || authority.section != expected.section
            || authority.count != expected.count
        {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
        for (offset, id) in prd_closure_ids(prd).iter().enumerate() {
            let row = manifest
                .requirements
                .iter()
                .find(|row| row.id == *id)
                .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)?;
            if row.authority != authority_id
                || row.item != offset + 1
                || row.line != expected.first_line + offset
            {
                return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
            }
        }
    }
    let passed = manifest
        .requirements
        .iter()
        .filter(|row| row.status == Status::Pass)
        .count();
    let summary = &manifest.summary;
    if summary.requirements != 45
        || summary.implementation_closure_evidence != 37
        || summary.prd007_final_conditions != 8
        || summary.passed != passed
        || summary.blocked != 45 - passed
        || summary.unmapped != 0
        || manifest.verdict
            != if passed == 45 {
                Status::Pass
            } else {
                Status::Blocked
            }
    {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    let required_gates = [
        "B-SOURCE",
        "external-owner-exact",
        "workspace-suite",
        "cleanup",
        "default-config-incident",
        "release-original-artifacts",
        "final-source-qa-and-docs",
    ];
    let mut gates = HashSet::new();
    for gate in &manifest.gates {
        if !gates.insert(gate.id.as_str())
            || !required_gates.contains(&gate.id.as_str())
            || !matches!(gate.status.as_str(), "PASS" | "BLOCKED" | "UNRESOLVED")
        {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
    }
    if gates.len() != required_gates.len() {
        return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
    }
    Ok(())
}

fn validate_inventory(repo: &Path) -> Result<HashMap<String, String>, Spec031ReleaseArtifactError> {
    let text = read_text(
        repo,
        ".omo/evidence/spec035/closure/f1-correction-artifacts.sha256",
    )?;
    let mut inventory = HashMap::new();
    for line in text.lines() {
        let (digest, path) = line
            .split_once("  ")
            .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
        let bytes = std::fs::read(safe_file(repo, path)?)
            .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
        if digest != sha256(&bytes) {
            return Err(evidence_error(format!(
                "classification SHA-256 mismatch for {path}"
            )));
        }
        if inventory
            .insert(path.to_owned(), digest.to_owned())
            .is_some()
        {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    Ok(inventory)
}

fn validate_owners(
    owners: &Owners,
    manifest: &Classification,
) -> Result<(), Spec031ReleaseArtifactError> {
    let expected = ["029", "030", "031", "032", "033", "034"];
    let mut seen = HashSet::new();
    let mut historical_passed = 0u64;
    let mut historical_failed = 0u64;
    if owners.schema != "spec035.external_owner_classification.v2" {
        return Err(Spec031ReleaseArtifactError::UnsupportedSchema);
    }
    for owner in &owners.owners {
        if !expected.contains(&owner.spec.as_str())
            || !seen.insert(owner.spec.as_str())
            || owner.facts.is_empty()
        {
            return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
        }
        let status = if owner.facts.iter().all(|fact| fact.status == Status::Pass) {
            Status::Pass
        } else {
            Status::Blocked
        };
        if owner.status != status {
            return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
        }
        historical_passed = historical_passed
            .checked_add(owner.historical_passed)
            .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
        historical_failed = historical_failed
            .checked_add(owner.historical_failed)
            .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
    }
    let passed = owners
        .owners
        .iter()
        .filter(|owner| owner.status == Status::Pass)
        .count();
    if seen.len() != 6
        || owners.summary.owners != 6
        || manifest.summary.external_owners != 6
        || owners.summary.passed != passed
        || owners.summary.blocked != 6 - passed
        || manifest.summary.external_owners_passed != passed
        || manifest.summary.external_owners_blocked != 6 - passed
        || owners.summary.historical_tests_passed != historical_passed
        || owners.summary.historical_tests_failed != historical_failed
        || owners.summary.new_tests_executed != 0
        || owners.result
            != if passed == 6 {
                Status::Pass
            } else {
                Status::Blocked
            }
    {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    Ok(())
}
