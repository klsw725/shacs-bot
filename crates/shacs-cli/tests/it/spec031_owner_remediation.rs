use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    ChildResultEnvelope, ChildResultStatus, ExecutionDomain, ExecutionIdentity, ExecutionOutcome,
    ExecutionOutcomeFact, ExecutionScope, PendingExecution, RuntimeExecutionLedger, SpawnEnvelope,
    SubagentRuntime, ToolOutcomeKind,
};
use shacs_session::{Session, SessionManager};
use std::{error::Error, fs, path::Path};

type TestResult = Result<(), Box<dyn Error>>;

#[path = "spec031_owner_remediation/context.rs"]
mod context;
#[path = "spec031_owner_remediation/edges.rs"]
mod edges;

fn invoke(config: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let mut argv = vec!["--config", config.to_str().ok_or("config path")?];
    argv.extend_from_slice(args);
    let output = shacs_cli::run_command(shacs_cli::parse_cli_args(argv)?)?;
    println!("command={args:?}\n{output}");
    Ok(output)
}

fn config(root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = root.join("config.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "agents":{"defaults":{"workspace":root}},
            "plugins":{"trustedWorkspaces":[root]}
        }))?,
    )?;
    Ok(path)
}

#[test]
fn session_inspect_preserves_completed_child_owner_lineage() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    SessionManager::new(root.path())?.save(&Session::new("session-1"))?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(shacs_session::durable_child::DurableChildRecorder::open(
            root.path().join("runtime/durable-events"),
        )?)
        .map_err(std::io::Error::other)?;
    let mut spawn = SpawnEnvelope::new("session-1", "g16-child", "controlled child");
    spawn.parent_turn_id = "turn:g16-parent".to_owned();
    spawn.refresh_execution_fields();
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("running child")?;
    runtime.finish_child(ChildResultEnvelope::from_spawn(
        &spawn,
        ChildResultStatus::Completed,
        "RAW_CHILD_OUTPUT_SENTINEL",
    ));
    let owner = shacs_session::durable_replay::evaluate_durable_recovery(
        root.path().join("runtime/durable-events"),
        root.path().join("runtime/durable-checkpoints"),
    )
    .state
    .ok_or("child owner")?;
    let child = owner.children.items.get("g16-child").ok_or("child")?;
    println!("owner={}", serde_json::to_string(child)?);

    let output = invoke(&config, &["session", "inspect", "--session", "session-1"])?;

    let line = output
        .lines()
        .find(|line| line.starts_with("Spec031 subagent:"))
        .ok_or("child row")?;
    assert!(line.contains("state=ready"), "{line}");
    assert!(line.contains("reason=completed"), "{line}");
    assert!(line.contains("freshness=current"), "{line}");
    assert!(!line.contains("parent=none"), "{line}");
    assert!(!line.contains("action=none"), "{line}");
    let parent_digest = format!("{:x}", Sha256::digest(b"turn:g16-parent"));
    let action_digest = format!("{:x}", Sha256::digest(b"spawn:g16-child"));
    assert!(line.contains(&format!("parent=parent:turn:{}", &parent_digest[..16])));
    assert!(line.contains(&format!("action=action:spawn:{}", &action_digest[..16])));
    assert!(!output.contains("RAW_CHILD_OUTPUT_SENTINEL"));
    assert!(!line.contains("g16-child"));
    assert!(line.contains(child.result_ref.as_deref().ok_or("result ref")?));
    root.close()?;
    Ok(())
}

#[test]
fn session_inspect_preserves_completed_tool_attempt_from_execution_owner() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    let identity = ExecutionIdentity::new(
        ExecutionScope::new("session-1", "turn-1"),
        "effect-1",
        "correlation-1",
    );
    let mut ledger = RuntimeExecutionLedger::default();
    ledger.begin(PendingExecution {
        identity: identity.clone(),
        domain: ExecutionDomain::Tool,
        started_at_ms: 1,
    });
    ledger.record(
        ExecutionOutcomeFact::new(
            identity,
            ExecutionOutcome::Tool(ToolOutcomeKind::Completed),
            2,
        )
        .with_detail("RAW_TOOL_OUTPUT_SENTINEL"),
    );
    let mut session = Session::new("session-1");
    session.metadata.insert(
        "runtime_execution".to_owned(),
        serde_json::to_value(&ledger)?,
    );
    SessionManager::new(root.path())?.save(&session)?;
    println!("owner={}", serde_json::to_string(&ledger)?);

    let output = invoke(&config, &["session", "inspect", "--session", "session-1"])?;

    let line = output
        .lines()
        .find(|line| line.starts_with("Spec031 tool:"))
        .ok_or("tool row")?;
    assert!(line.contains("state=ready"), "{line}");
    assert!(line.contains("reason=completed"), "{line}");
    assert!(line.contains("freshness=current"), "{line}");
    assert!(!line.contains("parent=none"), "{line}");
    assert!(!line.contains("action=none"), "{line}");
    assert!(output.contains("attempts=1"), "{output}");
    let parent_digest = format!("{:x}", Sha256::digest(b"turn-1"));
    let action_digest = format!("{:x}", Sha256::digest(b"effect-1"));
    assert!(line.contains(&format!("parent=parent:turn:{}", &parent_digest[..16])));
    assert!(line.contains(&format!("action=action:tool:{}", &action_digest[..16])));
    assert!(!output.contains("RAW_TOOL_OUTPUT_SENTINEL"));
    root.close()?;
    Ok(())
}

#[test]
fn context_resolve_formats_each_canonical_owner_row_with_current_freshness() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    fs::write(
        root.path().join("secret.txt"),
        "OPENAI_API_KEY=sk-context-sentinel",
    )?;
    let message = "read @secret.txt @/G16_HOST_SENTINEL/outside.txt @url:https://user:pass@example.invalid/context.txt";

    let output = invoke(
        &config,
        &["context", "refs", "resolve", "--message", message],
    )?;

    let rows: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("Spec031 context:"))
        .collect();
    assert_eq!(rows.len(), 3, "{output}");
    for (row, reason) in rows.iter().zip(["included", "blocked", "skipped"]) {
        assert!(row.contains(&format!("reason={reason}")), "{row}");
        assert!(row.contains("lineage=subject:context:inline:"), "{row}");
        assert!(row.contains("freshness=current"), "{row}");
    }
    for raw in [
        "G16_HOST_SENTINEL",
        "user:pass",
        "sk-context-sentinel",
        root.path().to_str().ok_or("root")?,
    ] {
        assert!(!output.contains(raw), "{output}");
    }
    root.close()?;
    Ok(())
}
