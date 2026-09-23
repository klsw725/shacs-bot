use super::model::{Spec031ReleaseArtifactError as Error, Spec031ReleaseRunArtifacts};
use super::spec035_catalog::{catalog_at, postrun_ids};
use super::spec035_classification_model::ClosureDocument;
use super::spec035_evidence_io::sha256;
use super::spec035_execution::{preflight, preflight_bound, MANIFEST};
use super::spec035_execution_io::{decode, read_bound, Evidence};
use super::spec035_execution_model::{FileRef, Receipt, Verdict};
use super::spec035_postrun_model::{Postrun, Seal, Spec035PreflightReport};
use std::collections::HashSet;
use std::path::Path;

pub(super) const SEAL: &str = "spec035-final-seal.json";

impl Spec031ReleaseRunArtifacts {
    pub fn inspect_spec035_preflight(repo: &Path) -> Result<Spec035PreflightReport, Error> {
        let preflight = preflight(repo)?;
        let catalog = catalog_at(repo)?;
        let mut blocked: Vec<_> = catalog
            .iter()
            .filter(|row| preflight.proof_for(row).is_none())
            .map(|row| row.id.clone())
            .collect();
        blocked.sort();
        let passed = catalog.len() - blocked.len();
        Ok(Spec035PreflightReport {
            status: "pending-final-audit",
            run_id: preflight.binding.run_id,
            manifest_sha256: preflight.binding.manifest_sha256,
            source_sha256: preflight.binding.source_sha256,
            requirements: catalog.len(),
            passed,
            blocked,
        })
    }

    pub fn finalize_spec035(&self, repo: &Path, postrun_path: &str) -> Result<String, Error> {
        let root = Path::new(&self.evidence_root);
        let bytes = std::fs::read(super::validate::require_safe_file(root, postrun_path)?)
            .map_err(|_| Error::MissingRequiredArtifact)?;
        let file = FileRef {
            path: postrun_path.to_owned(),
            sha256: sha256(&bytes),
        };
        validate_postrun(self, repo, &file)?;
        let seal = Seal {
            binding: preflight_bound(repo, root)?.binding,
            postrun: file,
            verdict: Verdict::Pass,
        };
        let writer =
            crate::release_evidence::EvidenceWriter::open_existing(root).map_err(|_| Error::Io)?;
        super::writer::write_json(&writer, SEAL, &seal)?;
        Ok(sha256(
            &serde_json::to_vec_pretty(&seal).map_err(|_| Error::Io)?,
        ))
    }
}

pub(super) fn validate_seal(
    artifacts: &Spec031ReleaseRunArtifacts,
    repo: &Path,
) -> Result<(), Error> {
    let root = Path::new(&artifacts.evidence_root);
    let seal: Seal = super::validate::read_json(root, SEAL)?;
    if seal.verdict != Verdict::Pass || seal.binding != preflight_bound(repo, root)?.binding {
        return Err(Error::ArtifactMismatch);
    }
    validate_postrun(artifacts, repo, &seal.postrun)
}

fn validate_postrun(
    artifacts: &Spec031ReleaseRunArtifacts,
    repo: &Path,
    file: &FileRef,
) -> Result<(), Error> {
    super::validate::validate_pending_artifacts(artifacts, repo)?;
    let root = Path::new(&artifacts.evidence_root);
    let postrun: Postrun = decode(&read_bound(root, file)?)?;
    let preflight = preflight_bound(repo, root)?;
    if postrun.binding != preflight.binding || postrun.runner_manifest.path != "manifest.json" {
        return Err(Error::ArtifactMismatch);
    }
    let original: Spec031ReleaseRunArtifacts =
        decode(&read_bound(root, &postrun.runner_manifest)?)?;
    if original != *artifacts {
        return Err(Error::ArtifactMismatch);
    }
    let expected = original_paths(artifacts);
    let mut seen = HashSet::new();
    for original in &postrun.originals {
        if !seen.insert(original.path.clone()) {
            return Err(Error::ArtifactMismatch);
        }
        read_bound(root, original)?;
    }
    if seen != expected || !postrun.originals.contains(&postrun.runner_manifest) {
        return Err(Error::MissingRequiredArtifact);
    }
    let exit = super::spec035_postrun_process::validate_processes(&postrun, artifacts, repo)?;
    let review = &postrun.review_command;
    let source_bytes = std::fs::read(super::validate::require_safe_file(repo, MANIFEST)?)
        .map_err(|_| Error::Io)?;
    let ClosureDocument::CurrentExecution(execution) = decode(&source_bytes)? else {
        return Err(Error::BlockedExternalEvidence);
    };
    let evidence_root = repo.join(".omo/evidence/spec035/prd000-009");
    let evidence = Evidence::open(repo, &evidence_root, &execution)?;
    let mut read_inputs = postrun.originals.clone();
    read_inputs.extend([
        postrun.exit.clone(),
        exit.stdout.clone(),
        exit.stderr.clone(),
        review.stdout.clone(),
        review.stderr.clone(),
    ]);
    let audit = validate_review_receipt(&evidence, root, &postrun.read_audit)?;
    if audit.subject != "spec035:postrun-read-audit" || !covers(&audit, &read_inputs) {
        return Err(Error::InvalidCoverageEvidence);
    }
    let mut subjects = HashSet::new();
    for row in &postrun.requirements {
        let receipt = validate_review_receipt(&evidence, root, &row.receipt)?;
        if receipt.subject != row.id
            || !subjects.insert(row.id.clone())
            || !covers(
                &receipt,
                &[
                    postrun.read_audit.clone(),
                    postrun.exit.clone(),
                    postrun.runner_manifest.clone(),
                ],
            )
        {
            return Err(Error::InvalidCoverageEvidence);
        }
    }
    if subjects != postrun_ids() {
        return Err(Error::UnmappedCoverageRequirement);
    }
    let mut supplements = HashSet::from([
        file.path.as_str(),
        postrun.exit.path.as_str(),
        postrun.read_audit.path.as_str(),
        review.stdout.path.as_str(),
        review.stderr.path.as_str(),
        exit.stdout.path.as_str(),
        exit.stderr.path.as_str(),
    ]);
    if supplements.len() != 7
        || supplements
            .iter()
            .any(|path| expected.contains(*path) || *path == SEAL)
    {
        return Err(Error::ArtifactMismatch);
    }
    for row in &postrun.requirements {
        if !supplements.insert(&row.receipt.path)
            || expected.contains(&row.receipt.path)
            || row.receipt.path == SEAL
        {
            return Err(Error::ArtifactMismatch);
        }
    }
    let execution = preflight.finish(
        postrun
            .requirements
            .into_iter()
            .map(|row| (row.id, row.receipt))
            .collect(),
    )?;
    if execution.binding != postrun.binding
        || catalog_at(repo)?
            .iter()
            .any(|row| execution.proof_for(row).is_none())
    {
        return Err(Error::UnmappedCoverageRequirement);
    }
    Ok(())
}

fn validate_review_receipt(
    evidence: &Evidence<'_>,
    root: &Path,
    file: &FileRef,
) -> Result<Receipt, Error> {
    let receipt: Receipt = decode(&read_bound(root, file)?)?;
    evidence.identity(&receipt.run_id, &receipt.source_sha256)?;
    if receipt.verdict != Verdict::Pass || receipt.checks.is_empty() {
        return Err(Error::BlockedExternalEvidence);
    }
    let mut checks = HashSet::new();
    for check in &receipt.checks {
        if check.verdict != Verdict::Pass
            || check.id.is_empty()
            || !checks.insert(&check.id)
            || check.commands != ["independent-read-audit"]
            || check.artifacts.is_empty()
        {
            return Err(Error::BlockedExternalEvidence);
        }
        evidence.source_locator(&check.producer)?;
        for artifact in &check.artifacts {
            read_bound(root, artifact)?;
        }
    }
    Ok(receipt)
}

fn covers(receipt: &Receipt, inputs: &[FileRef]) -> bool {
    inputs.iter().all(|input| {
        receipt
            .checks
            .iter()
            .any(|check| check.artifacts.contains(input))
    })
}

pub(super) fn original_paths(artifacts: &Spec031ReleaseRunArtifacts) -> HashSet<String> {
    artifacts
        .manifest_files
        .iter()
        .chain(&artifacts.fixture_registry)
        .chain(&artifacts.cleanup_registry)
        .chain(&artifacts.failure_triage)
        .chain(&artifacts.reproducibility_observations)
        .cloned()
        .chain(
            artifacts
                .command_registry
                .iter()
                .flat_map(|command| [command.stdout_path.clone(), command.stderr_path.clone()]),
        )
        .chain(
            artifacts
                .external_audits
                .iter()
                .map(|audit| audit.artifact.clone()),
        )
        .chain(
            artifacts
                .coverage_matrix
                .iter()
                .map(|row| row.artifact.clone()),
        )
        .collect()
}
