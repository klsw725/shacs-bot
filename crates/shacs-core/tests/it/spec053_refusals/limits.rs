use super::support;
use crate::spec048_auto_mode::harness::{self, RoutingProvider};
use crate::spec048_auto_mode::support::{Setup, TestResult};
use serde_json::json;
use shacs_core::runtime::{ToolFailureClass, ToolOutcomeKind, ToolSearchConfig, ToolSearchMode};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn the_third_identical_refusal_skips_the_following_write_in_both_execution_modes() -> TestResult {
    for concurrent in [false, true] {
        let setup = Setup::new()?;
        let outside = setup.root.join("outside.txt");
        fs::write(&outside, "outside")?;
        let mut tools = support::tools(&setup);
        let starts = Arc::new(AtomicUsize::new(0));
        tools.register(support::ReadOnlyProbe(starts.clone()));
        // Lexical aliases must still be the same target.
        let provider = RoutingProvider::new(
            vec![harness::calls(vec![
                harness::tool_call("a", "read_file", json!({"path": "../outside.txt"})),
                harness::tool_call("b", "read_file", json!({"path": outside})),
                harness::tool_call("c", "read_file", json!({"path": "../ws/../outside.txt"})),
                harness::tool_call("read-after-limit", "list_dir", json!({})),
                harness::tool_call(
                    "write",
                    "write_file",
                    json!({"path": "not-started.txt", "content": "bad"}),
                ),
            ])],
            vec![],
        );
        let mut config = support::config(&setup);
        config.concurrent_tools = concurrent;
        let agent = support::agent(&setup, &tools, &provider, config)?;
        let (mut agent, events) = support::record_refusals(agent);

        let result = agent.process_direct("do the task", Some("cli:refusals"))?;

        assert_eq!(result.stop_reason, "tool_error", "{concurrent}: {result:?}");
        assert!(!setup.workspace.join("not-started.txt").exists());
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(
            result.final_content,
            Some(support::file_refusal(&setup, "../ws/../outside.txt"))
        );
        let events = events.lock().unwrap();
        let payloads: Vec<_> = events.iter().map(|event| event.result.clone()).collect();
        assert_eq!(
            payloads,
            vec![
                Some(
                    json!({"tool": "read_file", "kind": "file_range", "count": 1, "fatal": false})
                ),
                Some(
                    json!({"tool": "read_file", "kind": "file_range", "count": 2, "fatal": false})
                ),
                Some(json!({"tool": "read_file", "kind": "file_range", "count": 3, "fatal": true})),
            ]
        );
        assert!(events.iter().all(|event| event.arguments.is_none()));
        assert_eq!(
            support::tool_outcomes(&agent)?,
            vec![
                ToolOutcomeKind::Failed {
                    class: ToolFailureClass::Recoverable
                },
                ToolOutcomeKind::Failed {
                    class: ToolFailureClass::Recoverable
                },
                ToolOutcomeKind::Failed {
                    class: ToolFailureClass::Fatal
                },
                ToolOutcomeKind::Skipped {
                    reason: "fatal tool error stopped the turn".to_owned()
                },
                ToolOutcomeKind::Skipped {
                    reason: "fatal tool error stopped the turn".to_owned()
                },
            ]
        );
        assert_eq!(provider.assistant_requests().len(), 1);
    }
    Ok(())
}

#[test]
fn different_targets_have_separate_limits() -> TestResult {
    let setup = Setup::new()?;
    for name in ["first.txt", "second.txt"] {
        fs::write(setup.root.join(name), "outside")?;
    }
    let tools = support::tools(&setup);
    let provider = RoutingProvider::new(
        vec![
            harness::calls(vec![
                harness::tool_call("a", "read_file", json!({"path": "../first.txt"})),
                harness::tool_call("b", "read_file", json!({"path": "../second.txt"})),
                harness::tool_call("c", "read_file", json!({"path": "../first.txt"})),
                harness::tool_call("d", "read_file", json!({"path": "../second.txt"})),
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
    let mut agent = support::agent(&setup, &tools, &provider, support::config(&setup))?;

    let result = agent.process_direct("do the task", Some("cli:refusals"))?;

    assert_eq!(result.stop_reason, "completed", "{result:?}");
    assert_eq!(
        fs::read_to_string(setup.workspace.join("done.txt"))?,
        "done"
    );
    Ok(())
}

#[test]
fn bridge_and_direct_calls_share_the_underlying_tools_counter() -> TestResult {
    let setup = Setup::new()?;
    fs::write(setup.root.join("outside.txt"), "outside")?;
    let tools = support::tools(&setup);
    let provider = RoutingProvider::new(
        vec![harness::calls(vec![
            harness::tool_call("a", "mcp_files_read", json!({"path": "../outside.txt"})),
            harness::tool_call(
                "b",
                "tool_call",
                json!({
                    "name": "mcp_files_read", "arguments": {"path": "../outside.txt"},
                }),
            ),
            harness::tool_call("c", "mcp_files_read", json!({"path": "../outside.txt"})),
            harness::tool_call(
                "write",
                "write_file",
                json!({"path": "not-started.txt", "content": "bad"}),
            ),
        ])],
        vec![],
    );
    let mut config = support::config(&setup);
    config.concurrent_tools = true;
    config.tool_search = ToolSearchConfig {
        enabled: ToolSearchMode::On,
        ..ToolSearchConfig::default()
    };
    let mut agent = support::agent(&setup, &tools, &provider, config)?;

    let result = agent.process_direct("do the task", Some("cli:refusals"))?;

    assert_eq!(result.stop_reason, "tool_error", "{result:?}");
    assert_eq!(
        result.final_content,
        Some(support::file_refusal(&setup, "../outside.txt"))
    );
    assert!(!setup.workspace.join("not-started.txt").exists());
    Ok(())
}

#[test]
fn fail_on_tool_error_stops_at_the_first_refusal() -> TestResult {
    let setup = Setup::new()?;
    fs::write(setup.root.join("outside.txt"), "outside")?;
    let tools = support::tools(&setup);
    let provider = RoutingProvider::new(
        vec![harness::calls(vec![
            harness::tool_call("refused", "read_file", json!({"path": "../outside.txt"})),
            harness::tool_call(
                "write",
                "write_file",
                json!({"path": "not-started.txt", "content": "bad"}),
            ),
        ])],
        vec![],
    );
    let mut config = support::config(&setup);
    config.fail_on_tool_error = true;
    let agent = support::agent(&setup, &tools, &provider, config)?;
    let (mut agent, events) = support::record_refusals(agent);

    let result = agent.process_direct("do the task", Some("cli:refusals"))?;

    assert_eq!(result.stop_reason, "tool_error", "{result:?}");
    assert_eq!(
        result.final_content,
        Some(support::file_refusal(&setup, "../outside.txt"))
    );
    assert_eq!(
        events.lock().unwrap()[0].result,
        Some(json!({
            "tool": "read_file", "kind": "file_range", "count": 1, "fatal": true,
        }))
    );
    assert!(!setup.workspace.join("not-started.txt").exists());
    Ok(())
}
