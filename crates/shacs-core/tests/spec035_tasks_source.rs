use shacs_core::runtime::build_spec035_tasks_projection;
use shacs_projection::{Spec035TaskCount, Spec035TaskOwnerKind, Spec035TaskState};
use shacs_session::durable_replay::{evaluate_durable_recovery, DurableRecoveryStatus};

#[test]
fn healthy_empty_durable_source_reports_observed_zero_children(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("runtime/durable-events");
    std::fs::create_dir_all(&event_root)?;
    std::fs::write(event_root.join("events.log"), b"")?;
    let admission =
        evaluate_durable_recovery(&event_root, root.path().join("runtime/durable-checkpoints"));
    assert_eq!(admission.status, DurableRecoveryStatus::Healthy);
    assert!(admission.state.is_some());

    let projection = build_spec035_tasks_projection(root.path(), root.path(), "cli:direct")?;

    assert_eq!(projection.coverage().child, Spec035TaskCount::available(0));
    Ok(())
}

#[test]
fn spec035_f2_corrupt_durable_source_keeps_child_unavailable(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("runtime/durable-events");
    std::fs::create_dir_all(&event_root)?;
    std::fs::write(
        event_root.join("events.log"),
        b"corrupt durable event store\n",
    )?;
    let admission =
        evaluate_durable_recovery(&event_root, root.path().join("runtime/durable-checkpoints"));
    assert_eq!(admission.status, DurableRecoveryStatus::Blocked);
    assert!(admission.state.is_none());

    let projection = build_spec035_tasks_projection(root.path(), root.path(), "cli:direct")?;

    assert_eq!(projection.coverage().child, Spec035TaskCount::Unavailable);
    assert_eq!(
        projection.coverage().recovery,
        Spec035TaskCount::available(1)
    );
    let recovery = projection
        .rows()
        .iter()
        .find(|row| row.owner().kind() == Spec035TaskOwnerKind::Recovery)
        .ok_or("recovery row")?;
    assert_eq!(recovery.state(), Spec035TaskState::Blocked);
    Ok(())
}
