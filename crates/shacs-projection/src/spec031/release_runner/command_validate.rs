use super::command::parse_cargo_test_counts_strict;
use super::coverage_ids::required_command_ids;
use super::current_commands::required_worktree_commands;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind, Spec031ReleaseRunArtifacts, Spec031ReleaseRunnerConfig,
    Spec031ReleaseRunnerMode, Spec031ReleaseTestCounts,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
#[path = "command_validate_test.rs"]
mod command_validate_test;

pub(super) fn validate_command_registry(
    artifacts: &Spec031ReleaseRunArtifacts,
    repo_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    let root = PathBuf::from(&artifacts.evidence_root);
    let current_worktree = is_current_worktree(artifacts);
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for record in &artifacts.command_registry {
        if !ids.insert(record.id.clone()) {
            return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
        }
        require_unique(&mut paths, &record.stdout_path)?;
        require_unique(&mut paths, &record.stderr_path)?;
        validate_command_record(&root, record, current_worktree)?;
    }
    validate_current_worktree_commands(artifacts, &ids, repo_root)?;
    Ok(())
}

fn validate_current_worktree_commands(
    artifacts: &Spec031ReleaseRunArtifacts,
    ids: &HashSet<String>,
    repo_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    if !is_current_worktree(artifacts) {
        return Ok(());
    }
    for &(_, command_id) in required_command_ids() {
        if !ids.contains(command_id) {
            return Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement);
        }
    }
    let repo_root = repo_root
        .canonicalize()
        .map_err(|_| Spec031ReleaseArtifactError::InvalidCommandEvidence)?;
    let config = Spec031ReleaseRunnerConfig {
        run_id: artifacts.run_id.clone(),
        evidence_root: PathBuf::from(&artifacts.evidence_root),
        repo_root,
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: std::time::Duration::ZERO,
    };
    for expected in required_worktree_commands(&config) {
        let record = artifacts
            .command_registry
            .iter()
            .find(|record| record.id == expected.id)
            .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)?;
        if record.gate != expected.gate
            || record.package != expected.package
            || record.filter != expected.filter
            || record.argv != expected.argv
            || record.cwd != expected.cwd.display().to_string()
        {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    Ok(())
}

fn is_current_worktree(artifacts: &Spec031ReleaseRunArtifacts) -> bool {
    artifacts
        .fixture_registry
        .iter()
        .any(|fixture| fixture == "fixtures/current-worktree.json")
}

fn validate_command_record(
    root: &Path,
    record: &Spec031ReleaseCommandRecord,
    validate_identity: bool,
) -> Result<(), Spec031ReleaseArtifactError> {
    validate_command_paths(record)?;
    validate_command_metadata(record)?;
    let stdout_path = super::validate::require_safe_file(root, &record.stdout_path)?;
    let stderr_path = super::validate::require_safe_file(root, &record.stderr_path)?;
    validate_recomputed_status(record)?;
    validate_cargo_test_evidence(record, &stdout_path, &stderr_path, validate_identity)?;
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
    stderr_path: &Path,
    validate_identity: bool,
) -> Result<(), Spec031ReleaseArtifactError> {
    if !is_cargo_test(&record.argv) {
        return Ok(());
    }
    if record.gate == Spec031ReleaseGateKind::FullCargoGate && record.tests.is_none() {
        return Ok(());
    }
    let stdout = fs::read_to_string(stdout_path)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    let stderr = fs::read_to_string(stderr_path)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    let recomputed = parse_cargo_test_counts_strict(&stdout)?;
    validate_test_counts(&recomputed)?;
    if validate_identity {
        validate_focused_test_identity(record, &stdout, &stderr)?;
    }
    if record.tests.as_ref() != Some(&recomputed) {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

fn validate_focused_test_identity(
    record: &Spec031ReleaseCommandRecord,
    stdout: &str,
    stderr: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    if record.gate != Spec031ReleaseGateKind::FocusedCargoTest {
        return Ok(());
    }
    let expected_targets = values_after(&record.argv, "--test");
    if expected_targets.is_empty()
        || expected_targets
            .iter()
            .any(|target| !stderr.contains(&format!("tests/{target}.rs")))
    {
        return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
    }
    if let Some(test_name) = exact_test_name(&record.argv) {
        let exact_pass = format!("test {test_name} ... ok");
        if !stdout.contains(&exact_pass) {
            return Err(Spec031ReleaseArtifactError::InvalidCommandEvidence);
        }
    }
    Ok(())
}

fn values_after<'a>(argv: &'a [String], flag: &str) -> Vec<&'a str> {
    argv.windows(2)
        .filter_map(|pair| (pair[0] == flag).then_some(pair[1].as_str()))
        .collect()
}

fn exact_test_name(argv: &[String]) -> Option<&str> {
    let separator = argv.iter().position(|arg| arg == "--")?;
    argv.get(separator.checked_sub(1)?).map(String::as_str)
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
