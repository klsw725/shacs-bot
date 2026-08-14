mod spec033_snapshot_replay_support;

use shacs_core::runtime::{
    build_spec033_snapshot_from, run_spec033_release_runner, validate_spec033_release_artifacts,
    validate_spec033_release_artifacts_against, validate_spec033_release_coverage,
    RecordedTrajectoryStore, Spec033ReleaseCheck, Spec033ReleaseConfig, Spec033ReleaseMode,
};
use shacs_projection::Spec033Availability;
use spec033_snapshot_replay_support::{recorded_trajectory, write_trajectory};
use std::error::Error;
use std::time::Duration;

#[test]
fn release_checks_render_distinct_shell_free_cargo_commands() {
    // Given
    let checks = Spec033ReleaseCheck::required();

    // When
    let commands = checks.map(Spec033ReleaseCheck::cargo_args);

    // Then
    assert_eq!(commands.iter().filter(|args| args[0] == "test").count(), 5);
    assert_eq!(
        commands
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5
    );
    assert!(commands
        .iter()
        .all(|args| args.iter().all(|arg| arg != "sh")));
}

#[test]
fn release_checks_are_command_evidence_kinds_not_review_verdicts() -> Result<(), Box<dyn Error>> {
    // Given
    let checks = Spec033ReleaseCheck::required();

    // When
    let serialized = checks
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;

    // Then
    assert_eq!(
        serialized,
        [
            "automation_dispatch",
            "goal_accounting",
            "snapshot_replay",
            "self_improvement",
            "review_artifacts",
        ]
        .map(serde_json::Value::from)
    );
    Ok(())
}

#[test]
fn redaction_receipt_authenticates_actual_bounded_transform() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.log");
    let output = root.path().join("redacted.log");
    std::fs::write(
        &source,
        b"OPENAI_API_KEY=sk-secret\nCompiling crate (/Users/local/checkout/crates/crate)",
    )?;

    // When
    let receipt = shacs_core::runtime::redact_spec033_artifact(&source, &output, 4096)?;

    // Then
    assert_ne!(receipt.source_digest, receipt.output_digest);
    let redacted = std::fs::read_to_string(output)?;
    assert!(!redacted.contains("sk-secret"));
    assert!(!redacted.contains("/Users/local/checkout"));
    assert!(redacted.contains("[REDACTED_PATH]"));
    Ok(())
}

#[test]
fn replay_evidence_comes_from_recorded_trajectory_store() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    write_trajectory(&store, recorded_trajectory())?;

    // When
    let receipt = shacs_core::runtime::collect_spec033_replay_evidence(
        root.path(),
        &root.path().join("replay-receipts"),
        "trajectory-004",
        "release-run",
    )?;

    // Then
    assert_eq!(receipt.trajectory_id, "trajectory-004");
    assert_eq!(receipt.compared_recorded_outcomes, 1);
    assert_eq!(receipt.result.run_id, "release-run");
    assert_eq!(receipt.correlation_id, "release-run");
    assert_eq!(
        receipt.redaction_status,
        shacs_eval::evaluator::RedactionStatus::AlreadySafe
    );
    assert!(root
        .path()
        .join("replay-receipts/release-run.json")
        .is_file());
    let snapshot = build_spec033_snapshot_from(root.path(), root.path(), "cli:direct")?;
    assert_eq!(snapshot.replay.availability, Spec033Availability::Available);
    assert_eq!(
        snapshot
            .replay
            .fact
            .as_ref()
            .map(|fact| fact.trajectory_id.as_str()),
        Some("trajectory-004")
    );
    assert_eq!(
        snapshot.diagnostics.trajectory_id.value.as_deref(),
        Some("trajectory-004")
    );
    assert_eq!(
        snapshot.diagnostics.execution_snapshot_id.value.as_deref(),
        Some(receipt.snapshot_id.as_str())
    );
    assert_eq!(
        snapshot
            .diagnostics
            .execution_snapshot_digest
            .value
            .as_deref(),
        Some(receipt.snapshot_digest.as_str())
    );
    assert_eq!(
        snapshot
            .replay
            .fact
            .as_ref()
            .map(|fact| fact.correlation_id.as_str()),
        Some("release-run")
    );
    Ok(())
}

#[test]
fn replay_projection_reads_receipt_from_explicit_root_when_trajectory_root_is_distinct(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let data_dir = root.path().join("runtime-data");
    let receipt_root = data_dir.join("replay-receipts");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;

    // When
    shacs_core::runtime::collect_spec033_replay_evidence(
        &trajectory_root,
        &receipt_root,
        "trajectory-004",
        "distinct-roots",
    )?;
    let snapshot = build_spec033_snapshot_from(root.path(), &data_dir, "cli:direct")?;

    // Then
    assert!(receipt_root.join("distinct-roots.json").is_file());
    assert_eq!(snapshot.replay.availability, Spec033Availability::Available);
    assert_eq!(
        snapshot
            .replay
            .fact
            .as_ref()
            .map(|fact| fact.correlation_id.as_str()),
        Some("distinct-roots")
    );
    Ok(())
}

#[test]
fn replay_projection_uses_completion_time_not_receipt_filename() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    write_trajectory(&store, recorded_trajectory())?;
    let old = shacs_core::runtime::collect_spec033_replay_evidence(
        root.path(),
        &root.path().join("replay-receipts"),
        "trajectory-004",
        "zzz-old",
    )?;
    let new = shacs_core::runtime::collect_spec033_replay_evidence(
        root.path(),
        &root.path().join("replay-receipts"),
        "trajectory-004",
        "aaa-new",
    )?;
    let mut old = serde_json::to_value(old)?;
    old["result"]["completed_at_ms"] = serde_json::json!(10);
    let mut new = serde_json::to_value(new)?;
    new["result"]["completed_at_ms"] = serde_json::json!(20);
    std::fs::write(
        root.path().join("replay-receipts/zzz-old.json"),
        serde_json::to_vec_pretty(&old)?,
    )?;
    std::fs::write(
        root.path().join("replay-receipts/aaa-new.json"),
        serde_json::to_vec_pretty(&new)?,
    )?;

    // When
    let snapshot = build_spec033_snapshot_from(root.path(), root.path(), "cli:direct")?;

    // Then
    assert_eq!(
        snapshot
            .replay
            .fact
            .as_ref()
            .map(|fact| fact.correlation_id.as_str()),
        Some("aaa-new")
    );
    Ok(())
}

#[test]
fn replay_projection_requires_a_persisted_receipt() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;

    // When
    let snapshot = build_spec033_snapshot_from(root.path(), root.path(), "cli:direct")?;

    // Then
    assert_eq!(
        snapshot.replay.availability,
        Spec033Availability::Unavailable
    );
    Ok(())
}

#[test]
fn release_runner_receipt_is_visible_to_projection_when_runtime_data_dir_is_distinct(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let data_dir = root.path().join("runtime-data");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    let config = Spec033ReleaseConfig {
        run_id: "spec033-distinct-roots".to_owned(),
        repo_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: root.path().join("release"),
        trajectory_root,
        data_dir: data_dir.clone(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(600),
    };

    // When
    run_spec033_release_runner(&config)?;
    let snapshot = build_spec033_snapshot_from(root.path(), &data_dir, "cli:direct")?;

    // Then
    assert!(data_dir
        .join("replay-receipts/spec033-distinct-roots.json")
        .is_file());
    assert!(!config
        .trajectory_root
        .join("replay-receipts/spec033-distinct-roots.json")
        .exists());
    assert_eq!(snapshot.replay.availability, Spec033Availability::Available);
    assert_eq!(
        snapshot
            .replay
            .fact
            .as_ref()
            .map(|fact| fact.correlation_id.as_str()),
        Some("spec033-distinct-roots")
    );
    Ok(())
}

#[test]
fn release_runner_publishes_digest_bound_distinct_evidence() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let output = root.path().join("release");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    let config = Spec033ReleaseConfig {
        run_id: "spec033-test".to_owned(),
        repo_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: output.clone(),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    };

    // When
    let manifest = run_spec033_release_runner(&config)?;

    // Then
    assert_eq!(manifest.commands.len(), 5);
    assert_eq!(
        manifest.trajectory.record_path,
        "trajectories/trajectory-004/record.json"
    );
    assert!(!std::path::Path::new(&manifest.trajectory.record_path).is_absolute());
    assert_eq!(manifest.coverage.len(), 37);
    assert!(manifest.coverage.iter().all(|row| {
        !row.code_path.is_empty()
            && !row.test_command.is_empty()
            && row.artifact_digest.starts_with("sha256:")
            && row.evidence_source == row.artifact
            && !row.non_guarantee.is_empty()
    }));
    let requirements = manifest
        .coverage
        .iter()
        .map(|row| row.requirement.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!((1..=11).all(|item| requirements.contains(format!("033-MH{item:03}").as_str())));
    assert!((1..=11).all(|item| requirements.contains(format!("033-AC{item:03}").as_str())));
    assert!((0..=14).all(|item| requirements.contains(format!("018-PRD{item:03}").as_str())));
    assert_eq!(requirements.len(), 37);
    assert_eq!(manifest.blocker_coverage.len(), 17);
    assert!(!manifest.source_manifest.files.is_empty());
    assert!(manifest.source_manifest.digest.starts_with("sha256:"));
    assert_eq!(
        manifest
            .commands
            .iter()
            .map(|command| command.stdout_transform.output_digest.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5
    );
    validate_spec033_release_artifacts(&output)?;
    Ok(())
}

#[test]
fn release_validation_rejects_current_source_mutation() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let output = root.path().join("release");
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    let manifest = run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: "spec033-source-mutation".to_owned(),
        repo_root: repo.clone(),
        evidence_root: output.clone(),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    })?;
    let copied_repo = root.path().join("copied-repo");
    for row in &manifest.source_manifest.files {
        let destination = copied_repo.join(&row.locator);
        std::fs::create_dir_all(destination.parent().ok_or("source parent")?)?;
        std::fs::copy(repo.join(&row.locator), destination)?;
    }
    let mutated = copied_repo.join(&manifest.source_manifest.files[0].locator);
    std::fs::write(mutated, b"source mutation")?;

    // When
    let result = validate_spec033_release_artifacts_against(&output, &copied_repo);

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn release_coverage_validation_rejects_missing_duplicate_and_unknown_entries(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    let manifest = run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: "spec033-coverage-validation".to_owned(),
        repo_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: root.path().join("release"),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    })?;
    let mut missing = manifest.coverage.clone();
    missing.pop();
    let mut duplicate = manifest.coverage.clone();
    duplicate.push(duplicate[0].clone());
    let mut unknown = manifest.coverage.clone();
    unknown[0].requirement = "018-PRD999".to_owned();
    let mut incomplete = manifest.coverage.clone();
    incomplete[0].artifact_digest.clear();
    let mut nonexistent = manifest.coverage.clone();
    nonexistent[0].code_path = "crates/shacs-core/src/runtime/does-not-exist.rs".to_owned();
    let mut mismatched_command = manifest.coverage.clone();
    mismatched_command[0].test_command = manifest.coverage[1].test_command.clone();

    // When / Then
    assert!(validate_spec033_release_coverage(&missing).is_err());
    assert!(validate_spec033_release_coverage(&duplicate).is_err());
    assert!(validate_spec033_release_coverage(&unknown).is_err());
    assert!(validate_spec033_release_coverage(&incomplete).is_err());
    assert!(validate_spec033_release_coverage(&nonexistent).is_err());
    assert!(validate_spec033_release_coverage(&mismatched_command).is_err());
    Ok(())
}

#[test]
fn release_runner_cleans_raw_evidence_when_a_command_fails() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let invalid_repo = root.path().join("invalid-repo");
    std::fs::create_dir(&invalid_repo)?;
    let config = Spec033ReleaseConfig {
        run_id: "spec033-failed-cleanup".to_owned(),
        repo_root: invalid_repo,
        evidence_root: root.path().join("release"),
        trajectory_root: root.path().join("trajectories"),
        data_dir: root.path().to_path_buf(),
        trajectory_id: "missing".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(5),
    };

    // When
    let result = run_spec033_release_runner(&config);

    // Then
    assert!(result.is_err());
    assert!(std::fs::read_dir(root.path())?.all(|entry| {
        !entry
            .map(|value| value.file_name().to_string_lossy().contains("spec033-raw"))
            .unwrap_or(false)
    }));
    Ok(())
}

#[test]
fn release_validation_rejects_tampered_redacted_transcript() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let output = root.path().join("release");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: "spec033-tamper".to_owned(),
        repo_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: output.clone(),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    })?;
    std::fs::write(
        output.join("gates/automationdispatch/stdout.log"),
        b"tampered",
    )?;

    // When
    let result = validate_spec033_release_artifacts(&output);

    // Then
    assert!(result.is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn release_validation_rejects_unsafe_artifact_references_and_transform_schema(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let output = root.path().join("release");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: "spec033-artifact-safety".to_owned(),
        repo_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: output.clone(),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    })?;
    let manifest_path = output.join("manifest.json");
    let manifest_bytes = std::fs::read(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    let stdout = output.join("gates/automationdispatch/stdout.log");
    let stdout_bytes = std::fs::read(&stdout)?;
    std::fs::write(root.path().join("outside.log"), &stdout_bytes)?;

    // When / Then: a locator that escapes the release root is rejected.
    let mut traversal = manifest.clone();
    traversal["commands"][0]["redacted_stdout"] = serde_json::json!("../outside.log");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&traversal)?)?;
    assert!(validate_spec033_release_artifacts(&output).is_err());

    // When / Then: a symlink artifact is rejected even when its bytes match.
    std::fs::write(&manifest_path, &manifest_bytes)?;
    std::fs::remove_file(&stdout)?;
    std::os::unix::fs::symlink(root.path().join("outside.log"), &stdout)?;
    assert!(validate_spec033_release_artifacts(&output).is_err());

    // When / Then: a redaction transform using another schema is rejected.
    std::fs::remove_file(&stdout)?;
    std::fs::write(&stdout, stdout_bytes)?;
    let mut schema_mutation = manifest;
    schema_mutation["commands"][0]["stdout_transform"]["schema"] =
        serde_json::json!("spec033.redaction_transform.v2");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&schema_mutation)?)?;
    assert!(validate_spec033_release_artifacts(&output).is_err());
    Ok(())
}

#[test]
fn release_validation_rejects_failed_replay_and_command_mismatch() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let output = root.path().join("release");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: "spec033-semantic-tamper".to_owned(),
        repo_root: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: output.clone(),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    })?;
    let manifest_path = output.join("manifest.json");
    let mut failed_replay: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    failed_replay["replay"]["result"]["status"] = serde_json::json!("failed");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&failed_replay)?)?;

    // When
    let replay_result = validate_spec033_release_artifacts(&output);
    failed_replay["replay"]["result"]["status"] = serde_json::json!("passed");
    failed_replay["commands"][0]["command"]["argv"] =
        failed_replay["commands"][1]["command"]["argv"].clone();
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&failed_replay)?)?;
    let command_result = validate_spec033_release_artifacts(&output);

    // Then
    assert!(replay_result.is_err());
    assert!(command_result.is_err());
    Ok(())
}

#[test]
#[ignore = "manual CLI fixture generator"]
fn writes_manual_release_runner_trajectory_fixture() -> Result<(), Box<dyn Error>> {
    // Given
    let root = std::env::var_os("SPEC033_TRAJECTORY_FIXTURE")
        .map(std::path::PathBuf::from)
        .ok_or("missing SPEC033_TRAJECTORY_FIXTURE")?;
    let store = RecordedTrajectoryStore::open(root)?;

    // When
    let record = write_trajectory(&store, recorded_trajectory())?;

    // Then
    assert_eq!(record.trajectory_id, "trajectory-004");
    Ok(())
}
