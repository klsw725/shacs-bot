use shacs_projection::{
    run_spec030_release_runner, spec030_integration_targets, validate_spec030_release_artifacts,
    Spec030ReleaseArtifactError, Spec030ReleaseRunId, Spec030ReleaseRunnerConfig,
    Spec030ReleaseRunnerMode, Spec030ReleaseVerdict, Spec030SurfaceOwnerReadiness,
    Spec030SurfaceOwnerShutdown,
};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock follows epoch")
        .as_nanos();
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!(
            "shacs-spec030-{label}-{}-{nonce}",
            std::process::id()
        ))
}

fn fixture_config(label: &str) -> Spec030ReleaseRunnerConfig {
    Spec030ReleaseRunnerConfig {
        run_id: Spec030ReleaseRunId::try_new(label).expect("fixture id is safe"),
        evidence_root: temp_path(label),
        repo_root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("repository root exists")
            .to_path_buf(),
        mode: Spec030ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
        manual_records: Vec::new(),
        bwrap_record: None,
    }
}

#[test]
fn spec030_integration_catalog_includes_new_production_and_closure_targets() {
    let targets = spec030_integration_targets()
        .iter()
        .map(|target| target.target)
        .collect::<std::collections::BTreeSet<_>>();
    for expected in [
        "spec030_javascript_tool_before_host",
        "spec030_javascript_tool_before_exec",
        "spec030_provider_credential_invocation",
        "spec030_startup_facts",
        "spec030_mcp_startup_facts",
        "spec030_sandbox_invalid_plan_fallback",
        "spec030_trace_disclosure",
        "spec030_user_data_plugin",
        "spec030_semantic_evidence",
        "spec030_runner_integrity",
    ] {
        assert!(targets.contains(expected), "missing target {expected}");
    }
    assert!(spec030_integration_targets()
        .iter()
        .any(|target| target.prds.contains(&"006")));
}

#[test]
fn spec030_release_runner_success_fixture_writes_complete_artifact_tree(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("success-fixture");

    // When
    let artifacts = run_spec030_release_runner(&config)?;

    // Then
    validate_spec030_release_artifacts(&artifacts)?;
    assert_eq!(artifacts.verdict, Spec030ReleaseVerdict::Pass);
    assert_eq!(artifacts.mode, Spec030ReleaseRunnerMode::SuccessFixture);
    assert_eq!(artifacts.coverage.len(), 7);
    assert_eq!(artifacts.owner_audits.len(), 3);
    assert_eq!(artifacts.surface_owner.requested_port, 0);
    assert!(artifacts.surface_owner.bound_port > 0);
    assert!(artifacts.surface_owner.owner_pid > 0);
    assert_eq!(
        artifacts.surface_owner.readiness,
        Spec030SurfaceOwnerReadiness::Observed
    );
    assert_eq!(
        artifacts.surface_owner.shutdown,
        Spec030SurfaceOwnerShutdown::Reaped
    );
    assert!(artifacts
        .commands
        .iter()
        .any(|command| command.id == "cargo-fmt"));
    assert!(artifacts
        .commands
        .iter()
        .any(|command| command.id == "cargo-clippy-workspace"));
    assert!(artifacts
        .commands
        .iter()
        .any(|command| command.id == "cargo-test-workspace"));
    assert!(artifacts
        .surfaces
        .iter()
        .all(|surface| surface.artifact.starts_with("surface/")));
    assert!(artifacts.surfaces.iter().any(|surface| {
        surface.surface == "tui_no_session"
            && surface.command_id == "surface-tui-no-session"
            && surface.artifact == "surface/tui-no-session.txt"
    }));
    assert!(artifacts.surfaces.iter().any(|surface| {
        surface.surface == "tui_runtime"
            && surface.command_id == "surface-tui-runtime"
            && surface.artifact == "surface/tui-runtime.txt"
    }));
    let command_position = |id: &str| {
        artifacts
            .commands
            .iter()
            .position(|command| command.id == id)
            .expect("surface command exists")
    };
    assert!(command_position("surface-tui-no-session") < command_position("surface-cli-json"));
    assert!(command_position("surface-cli-human") < command_position("surface-tui-runtime"));
    assert!(command_position("surface-tui-runtime") < command_position("surface-api-schema"));
    for target in spec030_integration_targets() {
        let command = artifacts
            .commands
            .iter()
            .find(|command| command.id == target.command_id)
            .expect("required target command exists");
        assert!(command
            .argv
            .windows(2)
            .any(|pair| pair == ["--test", target.target]));
        assert!(command
            .tests
            .as_ref()
            .is_some_and(|tests| tests.tests_run > 0));
        let receipt = command
            .process_receipt
            .as_ref()
            .expect("executor receipt exists");
        assert!(receipt.pid > 0 && receipt.reaped && receipt.temp_paths_published);
    }
    assert!(artifacts.coverage.iter().all(|row| {
        !row.assertions.is_empty() && row.assertions.iter().all(|assertion| assertion.passed)
    }));
    let cleanup: serde_json::Value = serde_json::from_slice(&fs::read(
        config.evidence_root.join(&artifacts.cleanup_records[0]),
    )?)?;
    assert_eq!(cleanup["processes_started"], artifacts.commands.len());
    assert_eq!(
        cleanup["temporary_artifacts_removed"],
        artifacts.commands.len() * 2 + 1
    );
    for path in [
        "manifest.json",
        "coverage-matrix.json",
        "owner-audits.json",
        "facts.json",
        "surfaces.json",
        "surface-owner.json",
        "results.json",
        "failure-triage.json",
        "summary.md",
    ] {
        assert!(config.evidence_root.join(path).is_file(), "missing {path}");
    }
    assert!(!config
        .evidence_root
        .join("fixtures/success/target")
        .exists());
    let runtime_tui_path = config.evidence_root.join("surface/tui-runtime.txt");
    let runtime_tui = fs::read(&runtime_tui_path)?;
    fs::write(&runtime_tui_path, b"stale runtime TUI")?;
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts)
            .expect_err("runtime TUI tamper must break artifact integrity"),
        Spec030ReleaseArtifactError::ManifestMismatch
    );
    fs::write(&runtime_tui_path, runtime_tui)?;
    let no_session_tui_path = config.evidence_root.join("surface/tui-no-session.txt");
    let no_session_tui = fs::read(&no_session_tui_path)?;
    fs::remove_file(&no_session_tui_path)?;
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts)
            .expect_err("omitted no-session TUI must break artifact integrity"),
        Spec030ReleaseArtifactError::ManifestMismatch
    );
    fs::write(no_session_tui_path, no_session_tui)?;
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_tampered_surface_owner_lifecycle(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("surface-owner-tamper");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.surface_owner.bound_port = 8080;

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("surface owner port must remain bound to its receipt");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_unreaped_surface_owner() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let config = fixture_config("surface-owner-live");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.surface_owner.shutdown = Spec030SurfaceOwnerShutdown::Requested;

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("surface owner shutdown must be complete");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::InvalidSurfaceEvidence);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_zero_tests() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("zero-tests");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.commands[0]
        .tests
        .as_mut()
        .expect("test counts")
        .tests_run = 0;

    // When
    let error = validate_spec030_release_artifacts(&artifacts).expect_err("zero tests block");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::ZeroTestsRun);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_target_without_explicit_test_selector(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("missing-test-selector");
    let mut artifacts = run_spec030_release_runner(&config)?;
    let command = artifacts
        .commands
        .iter_mut()
        .find(|command| command.id == spec030_integration_targets()[0].command_id)
        .expect("target command exists");
    command.argv = vec!["cargo".to_owned(), "test".to_owned()];

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("broad cargo filter is not exact target evidence");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::CommandFailed);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_missing_semantic_assertion(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("missing-assertion");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.coverage[0].assertions.clear();

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("coverage row requires named assertions");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::MissingCoverageRow);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_raw_credential_canary() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let config = fixture_config("raw-canary");
    let artifacts = run_spec030_release_runner(&config)?;
    fs::write(
        config.evidence_root.join("commands/injected.stdout"),
        "Authorization: Bearer SPEC030_RAW_CREDENTIAL_CANARY",
    )?;

    // When
    let error = validate_spec030_release_artifacts(&artifacts).expect_err("raw material blocks");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::RawCredentialMaterial);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_false_supported_claim() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let config = fixture_config("false-supported");
    let mut artifacts = run_spec030_release_runner(&config)?;
    let sandbox = artifacts
        .facts
        .iter_mut()
        .find(|fact| fact.id == "sandbox")
        .expect("sandbox fact exists");
    sandbox.status = "supported".to_owned();
    sandbox.evidence.clear();

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("supported requires concrete evidence");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::FalseSupportedClaim);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_credential_fact_mismatched_with_surface_evidence(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("fake-resolved");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts
        .facts
        .iter_mut()
        .find(|fact| fact.id == "credential")
        .expect("credential fact exists")
        .status = "missing".to_owned();

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("credential fact must match captured surface status");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::FalseSupportedClaim);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_tampered_cleanup_payload(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("record-tamper");
    let artifacts = run_spec030_release_runner(&config)?;
    let cleanup_path = config.evidence_root.join(&artifacts.cleanup_records[0]);
    let mut cleanup: serde_json::Value = serde_json::from_slice(&fs::read(&cleanup_path)?)?;
    cleanup["processes_started"] = serde_json::json!(999);
    fs::write(&cleanup_path, serde_json::to_vec_pretty(&cleanup)?)?;

    // When / Then
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("cleanup tamper fails"),
        Spec030ReleaseArtifactError::InvalidCleanupRecord
    );
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_process_receipt_with_remaining_temp_path(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("remaining-temp-path");
    let mut artifacts = run_spec030_release_runner(&config)?;
    let receipt = artifacts.commands[0]
        .process_receipt
        .as_mut()
        .expect("process receipt exists");
    receipt.stdout_temp_path = config
        .evidence_root
        .join("summary.md")
        .display()
        .to_string();

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("an existing temporary path invalidates cleanup");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::InvalidCleanupRecord);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_unbound_cleanup_receipts(
) -> Result<(), Box<dyn std::error::Error>> {
    let config = fixture_config("unbound-cleanup");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.commands[0].process_receipt = None;
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("receipt is required"),
        Spec030ReleaseArtifactError::InvalidCleanupRecord
    );

    let mut artifacts = run_spec030_release_runner(&fixture_config("live-pid"))?;
    let receipt = artifacts.commands[0]
        .process_receipt
        .as_mut()
        .expect("receipt exists");
    receipt.pid = std::process::id();
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("live pid fails"),
        Spec030ReleaseArtifactError::InvalidCleanupRecord
    );

    let mut artifacts = run_spec030_release_runner(&fixture_config("escaped-temp"))?;
    let receipt = artifacts.commands[0]
        .process_receipt
        .as_mut()
        .expect("receipt exists");
    receipt.stdout_temp_path = "../outside.tmp".to_owned();
    receipt.stderr_temp_path = "/tmp/outside.tmp".to_owned();
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("paths are root-bound"),
        Spec030ReleaseArtifactError::InvalidCleanupRecord
    );
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_placeholder_manual_payload(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("manual-tamper");
    let artifacts = run_spec030_release_runner(&config)?;
    let manual_path = config.evidence_root.join(&artifacts.manual_records[0]);
    fs::write(&manual_path, br#"{"status":"provided","record":0}"#)?;

    // When / Then
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("placeholder manual fails"),
        Spec030ReleaseArtifactError::InvalidManualRecord
    );
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_missing_cleanup_and_manual_records(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("missing-records");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.cleanup_records.clear();

    // When
    let cleanup_error =
        validate_spec030_release_artifacts(&artifacts).expect_err("cleanup receipt is mandatory");

    // Then
    assert_eq!(
        cleanup_error,
        Spec030ReleaseArtifactError::MissingCleanupRecord
    );
    artifacts
        .cleanup_records
        .push("cleanup/fixture.json".to_owned());
    artifacts.manual_records.clear();
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("manual record is mandatory"),
        Spec030ReleaseArtifactError::MissingManualRecord
    );
    Ok(())
}

#[test]
fn spec030_release_runner_current_dirty_worktree_writes_blocked_artifacts(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let repo = temp_path("dirty-repo");
    fs::create_dir_all(&repo)?;
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&repo)
        .output()?;
    fs::write(repo.join("dirty.txt"), "dirty")?;
    let mut config = fixture_config("dirty-worktree");
    config.repo_root = repo;
    config.mode = Spec030ReleaseRunnerMode::CurrentWorktree;

    // When
    let artifacts = run_spec030_release_runner(&config)?;

    // Then
    assert_eq!(artifacts.verdict, Spec030ReleaseVerdict::Blocked);
    assert_eq!(artifacts.mode, Spec030ReleaseRunnerMode::CurrentWorktree);
    assert!(artifacts
        .blockers
        .iter()
        .any(|blocker| blocker.code == "dirty_worktree"));
    assert!(artifacts
        .facts
        .iter()
        .filter(|fact| fact.status == "unverified")
        .all(|fact| fact.evidence.is_empty()));
    assert!(config.evidence_root.join("failure-triage.json").is_file());
    Ok(())
}

#[cfg(unix)]
#[test]
fn spec030_release_runner_rejects_symlink_evidence_root() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::symlink;

    // Given
    let target = temp_path("symlink-target");
    fs::create_dir_all(&target)?;
    let config = fixture_config("symlink-root");
    symlink(&target, &config.evidence_root)?;

    // When
    let error = run_spec030_release_runner(&config).expect_err("symlink root is rejected");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::Io);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_missing_artifact_and_coverage_row(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("missing-artifact");
    let mut artifacts = run_spec030_release_runner(&config)?;
    fs::remove_file(config.evidence_root.join("summary.md"))?;

    // When
    let artifact_error =
        validate_spec030_release_artifacts(&artifacts).expect_err("required summary is mandatory");

    // Then
    assert_eq!(
        artifact_error,
        Spec030ReleaseArtifactError::InvalidArtifactPath
    );
    fs::write(config.evidence_root.join("summary.md"), "restored")?;
    artifacts.coverage.pop();
    assert_eq!(
        validate_spec030_release_artifacts(&artifacts).expect_err("every PRD row is mandatory"),
        Spec030ReleaseArtifactError::MissingCoverageRow
    );
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_inexact_owner_audit() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("inexact-owner-audit");
    let mut artifacts = run_spec030_release_runner(&config)?;
    artifacts.owner_audits[0].source_locator = "generic/prose.md".to_owned();

    // When
    let error = validate_spec030_release_artifacts(&artifacts)
        .expect_err("generic owner prose is not exact evidence");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::InvalidOwnerAudit);
    Ok(())
}

#[test]
fn spec030_release_runner_rejects_preexisting_evidence_root(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let config = fixture_config("preexisting-root");
    fs::create_dir_all(&config.evidence_root)?;
    fs::write(config.evidence_root.join("sentinel"), "keep")?;

    // When
    let error = run_spec030_release_runner(&config).expect_err("nonempty root is immutable");

    // Then
    assert_eq!(error, Spec030ReleaseArtifactError::Io);
    assert_eq!(
        fs::read_to_string(config.evidence_root.join("sentinel"))?,
        "keep"
    );
    Ok(())
}
