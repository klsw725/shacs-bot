use super::command_validate::validate_command_registry;
use super::coverage::Spec031ReleaseCoverageEntry;
use super::coverage_audit_validate::validate_external_audits;
use super::coverage_validate::validate_coverage_matrix;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseRunArtifacts,
    SPEC031_RELEASE_RUNNER_SCHEMA,
};
use super::receipts::{
    validate_cleanup_receipts, validate_reproducibility_observations, validate_triage_receipts,
};
use super::REQUIRED_ARTIFACTS;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn validate_spec031_release_artifacts(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    validate_spec031_release_artifacts_with_repo_root(artifacts, &default_repo_root()?)
}

pub fn validate_spec031_release_artifacts_with_repo_root(
    artifacts: &Spec031ReleaseRunArtifacts,
    repo_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    if artifacts.schema != SPEC031_RELEASE_RUNNER_SCHEMA {
        return Err(Spec031ReleaseArtifactError::UnsupportedSchema);
    }
    if REQUIRED_ARTIFACTS
        .iter()
        .any(|required| !artifacts.manifest_files.iter().any(|file| file == required))
    {
        return Err(Spec031ReleaseArtifactError::MissingRequiredArtifact);
    }
    if artifacts.cleanup_registry.is_empty() {
        return Err(Spec031ReleaseArtifactError::MissingCleanupReceipt);
    }
    validate_evidence_root(artifacts)?;
    validate_command_registry(artifacts, repo_root)?;
    validate_external_audits(artifacts, repo_root)?;
    validate_cleanup_receipts(artifacts)?;
    validate_reproducibility_observations(artifacts)?;
    validate_coverage_matrix(artifacts)?;
    let triage_codes = validate_triage_receipts(artifacts)?;
    if triage_codes
        .iter()
        .any(|code| code == "blocked_external_evidence")
    {
        return Err(Spec031ReleaseArtifactError::BlockedExternalEvidence);
    }
    Ok(())
}

fn default_repo_root() -> Result<PathBuf, Spec031ReleaseArtifactError> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or(Spec031ReleaseArtifactError::InvalidCoverageEvidence)
}

fn validate_evidence_root(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = PathBuf::from(&artifacts.evidence_root);
    let mut seen_paths = HashSet::new();
    for file in &artifacts.manifest_files {
        require_unique_path(&mut seen_paths, file)?;
        require_safe_file(&root, file)?;
    }
    let manifest: Spec031ReleaseRunArtifacts = read_json(&root, "manifest.json")?;
    if manifest != *artifacts {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let coverage: Vec<Spec031ReleaseCoverageEntry> = read_json(&root, "coverage-matrix.json")?;
    if coverage != artifacts.coverage_matrix {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let results: Vec<Spec031ReleaseCommandRecord> = read_json(&root, "results.json")?;
    if results != artifacts.command_registry {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let commands: Vec<Spec031ReleaseCommandRecord> = read_json(&root, "command-registry.json")?;
    if commands != artifacts.command_registry {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let triage: Vec<String> = read_json(&root, "failure-triage.json")?;
    if triage != artifacts.failure_triage {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let observations: Vec<String> = read_json(&root, "reproducibility-observations.json")?;
    if observations != artifacts.reproducibility_observations {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let fixtures: Vec<String> = read_json(&root, "fixture-registry.json")?;
    if fixtures != artifacts.fixture_registry {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    let cleanup: Vec<String> = read_json(&root, "cleanup-registry.json")?;
    if cleanup != artifacts.cleanup_registry {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    validate_summary(&root, artifacts)?;
    for entry in &artifacts.fixture_registry {
        require_unique_path(&mut seen_paths, entry)?;
        require_safe_file(&root, entry)?;
    }
    for entry in &artifacts.cleanup_registry {
        require_unique_path(&mut seen_paths, entry)?;
        require_safe_file(&root, entry)?;
    }
    for entry in &artifacts.failure_triage {
        require_unique_path(&mut seen_paths, entry)?;
        require_safe_file(&root, entry)?;
    }
    for entry in &artifacts.reproducibility_observations {
        require_unique_path(&mut seen_paths, entry)?;
        require_safe_file(&root, entry)?;
    }
    for coverage in &artifacts.coverage_matrix {
        require_safe_file(&root, &coverage.artifact)?;
    }
    for audit in &artifacts.external_audits {
        require_safe_file(&root, &audit.artifact)?;
    }
    Ok(())
}

fn validate_summary(
    root: &Path,
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let path = require_safe_file(root, "summary.md")?;
    let text = fs::read_to_string(path)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    for required in [
        "## Commands",
        "## Cleanup Receipts",
        "## Failure Triage",
        "## Coverage",
        "## External Audits",
        "## Reproducibility Observations",
    ] {
        if !text.contains(required) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    for command in &artifacts.command_registry {
        for required in [
            command.id.as_str(),
            command.cwd.as_str(),
            command.stdout_path.as_str(),
            command.stderr_path.as_str(),
        ] {
            if !text.contains(required) {
                return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
            }
        }
    }
    for receipt in &artifacts.cleanup_registry {
        if !text.contains(receipt) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    for triage in &artifacts.failure_triage {
        if !text.contains(triage) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    for observation in &artifacts.reproducibility_observations {
        if !text.contains(observation) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    for audit in &artifacts.external_audits {
        if !text.contains(&audit.artifact) || !text.contains(&audit.source_status_locator) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    Ok(())
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(
    root: &Path,
    relative: &str,
) -> Result<T, Spec031ReleaseArtifactError> {
    let path = require_safe_file(root, relative)?;
    let bytes = fs::read(path).map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    serde_json::from_slice(&bytes).map_err(|_| Spec031ReleaseArtifactError::InvalidCommandEvidence)
}

pub(super) fn require_safe_file(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, Spec031ReleaseArtifactError> {
    let relative_path = Path::new(relative);
    let safe = !relative.is_empty()
        && relative_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    let full_path = root.join(relative_path);
    let metadata = fs::symlink_metadata(&full_path)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    let root = root
        .canonicalize()
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    let canonical = full_path
        .canonicalize()
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    if !canonical.starts_with(root) {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    Ok(full_path)
}

fn require_unique_path(
    seen: &mut HashSet<String>,
    relative: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    if !seen.insert(relative.to_owned()) {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}
