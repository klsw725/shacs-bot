//! Spec 053 PRD 010 acceptance: `exec` naming paths outside the workspace.

use crate::spec048_auto_mode::harness::{self, RoutingProvider};
use crate::spec048_auto_mode::support::{call, run, Exec, Options, Outcome, Setup, Verdict};
use crate::spec053_refusals::support;
use serde_json::{json, Value};
use shacs_core::runtime::{
    AgentLoopConfig, AutomationRunSource, PermissionMode, ToolFailureClass, ToolOutcomeKind,
    ToolSearchConfig, ToolSearchMode,
};
use shacs_core::tools::{
    ExecConfig, ExecTool, JsonMap, ReadFileTool, Tool, ToolRegistry, ToolResult, WriteFileTool,
};
use std::error::Error;
use std::fs;

type TestResult = Result<(), Box<dyn Error>>;

/// The 2026-11-15 Discord turn: the transcript script's output is written
/// outside the workspace and counted.
fn transcript_command(setup: &Setup) -> String {
    let output = setup.root.join("u0RtU4u8Pw0.txt");
    format!(
        "python3 builtin_skills/youtube-content/scripts/fetch_transcript.py 'https://youtu.be/u0RtU4u8Pw0' --text-only > {0}; wc -lc {0}",
        output.display()
    )
}

#[test]
fn the_classifier_allows_writing_the_transcript_outside_the_workspace() -> TestResult {
    // Given: auto mode in conversation, `restrictToWorkspace` off.
    let setup = Setup::new()?;
    let (registry, recorded) = setup.registry(Exec::Gated);
    let context = setup.context(&Options::auto(true).verdict(Verdict::Allow));
    let command = transcript_command(&setup);

    // When
    let outcome = run(
        &registry,
        &context,
        call("exec", json!({"command": command})),
    );

    // Then
    assert!(outcome.ran(), "{outcome:?}");
    assert_eq!(
        recorded.launched(),
        vec![command],
        "the classifier's allow launches the command"
    );
    Ok(())
}

const SECRET_VALUE: &str = "spec053-value-6721";
const SECRET_MARKER: &str = "[secret SHACS_CHANNELS_DISCORD_TOKEN]";

/// An `.env.json` value that names the outside file.
fn secret_target(setup: &Setup) -> Result<std::path::PathBuf, Box<dyn Error>> {
    fs::write(
        setup.data.join(".env.json"),
        serde_json::to_vec(&json!({"SHACS_CHANNELS_DISCORD_TOKEN": SECRET_VALUE}))?,
    )?;
    Ok(setup.root.join(format!("{SECRET_VALUE}.txt")))
}

#[test]
fn a_default_mode_question_says_the_command_reaches_outside_the_workspace() -> TestResult {
    // Given: `default` mode in conversation asks for any command; the
    // outside path holds an `.env.json` value.
    let setup = Setup::new()?;
    let target = secret_target(&setup)?;
    let (registry, recorded) = setup.registry(Exec::Gated);
    let context = setup.context(&Options::mode(PermissionMode::Default, true));

    // When
    let outcome = run(
        &registry,
        &context,
        call(
            "exec",
            json!({"command": format!("cat {}", target.display())}),
        ),
    );

    // Then
    let Outcome::Asked(question) = outcome else {
        return Err(format!("expected a question: {outcome:?}").into());
    };
    let shown = setup.root.join(format!("{SECRET_MARKER}.txt"));
    assert!(
        question.text.contains(&format!(
            "this call reaches outside the workspace: `{}`.",
            shown.display()
        )),
        "{}",
        question.text
    );
    assert!(!question.text.contains(SECRET_VALUE), "{}", question.text);
    assert!(recorded.launched().is_empty());
    Ok(())
}

#[test]
fn the_stored_question_masks_secret_values_in_outside_targets() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let target = secret_target(&setup)?;
    let tools = exec_tools(&setup, false);
    let provider = RoutingProvider::new(
        vec![harness::calls(vec![harness::tool_call(
            "read",
            "exec",
            json!({"command": format!("cat {}", target.display())}),
        )])],
        vec![],
    );
    let mut config = harness::agent_loop_config(&setup, true);
    let context = setup.context(&Options::mode(PermissionMode::Default, true));
    config.permission_mode_snapshot = context.permission_mode_snapshot;
    config.permission_ceiling_snapshot = context.permission_ceiling_snapshot;
    let mut agent = support::agent(&setup, &tools, &provider, config)?;

    // When
    agent.process_direct("read it", Some("cli:secret"))?;

    // Then
    let session = agent
        .session_manager()
        .read_session_file("cli:secret")
        .ok_or("session")?;
    let question = session["metadata"]["pending_permission_approval"]["question"]
        .as_str()
        .ok_or("no stored question")?;
    assert!(
        question.contains("reaches outside the workspace"),
        "{question}"
    );
    assert!(question.contains(SECRET_MARKER), "{question}");
    assert!(!question.contains(SECRET_VALUE), "{question}");
    Ok(())
}

#[test]
fn restrict_to_workspace_still_refuses_the_transcript_outside_the_workspace() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let (registry, recorded) = setup.registry(Exec::Restricted);
    let context = setup.context(&Options::auto(true).verdict(Verdict::Allow));

    // When
    let outcome = run(
        &registry,
        &context,
        call("exec", json!({"command": transcript_command(&setup)})),
    );

    // Then
    assert!(
        matches!(&outcome, Outcome::Ran(text) if text.contains("path outside working dir")),
        "the guard refuses the call itself: {outcome:?}"
    );
    assert!(recorded.launched().is_empty(), "nothing is launched");
    Ok(())
}

fn cron() -> AutomationRunSource {
    AutomationRunSource::Cron {
        job_id: Some("job-1".to_owned()),
        grant_digest: None,
        work_id: "work-1".to_owned(),
    }
}

fn exec_tools(setup: &Setup, restrict_to_workspace: bool) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    let mut exec = ExecConfig::new(setup.path_context());
    exec.restrict_to_workspace = restrict_to_workspace;
    tools.register(ExecTool::new(exec));
    tools.register(ReadFileTool::new(setup.path_context()));
    tools.register(WriteFileTool::new(setup.path_context()));
    tools
}

/// A cron run in `bypass_permissions`, which still refuses workspace-outside
/// access.
fn cron_config(setup: &Setup) -> AgentLoopConfig {
    let mut config = support::config(setup);
    config.permission_interactive = false;
    config.automation_source = Some(cron());
    config
}

fn refusal_counts(events: &[shacs_core::runtime::ToolEvent]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| event.result.clone())
        .collect()
}

/// A deferred MCP-shaped shell that names the same outside targets as `exec`.
struct DeferredShell(ExecTool);

impl Tool for DeferredShell {
    fn name(&self) -> &str {
        "mcp_shell_run"
    }
    fn description(&self) -> &str {
        "Run a command through the deferred bridge"
    }
    fn parameters(&self) -> Value {
        self.0.parameters()
    }
    fn outside_workspace_targets(&self, params: &JsonMap) -> Vec<String> {
        self.0.outside_workspace_targets(params)
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        self.0.execute(params)
    }
}

#[test]
fn a_bridged_unattended_refusal_is_counted_and_stops_on_the_third() -> TestResult {
    // Given: unattended auto mode; the classifier allows each bridged call
    // while the run resolves it.
    let setup = Setup::new()?;
    let outside = setup.root.join("outside.txt");
    fs::write(&outside, "outside")?;
    let mut tools = exec_tools(&setup, false);
    tools.register(DeferredShell(ExecTool::new(ExecConfig::new(
        setup.path_context(),
    ))));
    let bridged = |id: &str| {
        harness::tool_call(
            id,
            "tool_call",
            json!({
                "name": "mcp_shell_run",
                "arguments": {"command": format!("cat {}", outside.display())},
            }),
        )
    };
    let provider = RoutingProvider::new(
        vec![harness::calls(vec![
            bridged("a"),
            bridged("b"),
            bridged("c"),
            harness::tool_call(
                "write",
                "write_file",
                json!({"path": "not-started.txt", "content": "bad"}),
            ),
        ])],
        vec![Ok("no"), Ok("no"), Ok("no")],
    );
    let mut config = harness::agent_loop_config(&setup, false);
    config.automation_source = Some(cron());
    config.tool_search = ToolSearchConfig {
        enabled: ToolSearchMode::On,
        ..ToolSearchConfig::default()
    };
    let agent = support::agent(&setup, &tools, &provider, config)?;
    let (mut agent, events) = support::record_refusals(agent);

    // When
    let result = agent.process_direct("do the task", Some("cli:refusals"))?;

    // Then
    assert_eq!(result.stop_reason, "tool_error", "{result:?}");
    assert_eq!(provider.classifier_requests().len(), 3);
    assert!(!setup.workspace.join("not-started.txt").exists());
    let final_content = result.final_content.unwrap_or_default();
    assert!(
        final_content.contains("reaches outside the workspace in an unattended run"),
        "{final_content}"
    );
    let events = events.lock().map_err(|_| "events lock poisoned")?;
    assert_eq!(
        refusal_counts(&events),
        (1..=3)
            .map(|count| json!({
                "tool": "mcp_shell_run", "kind": "unattended_outside", "count": count,
                "fatal": count == 3,
            }))
            .collect::<Vec<Value>>()
    );
    let recoverable = ToolOutcomeKind::Failed {
        class: ToolFailureClass::Recoverable,
    };
    assert_eq!(
        support::tool_outcomes(&agent)?,
        vec![
            recoverable.clone(),
            recoverable,
            ToolOutcomeKind::Failed {
                class: ToolFailureClass::Fatal
            },
            ToolOutcomeKind::Skipped {
                reason: "fatal tool error stopped the turn".to_owned()
            },
        ]
    );
    // A policy refusal after the classifier allowed is no classifier block.
    let session = agent
        .session_manager()
        .read_session_file("cli:refusals")
        .ok_or("session")?;
    let blocks = &session["metadata"]["auto_mode_block_counts"];
    assert!(
        blocks.is_null() || *blocks == json!({"consecutive": 0, "total": 0}),
        "{blocks}"
    );
    Ok(())
}

#[test]
fn a_changed_operand_inside_the_workspace_does_not_reset_the_count() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let outside = setup.root.join("outside.txt");
    fs::write(&outside, "outside")?;
    let tools = exec_tools(&setup, false);
    let command = |inside: &str| {
        json!({"command": format!(
            "cat {} {}",
            outside.display(),
            setup.workspace.join(inside).display()
        )})
    };
    let provider = RoutingProvider::new(
        vec![harness::calls(vec![
            harness::tool_call("a", "exec", command("a.txt")),
            harness::tool_call("b", "exec", command("b.txt")),
            harness::tool_call("c", "exec", command("c.txt")),
            harness::tool_call(
                "write",
                "write_file",
                json!({"path": "not-started.txt", "content": "bad"}),
            ),
        ])],
        vec![],
    );
    let agent = support::agent(&setup, &tools, &provider, cron_config(&setup))?;
    let (mut agent, events) = support::record_refusals(agent);

    // When
    let result = agent.process_direct("do the task", Some("cli:refusals"))?;

    // Then
    assert_eq!(result.stop_reason, "tool_error", "{result:?}");
    assert!(!setup.workspace.join("not-started.txt").exists());
    let events = events.lock().map_err(|_| "events lock poisoned")?;
    let counts: Vec<Value> = refusal_counts(&events)
        .iter()
        .map(|payload| payload["count"].clone())
        .collect();
    assert_eq!(counts, vec![json!(1), json!(2), json!(3)]);
    Ok(())
}

#[test]
fn different_outside_targets_keep_separate_counts() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let cat = |name: &str| json!({"command": format!("cat {}", setup.root.join(name).display())});
    let tools = exec_tools(&setup, false);
    let provider = RoutingProvider::new(
        vec![
            harness::calls(vec![
                harness::tool_call("a", "exec", cat("first.txt")),
                harness::tool_call("b", "exec", cat("second.txt")),
                harness::tool_call("c", "exec", cat("first.txt")),
                harness::tool_call("d", "exec", cat("second.txt")),
                harness::tool_call(
                    "write",
                    "write_file",
                    json!({"path": "done.txt", "content": "done"}),
                ),
            ]),
            harness::answer("done"),
        ],
        vec![],
    );
    let mut agent = support::agent(&setup, &tools, &provider, cron_config(&setup))?;

    // When
    let result = agent.process_direct("do the task", Some("cli:refusals"))?;

    // Then
    assert_eq!(result.stop_reason, "completed", "{result:?}");
    assert_eq!(
        fs::read_to_string(setup.workspace.join("done.txt"))?,
        "done"
    );
    Ok(())
}

/// The 2026-11-15 turn end to end with real execution: read the skill,
/// write the transcript, count it, summarize.
fn transcript_turn(
    setup: &Setup,
    restrict_to_workspace: bool,
) -> Result<(RoutingProvider, String), Box<dyn Error>> {
    let skill = setup.workspace.join("skills/youtube-content/SKILL.md");
    fs::create_dir_all(skill.parent().ok_or("skill folder")?)?;
    fs::write(&skill, "Fetch the transcript, then summarize it.")?;
    let output = setup.root.join("u0RtU4u8Pw0.txt");
    let fetch = format!(
        "printf 'first line\\nsecond line\\n' > {0}; wc -lc {0}",
        output.display()
    );
    let mut answers = vec![
        harness::calls(vec![harness::tool_call(
            "skill",
            "read_file",
            json!({"path": "skills/youtube-content/SKILL.md"}),
        )]),
        harness::calls(vec![harness::tool_call(
            "fetch",
            "exec",
            json!({"command": fetch}),
        )]),
    ];
    if restrict_to_workspace {
        answers.push(harness::calls(vec![harness::tool_call(
            "inside",
            "write_file",
            json!({"path": "u0RtU4u8Pw0.txt", "content": "first line\nsecond line\n"}),
        )]));
    }
    answers.push(harness::answer("The video is about two lines."));
    Ok((
        RoutingProvider::new(answers, vec![Ok("no"), Ok("no")]),
        fetch,
    ))
}

#[test]
fn the_transcript_turn_writes_outside_the_workspace_without_a_question() -> TestResult {
    // Given: auto mode in conversation, `restrictToWorkspace` off.
    let setup = Setup::new()?;
    let tools = exec_tools(&setup, false);
    let (provider, _) = transcript_turn(&setup, false)?;
    let mut agent = support::agent(
        &setup,
        &tools,
        &provider,
        harness::agent_loop_config(&setup, true),
    )?;

    // When
    let result = agent.process_direct("summarize https://youtu.be/u0RtU4u8Pw0", Some("cli:yt"))?;

    // Then
    assert_eq!(result.stop_reason, "completed", "{result:?}");
    assert_eq!(
        result.final_content.as_deref(),
        Some("The video is about two lines.")
    );
    assert_eq!(
        fs::read_to_string(setup.root.join("u0RtU4u8Pw0.txt"))?,
        "first line\nsecond line\n"
    );
    Ok(())
}

#[test]
fn restrict_to_workspace_refuses_the_outside_write_and_the_turn_recovers_inside() -> TestResult {
    // Given
    let setup = Setup::new()?;
    let tools = exec_tools(&setup, true);
    let (provider, _) = transcript_turn(&setup, true)?;
    let mut agent = support::agent(
        &setup,
        &tools,
        &provider,
        harness::agent_loop_config(&setup, true),
    )?;

    // When
    let result = agent.process_direct("summarize https://youtu.be/u0RtU4u8Pw0", Some("cli:yt"))?;

    // Then
    assert_eq!(result.stop_reason, "completed", "{result:?}");
    assert!(!setup.root.join("u0RtU4u8Pw0.txt").exists());
    assert_eq!(
        fs::read_to_string(setup.workspace.join("u0RtU4u8Pw0.txt"))?,
        "first line\nsecond line\n"
    );
    let requests = provider.assistant_requests();
    let refused = requests
        .last()
        .map(|request| serde_json::to_string(&request.messages))
        .transpose()?
        .unwrap_or_default();
    assert!(refused.contains("Refusal repetition: 1/3"), "{refused}");
    Ok(())
}
