use super::model::{Spec031ReleaseArtifactError as Error, Spec031ReleaseRunId};
use super::spec035_catalog::{catalog_at, postrun_ids, Requirement};
use super::spec035_classification_model::ClosureDocument;
use super::spec035_evidence_io::sha256;
use super::spec035_execution_contract::{GATES, INCIDENTS, OWNERS};
use super::spec035_execution_io::{decode, Evidence};
use super::spec035_execution_model::{Execution, ExecutionBinding, FileRef};
use super::spec035_execution_receipts::{validate_cleanup, validate_incident, validate_receipt};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) const MANIFEST: &str = ".omo/evidence/spec035/prd000-009/manifest.json";
pub(super) const BINDING: &str = "spec035-execution-binding.json";

pub(super) struct ValidatedExecution {
    pub(super) binding: ExecutionBinding,
    requirements: HashMap<String, FileRef>,
}

pub(super) struct Preflight {
    pub(super) binding: ExecutionBinding,
    requirements: HashMap<String, FileRef>,
}

impl Preflight {
    pub(super) fn proof_for(&self, row: &Requirement) -> Option<&FileRef> {
        self.requirements.get(&row.id)
    }

    pub(super) fn finish(
        mut self,
        supplements: HashMap<String, FileRef>,
    ) -> Result<ValidatedExecution, Error> {
        if supplements.keys().cloned().collect::<HashSet<_>>() != postrun_ids() {
            return Err(Error::UnmappedCoverageRequirement);
        }
        self.requirements.extend(supplements);
        Ok(ValidatedExecution {
            binding: self.binding,
            requirements: self.requirements,
        })
    }
}

pub(super) fn preflight(repo: &Path) -> Result<Preflight, Error> {
    let bytes = std::fs::read(super::validate::require_safe_file(repo, MANIFEST)?)
        .map_err(|_| Error::MissingRequiredArtifact)?;
    let ClosureDocument::CurrentExecution(execution) = decode(&bytes)? else {
        admit(repo)?;
        return Err(Error::BlockedExternalEvidence);
    };
    let (binding, requirements) = validate(repo, &execution, sha256(&bytes), true)?;
    Ok(Preflight {
        binding,
        requirements,
    })
}

pub(super) fn preflight_bound(repo: &Path, output: &Path) -> Result<Preflight, Error> {
    let binding: ExecutionBinding = super::validate::read_json(output, BINDING)?;
    let preflight = preflight(repo)?;
    if binding != preflight.binding {
        return Err(Error::ArtifactMismatch);
    }
    Ok(preflight)
}

impl ValidatedExecution {
    pub(super) fn proof_for(&self, row: &Requirement) -> Option<&FileRef> {
        if !row
            .closure_ids
            .iter()
            .all(|id| self.requirements.contains_key(&format!("spec035:{id}")))
        {
            return None;
        }
        self.requirements.get(&row.id)
    }
}

pub(super) fn admit(repo: &Path) -> Result<ValidatedExecution, Error> {
    let bytes = std::fs::read(super::validate::require_safe_file(repo, MANIFEST)?)
        .map_err(|_| Error::MissingRequiredArtifact)?;
    match decode(&bytes)? {
        ClosureDocument::CurrentExecution(execution) => {
            let (binding, requirements) = validate(repo, &execution, sha256(&bytes), false)?;
            Ok(ValidatedExecution {
                binding,
                requirements,
            })
        }
        ClosureDocument::ClassificationV2(classification) => {
            super::spec035_classification::validate_classification(repo, &classification)?;
            Err(Error::BlockedExternalEvidence)
        }
        ClosureDocument::ExecutionV1(_) => {
            super::spec035_evidence::validate_spec035_closure_evidence(repo)?;
            Err(Error::BlockedExternalEvidence)
        }
    }
}

#[cfg(test)]
pub(super) fn validate_bound(repo: &Path, output: &Path) -> Result<ValidatedExecution, Error> {
    let binding: ExecutionBinding = super::validate::read_json(output, BINDING)?;
    let execution = admit(repo)?;
    if binding != execution.binding {
        return Err(Error::ArtifactMismatch);
    }
    Ok(execution)
}

fn validate(
    repo: &Path,
    execution: &Execution,
    manifest_sha256: String,
    preflight: bool,
) -> Result<(ExecutionBinding, HashMap<String, FileRef>), Error> {
    Spec031ReleaseRunId::try_new(&execution.run_id)?;
    let root = repo.join(".omo/evidence/spec035/prd000-009");
    let evidence = Evidence::open(repo, &root, execution)?;
    super::spec035_execution_commands::validate_commands(&evidence)?;
    let expected = catalog_at(repo)?;
    let deferred = postrun_ids();
    let mut requirements = HashMap::new();
    for row in &execution.requirements {
        if !expected.contains(&row.authority)
            || requirements
                .insert(row.authority.id.clone(), row.receipt.clone())
                .is_some()
        {
            return Err(Error::InvalidCoverageEvidence);
        }
        evidence.source_locator(&row.authority.source_locator)?;
        super::spec035_execution_receipts::validate_receipt_phase(
            &evidence,
            &row.receipt,
            &row.authority.id,
            preflight && deferred.contains(&row.authority.id),
        )?;
    }
    if requirements.len() != expected.len() {
        return Err(Error::UnmappedCoverageRequirement);
    }
    if preflight {
        requirements.retain(|id, _| !deferred.contains(id));
    }
    validate_owners(&evidence)?;
    validate_gates(&evidence)?;
    validate_cleanup(&evidence)?;
    let mut incidents = HashSet::new();
    for incident in &execution.incidents {
        if !INCIDENTS.contains(&incident.id.as_str()) || !incidents.insert(incident.id.as_str()) {
            return Err(Error::InvalidCoverageEvidence);
        }
        validate_incident(&evidence, incident)?;
    }
    if incidents.len() != INCIDENTS.len() {
        return Err(Error::BlockedExternalEvidence);
    }
    Ok((
        ExecutionBinding {
            run_id: execution.run_id.clone(),
            manifest_sha256,
            source_sha256: execution.source.before.sha256.clone(),
        },
        requirements,
    ))
}

fn validate_owners(evidence: &Evidence<'_>) -> Result<(), Error> {
    let mut owners = HashSet::new();
    for owner in &evidence.execution.owners {
        let (_, expected) = OWNERS
            .iter()
            .find(|(id, _)| *id == owner.owner)
            .ok_or(Error::InvalidCoverageEvidence)?;
        if !owners.insert(owner.owner.as_str()) {
            return Err(Error::InvalidCoverageEvidence);
        }
        let required: HashSet<_> = expected
            .iter()
            .map(|fact| format!("spec{}:{fact}", owner.owner))
            .collect();
        let mut seen = HashSet::new();
        for fact in &owner.facts {
            if !required.contains(&fact.id) || !seen.insert(fact.id.clone()) {
                return Err(Error::InvalidCoverageEvidence);
            }
            validate_receipt(evidence, &fact.receipt, &fact.id)?;
        }
        if seen != required {
            return Err(Error::BlockedExternalEvidence);
        }
    }
    if owners.len() != OWNERS.len() {
        return Err(Error::BlockedExternalEvidence);
    }
    Ok(())
}

fn validate_gates(evidence: &Evidence<'_>) -> Result<(), Error> {
    let mut seen = HashSet::new();
    for gate in &evidence.execution.gates {
        let (_, kind) = GATES
            .iter()
            .find(|(id, _)| *id == gate.id)
            .ok_or(Error::InvalidCoverageEvidence)?;
        if !seen.insert(gate.id.as_str()) {
            return Err(Error::InvalidCoverageEvidence);
        }
        let commands = validate_receipt(evidence, &gate.receipt, &gate.id)?;
        if !commands.iter().any(|id| {
            evidence.execution.commands.iter().any(|command| {
                command.id == *id
                    && command.kind == *kind
                    && match gate.id.as_str() {
                        "build-cli" => super::spec035_execution_commands::pair(
                            &command.argv,
                            "-p",
                            "shacs-cli",
                        ),
                        "build-tui" => super::spec035_execution_commands::pair(
                            &command.argv,
                            "-p",
                            "shacs-tui",
                        ),
                        _ => true,
                    }
            })
        }) {
            return Err(Error::InvalidCommandEvidence);
        }
    }
    if seen.len() != GATES.len() {
        return Err(Error::UnmappedCoverageRequirement);
    }
    Ok(())
}
