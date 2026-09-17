use super::model::Spec031ReleaseArtifactError;
use super::spec035_classification::validate_classification;
use super::spec035_classification_model::ClosureDocument;
use super::spec035_evidence_commands::validate_commands;
use super::spec035_evidence_io::{evidence_error, read_json, require_inventory, safe_file, sha256};
use super::spec035_evidence_model::{
    Gate, Manifest, Requirement, SourceBindingDocument, WorkspaceDocument,
};
use super::spec035_evidence_nested::{validate_cleanup, validate_incidents, validate_owners};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

const ROOT: &str = ".omo/evidence/spec035/prd000-009";

pub(super) fn validate_spec035_closure_evidence(
    repo_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = repo_root.join(ROOT);
    let document: ClosureDocument = read_json(&root, "manifest.json")?;
    let manifest = match document {
        ClosureDocument::ExecutionV1(manifest) => manifest,
        ClosureDocument::ClassificationV2(classification) => {
            validate_classification(repo_root, &classification)?;
            return Err(Spec031ReleaseArtifactError::BlockedExternalEvidence);
        }
        ClosureDocument::CurrentExecution(_) => {
            return super::spec035_execution::admit(repo_root).map(|_| ());
        }
    };
    if manifest.summary.requirements != 40 {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    let inventory = validate_inventory(&root, &manifest.registries.artifact_hashes)?;
    validate_mandatory_inventory(&manifest, &inventory)?;
    validate_requirements(&root, &manifest.requirements, &inventory)?;
    validate_source_binding(repo_root, &root, &manifest, &inventory)?;
    let gates_blocked = validate_gates(&manifest.gates)?;
    let commands_blocked = validate_commands(&root, &manifest.registries.commands, &inventory)?;
    let owners_blocked = validate_owners(&root, &manifest.registries.external_owners, &inventory)?;
    let cleanup_blocked = validate_cleanup(&root, &manifest.registries.cleanup, &inventory)?;
    let incidents_blocked = validate_incidents(&root, &manifest.registries.incidents, &inventory)?;
    let workspace_blocked =
        validate_workspace(&root, &manifest.registries.workspace_failure_triage)?;
    let blocked = manifest.verdict != "PASS"
        || manifest.source_binding.drift
        || manifest.summary.passed != 40
        || manifest.summary.blocked != 0
        || manifest.summary.focused_tests_failed != 0
        || manifest.summary.workspace_tests_failed != 0
        || gates_blocked
        || commands_blocked
        || owners_blocked
        || cleanup_blocked
        || incidents_blocked
        || workspace_blocked;
    if blocked {
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    } else {
        Ok(())
    }
}

fn validate_mandatory_inventory(
    manifest: &Manifest,
    inventory: &HashMap<String, String>,
) -> Result<(), Spec031ReleaseArtifactError> {
    for path in [
        manifest.registries.commands.as_str(),
        manifest.registries.external_owners.as_str(),
        manifest.registries.visual_review.as_str(),
        manifest.registries.cleanup.as_str(),
        manifest.registries.workspace_failure_triage.as_str(),
        manifest.registries.correction_commands.as_str(),
        manifest.registries.incidents.as_str(),
        manifest.registries.correction_source_binding.as_str(),
        manifest.registries.corrected_closure_mapping.as_str(),
        manifest.source_binding.final_source_files.as_str(),
    ] {
        require_inventory(inventory, path)?;
    }
    Ok(())
}

fn validate_requirements(
    root: &Path,
    requirements: &[Requirement],
    inventory: &HashMap<String, String>,
) -> Result<(), Spec031ReleaseArtifactError> {
    if requirements.len() != 40 {
        return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
    }
    let mut ids = HashSet::new();
    for requirement in requirements {
        if requirement.status != "PASS"
            || requirement.evidence.is_empty()
            || !ids.insert(requirement.id.as_str())
        {
            return Err(Spec031ReleaseArtifactError::BlockedExternalEvidence);
        }
        for artifact in &requirement.evidence {
            safe_file(root, artifact)?;
            if !matches!(
                artifact.as_str(),
                "manifest.json" | "artifact-hashes.sha256"
            ) && !inventory.contains_key(artifact)
            {
                return Err(evidence_error(format!(
                    "requirement {} evidence is absent from SHA-256 inventory: {artifact}",
                    requirement.id
                )));
            }
        }
    }
    for prd in [8, 9] {
        for row in 1..=4 {
            if !ids.contains(format!("PRD{prd:03}-{row}").as_str()) {
                return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
            }
        }
    }
    Ok(())
}

fn validate_gates(gates: &[Gate]) -> Result<bool, Spec031ReleaseArtifactError> {
    let mut blocked = false;
    for required in [
        "external-owner-exact",
        "workspace-suite",
        "cleanup",
        "default-config-incident",
    ] {
        let gate = gates
            .iter()
            .find(|gate| gate.id == required)
            .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)?;
        blocked |= gate.status != "PASS";
    }
    Ok(blocked)
}

fn validate_inventory(
    root: &Path,
    relative: &str,
) -> Result<HashMap<String, String>, Spec031ReleaseArtifactError> {
    let text = fs::read_to_string(safe_file(root, relative)?)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    let mut inventory = HashMap::new();
    for line in text.lines() {
        let (digest, path) = line
            .split_once("  ")
            .ok_or(Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
        let relative = path
            .strip_prefix(&format!("{ROOT}/"))
            .ok_or(Spec031ReleaseArtifactError::InvalidArtifactPath)?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
        let bytes = fs::read(safe_file(root, relative)?)
            .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
        if sha256(&bytes) != digest {
            return Err(evidence_error(format!(
                "SHA-256 inventory digest mismatch for {relative}"
            )));
        }
        if inventory
            .insert(relative.to_owned(), digest.to_owned())
            .is_some()
        {
            return Err(evidence_error(format!(
                "duplicate SHA-256 inventory entry for {relative}"
            )));
        }
    }
    Ok(inventory)
}

fn validate_workspace(root: &Path, relative: &str) -> Result<bool, Spec031ReleaseArtifactError> {
    let workspace: WorkspaceDocument = read_json(root, relative)?;
    Ok(workspace.workspace_result != "PASS")
}

fn validate_source_binding(
    repo_root: &Path,
    root: &Path,
    manifest: &Manifest,
    inventory: &HashMap<String, String>,
) -> Result<(), Spec031ReleaseArtifactError> {
    let binding: SourceBindingDocument =
        read_json(root, &manifest.registries.correction_source_binding)?;
    if binding.drift || binding.pre_sha256 != binding.final_sha256 {
        return Err(evidence_error("source binding reports drift"));
    }
    let list_path = Path::new(&manifest.registries.correction_source_binding)
        .parent()
        .ok_or(Spec031ReleaseArtifactError::InvalidArtifactPath)?
        .join(&binding.final_source_list);
    let list_relative = list_path
        .to_str()
        .ok_or(Spec031ReleaseArtifactError::InvalidArtifactPath)?;
    if list_relative != manifest.source_binding.final_source_files {
        return Err(evidence_error(
            "source binding list path does not match manifest",
        ));
    }
    let bytes = fs::read(safe_file(root, list_relative)?)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    if sha256(&bytes) != binding.final_sha256
        || inventory.get(list_relative) != Some(&binding.final_sha256)
    {
        return Err(evidence_error("source binding list digest mismatch"));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| evidence_error("source binding list is not UTF-8"))?;
    let mut count = 0usize;
    for line in text.lines() {
        let (digest, relative) = line
            .split_once("  ")
            .ok_or_else(|| evidence_error("invalid source binding list entry"))?;
        let source = fs::read(safe_file(repo_root, relative)?)
            .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
        if sha256(&source) != digest {
            return Err(evidence_error(format!(
                "source binding digest mismatch for {relative}"
            )));
        }
        count += 1;
    }
    if count != binding.files {
        return Err(evidence_error(format!(
            "source binding file count mismatch: declared {}, observed {count}",
            binding.files
        )));
    }
    Ok(())
}
