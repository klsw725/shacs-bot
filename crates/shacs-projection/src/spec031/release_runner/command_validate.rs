use super::command::parse_cargo_test_counts_strict;
use super::coverage_ids::required_command_ids;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind, Spec031ReleaseRunArtifacts, Spec031ReleaseTestCounts,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn validate_command_registry(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = PathBuf::from(&artifacts.evidence_root);
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for record in &artifacts.command_registry {
        if !ids.insert(record.id.clone()) {
            return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
        }
        require_unique(&mut paths, &record.stdout_path)?;
        require_unique(&mut paths, &record.stderr_path)?;
        validate_command_record(&root, record)?;
    }
    validate_current_worktree_commands(artifacts, &ids)?;
    Ok(())
}

fn validate_current_worktree_commands(
    artifacts: &Spec031ReleaseRunArtifacts,
    ids: &HashSet<String>,
) -> Result<(), Spec031ReleaseArtifactError> {
    if !artifacts
        .fixture_registry
        .iter()
        .any(|fixture| fixture == "fixtures/current-worktree.json")
    {
        return Ok(());
    }
    for &(_, command_id) in required_command_ids() {
        if !ids.contains(command_id) {
            return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
        }
    }
    Ok(())
}

fn validate_command_record(
    root: &Path,
    record: &Spec031ReleaseCommandRecord,
) -> Result<(), Spec031ReleaseArtifactError> {
    validate_command_paths(record)?;
    validate_command_metadata(record)?;
    let stdout_path = super::validate::require_safe_file(root, &record.stdout_path)?;
    super::validate::require_safe_file(root, &record.stderr_path)?;
    validate_recomputed_status(record)?;
    validate_cargo_test_evidence(record, &stdout_path)?;
    match record.status {
        Spec031ReleaseCommandStatus::Passed => Ok(()),
        Spec031ReleaseCommandStatus::Failed => Err(Spec031ReleaseArtifactError::CommandFailed),
        Spec031ReleaseCommandStatus::TimedOut => Err(Spec031ReleaseArtifactError::CommandTimedOut),
    }
}

fn validate_command_metadata(
    record: &Spec031ReleaseCommandRecord,
) -> Result<(), Spec031ReleaseArtifactError> {
    if record.cwd.is_empty() || record.argv.is_empty() {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    if record.gate == Spec031ReleaseGateKind::FocusedCargoTest
        && record.filter.as_deref().unwrap_or_default().is_empty()
    {
        return Err(Spec031ReleaseArtifactError::ZeroTestsRun);
    }
    Ok(())
}

fn validate_recomputed_status(
    record: &Spec031ReleaseCommandRecord,
) -> Result<(), Spec031ReleaseArtifactError> {
    let recomputed = match record.exit_code {
        Some(0) => Spec031ReleaseCommandStatus::Passed,
        Some(_) => Spec031ReleaseCommandStatus::Failed,
        None => Spec031ReleaseCommandStatus::TimedOut,
    };
    if record.status != recomputed {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    Ok(())
}

fn validate_command_paths(
    record: &Spec031ReleaseCommandRecord,
) -> Result<(), Spec031ReleaseArtifactError> {
    if record.stdout_path != format!("commands/{}.stdout", record.id)
        || record.stderr_path != format!("commands/{}.stderr", record.id)
    {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    Ok(())
}

fn validate_cargo_test_evidence(
    record: &Spec031ReleaseCommandRecord,
    stdout_path: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    if !is_cargo_test(&record.argv) {
        return Ok(());
    }
    let stdout = fs::read_to_string(stdout_path)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    let recomputed = parse_cargo_test_counts_strict(&stdout)?;
    validate_test_counts(&recomputed)?;
    if record.tests.as_ref() != Some(&recomputed) {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

fn is_cargo_test(argv: &[String]) -> bool {
    matches!(argv, [program, subcommand, ..] if program == "cargo" && subcommand == "test")
}

fn validate_test_counts(
    counts: &Spec031ReleaseTestCounts,
) -> Result<(), Spec031ReleaseArtifactError> {
    if counts.tests_run == 0 {
        return Err(Spec031ReleaseArtifactError::ZeroTestsRun);
    }
    if counts.tests_failed > 0 {
        return Err(Spec031ReleaseArtifactError::NonzeroTestsFailed);
    }
    Ok(())
}

fn require_unique(
    seen: &mut HashSet<String>,
    relative: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    if !seen.insert(relative.to_owned()) {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}
