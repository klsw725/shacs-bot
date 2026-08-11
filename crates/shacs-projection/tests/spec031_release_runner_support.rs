use shacs_projection::{
    run_spec031_release_runner, write_spec031_release_artifacts, Spec031ReleaseCommandRecord,
    Spec031ReleaseCommandStatus, Spec031ReleaseGateKind, Spec031ReleaseRunArtifacts,
    Spec031ReleaseRunId, Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode,
    Spec031ReleaseTestCounts, SPEC031_RELEASE_RUNNER_SCHEMA,
};
use std::process::Command;
use std::time::Duration;

pub fn command(id: &str, tests_run: u64, tests_failed: u64) -> Spec031ReleaseCommandRecord {
    Spec031ReleaseCommandRecord {
        id: id.to_owned(),
        gate: Spec031ReleaseGateKind::FocusedCargoTest,
        package: Some("shacs-projection".to_owned()),
        filter: Some("spec031_release_runner".to_owned()),
        argv: vec!["cargo".to_owned(), "test".to_owned()],
        cwd: ".".to_owned(),
        status: Spec031ReleaseCommandStatus::Passed,
        exit_code: Some(0),
        duration_ms: 7,
        stdout_path: format!("commands/{id}.stdout"),
        stderr_path: format!("commands/{id}.stderr"),
        tests: Some(Spec031ReleaseTestCounts {
            tests_run,
            tests_failed,
        }),
        process_receipt: None,
    }
}

pub fn valid_artifacts() -> Spec031ReleaseRunArtifacts {
    Spec031ReleaseRunArtifacts {
        schema: SPEC031_RELEASE_RUNNER_SCHEMA.to_owned(),
        run_id: Spec031ReleaseRunId::try_new("spec031-run-20").expect("safe run id"),
        evidence_root: ".omo/evidence/spec031/prd007/task-20-spec031-implementation".to_owned(),
        fixture_registry: vec!["fixtures/success-fixture/Cargo.toml".to_owned()],
        command_registry: required_commands(),
        cleanup_registry: vec!["cleanup/success-receipt.json".to_owned()],
        manifest_files: vec![
            "manifest.json".to_owned(),
            "coverage-matrix.json".to_owned(),
            "results.json".to_owned(),
            "failure-triage.json".to_owned(),
            "summary.md".to_owned(),
            "evidence-index.json".to_owned(),
        ],
        coverage_matrix: Vec::new(),
        external_audits: Vec::new(),
        failure_triage: Vec::new(),
    }
}

pub fn valid_artifacts_on_disk(
    label: &str,
) -> Result<(std::path::PathBuf, Spec031ReleaseRunArtifacts), Box<dyn std::error::Error>> {
    let root = temp_path(label);
    let artifacts = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("spec031-run-20")?,
        evidence_root: root.clone(),
        repo_root: workspace_root(),
        mode: Spec031ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
    })?;
    Ok((root, artifacts))
}

fn required_commands() -> Vec<Spec031ReleaseCommandRecord> {
    required_command_ids()
        .into_iter()
        .map(|(_, id)| command(id, 1, 0))
        .collect()
}

fn required_command_ids() -> [(&'static str, &'static str); 10] {
    [
        ("fmt", "spec031-fmt"),
        ("clippy-workspace", "spec031-clippy-workspace"),
        ("test-workspace", "spec031-test-workspace"),
        ("test-release-runner", "spec031-test-release-runner"),
        ("test-lifecycle", "spec031-test-lifecycle"),
        ("test-projection-parity", "spec031-test-projection-parity"),
        ("test-surface-smoke", "spec031-test-surface-smoke"),
        ("test-failure-injection", "spec031-test-failure-injection"),
        ("build-cli", "spec031-build-cli"),
        ("build-tui", "spec031-build-tui"),
    ]
}

pub fn write_artifacts(
    root: &std::path::Path,
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Box<dyn std::error::Error>> {
    remove_manifest_files(root)?;
    write_spec031_release_artifacts(artifacts, root)?;
    Ok(())
}

fn remove_manifest_files(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    for file in [
        "manifest.json",
        "coverage-matrix.json",
        "results.json",
        "failure-triage.json",
        "fixture-registry.json",
        "command-registry.json",
        "cleanup-registry.json",
        "summary.md",
    ] {
        let path = root.join(file);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn make_symlink(
    source: std::path::PathBuf,
    link: std::path::PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    std::os::unix::fs::symlink(source, link)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn make_symlink(
    _source: std::path::PathBuf,
    _link: std::path::PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

pub fn run_git(repo: &std::path::Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git").args(args).current_dir(repo).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git {args:?} failed").into())
    }
}

#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn process_alive(_pid: u32) -> bool {
    false
}

pub fn temp_path(label: &str) -> std::path::PathBuf {
    temp_base().join(format!(
        "shacs-spec031-release-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ))
}

fn temp_base() -> std::path::PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root exists")
        .to_path_buf()
}
