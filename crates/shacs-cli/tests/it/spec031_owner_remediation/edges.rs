use super::{config, invoke, TestResult};
use shacs_core::app::{AppId, AppLifecycleState, AppRegistry, AppRegistryEntry, AppRegistryStore};
use shacs_core::app_lifecycle::{
    AppLifecycleAction, AppLifecycleBlocker, AppProcessState, AppSupervisorJournal,
};
use shacs_core::runtime::{
    ChildResultEnvelope, ChildResultStatus, ExecutionDomain, ExecutionIdentity, ExecutionOutcome,
    ExecutionOutcomeFact, ExecutionScope, PendingExecution, RuntimeExecutionLedger, SpawnEnvelope,
    SubagentRuntime, ToolFailureClass, ToolOutcomeKind,
};
use shacs_session::{Session, SessionManager};

#[test]
fn app_registry_intent_pending_and_terminal_receipts_remain_distinct() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    let app_id = AppId::parse("g16.app")?;
    let entry = AppRegistryEntry {
        app_id: app_id.clone(),
        version: "1".to_owned(),
        digest: "sha256:fixture".to_owned(),
        bundle_path: root.path().join("HOST_PATH_SENTINEL"),
        lifecycle_state: AppLifecycleState::Enabled,
        permission_requests: Vec::new(),
        secret_requests: Vec::new(),
        resource_summaries: Vec::new(),
        grant_reference: None,
        unavailable_reasons: Vec::new(),
        process_snapshots: Vec::new(),
        installed_at_unix_ms: 1,
    };
    AppRegistryStore::new(root.path()).save(&AppRegistry {
        entries: [(app_id.clone(), entry)].into(),
    })?;
    let journal = AppSupervisorJournal::new(root.path().join("apps"));
    let requested = journal.request(&app_id, AppLifecycleAction::Start)?;

    let output = invoke(&config, &["apps", "inspect", "g16.app"])?;

    assert!(output.contains("state=unknown"), "{output}");
    assert!(output.contains("reason=requested"), "{output}");
    assert!(output.contains("completed=false"), "{output}");
    assert!(!output.contains("state=ready"), "{output}");
    assert!(!output.contains("HOST_PATH_SENTINEL"), "{output}");
    assert_eq!(journal.replay(&app_id)?.receipts, vec![requested]);
    root.close()?;
    Ok(())
}

#[test]
fn app_blocker_paths_are_opaque_while_safe_owner_refs_survive() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    let bundle = root.path().join("apps/g16.app.shacsapp");
    std::fs::create_dir_all(&bundle)?;
    std::fs::write(bundle.join("README.md"), "fixture")?;
    std::fs::write(
        bundle.join("manifest.json"),
        br#"{"id":"g16.app","version":"1","entry":"README.md"}"#,
    )?;
    invoke(
        &config,
        &["apps", "install", bundle.to_str().ok_or("bundle")?],
    )?;
    let journal = AppSupervisorJournal::new(root.path().join("apps"));
    let app_id = AppId::parse("g16.app")?;
    let mut request = journal.request(&app_id, AppLifecycleAction::Start)?;
    journal.attach_start_evidence(
        &mut request,
        vec!["activation:safe:opaque".to_owned()],
        Some("snapshot:safe:opaque".to_owned()),
    );
    let receipt = journal.block(
        &request,
        vec![AppLifecycleBlocker::ActivationMissing {
            resource_ref: "/HOST_PATH_SENTINEL/secret".to_owned(),
        }],
    )?;

    let output = invoke(&config, &["apps", "inspect", "g16.app"])?;

    assert!(!output.contains("HOST_PATH_SENTINEL"), "{output}");
    assert!(output.contains("activation_missing:ref:"), "{output}");
    assert!(output.contains("activation:safe:opaque"), "{output}");
    assert!(output.contains("snapshot:safe:opaque"), "{output}");
    assert!(output.contains(&receipt.receipt_id), "{output}");
    assert_eq!(receipt.current_state, AppProcessState::Failed);
    root.close()?;
    Ok(())
}

#[test]
fn session_tools_keep_failed_ignored_and_pending_owner_facts_separate() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    let identity = ExecutionIdentity::new(
        ExecutionScope::new("session-1", "turn"),
        "effect",
        "correlation",
    );
    let mut ledger = RuntimeExecutionLedger::default();
    ledger.record(ExecutionOutcomeFact::new(
        identity.clone(),
        ExecutionOutcome::Tool(ToolOutcomeKind::Failed {
            class: ToolFailureClass::Fatal,
        }),
        2,
    ));
    ledger.record(ExecutionOutcomeFact::new(
        identity,
        ExecutionOutcome::Tool(ToolOutcomeKind::Completed),
        3,
    ));
    ledger.begin(PendingExecution {
        identity: ExecutionIdentity::new(
            ExecutionScope::new("session-1", "turn"),
            "pending",
            "pending-correlation",
        ),
        domain: ExecutionDomain::Tool,
        started_at_ms: 4,
    });
    let mut session = Session::new("session-1");
    session.metadata.insert(
        "runtime_execution".to_owned(),
        serde_json::to_value(&ledger)?,
    );
    SessionManager::new(root.path())?.save(&session)?;

    let output = invoke(&config, &["session", "inspect", "--session", "session-1"])?;

    let rows: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("Spec031 tool:"))
        .collect();
    assert_eq!(rows.len(), 3, "{output}");
    assert!(rows[0].contains("state=blocked"), "{output}");
    assert!(
        rows[1].contains("state=degraded") && rows[1].contains("decision=duplicate"),
        "{output}"
    );
    assert!(
        rows[2].contains("reason=requested") && rows[2].contains("pending=1"),
        "{output}"
    );
    assert!(rows
        .iter()
        .all(|row| row.contains("freshness=current") && !row.contains("state=ready")));
    assert!(
        output.contains("pending=1 outcomes=2 retained_rows=2"),
        "{output}"
    );
    assert!(
        output.contains("attempts=2"),
        "unique failed and pending attempts; duplicate is not a third attempt: {output}"
    );
    assert!(rows[0].contains("parent=parent:turn:"), "{output}");
    root.close()?;
    Ok(())
}

#[test]
fn child_cancellation_request_does_not_become_a_terminal_result() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    SessionManager::new(root.path())?.save(&Session::new("session-1"))?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(shacs_session::durable_child::DurableChildRecorder::open(
            root.path().join("runtime/durable-events"),
        )?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session-1", "child", "controlled");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("running")?;
    runtime.cancel_by_session("session-1");

    let output = invoke(&config, &["session", "inspect", "--session", "session-1"])?;

    assert!(
        output.contains("outcome=Running cancellation_requested=true result=none"),
        "{output}"
    );
    assert!(!output.contains("reason=completed"), "{output}");
    runtime.finish_child(ChildResultEnvelope::from_spawn(
        &spawn,
        ChildResultStatus::Cancelled,
        "controlled cancellation",
    ));
    root.close()?;
    Ok(())
}

#[test]
fn session_diagnostics_uses_the_same_owner_rows_as_inspect() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    let mut session = Session::new("session-1");
    let mut ledger = RuntimeExecutionLedger::default();
    ledger.record(ExecutionOutcomeFact::new(
        ExecutionIdentity::new(
            ExecutionScope::new("session-1", "turn"),
            "effect",
            "correlation",
        ),
        ExecutionOutcome::Tool(ToolOutcomeKind::Completed),
        1,
    ));
    session.metadata.insert(
        "runtime_execution".to_owned(),
        serde_json::to_value(&ledger)?,
    );
    SessionManager::new(root.path())?.save(&session)?;
    let inspect = invoke(&config, &["session", "inspect", "--session", "session-1"])?;

    let diagnostics = invoke(
        &config,
        &["session", "diagnostics", "--session", "session-1"],
    )?;

    assert_eq!(
        inspect
            .lines()
            .find(|line| line.starts_with("Spec031 tool:")),
        diagnostics
            .lines()
            .find(|line| line.starts_with("Spec031 tool:"))
    );
    root.close()?;
    Ok(())
}
