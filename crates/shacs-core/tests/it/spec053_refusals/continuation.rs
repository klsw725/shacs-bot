use super::support;
use crate::spec048_auto_mode::harness::{self, RoutingProvider};
use crate::spec048_auto_mode::support::{Setup, TestResult};
use serde_json::json;
use shacs_core::runtime::{AgentLoop, PermissionMode};
use std::error::Error;
use std::fs;

#[test]
fn refused_calls_return_guidance_and_the_agent_finishes_inside_the_workspace() -> TestResult {
    let cases = [
        (
            "exec",
            json!({"command": "cat ../outside.txt"}),
            PermissionMode::BypassPermissions,
            Some("tools.restrictToWorkspace"),
        ),
        (
            "exec",
            json!({"command": "rm -rf doomed"}),
            PermissionMode::BypassPermissions,
            None,
        ),
        (
            "exec",
            json!({"command": "curl http://127.0.0.1/private"}),
            PermissionMode::BypassPermissions,
            Some("tools.ssrfWhitelist"),
        ),
        (
            "read_file",
            json!({"path": "../outside.txt"}),
            PermissionMode::BypassPermissions,
            None,
        ),
        (
            "read_file",
            json!({"path": "../outside.txt"}),
            PermissionMode::Auto,
            None,
        ),
    ];
    for (tool, arguments, mode, hint) in cases {
        let setup = Setup::new()?;
        fs::write(setup.root.join("outside.txt"), "untouched")?;
        let tools = support::tools(&setup);
        let provider = RoutingProvider::new(
            vec![
                harness::calls(vec![harness::tool_call("refused", tool, arguments)]),
                harness::calls(vec![harness::tool_call(
                    "write",
                    "write_file",
                    json!({"path": "done.txt", "content": "finished"}),
                )]),
                harness::answer("done"),
            ],
            vec![],
        );
        let mut config = support::config(&setup);
        config.permission_mode_snapshot.mode = mode;
        let mut agent = support::agent(&setup, &tools, &provider, config)?;

        let result = agent.process_direct("do the task", Some("cli:refusals"))?;

        assert_eq!(
            result.stop_reason, "completed",
            "{tool} {mode:?}: {result:?}"
        );
        assert_eq!(
            fs::read_to_string(setup.workspace.join("done.txt"))?,
            "finished"
        );
        assert_eq!(
            fs::read_to_string(setup.root.join("outside.txt"))?,
            "untouched"
        );
        let requests = provider.assistant_requests();
        let refusal = requests[1]
            .messages
            .iter()
            .find(|message| message["tool_call_id"] == "refused")
            .ok_or("missing refusal in the next model request")?;
        let text = refusal["content"].as_str().ok_or("refusal text")?;
        assert!(text.contains("policy boundary"), "{text}");
        assert!(text.contains("working_dir"), "{text}");
        assert!(text.contains("ask how to proceed"), "{text}");
        if let Some(hint) = hint {
            assert!(text.contains(hint), "{text}");
        }
        assert!(provider.classifier_requests().is_empty());
    }
    Ok(())
}

#[test]
fn approval_continuations_keep_counts_after_restart_and_a_new_request_starts_over() -> TestResult {
    let setup = Setup::new()?;
    fs::write(setup.root.join("outside.txt"), "untouched")?;
    let tools = support::tools(&setup);
    let refusal = |id, path| {
        harness::calls(vec![harness::tool_call(
            id,
            "exec",
            json!({"command": format!("cat {path}")}),
        )])
    };
    let provider = RoutingProvider::new(
        vec![
            refusal("first", "../outside.txt"),
            refusal("different-target", "../another.txt"),
            refusal("second", "../outside.txt"),
            refusal("third", "../outside.txt"),
            refusal("new-request", "../outside.txt"),
            harness::answer("done"),
        ],
        vec![],
    );
    let mut config = support::config(&setup);
    config.permission_mode_snapshot.mode = PermissionMode::Default;
    let mut agent = support::agent(&setup, &tools, &provider, config.clone())?;
    let session = Some("cli:refusals");
    let asked = agent.process_direct("read the file", session)?;
    assert_eq!(asked.stop_reason, "ask_user");
    let second_question = agent.process_direct("approve", session)?;
    assert_eq!(second_question.stop_reason, "ask_user");
    drop(agent);

    let agent = support::agent(&setup, &tools, &provider, config)?;
    let (mut agent, events) = support::record_refusals(agent);
    let repeated_target_question = agent.process_direct("approve", session)?;
    assert_eq!(repeated_target_question.stop_reason, "ask_user");
    let third_question = agent.process_direct("approve", session)?;
    assert_eq!(third_question.stop_reason, "ask_user");
    let stopped = agent.process_direct("approve", session)?;
    assert_eq!(stopped.stop_reason, "tool_error", "{stopped:?}");
    assert_eq!(stopped.final_content.as_deref(), Some(
        "Error executing command: Error: Command blocked by safety guard (path traversal detected)\n\n[Analyze the error above and try a different approach.]"
    ));
    assert_eq!(
        agent.process_direct("a new task", session)?.stop_reason,
        "ask_user"
    );
    let fresh = agent.process_direct("approve", session)?;
    assert_eq!(fresh.stop_reason, "completed", "{fresh:?}");
    assert_eq!(fresh.final_content.as_deref(), Some("done"));
    let counts: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .map(|event| event.result.as_ref().unwrap()["count"].clone())
        .collect();
    assert_eq!(counts, vec![json!(1), json!(2), json!(3), json!(1)]);
    assert_eq!(stored_roots(&agent)?, 0);
    Ok(())
}

#[test]
fn denying_an_approval_removes_the_requests_counts() -> TestResult {
    let setup = Setup::new()?;
    let tools = support::tools(&setup);
    let provider = RoutingProvider::new(
        vec![
            harness::calls(vec![harness::tool_call(
                "a",
                "exec",
                json!({"command": "cat ../outside.txt"}),
            )]),
            harness::calls(vec![harness::tool_call(
                "b",
                "exec",
                json!({"command": "cat ../outside.txt"}),
            )]),
        ],
        vec![],
    );
    let mut config = support::config(&setup);
    config.permission_mode_snapshot.mode = PermissionMode::Default;
    let mut agent = support::agent(&setup, &tools, &provider, config)?;
    let session = Some("cli:refusals");
    assert_eq!(
        agent.process_direct("read the file", session)?.stop_reason,
        "ask_user"
    );
    assert_eq!(
        stored_roots(&agent)?,
        0,
        "an empty counter must not be stored"
    );
    assert_eq!(
        agent.process_direct("approve", session)?.stop_reason,
        "ask_user"
    );
    assert_eq!(stored_roots(&agent)?, 1);

    assert_eq!(
        agent.process_direct("deny", session)?.stop_reason,
        "permission_denied_by_user"
    );

    assert_eq!(stored_roots(&agent)?, 0);
    Ok(())
}

fn stored_roots(agent: &AgentLoop<'_>) -> Result<usize, Box<dyn Error>> {
    let session = agent
        .session_manager()
        .read_session_file("cli:refusals")
        .ok_or("session")?;
    Ok(session["metadata"]["tool_refusal_counts"]
        .as_object()
        .map_or(0, |roots| roots.len()))
}
