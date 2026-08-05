mod spec031_release_runner_support;

use shacs_projection::{
    execute_spec031_release_command, parse_cargo_test_counts, run_spec031_release_runner,
    validate_spec031_release_artifacts, Spec031CoverageEvidenceKind,
    Spec031CoverageRequirementKind, Spec031ExternalAuditStatus, Spec031ExternalOwnerId,
    Spec031ReleaseArtifactError, Spec031ReleaseCommandSpec, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind, Spec031ReleaseRunArtifacts, Spec031ReleaseRunId,
    Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode,
};
use spec031_release_runner_support::{
    command, make_symlink, process_alive, run_git, temp_path, valid_artifacts,
    valid_artifacts_on_disk, write_artifacts,
};
use std::fs;
use std::time::Duration;

#[test]
fn spec031_release_artifact_contract_accepts_success_fixture() {
    let (root, artifacts) = valid_artifacts_on_disk("contract-accepts-success-fixture")
        .expect("valid artifact fixture writes");

    validate_spec031_release_artifacts(&artifacts).expect("success fixture is complete");
    assert!(root.join("manifest.json").exists());
}

#[test]
fn spec031_release_artifact_contract_rejects_zero_tests() {
    let (root, mut artifacts) =
        valid_artifacts_on_disk("rejects-zero-tests").expect("fixture writes");
    artifacts.command_registry[0] = command("spec031-fmt", 0, 0);
    fs::write(
        root.join("commands/spec031-fmt.stdout"),
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    )
    .expect("stdout writes");
    write_artifacts(&root, &artifacts).expect("mutated fixture writes");

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("zero tests fail");

    assert_eq!(error, Spec031ReleaseArtifactError::ZeroTestsRun);
}

#[test]
fn spec031_release_artifact_contract_rejects_missing_required_artifact() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-missing-required").expect("fixture writes");
    artifacts.manifest_files.retain(|file| file != "summary.md");

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("missing summary fails");

    assert_eq!(error, Spec031ReleaseArtifactError::MissingRequiredArtifact);
}

#[test]
fn spec031_release_artifact_contract_rejects_missing_cleanup_receipt() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-missing-cleanup").expect("fixture writes");
    artifacts.cleanup_registry.clear();

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("cleanup is required");

    assert_eq!(error, Spec031ReleaseArtifactError::MissingCleanupReceipt);
}

#[test]
fn spec031_release_artifact_contract_rejects_blocked_external_evidence() {
    let (root, mut artifacts) = valid_artifacts_on_disk("rejects-blocked").expect("fixture writes");
    fs::create_dir_all(root.join("triage")).expect("triage dir writes");
    fs::write(
        root.join("triage/blocked.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": shacs_projection::SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": artifacts.run_id.as_str(),
            "code": "blocked_external_evidence",
            "message": "spec032"
        }))
        .expect("triage json serializes"),
    )
    .expect("triage writes");
    artifacts
        .failure_triage
        .push("triage/blocked.json".to_owned());
    write_artifacts(&root, &artifacts).expect("mutated fixture writes");

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("blocked audit fails");

    assert_eq!(error, Spec031ReleaseArtifactError::BlockedExternalEvidence);
}

#[test]
fn spec031_release_artifact_contract_rejects_malformed_schema() {
    let (_, mut artifacts) = valid_artifacts_on_disk("rejects-schema").expect("fixture writes");
    artifacts.schema = "spec031.release_runner.v0".to_owned();

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("schema is strict");

    assert_eq!(error, Spec031ReleaseArtifactError::UnsupportedSchema);
}

#[test]
fn spec031_release_artifact_contract_rejects_nonzero_tests() {
    let (root, mut artifacts) = valid_artifacts_on_disk("rejects-nonzero").expect("fixture writes");
    artifacts.command_registry[0] = command("spec031-fmt", 3, 1);
    fs::write(
        root.join("commands/spec031-fmt.stdout"),
        "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    )
    .expect("stdout writes");
    write_artifacts(&root, &artifacts).expect("mutated fixture writes");

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("failed tests fail");

    assert_eq!(error, Spec031ReleaseArtifactError::NonzeroTestsFailed);
}

#[test]
fn spec031_release_command_records_hung_command_as_timeout(
) -> Result<(), Box<dyn std::error::Error>> {
    let output = temp_path("hung-command");
    fs::create_dir_all(&output)?;
    let record = execute_spec031_release_command(
        &Spec031ReleaseCommandSpec {
            id: "hung".to_owned(),
            gate: Spec031ReleaseGateKind::FailureInjection,
            package: None,
            filter: None,
            argv: vec!["sleep".to_owned(), "2".to_owned()],
            cwd: std::env::temp_dir(),
            timeout: Duration::from_millis(20),
        },
        &output,
    )?;

    assert_eq!(record.status, Spec031ReleaseCommandStatus::TimedOut);
    validate_spec031_release_artifacts(&Spec031ReleaseRunArtifacts {
        command_registry: vec![record],
        ..valid_artifacts()
    })
    .expect_err("timeout fails release");
    Ok(())
}

#[test]
fn spec031_release_command_timeout_kills_descendant_process(
) -> Result<(), Box<dyn std::error::Error>> {
    let output = temp_path("descendant-timeout");
    fs::create_dir_all(&output)?;
    let marker = output.join("child.pid");
    let record = execute_spec031_release_command(
        &Spec031ReleaseCommandSpec {
            id: "descendant_timeout".to_owned(),
            gate: Spec031ReleaseGateKind::FailureInjection,
            package: None,
            filter: None,
            argv: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "sleep 5 & printf '%s' \"$!\" > \"$1\"; wait".to_owned(),
                "sh".to_owned(),
                marker.display().to_string(),
            ],
            cwd: std::env::temp_dir(),
            timeout: Duration::from_millis(50),
        },
        &output,
    )?;

    assert_eq!(record.status, Spec031ReleaseCommandStatus::TimedOut);
    let child_pid = fs::read_to_string(marker)?.parse::<u32>()?;
    std::thread::sleep(Duration::from_millis(100));
    assert!(!process_alive(child_pid));
    Ok(())
}

#[test]
fn spec031_release_command_rejects_spoofed_non_cargo_test_counts(
) -> Result<(), Box<dyn std::error::Error>> {
    let output = temp_path("spoofed-command");
    fs::create_dir_all(&output)?;
    let record = execute_spec031_release_command(
        &Spec031ReleaseCommandSpec {
            id: "spoofed".to_owned(),
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: None,
            filter: None,
            argv: vec![
                "printf".to_owned(),
                "test result: ok. 1 passed; 0 failed; 0 ignored\n".to_owned(),
            ],
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(1),
        },
        &output,
    )?;

    assert_eq!(record.tests, None);
    Ok(())
}

#[test]
fn spec031_release_artifact_contract_rejects_path_escape() -> Result<(), Box<dyn std::error::Error>>
{
    let (root, mut artifacts) = valid_artifacts_on_disk("rejects-path-escape")?;
    artifacts.command_registry[0].stdout_path = "../outside.stdout".to_owned();
    write_artifacts(&root, &artifacts)?;

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("path escape fails");

    assert_eq!(error, Spec031ReleaseArtifactError::InvalidArtifactPath);
    Ok(())
}

#[test]
fn spec031_release_artifact_contract_rejects_symlinked_artifact(
) -> Result<(), Box<dyn std::error::Error>> {
    let (root, artifacts) = valid_artifacts_on_disk("rejects-symlink")?;
    fs::remove_file(root.join("summary.md"))?;
    make_symlink(root.join("manifest.json"), root.join("summary.md"))?;

    let error = validate_spec031_release_artifacts(&artifacts).expect_err("symlink fails");

    assert_eq!(error, Spec031ReleaseArtifactError::InvalidArtifactPath);
    Ok(())
}

#[test]
fn spec031_release_command_parses_cargo_counts() {
    let output = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s";

    let counts = parse_cargo_test_counts(output).expect("cargo summary is parsed");

    assert_eq!(counts.tests_run, 3);
    assert_eq!(counts.tests_failed, 1);
}

#[test]
fn spec031_release_runner_writes_all_success_fixture_artifacts(
) -> Result<(), Box<dyn std::error::Error>> {
    let evidence_root = temp_path("success-fixture-runner");
    let artifacts = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("success-fixture-run")?,
        evidence_root: evidence_root.clone(),
        repo_root: std::env::current_dir()?,
        mode: Spec031ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
    })?;

    validate_spec031_release_artifacts(&artifacts)?;
    for file in [
        "manifest.json",
        "coverage-matrix.json",
        "results.json",
        "failure-triage.json",
        "summary.md",
    ] {
        assert!(evidence_root.join(file).exists(), "missing {file}");
    }
    assert_eq!(
        artifacts.command_registry[0].argv,
        vec!["cargo".to_owned(), "test".to_owned()]
    );
    assert!(artifacts.command_registry[0]
        .stdout_path
        .starts_with("commands/"));
    assert_eq!(
        artifacts.command_registry[0]
            .tests
            .as_ref()
            .map(|tests| tests.tests_run),
        Some(1)
    );
    Ok(())
}

#[test]
fn spec031_release_runner_fails_dirty_temp_git_repo() -> Result<(), Box<dyn std::error::Error>> {
    let repo = temp_path("dirty-repo");
    fs::create_dir_all(&repo)?;
    run_git(&repo, &["init"])?;
    fs::write(repo.join("dirty.txt"), "untracked\n")?;
    let evidence_root = temp_path("dirty-repo-evidence");

    let error = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("dirty-repo-run")?,
        evidence_root,
        repo_root: repo,
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::from_secs(1),
    })
    .expect_err("dirty repo fails release runner");

    assert_eq!(
        error,
        Spec031ReleaseArtifactError::UnmappedCoverageRequirement
    );
    Ok(())
}

#[test]
fn spec031_release_artifact_json_rejects_unknown_fields() -> Result<(), Box<dyn std::error::Error>>
{
    let mut value = serde_json::to_value(valid_artifacts())?;
    value["unexpected"] = serde_json::json!(true);

    let parsed = serde_json::from_value::<Spec031ReleaseRunArtifacts>(value);

    assert!(parsed.is_err());
    Ok(())
}

#[test]
fn spec031_coverage_contract_rejects_prose_and_screenshot_evidence() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-prose-screenshot").expect("fixture writes");
    artifacts.coverage_matrix[0].evidence_kind = Spec031CoverageEvidenceKind::PlannedProse;
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");

    let prose_error =
        validate_spec031_release_artifacts(&artifacts).expect_err("planned prose is not proof");
    assert_eq!(
        prose_error,
        Spec031ReleaseArtifactError::InvalidCoverageEvidence
    );

    artifacts.coverage_matrix[0].evidence_kind = Spec031CoverageEvidenceKind::Screenshot;
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");
    let screenshot_error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("screenshots do not prove machine requirements");
    assert_eq!(
        screenshot_error,
        Spec031ReleaseArtifactError::InvalidCoverageEvidence
    );
}

#[test]
fn spec031_coverage_contract_rejects_duplicate_unknown_and_unmapped_rows() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-coverage-rows").expect("fixture writes");
    artifacts.coverage_matrix[0].requirement_id = "spec031:unknown:999".to_owned();
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&artifacts).expect_err("unknown id fails"),
        Spec031ReleaseArtifactError::UnknownCoverageRequirement
    );

    let (_, mut duplicate) =
        valid_artifacts_on_disk("rejects-duplicate-row").expect("fixture writes");
    duplicate
        .coverage_matrix
        .push(duplicate.coverage_matrix[0].clone());
    write_artifacts(&root_from(&duplicate), &duplicate).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&duplicate).expect_err("duplicate id fails"),
        Spec031ReleaseArtifactError::DuplicateCoverageRequirement
    );

    let (_, mut missing) = valid_artifacts_on_disk("rejects-missing-row").expect("fixture writes");
    missing.coverage_matrix.retain(|entry| {
        entry.kind != Spec031CoverageRequirementKind::ParentMustHave
            || entry.requirement_id != "spec031:must:01"
    });
    write_artifacts(&root_from(&missing), &missing).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&missing).expect_err("required row fails"),
        Spec031ReleaseArtifactError::UnmappedCoverageRequirement
    );
}

#[test]
fn spec031_external_audit_rejects_blocked_as_pass_and_missing_artifact() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-external-audit").expect("fixture writes");
    let audit = artifacts
        .external_audits
        .iter_mut()
        .find(|row| row.owner == Spec031ExternalOwnerId::Spec032)
        .expect("spec032 audit row exists");
    audit.status = Spec031ExternalAuditStatus::Pass;
    audit.reason = "closure evidence absent".to_owned();
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&artifacts).expect_err("blocked-as-pass fails"),
        Spec031ReleaseArtifactError::BlockedAsPass
    );

    let (_, mut missing) =
        valid_artifacts_on_disk("rejects-external-missing").expect("fixture writes");
    missing.external_audits[0].artifact = "external/missing.md".to_owned();
    write_artifacts(&root_from(&missing), &missing).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&missing).expect_err("missing audit artifact fails"),
        Spec031ReleaseArtifactError::MissingRequiredArtifact
    );
}

#[test]
fn spec031_coverage_contract_rejects_command_pass_with_empty_registry() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-empty-command-registry").expect("fixture writes");
    artifacts.command_registry.clear();
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("required commands need concrete command records");

    assert_eq!(
        error,
        Spec031ReleaseArtifactError::UnmappedCoverageRequirement
    );
}

#[test]
fn spec031_current_artifact_rejects_missing_required_current_gate() {
    let (root, mut artifacts) =
        valid_artifacts_on_disk("rejects-missing-current-gate").expect("fixture writes");
    artifacts
        .fixture_registry
        .push("fixtures/current-worktree.json".to_owned());
    fs::write(
        root.join("fixtures/current-worktree.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": shacs_projection::SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": artifacts.run_id.as_str(),
            "resource_id": "current-worktree"
        }))
        .expect("fixture json serializes"),
    )
    .expect("current fixture writes");
    artifacts
        .command_registry
        .retain(|command| command.id != "spec031-test-lifecycle");
    write_artifacts(&root, &artifacts).expect("mutated artifact writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("current-worktree requires every PRD007 gate");

    assert_eq!(
        error,
        Spec031ReleaseArtifactError::UnmappedCoverageRequirement
    );
}

#[test]
fn spec031_command_contract_rejects_empty_focused_filter() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-empty-focused-filter").expect("fixture writes");
    let command = artifacts
        .command_registry
        .iter_mut()
        .find(|command| command.id == "spec031-test-release-runner")
        .expect("focused command exists");
    command.filter = Some(String::new());
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("focused tests need a non-empty filter");

    assert_eq!(error, Spec031ReleaseArtifactError::ZeroTestsRun);
}

#[test]
fn spec031_summary_contract_rejects_thin_summary() {
    let (root, artifacts) =
        valid_artifacts_on_disk("rejects-thin-summary").expect("fixture writes");
    fs::write(
        root.join("summary.md"),
        "# Spec031 Release Runner Summary\n\n- status: PASS\n",
    )
    .expect("thin summary writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("summary must include auditable details");

    assert_eq!(error, Spec031ReleaseArtifactError::InvalidCommandEvidence);
}

#[test]
fn spec031_coverage_contract_rejects_unrelated_artifact_substitution() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-unrelated-coverage-artifact").expect("fixture writes");
    let command_row = artifacts
        .coverage_matrix
        .iter_mut()
        .find(|entry| entry.requirement_id == "spec031:command:fmt")
        .expect("fmt row exists");
    command_row.artifact = "summary.md".to_owned();
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("command rows must cite the matching command transcript");

    assert_eq!(error, Spec031ReleaseArtifactError::InvalidCoverageEvidence);
}

#[test]
fn spec031_external_audit_rejects_wrong_spec_path_and_status_conflict() {
    let (_, mut artifacts) =
        valid_artifacts_on_disk("rejects-wrong-spec-path").expect("fixture writes");
    artifacts.external_audits[0].source_locator = "docs/specs/Spec029/SPEC.md".to_owned();
    write_artifacts(&root_from(&artifacts), &artifacts).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&artifacts).expect_err("generated enum path fails"),
        Spec031ReleaseArtifactError::InvalidCoverageEvidence
    );

    let (_, mut conflict) =
        valid_artifacts_on_disk("rejects-spec029-conflict").expect("fixture writes");
    let spec029 = conflict
        .external_audits
        .iter_mut()
        .find(|row| row.owner == Spec031ExternalOwnerId::Spec029)
        .expect("spec029 audit row exists");
    spec029.status = Spec031ExternalAuditStatus::Blocked;
    spec029.reason = "source status is complete but row claims blocked".to_owned();
    write_artifacts(&root_from(&conflict), &conflict).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&conflict).expect_err("owner has one verdict"),
        Spec031ReleaseArtifactError::ArtifactMismatch
    );
}

#[test]
fn spec031_external_audit_rejects_mislabeled_prose_and_blocked_reason_pass() {
    let (root, mut artifacts) =
        valid_artifacts_on_disk("rejects-mislabeled-prose").expect("fixture writes");
    fs::write(
        root.join("external/spec030-read-audit.md"),
        "# prose only\n\nThis says implementation is planned later.\n",
    )
    .expect("prose audit writes");
    let spec030 = artifacts
        .external_audits
        .iter_mut()
        .find(|row| row.owner == Spec031ExternalOwnerId::Spec030)
        .expect("spec030 audit row exists");
    spec030.status = Spec031ExternalAuditStatus::Pass;
    spec030.reason = "source status remains open and blocks closure".to_owned();
    spec030.artifact_hash = hash_file(&root.join("external/spec030-read-audit.md"));
    write_artifacts(&root, &artifacts).expect("mutated artifact writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("pass requires typed artifact facts, not prose");

    assert_eq!(error, Spec031ReleaseArtifactError::BlockedAsPass);
}

#[test]
fn spec031_coverage_contract_rejects_screenshot_renamed_json_and_hash_mismatch() {
    let (root, mut artifacts) =
        valid_artifacts_on_disk("rejects-screenshot-renamed-json").expect("fixture writes");
    fs::write(root.join("screenshot.json"), b"\x89PNG\r\n\x1a\n").expect("screenshot bytes write");
    let row = artifacts
        .coverage_matrix
        .iter_mut()
        .find(|entry| entry.requirement_id == "spec031:artifact:results")
        .expect("results row exists");
    row.artifact = "screenshot.json".to_owned();
    write_artifacts(&root, &artifacts).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&artifacts).expect_err("renamed screenshot fails"),
        Spec031ReleaseArtifactError::InvalidCoverageEvidence
    );

    let (_, mut hash_mismatch) =
        valid_artifacts_on_disk("rejects-audit-hash-mismatch").expect("fixture writes");
    hash_mismatch.external_audits[0].artifact_hash = "fnv64:0000000000000000".to_owned();
    write_artifacts(&root_from(&hash_mismatch), &hash_mismatch).expect("mutated artifact writes");
    assert_eq!(
        validate_spec031_release_artifacts(&hash_mismatch).expect_err("hash mismatch fails"),
        Spec031ReleaseArtifactError::ArtifactMismatch
    );
}

fn hash_file(path: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};

    let bytes = fs::read(path).expect("file exists");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("fnv64:{:016x}", hasher.finish())
}

fn root_from(artifacts: &Spec031ReleaseRunArtifacts) -> std::path::PathBuf {
    std::path::PathBuf::from(&artifacts.evidence_root)
}

#[test]
fn spec031_current_runner_reports_external_blockers_before_dirty_masking(
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = temp_path("dirty-blocked-repo");
    fs::create_dir_all(&repo)?;
    run_git(&repo, &["init"])?;
    fs::write(repo.join("dirty.txt"), "untracked\n")?;
    let evidence_root = temp_path("dirty-blocked-evidence");

    let error = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("dirty-blocked-run")?,
        evidence_root,
        repo_root: repo,
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::from_secs(1),
    })
    .expect_err("required external blockers win over dirty-only masking");

    assert_eq!(
        error,
        Spec031ReleaseArtifactError::UnmappedCoverageRequirement
    );
    Ok(())
}

#[test]
fn spec031_coverage_required_artifacts_cite_their_own_artifacts(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, artifacts) = valid_artifacts_on_disk("required-artifacts-cite-self")?;

    for (name, artifact) in [
        ("manifest", "manifest.json"),
        ("coverage-matrix", "coverage-matrix.json"),
        ("results", "results.json"),
        ("failure-triage", "failure-triage.json"),
        ("summary", "summary.md"),
    ] {
        let row = artifacts
            .coverage_matrix
            .iter()
            .find(|entry| entry.requirement_id == format!("spec031:artifact:{name}"))
            .expect("required artifact coverage row exists");
        assert_eq!(row.artifact, artifact);
        assert_ne!(row.artifact, "evidence-index.json");
        assert_eq!(row.command_result_id, None);
    }
    Ok(())
}

#[test]
fn spec031_coverage_requirement_rows_do_not_round_robin_commands(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, artifacts) = valid_artifacts_on_disk("requirement-commands-exact")?;

    for row in artifacts.coverage_matrix.iter().filter(|entry| {
        matches!(
            entry.kind,
            Spec031CoverageRequirementKind::ParentMustHave
                | Spec031CoverageRequirementKind::AcceptanceCriterion
                | Spec031CoverageRequirementKind::ClosureEvidence
                | Spec031CoverageRequirementKind::PrdTask
        ) && !row_artifact_is_command(&entry.artifact)
    }) {
        assert_eq!(row.command_result_id, None, "{}", row.requirement_id);
    }
    Ok(())
}

#[test]
fn spec031_coverage_source_locators_resolve_to_real_source_lines(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, artifacts) = valid_artifacts_on_disk("source-locators-real")?;

    for row in &artifacts.coverage_matrix {
        assert!(
            source_locator_resolves(&row.source_locator),
            "synthetic or stale locator: {}",
            row.source_locator
        );
    }
    Ok(())
}

#[test]
fn spec031_external_audit_rejects_generic_pass_without_exact_owner_facts() {
    let (root, mut artifacts) =
        valid_artifacts_on_disk("rejects-generic-pass-facts").expect("fixture writes");
    let spec030 = artifacts
        .external_audits
        .iter_mut()
        .find(|row| row.owner == Spec031ExternalOwnerId::Spec030)
        .expect("spec030 audit row exists");
    spec030.status = Spec031ExternalAuditStatus::Pass;
    spec030.reason = "artifact-backed exact fact audit passes".to_owned();
    spec030.implementation_artifacts = vec![spec030.source_locator.clone()];
    spec030.command_result_ids = artifacts
        .command_registry
        .iter()
        .map(|record| record.id.clone())
        .collect();
    write_artifacts(&root, &artifacts).expect("mutated artifact writes");

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("generic PASS without exact owner facts is rejected");

    assert_eq!(error, Spec031ReleaseArtifactError::BlockedAsPass);
}

#[test]
fn spec031_success_fixture_non_spec029_pass_cites_exact_owner_fact_artifacts(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, artifacts) = valid_artifacts_on_disk("non-spec029-exact-facts")?;
    let spec030 = artifacts
        .external_audits
        .iter()
        .find(|row| row.owner == Spec031ExternalOwnerId::Spec030)
        .expect("spec030 audit row exists");

    assert_eq!(spec030.status, Spec031ExternalAuditStatus::Pass);
    assert!(spec030.implementation_artifacts.iter().any(|artifact| {
        artifact == "fixtures/success-fixture/external-owner-facts/spec030.json"
    }));
    Ok(())
}

fn row_artifact_is_command(artifact: &str) -> bool {
    artifact.starts_with("commands/") && artifact.ends_with(".stdout")
}

fn source_locator_resolves(locator: &str) -> bool {
    let Some((path, line)) = locator.rsplit_once(':') else {
        return false;
    };
    let Ok(line) = line.parse::<usize>() else {
        return false;
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root exists");
    let Ok(text) = fs::read_to_string(root.join(path)) else {
        return false;
    };
    text.lines()
        .nth(line.saturating_sub(1))
        .is_some_and(|line| !line.trim().is_empty())
}
