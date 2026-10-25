use super::{config, invoke, TestResult};
use shacs_core::runtime::{
    ChildResultEnvelope, ChildResultStatus, ExecutionDomain, ExecutionIdentity, ExecutionOutcome,
    ExecutionOutcomeFact, ExecutionScope, PendingExecution, RuntimeExecutionLedger, SpawnEnvelope,
    SubagentRuntime, ToolFailureClass, ToolOutcomeKind,
};
use shacs_session::{Session, SessionManager};

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
