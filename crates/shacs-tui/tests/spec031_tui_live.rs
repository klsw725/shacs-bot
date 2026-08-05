use serde_json::{json, Map, Value};
use shacs_cli::spec031_surface_approval_fixture::FixtureRuntime;
use shacs_config::config_context;
use shacs_core::runtime::{SurfaceAction, SurfaceActionOutcomeKind};
use shacs_session::{Session, SessionManager};
use shacs_tui::action_runner::run_surface_action;
use shacs_tui::live_source::{RuntimeProjectionSource, SessionRuntimeSource};
use std::error::Error;
use std::fs;
use std::thread;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn real_live_source_detects_owner_state_changes() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    save_session(workspace.path(), "cli:live", 1, Some("approval-before"))?;
    let config_path = workspace.path().join("data").join("config.json");
    let source = SessionRuntimeSource::with_config(Some(config_path), workspace.path());

    let before = source.load()?;
    save_session(workspace.path(), "cli:live", 7, Some("approval-after"))?;
    let after = source.load()?;

    let before_session = before.sessions.first().ok_or("missing before session")?;
    let after_session = after.sessions.first().ok_or("missing after session")?;
    assert_eq!(
        before_session
            .workflow
            .as_ref()
            .and_then(|workflow| workflow.progress_count),
        Some(1)
    );
    assert_eq!(
        after_session
            .workflow
            .as_ref()
            .and_then(|workflow| workflow.progress_count),
        Some(7)
    );
    assert_eq!(
        after_session
            .pending_approval
            .as_ref()
            .map(|approval| approval.lineage.as_str()),
        Some("approval-after")
    );
    Ok(())
}

#[test]
fn live_source_marks_formal_approval_actionable_only_with_active_owner(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let context = config_context(
        Some(workspace.path().join("data").join("config.json")),
        Some(workspace.path().to_path_buf()),
    );
    save_session(workspace.path(), "cli:live", 1, Some("approval-before"))?;
    let source =
        SessionRuntimeSource::with_config(Some(context.config_path.clone()), workspace.path());

    let missing_owner = source.load()?;
    let approval = missing_owner
        .sessions
        .first()
        .and_then(|session| session.pending_approval.as_ref())
        .ok_or("missing pending approval")?;
    assert_eq!(
        approval.action,
        shacs_tui::state::ApprovalActionState::unavailable("no active runtime owner found")
    );

    let active_owner = write_owner_marker(&context.data_dir, now_ms(), now_ms())?;
    let active = source.load()?;
    let approval = active
        .sessions
        .first()
        .and_then(|session| session.pending_approval.as_ref())
        .ok_or("missing active pending approval")?;
    assert_eq!(
        approval.action,
        shacs_tui::state::ApprovalActionState::Actionable {
            target_owner_id: active_owner,
        }
    );

    write_owner_marker(&context.data_dir, now_ms(), 1)?;
    let stale = source.load()?;
    let approval = stale
        .sessions
        .first()
        .and_then(|session| session.pending_approval.as_ref())
        .ok_or("missing stale pending approval")?;
    assert_eq!(
        approval.action,
        shacs_tui::state::ApprovalActionState::unavailable(
            "stale ownership marker exists; run `shacs-bot runtime recover`"
        )
    );
    Ok(())
}

#[test]
fn action_runner_approval_reaches_production_consumer_once() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("data").join("config.json");
    let workspace = root.path().join("workspace");
    let runtime = FixtureRuntime::start(config_path.clone(), workspace.clone())?;
    let lineage = runtime
        .pending_lineage()?
        .ok_or("missing initial pending lineage")?;

    let requested = run_surface_action(
        Some(&config_path),
        &workspace,
        SurfaceAction::Approve {
            session_key: "cli:surface-approval".to_owned(),
            lineage: lineage.clone(),
        },
    );
    assert_eq!(requested.kind, SurfaceActionOutcomeKind::Requested);

    wait_until(|| runtime.execution_count() == 1 && matches!(runtime.pending_lineage(), Ok(None)))?;
    assert_eq!(runtime.execution_count(), 1);
    assert_eq!(runtime.pending_lineage()?, None);

    let duplicate = run_surface_action(
        Some(&config_path),
        &workspace,
        SurfaceAction::Approve {
            session_key: "cli:surface-approval".to_owned(),
            lineage,
        },
    );
    assert_eq!(duplicate.kind, SurfaceActionOutcomeKind::Requested);
    thread::yield_now();
    assert_eq!(runtime.execution_count(), 1);

    let deny_lineage = runtime.create_pending()?;
    let denied = run_surface_action(
        Some(&config_path),
        &workspace,
        SurfaceAction::Deny {
            session_key: "cli:surface-approval".to_owned(),
            lineage: deny_lineage,
        },
    );
    assert_eq!(denied.kind, SurfaceActionOutcomeKind::Requested);
    wait_until(|| runtime.execution_count() == 1 && matches!(runtime.pending_lineage(), Ok(None)))?;
    assert_eq!(runtime.execution_count(), 1);
    runtime.stop()?;
    Ok(())
}

#[test]
fn action_runner_stale_owner_generation_does_not_execute_tool() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("data").join("config.json");
    let workspace = root.path().join("workspace");
    let mut runtime = FixtureRuntime::start(config_path.clone(), workspace.clone())?;
    let lineage = runtime
        .pending_lineage()?
        .ok_or("missing initial pending lineage")?;
    let requested = run_surface_action(
        Some(&config_path),
        &workspace,
        SurfaceAction::Approve {
            session_key: "cli:surface-approval".to_owned(),
            lineage,
        },
    );
    assert_eq!(requested.kind, SurfaceActionOutcomeKind::Requested);
    runtime.replace_owner_generation()?;
    wait_until(|| {
        runtime
            .terminal_summary()
            .map(|summary| summary.to_string().contains("Superseded"))
            .unwrap_or(false)
    })?;
    assert_eq!(runtime.execution_count(), 0);
    runtime.stop()?;
    Ok(())
}

fn save_session(
    workspace: &std::path::Path,
    key: &str,
    progress: u64,
    approval: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let mut session = Session::new(key);
    session.metadata.insert("runtime_workflow".to_owned(), json!({"projection": {"schema_label": "024WorkflowProjection", "schema_version": "024WorkflowProjection.v1", "workflow_id": "wf-live", "pattern": "workflow_sequence", "state": "running", "progress_count": progress, "active_child_count": 1, "pending_barrier_count": 0, "verifier_status": "pending", "resume_available": true, "worktree_refs": [], "evidence_refs": []}}));
    session.metadata.insert(
        "runtime_execution".to_owned(),
        json!({"pending": [{"domain": "tool"}], "outcomes": []}),
    );
    if let Some(lineage) = approval {
        session.metadata.insert(
            "pending_permission_approval".to_owned(),
            approval_value(lineage),
        );
    }
    session.add_message("user", "점검 요청", Map::new());
    manager.save(&session)?;
    Ok(())
}

fn approval_value(lineage: &str) -> Value {
    json!({"approval_request_id": lineage, "approval_request": {"approval_request_id": lineage, "expires_at_unix_ms": 9999}, "tool_call": {"name": "exec"}, "status": "pending"})
}

fn write_owner_marker(
    data_dir: &std::path::Path,
    acquired_at_ms: u64,
    renewed_at_ms: u64,
) -> Result<String, Box<dyn Error>> {
    let pid = std::process::id();
    let owner_id = format!("owner-{pid}-{acquired_at_ms}");
    let marker_path = data_dir.join("runtime").join("ownership-marker.json");
    if let Some(parent) = marker_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        marker_path,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "owner_id": owner_id,
            "pid": pid,
            "acquired_at_ms": acquired_at_ms,
            "renewed_at_ms": renewed_at_ms,
            "expires_at_ms": renewed_at_ms.saturating_add(60_000),
        }))?,
    )?;
    Ok(owner_id)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn wait_until(mut ready: impl FnMut() -> bool) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    while started.elapsed().as_secs() < 5 {
        if ready() {
            return Ok(());
        }
        thread::yield_now();
    }
    Err("condition did not become true before timeout".into())
}
