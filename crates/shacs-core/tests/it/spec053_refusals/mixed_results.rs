use super::support;
use crate::spec048_auto_mode::harness::{self, RoutingProvider};
use crate::spec048_auto_mode::support::{Setup, TestResult};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use shacs_core::runtime::{
    AgentLoop, ExecutionOutcome, LateResultDecision, PermissionMode, RuntimeExecutionLedger,
    ToolFailureClass, ToolImageSettings, ToolOutcomeKind, ToolSearchMode,
};
use shacs_core::tools::{JsonMap, Tool, ToolResult};
use std::error::Error;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrefusal";

struct MixedRefusal;

impl Tool for MixedRefusal {
    fn name(&self) -> &str {
        "mcp_files_preview"
    }

    fn description(&self) -> &str {
        "Return a refused preview with its image"
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    fn read_only(&self) -> bool {
        true
    }

    fn execute(&self, _: JsonMap) -> ToolResult {
        ToolResult::Json(json!([
            {"type": "text", "text": "Error: preview is outside the configured workspace"},
            {"type": "image_url", "image_url": {
                "url": format!("data:image/png;base64,{}", STANDARD.encode(PNG)),
            }},
        ]))
    }
}

#[derive(Clone, Copy, Debug)]
enum Entry {
    Direct,
    Bridge,
    Approval,
}

#[test]
fn image_conversion_preserves_the_third_refusals_fatal_decision() -> TestResult {
    for entry in [Entry::Direct, Entry::Bridge, Entry::Approval] {
        let setup = Setup::new()?;
        let mut tools = support::tools(&setup);
        tools.register(MixedRefusal);
        let call = |id| match entry {
            Entry::Bridge => harness::tool_call(
                id,
                "tool_call",
                json!({"name": "mcp_files_preview", "arguments": {}}),
            ),
            Entry::Direct | Entry::Approval => {
                harness::tool_call(id, "mcp_files_preview", json!({}))
            }
        };
        let provider = RoutingProvider::new(
            vec![
                harness::calls(vec![call("a")]),
                harness::calls(vec![call("b")]),
                harness::calls(vec![
                    call("c"),
                    harness::tool_call(
                        "write",
                        "write_file",
                        json!({"path": "not-started.txt", "content": "bad"}),
                    ),
                ]),
            ],
            vec![],
        );
        let mut config = support::config(&setup);
        config.concurrent_tools = false;
        config.tool_images = Some(ToolImageSettings {
            media_root: setup.media.clone(),
            attach: true,
        });
        config.tool_search.enabled = match entry {
            Entry::Bridge => ToolSearchMode::On,
            Entry::Direct | Entry::Approval => ToolSearchMode::Off,
        };
        if let Entry::Approval = entry {
            config.permission_mode_snapshot.mode = PermissionMode::Default;
        }
        let agent = support::agent(&setup, &tools, &provider, config)?;
        let (mut agent, events) = support::record_refusals(agent);
        let mut result = agent.process_direct("preview the result", Some("cli:refusals"))?;
        let mut failures = Vec::new();
        if let Entry::Approval = entry {
            for _ in 0..3 {
                assert_eq!(result.stop_reason, "ask_user", "{entry:?}: {result:?}");
                result = agent.process_direct("approve", Some("cli:refusals"))?;
                failures.extend(failure_classes(&agent)?);
            }
        } else {
            failures = failure_classes(&agent)?;
        }

        assert_eq!(
            events.lock().unwrap()[2].result.as_ref().unwrap()["fatal"],
            true
        );
        assert_eq!(result.stop_reason, "tool_error", "{entry:?}: {result:?}");
        assert!(!setup.workspace.join("not-started.txt").exists());
        let requests = provider.assistant_requests();
        assert_eq!(
            requests.len(),
            3,
            "{entry:?}: the fatal result must not reach another model turn"
        );
        let image_url = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
        for (index, id) in [(1, "a"), (2, "b")] {
            let messages = &requests[index].messages;
            let refusal = messages
                .iter()
                .find(|message| message["tool_call_id"] == id)
                .ok_or("refusal")?;
            assert!(refusal["content"]
                .as_str()
                .ok_or("text")?
                .contains("policy boundary"));
            assert!(
                messages
                    .iter()
                    .filter_map(|message| message["content"].as_array())
                    .flatten()
                    .any(|part| part.pointer("/image_url/url") == Some(&json!(image_url))),
                "{entry:?}: recoverable refusal lost its image"
            );
        }
        assert_eq!(
            failures,
            vec![
                ToolFailureClass::Recoverable,
                ToolFailureClass::Recoverable,
                ToolFailureClass::Fatal
            ],
            "{entry:?}"
        );
    }
    Ok(())
}

fn failure_classes(agent: &AgentLoop<'_>) -> Result<Vec<ToolFailureClass>, Box<dyn Error>> {
    let session = agent
        .session_manager()
        .read_session_file("cli:refusals")
        .ok_or("session")?;
    let ledger: RuntimeExecutionLedger =
        serde_json::from_value(session["metadata"]["runtime_execution"].clone())?;
    Ok(ledger
        .outcomes
        .into_iter()
        .filter_map(|entry| match (entry.decision, entry.fact.outcome) {
            (
                LateResultDecision::Accepted,
                ExecutionOutcome::Tool(ToolOutcomeKind::Failed { class }),
            ) => Some(class),
            _ => None,
        })
        .collect())
}
