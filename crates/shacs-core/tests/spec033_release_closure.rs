mod spec033_snapshot_replay_support;

use shacs_core::runtime::{
    run_spec033_release_runner, validate_spec033_release_artifacts, RecordedTrajectoryStore,
    Spec033ReleaseConfig, Spec033ReleaseMode,
};
use spec033_snapshot_replay_support::{recorded_trajectory, write_trajectory};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BLOCKERS: [&str; 17] = [
    "HookVeto",
    "HeadlessConfirmationDenied",
    "MissingHookEvidence",
    "ProcessTimeout",
    "AbortCleanupIncomplete",
    "SnapshotMissing",
    "SandboxUnsupported",
    "SandboxFailed",
    "Credential",
    "SnapshotMismatch",
    "SourceMutation",
    "MissingRedactionEvidence",
    "Duplicate",
    "Superseded",
    "Recursion",
    "Delivery",
    "ReplayMismatch",
];

#[test]
fn closure_executes_one_exact_passing_test_per_blocker() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let output = run_release(root.path(), "edge-matrix")?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("manifest.json"))?)?;

    // When
    let edges = manifest["edge_commands"]
        .as_array()
        .ok_or("missing edge_commands")?;

    // Then
    assert_eq!(edges.len(), BLOCKERS.len());
    for blocker in BLOCKERS {
        let edge = edges
            .iter()
            .find(|edge| edge["blocker"] == blocker)
            .ok_or("missing blocker edge")?;
        assert_eq!(edge["command"]["status"], "passed");
        assert_eq!(edge["command"]["tests"]["tests_run"], 1);
        assert_eq!(edge["command"]["argv"].as_array().map(Vec::len), Some(11));
        assert_eq!(edge["command"]["argv"][10], "--exact");
        assert_eq!(edge["test_id"], edge["command"]["argv"][8]);
        assert!(edge["artifact_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")));
    }
    Ok(())
}

#[test]
fn closure_summary_is_canonical_and_digest_bound() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let output = run_release(root.path(), "summary-contract")?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("manifest.json"))?)?;

    // When
    let summary = std::fs::read_to_string(output.join("summary.md"))?;

    // Then
    for heading in [
        "## Commands",
        "## Artifacts",
        "## Failures",
        "## Disclosure",
        "## Cleanup",
        "## Non-guarantees",
    ] {
        assert!(summary.contains(heading), "missing {heading}");
    }
    assert!(manifest["blocked_non_guarantees"]
        .as_array()
        .is_some_and(|values| !values.is_empty()));
    assert!(manifest["artifact_digests"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["locator"] == "summary.md")));
    Ok(())
}

#[test]
fn closure_validation_rejects_summary_tamper_and_deletion() -> Result<(), Box<dyn Error>> {
    // Given
    let tampered = tempfile::tempdir()?;
    let tampered_output = run_release(tampered.path(), "summary-tamper")?;
    std::fs::write(tampered_output.join("summary.md"), b"tampered")?;
    let deleted = tempfile::tempdir()?;
    let deleted_output = run_release(deleted.path(), "summary-delete")?;
    std::fs::remove_file(deleted_output.join("summary.md"))?;

    // When / Then
    assert!(validate_spec033_release_artifacts(&tampered_output).is_err());
    assert!(validate_spec033_release_artifacts(&deleted_output).is_err());
    Ok(())
}

#[test]
fn closure_validation_rejects_edge_identity_result_and_digest_tamper() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let output = run_release(root.path(), "edge-tamper")?;
    let path = output.join("manifest.json");
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;

    // When / Then
    for (field, replacement) in [
        ("test_id", serde_json::json!("wrong_test")),
        ("artifact_digest", serde_json::json!("sha256:wrong")),
    ] {
        let mut manifest = original.clone();
        manifest["edge_commands"][0][field] = replacement;
        std::fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
        assert!(validate_spec033_release_artifacts(&output).is_err());
    }
    let mut manifest = original;
    manifest["edge_commands"][0]["command"]["status"] = serde_json::json!("failed");
    std::fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    assert!(validate_spec033_release_artifacts(&output).is_err());
    Ok(())
}

#[test]
fn closure_validation_rejects_manifest_deletion() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let output = run_release(root.path(), "manifest-delete")?;
    std::fs::remove_file(output.join("manifest.json"))?;

    // When / Then
    assert!(validate_spec033_release_artifacts(&output).is_err());
    Ok(())
}

#[test]
fn current_worktree_closure_rejects_fixture_trajectory_id() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let trajectory_root = root.path().join("trajectories");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;

    // When
    let result = run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: "fixture-rejected".to_owned(),
        repo_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: root.path().join("release"),
        trajectory_root,
        data_dir: root.path().to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::CurrentWorktree,
        command_timeout: Duration::from_secs(120),
    });

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn closure_validation_rejects_non_guarantee_and_digest_row_deletion() -> Result<(), Box<dyn Error>>
{
    // Given
    let non_guarantee = tempfile::tempdir()?;
    let non_guarantee_output = run_release(non_guarantee.path(), "non-guarantee-delete")?;
    mutate_manifest(&non_guarantee_output, |manifest| {
        manifest["blocked_non_guarantees"] = serde_json::json!([]);
    })?;
    let digest = tempfile::tempdir()?;
    let digest_output = run_release(digest.path(), "digest-delete")?;
    mutate_manifest(&digest_output, |manifest| {
        manifest["artifact_digests"]
            .as_array_mut()
            .expect("digests")
            .pop();
    })?;

    // When / Then
    assert!(validate_spec033_release_artifacts(&non_guarantee_output).is_err());
    assert!(validate_spec033_release_artifacts(&digest_output).is_err());
    Ok(())
}

fn run_release(root: &Path, run_id: &str) -> Result<PathBuf, Box<dyn Error>> {
    let trajectory_root = root.join("trajectories");
    let output = root.join("release");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    write_trajectory(&store, recorded_trajectory())?;
    run_spec033_release_runner(&Spec033ReleaseConfig {
        run_id: run_id.to_owned(),
        repo_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        evidence_root: output.clone(),
        trajectory_root,
        data_dir: root.to_path_buf(),
        trajectory_id: "trajectory-004".to_owned(),
        mode: Spec033ReleaseMode::Fixture,
        command_timeout: Duration::from_secs(120),
    })?;
    Ok(output)
}

fn mutate_manifest(
    output: &Path,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<(), Box<dyn Error>> {
    let path = output.join("manifest.json");
    let mut manifest = serde_json::from_slice(&std::fs::read(&path)?)?;
    mutate(&mut manifest);
    std::fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}
