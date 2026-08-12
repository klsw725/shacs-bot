use super::*;
use crate::spec031::release_runner::model::{
    Spec031ReleaseCommandSpec, Spec031ReleaseRunId, SPEC031_RELEASE_RUNNER_SCHEMA,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn current_worktree_rejects_relabeled_command_metadata() {
    // Given
    let (root, repo, mut artifacts) = current_artifacts("relabel");
    let first = artifacts.command_registry[0].clone();
    artifacts.command_registry[1].gate = first.gate;
    artifacts.command_registry[1].package = first.package;
    artifacts.command_registry[1].filter = first.filter;
    artifacts.command_registry[1].argv = first.argv;

    // When
    let result = validate_command_registry(&artifacts, &repo);

    // Then
    assert_eq!(
        result,
        Err(Spec031ReleaseArtifactError::InvalidCommandEvidence)
    );
    drop(root);
}

#[test]
fn current_worktree_rejects_altered_package_filter_argv_and_cwd() {
    for field in ["package", "filter", "argv", "cwd"] {
        // Given
        let (root, repo, mut artifacts) = current_artifacts(field);
        let command = artifacts
            .command_registry
            .iter_mut()
            .find(|record| record.id == "spec031-test-release-runner")
            .expect("canonical command exists");
        match field {
            "package" => command.package = Some("shacs-core".to_owned()),
            "filter" => command.filter = Some("unrelated".to_owned()),
            "argv" => command.argv.push("unrelated".to_owned()),
            "cwd" => command.cwd = root.display().to_string(),
            _ => unreachable!(),
        }

        // When
        let result = validate_command_registry(&artifacts, &repo);

        // Then
        assert_eq!(
            result,
            Err(Spec031ReleaseArtifactError::InvalidCommandEvidence)
        );
    }
}

#[test]
fn current_worktree_rejects_unrelated_focused_test_transcript() {
    // Given
    let (root, repo, artifacts) = current_artifacts("unrelated-transcript");
    std::fs::write(
        root.join("commands/spec031-owner-spec030.stdout"),
        "test unrelated_test ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    )
    .expect("transcript writes");

    // When
    let result = validate_command_registry(&artifacts, &repo);

    // Then
    assert_eq!(
        result,
        Err(Spec031ReleaseArtifactError::InvalidCommandEvidence)
    );
}

#[test]
fn current_worktree_rejects_ignored_exact_owner_test() {
    // Given
    let (root, repo, mut artifacts) = current_artifacts("ignored-owner-test");
    let command = artifacts
        .command_registry
        .iter_mut()
        .find(|record| record.id == "spec031-owner-spec030")
        .expect("owner command exists");
    command.tests = Some(Spec031ReleaseTestCounts {
        tests_run: 0,
        tests_failed: 0,
    });
    std::fs::write(
        root.join("commands/spec031-owner-spec030.stdout"),
        "test local_spec030_provider_discovers_live_resources_diagnostics_and_trace ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 5 filtered out; finished in 0.00s\n",
    )
    .expect("transcript writes");

    // When
    let result = validate_command_registry(&artifacts, &repo);

    // Then
    assert_eq!(result, Err(Spec031ReleaseArtifactError::ZeroTestsRun));
}

#[test]
fn current_worktree_rejects_substituted_target_transcript() {
    // Given
    let (root, repo, artifacts) = current_artifacts("substituted-target");
    std::fs::write(
        root.join("commands/spec031-test-lifecycle.stderr"),
        "Running tests/unrelated.rs\n",
    )
    .expect("transcript writes");

    // When
    let result = validate_command_registry(&artifacts, &repo);

    // Then
    assert_eq!(
        result,
        Err(Spec031ReleaseArtifactError::InvalidCommandEvidence)
    );
}

fn current_artifacts(label: &str) -> (PathBuf, PathBuf, Spec031ReleaseRunArtifacts) {
    let repo = workspace_root();
    let root = temp_path(label);
    std::fs::create_dir_all(root.join("commands")).expect("command evidence directory writes");
    let config = Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("command-validation-test").expect("safe run id"),
        evidence_root: root.clone(),
        repo_root: repo.clone(),
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::ZERO,
    };
    let specs = required_worktree_commands(&config);
    let records = specs
        .iter()
        .map(|spec| record_with_transcript(&root, spec))
        .collect();
    (
        root.clone(),
        repo,
        Spec031ReleaseRunArtifacts {
            schema: SPEC031_RELEASE_RUNNER_SCHEMA.to_owned(),
            run_id: config.run_id,
            evidence_root: root.display().to_string(),
            fixture_registry: vec!["fixtures/current-worktree.json".to_owned()],
            command_registry: records,
            cleanup_registry: Vec::new(),
            manifest_files: Vec::new(),
            coverage_matrix: Vec::new(),
            external_audits: Vec::new(),
            failure_triage: Vec::new(),
            reproducibility_observations: Vec::new(),
        },
    )
}

fn record_with_transcript(
    root: &Path,
    spec: &Spec031ReleaseCommandSpec,
) -> Spec031ReleaseCommandRecord {
    let is_test = matches!(spec.argv.get(1).map(String::as_str), Some("test"));
    let exact_name = exact_test_name(&spec.argv);
    let stdout = if let Some(name) = exact_name {
        format!("test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n")
    } else if is_test {
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n".to_owned()
    } else {
        String::new()
    };
    let stderr = values_after(&spec.argv, "--test")
        .iter()
        .map(|target| format!("Running tests/{target}.rs\n"))
        .collect::<String>();
    let stdout_path = format!("commands/{}.stdout", spec.id);
    let stderr_path = format!("commands/{}.stderr", spec.id);
    std::fs::write(root.join(&stdout_path), stdout).expect("stdout writes");
    std::fs::write(root.join(&stderr_path), stderr).expect("stderr writes");
    Spec031ReleaseCommandRecord {
        id: spec.id.clone(),
        gate: spec.gate,
        package: spec.package.clone(),
        filter: spec.filter.clone(),
        argv: spec.argv.clone(),
        cwd: spec.cwd.display().to_string(),
        status: Spec031ReleaseCommandStatus::Passed,
        exit_code: Some(0),
        duration_ms: 1,
        stdout_path,
        stderr_path,
        tests: is_test.then_some(Spec031ReleaseTestCounts {
            tests_run: 1,
            tests_failed: 0,
        }),
        process_receipt: None,
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root exists")
        .canonicalize()
        .expect("workspace root canonicalizes")
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "shacs-spec031-command-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ))
}
