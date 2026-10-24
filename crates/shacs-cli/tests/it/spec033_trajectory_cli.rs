use shacs_cli::{parse_cli_args, run_command};
use shacs_core::runtime::{RecordedBoundaryRequirement, RecordedTrajectoryStore};
use shacs_eval::evaluator::VerdictKind;

#[test]
fn local_no_provider_automation_records_real_owner_trajectory(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config = root.path().join("config.json");
    let trajectory_root = root.path().join("trajectory-store");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(
        &config,
        serde_json::to_vec_pretty(&serde_json::json!({
            "agents": {"defaults": {"workspace": workspace}},
        }))?,
    )?;

    // When
    let output = run_command(parse_cli_args([
        "trajectory",
        "record",
        "--config",
        &config.display().to_string(),
        "--workspace",
        &workspace.display().to_string(),
        "--store",
        &trajectory_root.display().to_string(),
        "--trajectory-id",
        "production-no-provider-1",
        "--instruction",
        "check local runtime health",
    ])?)?;

    // Then
    let receipt: serde_json::Value = serde_json::from_str(&output)?;
    assert_eq!(receipt["trajectory_id"], "production-no-provider-1");
    let store = RecordedTrajectoryStore::open(&trajectory_root)?;
    let record = store.read("production-no-provider-1")?;
    assert_eq!(
        record.boundary_requirement,
        RecordedBoundaryRequirement::RecordedOnly
    );
    assert_eq!(record.owner_outcome.actual_verdict, Some(VerdictKind::Pass));
    assert_eq!(
        record.owner_outcome.actual_outcome.as_ref(),
        Some(&record.owner_outcome.expected_outcome)
    );
    assert_eq!(
        record.owner_outcome.actual_projection_status,
        Some(record.owner_outcome.expected_projection_status)
    );
    assert_eq!(record.sources.len(), 1);
    assert!(!record.owner_outcome.diagnostics_refs.is_empty());
    assert_eq!(receipt["record_digest"], record.record_digest);
    Ok(())
}
