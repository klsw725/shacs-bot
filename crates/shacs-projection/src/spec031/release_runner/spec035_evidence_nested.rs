use super::command::parse_cargo_test_counts_strict;
use super::model::Spec031ReleaseArtifactError;
use super::spec035_evidence_io::{evidence_error, read_json, read_text, require_inventory};
use super::spec035_evidence_model::{CleanupRegistry, ExternalOwnerRegistry, IncidentRegistry};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) fn validate_owners(
    root: &Path,
    relative: &str,
    inventory: &HashMap<String, String>,
) -> Result<bool, Spec031ReleaseArtifactError> {
    require_inventory(inventory, relative)?;
    let registry: ExternalOwnerRegistry = read_json(root, relative)?;
    if registry.schema != "spec035.external_owner_audits.v1" || registry.owners.is_empty() {
        return Err(evidence_error("invalid external owner registry schema"));
    }
    let mut owners = HashSet::new();
    for owner in registry.owners {
        if !owners.insert(owner.spec.clone()) || owner.tests.is_empty() {
            return Err(evidence_error(format!(
                "invalid external owner {}",
                owner.spec
            )));
        }
        if owner.status != "PASS" {
            return Err(evidence_error(format!(
                "owner {} status {}",
                owner.spec, owner.status
            )));
        }
        let mut tests_run = 0u64;
        let mut tests_failed = 0u64;
        for transcript in owner.tests {
            require_inventory(inventory, &transcript)?;
            let text = read_text(root, &transcript)?;
            let counts = parse_cargo_test_counts_strict(&text).map_err(|_| {
                evidence_error(format!("owner {} has invalid test transcript", owner.spec))
            })?;
            tests_run += counts.tests_run;
            tests_failed += counts.tests_failed;
        }
        if owner.failed != 0 || tests_failed != 0 {
            return Err(Spec031ReleaseArtifactError::BlockedExternalEvidence);
        }
        if owner.passed == 0
            || tests_run != owner.passed + owner.failed
            || tests_failed != owner.failed
        {
            return Err(evidence_error(format!(
                "owner {} test count mismatch",
                owner.spec
            )));
        }
    }
    Ok(registry.result != "PASS")
}

pub(super) fn validate_cleanup(
    root: &Path,
    relative: &str,
    inventory: &HashMap<String, String>,
) -> Result<bool, Spec031ReleaseArtifactError> {
    require_inventory(inventory, relative)?;
    let registry: CleanupRegistry = read_json(root, relative)?;
    if registry.schema != "spec035.cleanup_registry.v1" {
        return Err(evidence_error("invalid cleanup registry schema"));
    }
    for item in registry.items {
        if !matches!(item.disposition.as_str(), "removed" | "stopped" | "absent") {
            return Err(evidence_error(format!(
                "cleanup {} disposition {}",
                item.id, item.disposition
            )));
        }
        require_inventory(inventory, &item.absence_proof)?;
    }
    Ok(registry.result != "PASS")
}

pub(super) fn validate_incidents(
    root: &Path,
    relative: &str,
    inventory: &HashMap<String, String>,
) -> Result<bool, Spec031ReleaseArtifactError> {
    require_inventory(inventory, relative)?;
    let registry: IncidentRegistry = read_json(root, relative)?;
    if registry.schema != "spec035.todo10_incidents.v1" {
        return Err(evidence_error("invalid incident registry schema"));
    }
    for incident in registry.incidents {
        if !matches!(incident.status.as_str(), "PASS" | "RESOLVED") {
            return Err(evidence_error(format!(
                "incident {} status {}",
                incident.id, incident.status
            )));
        }
    }
    Ok(registry.result != "PASS")
}
