use serde_json::json;
use shacs_core::runtime::{
    recover_runtime_surface, request_runtime_control, request_surface_approval,
    runtime_stop_request_marker_path, SurfaceActionOutcomeKind, SurfaceActionRequestKind,
    SURFACE_APPROVAL_WORK_KIND,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use std::error::Error;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn runtime_control_is_unavailable_when_owner_is_absent() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;

    let outcome = request_runtime_control(root.path(), SurfaceActionRequestKind::Stop, now_ms())?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Unavailable);
    assert!(!outcome.changed);
    assert!(outcome.detail.contains("no active runtime owner"));
    Ok(())
}

#[test]
fn runtime_control_writes_request_for_active_owner() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(root.path(), std::process::id(), now, now + 60_000)?;

    let outcome = request_runtime_control(root.path(), SurfaceActionRequestKind::Restart, now)?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Requested);
    assert!(outcome.changed);
    let raw = fs::read_to_string(runtime_stop_request_marker_path(root.path()))?;
    let marker: serde_json::Value = serde_json::from_str(&raw)?;
    assert_eq!(marker["request"], json!("restart"));
    assert!(marker["request_id"]
        .as_str()
        .is_some_and(|value| value.starts_with("restart-owner-")));
    assert!(marker["event_sequence"].as_u64().is_some());
    Ok(())
}

#[test]
fn runtime_control_is_unavailable_for_stale_owner() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(
        root.path(),
        std::process::id(),
        now.saturating_sub(60_000),
        now.saturating_sub(1),
    )?;

    let outcome = request_runtime_control(root.path(), SurfaceActionRequestKind::Stop, now)?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Unavailable);
    assert!(!outcome.changed);
    assert!(outcome.detail.contains("stale ownership marker"));
    Ok(())
}

#[test]
fn runtime_recover_completes_noop_when_state_is_healthy() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;

    let outcome = recover_runtime_surface(root.path(), now_ms())?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Completed);
    assert!(!outcome.changed);
    assert!(outcome.detail.contains("no runtime update"));
    Ok(())
}

#[test]
fn runtime_recover_clears_stale_dead_owner_marker() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(root.path(), u32::MAX, now.saturating_sub(60_000), now - 1)?;

    let outcome = recover_runtime_surface(root.path(), now)?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Completed);
    assert!(outcome.changed);
    assert!(!root.path().join("runtime/ownership-marker.json").exists());
    Ok(())
}

#[test]
fn runtime_recover_blocks_active_owner() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(root.path(), std::process::id(), now, now + 60_000)?;

    let outcome = recover_runtime_surface(root.path(), now)?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Unavailable);
    assert!(!outcome.changed);
    assert!(outcome.detail.contains("active runtime owner"));
    Ok(())
}

#[test]
fn runtime_recover_blocks_partial_migration_marker() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let marker_path = root.path().join("runtime/update-marker.json");
    fs::create_dir_all(marker_path.parent().ok_or("missing marker parent")?)?;
    fs::write(
        marker_path,
        serde_json::to_string_pretty(&json!({
            "phase": "partial_migration",
            "fromVersion": "0.1.0",
            "targetVersion": "0.2.0",
            "migrationRequired": true
        }))?,
    )?;

    let outcome = recover_runtime_surface(root.path(), now_ms())?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Unavailable);
    assert!(!outcome.changed);
    assert!(outcome.detail.contains("partial migration"));
    Ok(())
}

#[test]
fn surface_approval_enqueues_request_for_active_owner() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(root.path(), std::process::id(), now, now + 60_000)?;

    let outcome = request_surface_approval(root.path(), "cli:direct", "approval-1", true, now)?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::Requested);
    assert!(outcome.changed);
    let replay = durable_replay(root.path());
    let work = replay
        .state
        .ok_or("missing replay state")?
        .work
        .items
        .into_values()
        .find(|item| item.work_kind == SURFACE_APPROVAL_WORK_KIND)
        .ok_or("missing surface approval work")?;
    assert_eq!(work.session_key, "cli:direct");
    assert_eq!(work.effect_id.as_deref(), Some("approval-1"));
    assert_eq!(
        work.dedupe_hint.as_deref(),
        Some("surface_approval:cli:direct:approval-1")
    );
    Ok(())
}

#[test]
fn surface_approval_dedupes_matching_open_request() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(root.path(), std::process::id(), now, now + 60_000)?;

    let first = request_surface_approval(root.path(), "cli:direct", "approval-1", true, now)?;
    let second = request_surface_approval(root.path(), "cli:direct", "approval-1", true, now + 1)?;

    assert_eq!(first.kind, SurfaceActionOutcomeKind::Requested);
    assert!(first.changed);
    assert_eq!(second.kind, SurfaceActionOutcomeKind::Requested);
    assert!(!second.changed);
    let open_count = durable_replay(root.path())
        .state
        .ok_or("missing replay state")?
        .work
        .items
        .values()
        .filter(|item| item.work_kind == SURFACE_APPROVAL_WORK_KIND)
        .count();
    assert_eq!(open_count, 1);
    Ok(())
}

#[test]
fn surface_approval_rejects_conflicting_open_request() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let now = now_ms();
    write_owner_marker(root.path(), std::process::id(), now, now + 60_000)?;

    request_surface_approval(root.path(), "cli:direct", "approval-1", true, now)?;
    let outcome =
        request_surface_approval(root.path(), "cli:direct", "approval-1", false, now + 1)?;

    assert_eq!(outcome.kind, SurfaceActionOutcomeKind::StaleLineage);
    assert!(!outcome.changed);
    let open_count = durable_replay(root.path())
        .state
        .ok_or("missing replay state")?
        .work
        .items
        .values()
        .filter(|item| item.work_kind == SURFACE_APPROVAL_WORK_KIND)
        .count();
    assert_eq!(open_count, 1);
    Ok(())
}

fn write_owner_marker(
    data_dir: &std::path::Path,
    pid: u32,
    acquired_at_ms: u64,
    expires_at_ms: u64,
) -> Result<(), Box<dyn Error>> {
    let marker_path = data_dir.join("runtime").join("ownership-marker.json");
    let parent = marker_path.parent().ok_or("missing marker parent")?;
    fs::create_dir_all(parent)?;
    fs::write(
        marker_path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "owner_id": format!("owner-{pid}-{acquired_at_ms}"),
            "pid": pid,
            "acquired_at_ms": acquired_at_ms,
            "renewed_at_ms": acquired_at_ms,
            "expires_at_ms": expires_at_ms,
            "lifecycle": "active",
            "binary_version": "test",
            "data_schema_version": 1,
            "mode": "runtime-start",
            "config_path": "/tmp/config.json",
            "workspace": "/tmp/workspace",
            "process_evidence": {
                "pid": pid,
                "pid_alive": true,
                "process_started_after_marker": false
            }
        }))?,
    )?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn durable_replay(
    data_dir: &std::path::Path,
) -> shacs_session::durable_replay::DurableReplayAdmission {
    evaluate_durable_recovery(
        data_dir.join("runtime").join("durable-events"),
        data_dir.join("runtime").join("durable-checkpoints"),
    )
}
