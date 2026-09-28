use super::model::{Spec031ReleaseArtifactError as Error, Spec031ReleaseRunArtifacts};
use super::spec035_evidence_io::sha256;
use super::spec035_execution_io::{decode, read_bound};
use super::spec035_execution_model::CommandKind;
use super::spec035_postrun_model::{ExitObservation, Postrun};
use std::collections::HashSet;
use std::path::Path;

#[cfg(test)]
#[path = "spec035_postrun_process_test.rs"]
mod tests;

pub(super) fn validate_processes(
    postrun: &Postrun,
    artifacts: &Spec031ReleaseRunArtifacts,
    repo: &Path,
) -> Result<ExitObservation, Error> {
    let root = Path::new(&artifacts.evidence_root);
    let exit: ExitObservation = decode(&read_bound(root, &postrun.exit)?)?;
    if exit.binding != postrun.binding
        || exit.runner_manifest != postrun.runner_manifest
        || exit.originals_sha256
            != sha256(&serde_json::to_vec(&postrun.originals).map_err(|_| Error::Io)?)
        || exit.exit_code != 0
        || !exit.reaped
    {
        return Err(Error::CommandFailed);
    }
    let repo_path = repo.display().to_string();
    let arguments = runner_arguments(&exit.argv).ok_or(Error::InvalidCommandEvidence)?;
    let expected = [
        ("--run-id", artifacts.run_id.as_str()),
        ("--mode", "current-worktree"),
        ("--repo-root", repo_path.as_str()),
        ("--evidence-root", artifacts.evidence_root.as_str()),
    ];
    let mut flags = HashSet::new();
    let mut pairs = arguments.chunks_exact(2);
    for pair in &mut pairs {
        if !flags.insert(pair[0].as_str())
            || !(expected.contains(&(pair[0].as_str(), pair[1].as_str()))
                || pair == ["--phase", "run"])
        {
            return Err(Error::InvalidCommandEvidence);
        }
    }
    if !pairs.remainder().is_empty() || expected.iter().any(|(flag, _)| !flags.contains(flag)) {
        return Err(Error::InvalidCommandEvidence);
    }
    if read_bound(root, &exit.stdout)? != b"pending-final-audit\n" {
        return Err(Error::InvalidCommandEvidence);
    }
    read_bound(root, &exit.stderr)?;
    let review = &postrun.review_command;
    let program = review
        .argv
        .first()
        .filter(|arg| !arg.trim().is_empty())
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(|name| name.to_str());
    let cargo_arguments = review
        .argv
        .split(|arg| arg == "--")
        .next()
        .unwrap_or_default();
    let cargo_runner = program == Some("cargo")
        && (cargo_arguments
            .windows(2)
            .any(|pair| pair == ["--bin", "spec031-release-runner"])
            || cargo_arguments
                .iter()
                .any(|arg| arg == "--bin=spec031-release-runner"));
    if review.id != "independent-read-audit"
        || review.kind != CommandKind::Review
        || review.run_id != postrun.binding.run_id
        || review.source_sha256 != postrun.binding.source_sha256
        || review.exit_code != 0
        || matches!(program, None | Some("spec031-release-runner"))
        || cargo_runner
        || review.tests.is_some()
        || review.package.is_some()
        || review.filter.is_some()
        || review.test_accounting.is_some()
        || read_bound(root, &review.stdout)?.is_empty()
    {
        return Err(Error::InvalidCommandEvidence);
    }
    read_bound(root, &review.stderr)?;
    Ok(exit)
}

fn runner_arguments(argv: &[String]) -> Option<&[String]> {
    let cargo_prefix = [
        "cargo",
        "run",
        "--manifest-path",
        "crates/Cargo.toml",
        "--locked",
        "-p",
        "shacs-projection",
        "--bin",
        "spec031-release-runner",
        "--",
    ];
    if argv.len() >= cargo_prefix.len()
        && argv
            .iter()
            .take(cargo_prefix.len())
            .map(String::as_str)
            .eq(cargo_prefix)
    {
        return Some(&argv[cargo_prefix.len()..]);
    }
    if argv
        .first()
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(|name| name.to_str())
        == Some("spec031-release-runner")
    {
        return Some(&argv[1..]);
    }
    None
}
