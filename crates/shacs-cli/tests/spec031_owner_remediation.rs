use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_core::app::AppId;
use shacs_core::app_lifecycle::AppSupervisorJournal;
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
fn app_inspect_and_list_preserve_failed_owner_receipt_without_launch() -> TestResult {
    let root = tempfile::tempdir()?;
    let config = config(root.path())?;
    let bundle = root.path().join("apps/g16.app.shacsapp");
    fs::create_dir_all(&bundle)?;
    fs::write(bundle.join("README.md"), "controlled local app")?;
    fs::write(
        bundle.join("SKILL.md"),
        "---\nname: fixture\ndescription: local\n---\ncontrolled",
    )?;
    let sentinel = root.path().join("MUST_NOT_LAUNCH");
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec(&json!({
            "id":"g16.app", "version":"0.1.0", "entry":"README.md", "skills":["SKILL.md"],
            "secrets":[{"key":"G16_REMEDIATION_MISSING_TOKEN","required":true}],
            "runtime":{"command":["/bin/sh","-c",format!("touch '{}'",sentinel.display())],"timeoutSeconds":2}
        }))?,
    )?;
    invoke(
        &config,
        &["apps", "install", bundle.to_str().ok_or("bundle path")?],
    )?;
    invoke(&config, &["apps", "enable", "g16.app"])?;
    let start = invoke(&config, &["apps", "start", "g16.app"])?;
    assert!(start.contains("Process dispatches: 0"));
    let journal = AppSupervisorJournal::new(root.path().join("apps"));
    let app_id = AppId::parse("g16.app")?;
    let receipts = journal.replay(&app_id)?.receipts;
    let receipt = receipts.last().ok_or("terminal receipt")?;
    println!("owner={}", serde_json::to_string(receipt)?);
    let registry = root.path().join("apps/registry.json");
    let registry_before = fs::read(&registry)?;
    let config_before = fs::read(&config)?;

    let outputs = [
        invoke(&config, &["apps", "inspect", "g16.app"])?,
        invoke(&config, &["apps", "list"])?,
    ];

    for output in outputs {
        let line = output
            .lines()
            .find(|line| line.starts_with("Spec031 app:"))
            .ok_or("app row")?;
        assert!(line.contains("state=blocked"), "{line}");
        assert!(line.contains("freshness=current"), "{line}");
        assert!(
            output.contains("credential_missing:G16_REMEDIATION_MISSING_TOKEN"),
            "{output}"
        );
        assert!(output.contains("activation_missing"), "{output}");
        assert!(output.contains(&receipt.receipt_id), "{output}");
        assert!(
            !output.contains(root.path().to_str().ok_or("root")?),
            "{output}"
        );
        assert!(output.contains("path:"), "{output}");
    }
    assert_eq!(journal.replay(&app_id)?.receipts, receipts);
    assert_eq!(fs::read(registry)?, registry_before);
    assert_eq!(fs::read(config)?, config_before);
    assert!(!sentinel.exists());
    root.close()?;
    Ok(())
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
