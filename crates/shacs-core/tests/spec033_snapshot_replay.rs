mod spec033_snapshot_replay_support;

use shacs_core::runtime::{
    replay_recorded_trajectory, RecordedBoundaryRequirement, RecordedTrajectoryReplayError,
    RecordedTrajectoryStore,
};
use spec033_snapshot_replay_support::{recorded_trajectory, write_trajectory};
use std::error::Error;
use std::sync::{Arc, Barrier};

#[test]
fn replay_reads_snapshot_and_owner_outcomes_by_trajectory_id() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    write_trajectory(&store, recorded_trajectory())?;

    // When
    let receipt = replay_recorded_trajectory(&store, "trajectory-004", "run-004")?;

    // Then
    assert_eq!(receipt.trajectory_id, "trajectory-004");
    assert_eq!(receipt.result.case_results.len(), 1);
    assert_eq!(receipt.compared_recorded_outcomes, 1);
    assert!(receipt.result.started_at_ms > 0);
    assert!(receipt.result.completed_at_ms >= receipt.result.started_at_ms);
    Ok(())
}

#[test]
fn replay_rejects_tampered_snapshot_or_recorded_source() -> Result<(), Box<dyn Error>> {
    // Given
    let snapshot_root = tempfile::tempdir()?;
    let snapshot_store = RecordedTrajectoryStore::open(snapshot_root.path())?;
    let snapshot_record = write_trajectory(&snapshot_store, recorded_trajectory())?;
    std::fs::write(
        snapshot_root.path().join(snapshot_record.snapshot.locator),
        b"tampered",
    )?;
    let source_root = tempfile::tempdir()?;
    let source_store = RecordedTrajectoryStore::open(source_root.path())?;
    let source_record = write_trajectory(&source_store, recorded_trajectory())?;
    std::fs::write(
        source_root.path().join(&source_record.sources[0].locator),
        b"tampered",
    )?;

    // When
    let snapshot_result = replay_recorded_trajectory(&snapshot_store, "trajectory-004", "run");
    let source_result = replay_recorded_trajectory(&source_store, "trajectory-004", "run");

    // Then
    assert_eq!(
        snapshot_result,
        Err(RecordedTrajectoryReplayError::ArtifactDigestMismatch)
    );
    assert_eq!(
        source_result,
        Err(RecordedTrajectoryReplayError::ArtifactDigestMismatch)
    );
    Ok(())
}

#[test]
fn replay_rejects_record_requiring_live_destructive_boundary() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    let mut trajectory = recorded_trajectory();
    trajectory.boundary_requirement = RecordedBoundaryRequirement::LiveDestructive;
    write_trajectory(&store, trajectory)?;

    // When
    let result = replay_recorded_trajectory(&store, "trajectory-004", "run");

    // Then
    assert_eq!(
        result,
        Err(RecordedTrajectoryReplayError::LiveBoundaryRequired)
    );
    Ok(())
}

#[test]
fn trajectory_store_is_append_only() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    write_trajectory(&store, recorded_trajectory())?;

    // When
    let result = write_trajectory(&store, recorded_trajectory());

    // Then
    assert!(result.is_err());
    Ok(())
}

#[test]
fn trajectory_publish_cleans_staging_residue_before_retry() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let residue = root.path().join(".trajectory-staging/interrupted");
    std::fs::create_dir_all(&residue)?;
    std::fs::write(residue.join("partial"), b"partial")?;

    // When
    let store = RecordedTrajectoryStore::open(root.path())?;
    let record = write_trajectory(&store, recorded_trajectory())?;

    // Then
    assert_eq!(record.trajectory_id, "trajectory-004");
    assert!(!root.path().join(".trajectory-staging").exists());
    Ok(())
}

#[test]
fn concurrent_same_trajectory_publishes_one_complete_record() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Arc::new(RecordedTrajectoryStore::open(root.path())?);
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.write(recorded_trajectory())
            })
        })
        .collect::<Vec<_>>();

    // When
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("writer thread completes"))
        .collect::<Vec<_>>();

    // Then
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    replay_recorded_trajectory(&store, "trajectory-004", "run")?;
    assert!(!root.path().join(".trajectory-staging").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn trajectory_store_rejects_symlink_root() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    // Given
    let parent = tempfile::tempdir()?;
    let target = parent.path().join("target");
    let link = parent.path().join("store");
    std::fs::create_dir(&target)?;
    symlink(&target, &link)?;

    // When
    let result = RecordedTrajectoryStore::open(&link);

    // Then
    assert!(result.is_err());
    Ok(())
}
