//! Spec 053 PRD 010 acceptance 4: the worker carries where an automation run
//! came from into the tool decision.

use super::super::tests::{scripted_agent_loop_adapter, CONFIRMED_CONTAINMENT};
use serde_json::{json, Value};
use shacs_core::runtime::{
    AutomationConfirmationFact, AutomationDispatchRequest, AutomationExecutionControl,
    AutomationExecutionRequirements, AutomationOutcomePolicy, AutomationRunSource,
    ContainmentSnapshotRef, PermissionMode,
};
use shacs_core::tools::{JsonMap, PathContext, ReadFileTool, Tool, ToolRegistry, ToolResult};
use shacs_eval::evaluator::{
    AutomationExecutionMode, AutomationRunRequest, AutomationRunTriggerKind, AutomationTriggerRef,
};
use shacs_providers::{LlmResponse, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type TestResult = Result<(), Box<dyn Error>>;

/// An MCP-shaped writer that names its path as workspace-outside when it is
/// not under the workspace, and records every path it was let to write.
struct PathWriter {
    workspace: PathBuf,
    written: Arc<Mutex<Vec<String>>>,
}

impl Tool for PathWriter {
    fn name(&self) -> &str {
        "mcp_notes_write"
    }
    fn description(&self) -> &str {
        "Write a note to a path"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}})
    }
    fn outside_workspace_targets(&self, params: &JsonMap) -> Vec<String> {
        params
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !Path::new(path).starts_with(&self.workspace))
            .map(|path| vec![path.to_owned()])
            .unwrap_or_default()
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params.get("path").and_then(Value::as_str).unwrap_or("");
        self.written
            .lock()
            .expect("written lock poisoned")
            .push(path.to_owned());
        "written".into()
    }
}

fn request(source: AutomationRunSource) -> AutomationDispatchRequest {
    let trigger_kind = match source {
        AutomationRunSource::Cron { .. } => AutomationRunTriggerKind::Cron,
        _ => AutomationRunTriggerKind::Heartbeat,
    };
    AutomationDispatchRequest {
        work_id: "automation-work-1".to_owned(),
        session_key: "automation:job-1".to_owned(),
        work_kind: "automation.run".to_owned(),
        dedupe_key: "dedupe-1".to_owned(),
        idempotency_key: "idempotency-1".to_owned(),
        run: AutomationRunRequest {
            run_id: "run-1".to_owned(),
            job_id: "job-1".to_owned(),
            trigger_kind,
            trigger_ref: AutomationTriggerRef {
                runtime_service_event_id: "event-1".to_owned(),
                source_type: "test".to_owned(),
                source_owner: "runtime".to_owned(),
                received_at_ms: 1,
                idempotency_key: "trigger-1".to_owned(),
            },
            session_id: None,
            goal_id: None,
            execution_mode: AutomationExecutionMode::SkillBackedAgent,
            timeout_policy_ref: "timeout".to_owned(),
            retry_policy_ref: "retry".to_owned(),
            delivery_policy_ref: "delivery".to_owned(),
            recursion_guard_token: "guard".to_owned(),
        },
        requirements: AutomationExecutionRequirements {
            execution_sensitive: false,
            credential_required: false,
            sandbox_required: false,
            confirmation: AutomationConfirmationFact::NotRequired,
        },
        instruction: Some("write the note".to_owned()),
        outcome_policy: AutomationOutcomePolicy::Suppress,
        owner_target_ref: None,
        run_source: source,
    }
}

/// What one automation run did: the paths the writer was let to write and
/// the tool results the run saved.
struct Run {
    written: Vec<String>,
    tool_results: Vec<String>,
}

/// Runs one automation through the worker in `bypass_permissions`, calling
/// `tool` on a file outside or inside the workspace.
fn run_automation(
    source: AutomationRunSource,
    tool: &str,
    outside: bool,
) -> Result<Run, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let path = if outside {
        root.path().join("outside.txt")
    } else {
        workspace.join("inside.txt")
    };
    fs::create_dir_all(&workspace)?;
    fs::write(&path, "outside note")?;
    let written = Arc::new(Mutex::new(Vec::new()));
    let mut tools = ToolRegistry::new();
    tools.register(PathWriter {
        workspace: workspace.clone(),
        written: written.clone(),
    });
    // The file tools as production registers them with `restrictToWorkspace`
    // off (Spec 053 PRD 020 scope 1).
    let mut files = PathContext::workspace(&workspace);
    files.outside_by_decision = true;
    tools.register(ReadFileTool::new(files));
    let call = ToolCallRequest::new(
        "call-1",
        tool,
        json!({"path": path.display().to_string()})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    );
    let responses = vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![call],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            finish_reason: "stop".to_owned(),
            ..LlmResponse::default()
        },
    ];
    let mut adapter = scripted_agent_loop_adapter(
        root.path(),
        json!({"permissions": {"mode": "bypass_permissions"}}),
        tools,
        responses,
        PermissionMode::BypassPermissions,
    )?;
    // `bypass_permissions` holds only in a contained runtime.
    adapter.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(true),
        backend: Some("official-container+test".to_owned()),
        digest: Some("test-containment".to_owned()),
        summary: Some("test containment".to_owned()),
    });
    let request = request(source);
    CONFIRMED_CONTAINMENT.set(true);
    adapter.execute_agent_automation(
        &request,
        "write the note",
        AutomationExecutionControl::runtime_default(),
    )?;
    let written = written.lock().map_err(|_| "written lock poisoned")?.clone();
    Ok(Run {
        written,
        tool_results: tool_results(&workspace.join("sessions"))?,
    })
}

fn tool_results(sessions: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut results = Vec::new();
    for entry in fs::read_dir(sessions)? {
        for line in fs::read_to_string(entry?.path())?.lines() {
            let message: Value = serde_json::from_str(line)?;
            if message["role"] == "tool" {
                results.push(message["content"].as_str().unwrap_or_default().to_owned());
            }
        }
    }
    Ok(results)
}

fn cron() -> AutomationRunSource {
    AutomationRunSource::Cron {
        job_id: Some("job-1".to_owned()),
        work_id: "automation-work-1".to_owned(),
    }
}

fn heartbeat() -> AutomationRunSource {
    AutomationRunSource::Heartbeat {
        work_id: "automation-work-1".to_owned(),
    }
}

#[test]
fn a_cron_run_is_refused_outside_the_workspace_even_in_bypass() -> TestResult {
    // When
    let run = run_automation(cron(), WRITER, true)?;

    // Then
    assert!(run.written.is_empty(), "{:?}", run.tool_results);
    assert!(refused_unattended(&run), "{:?}", run.tool_results);
    Ok(())
}

const WRITER: &str = "mcp_notes_write";

/// A PRD 000 refusal: the reason, then the guidance and its count.
fn refused_unattended(run: &Run) -> bool {
    run.tool_results.iter().any(|result| {
        result.contains("reaches outside the workspace in an unattended run")
            && result.contains("Refusal repetition: 1/3 in this request.")
    })
}

#[test]
fn a_cron_run_cannot_read_a_file_outside_the_workspace_even_in_bypass() -> TestResult {
    // When
    let run = run_automation(cron(), "read_file", true)?;

    // Then
    assert!(refused_unattended(&run), "{:?}", run.tool_results);
    assert!(
        !run.tool_results
            .iter()
            .any(|result| result.contains("outside note")),
        "{:?}",
        run.tool_results
    );
    Ok(())
}

#[test]
fn a_heartbeat_run_in_bypass_reads_a_file_outside_the_workspace() -> TestResult {
    // When
    let run = run_automation(heartbeat(), "read_file", true)?;

    // Then
    assert!(
        run.tool_results
            .iter()
            .any(|result| result.contains("outside note")),
        "{:?}",
        run.tool_results
    );
    Ok(())
}

#[test]
fn a_heartbeat_run_in_bypass_writes_outside_the_workspace() -> TestResult {
    // When
    let run = run_automation(heartbeat(), WRITER, true)?;

    // Then
    assert_eq!(run.written.len(), 1, "{:?}", run.tool_results);
    Ok(())
}

#[test]
fn a_cron_run_inside_the_workspace_is_unchanged() -> TestResult {
    // When
    let run = run_automation(cron(), WRITER, false)?;

    // Then
    assert_eq!(run.written.len(), 1, "{:?}", run.tool_results);
    Ok(())
}
