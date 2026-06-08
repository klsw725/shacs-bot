use serde_json::{json, Value};
use shacs_core::runtime::{
    dispatch_bridge_tool_call, dispatch_bridge_tool_calls, ActionNormalizationError,
    ActionNormalizationState, ContainmentSnapshotRef, PermissionedActionOrigin,
    RuntimeContextTools, RuntimeInterrupt, RuntimeToolCall, RuntimeToolExecutor,
    ToolExecutionContext,
};
use shacs_core::tools::{
    AskUserTool, CronTool, DeferredToolCatalog, DeferredToolCatalogEntry, JsonMap, MessageTool,
    OutboundMessage, SchemaFragment, SpawnRequest, SpawnTool, StringSchema, Tool, ToolParameters,
    ToolRegistry, ToolResult,
};
use shacs_cron::InMemoryCronService;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

struct RepeatTool;

impl Tool for RepeatTool {
    fn name(&self) -> &str {
        "repeat"
    }

    fn description(&self) -> &str {
        "Repeat text."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("text", StringSchema::new("Text"))
            .property(
                "times",
                shacs_core::tools::IntegerSchema::new("Repeat count").minimum(1),
            )
            .required(["text", "times"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let times = params.get("times").and_then(Value::as_u64).unwrap_or(1);
        ToolResult::Text(text.repeat(times as usize))
    }
}

struct CountingTool {
    calls: Arc<AtomicUsize>,
}

impl Tool for CountingTool {
    fn name(&self) -> &str {
        "count"
    }

    fn description(&self) -> &str {
        "Count executions."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "counted".into()
    }
}

struct JsonTool;

impl Tool for JsonTool {
    fn name(&self) -> &str {
        "json_tool"
    }

    fn description(&self) -> &str {
        "Return JSON."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Json(json!({ "ok": true }))
    }
}

struct ErrorTool;

impl Tool for ErrorTool {
    fn name(&self) -> &str {
        "error_tool"
    }

    fn description(&self) -> &str {
        "Return a tool error."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        "Error: simulated failure".into()
    }
}

struct DelayTool {
    name: &'static str,
    read_only: bool,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

struct NamedRepeatTool(&'static str);

impl Tool for NamedRepeatTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "Repeat text."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("text", StringSchema::new("Text"))
            .property(
                "times",
                shacs_core::tools::IntegerSchema::new("Repeat count").minimum(1),
            )
            .required(["text", "times"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        let text = params
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let times = params.get("times").and_then(Value::as_u64).unwrap_or(1);
        ToolResult::Text(text.repeat(times as usize))
    }
}

struct DeferredAskTool;

impl Tool for DeferredAskTool {
    fn name(&self) -> &str {
        "mcp_ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user from a deferred tool."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("question", StringSchema::new("Question"))
            .required(["question"])
            .to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        ToolResult::AskUserInterrupt {
            question: params
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            options: vec!["Yes".to_owned(), "No".to_owned()],
        }
    }
}

impl DelayTool {
    fn new(
        name: &'static str,
        read_only: bool,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        calls: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            name,
            read_only,
            active,
            max_active,
            calls,
        }
    }
}

impl Tool for DelayTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Delay and record concurrency."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn read_only(&self) -> bool {
        self.read_only
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        record_max(&self.max_active, active);
        thread::sleep(Duration::from_millis(25));
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.name.into()
    }
}

fn record_max(target: &AtomicUsize, value: usize) {
    let mut observed = target.load(Ordering::SeqCst);
    while observed < value {
        match target.compare_exchange(observed, value, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return,
            Err(next) => observed = next,
        }
    }
}

#[test]
fn runtime_executes_tool_calls_and_maps_result_messages() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(JsonTool);

    let executor = RuntimeToolExecutor::new(&registry);
    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new("call_repeat", "repeat", json!({ "text": 42, "times": "2" })),
            RuntimeToolCall::new("call_json", "json_tool", json!({})),
        ],
        &ToolExecutionContext::default(),
    );

    if report.interrupt.is_some() || !report.skipped_tool_calls.is_empty() {
        return Err(format!("unexpected runtime stop: {report:?}").into());
    }
    if report.messages.len() != 2
        || report.messages[0].tool_call_id != "call_repeat"
        || report.messages[0].content != "4242"
        || report.messages[1].to_json()
            != json!({
                "role": "tool",
                "tool_call_id": "call_json",
                "name": "json_tool",
                "content": "{\"ok\":true}"
            })
    {
        return Err(format!("unexpected runtime messages: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_assistant_tool_call_message_uses_openai_argument_string() -> Result<(), Box<dyn Error>> {
    let message = shacs_core::runtime::RuntimeAssistantToolCallMessage::new(
        Some("using tools".to_owned()),
        vec![RuntimeToolCall::new(
            "call_1",
            "repeat",
            json!({ "text": "ha", "times": 2 }),
        )],
    );

    if message.to_json()
        != json!({
            "role": "assistant",
            "content": "using tools",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "repeat",
                    "arguments": "{\"text\":\"ha\",\"times\":2}"
                }
            }]
        })
    {
        return Err(format!(
            "assistant tool call JSON shape drifted: {}",
            message.to_json()
        )
        .into());
    }
    Ok(())
}

#[test]
fn runtime_returns_tool_errors_without_stopping_batch() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(CountingTool {
        calls: calls.clone(),
    });

    let executor = RuntimeToolExecutor::new(&registry);
    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new("missing", "missing_tool", json!({})),
            RuntimeToolCall::new("count", "count", json!({})),
        ],
        &ToolExecutionContext::default(),
    );

    if report.messages.len() != 2
        || !report.messages[0]
            .content
            .contains("Error: Tool 'missing_tool' not found")
        || !report.messages[0]
            .content
            .contains("[Analyze the error above and try a different approach.]")
        || report.messages[1].content != "counted"
        || calls.load(Ordering::SeqCst) != 1
    {
        return Err(format!("runtime did not preserve tool error behavior: {report:?}").into());
    }
    if report.permissioned_actions.len() != 2
        || report.permissioned_actions[0].tool_name != "missing_tool"
        || report.permissioned_actions[0].normalization_state
            != ActionNormalizationState::DenyCandidate
        || !report.permissioned_actions[0]
            .normalization_errors
            .contains(&ActionNormalizationError::UnknownTool {
                tool_name: "missing_tool".to_owned(),
            })
        || report.permissioned_actions[1].tool_name != "count"
        || report.permissioned_actions[1].normalization_state != ActionNormalizationState::Ready
    {
        return Err(format!("runtime did not report pre-execution actions: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_appends_retry_hint_to_executed_tool_errors() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(ErrorTool);

    let executor = RuntimeToolExecutor::new(&registry);
    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new("error", "error_tool", json!({}))],
        &ToolExecutionContext::default(),
    );

    if report.messages.len() != 1
        || report.messages[0].content
            != "Error: simulated failure\n\n[Analyze the error above and try a different approach.]"
    {
        return Err(format!("runtime did not append tool error hint: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_batches_only_concurrency_safe_tools_when_enabled() -> Result<(), Box<dyn Error>> {
    let sequential_active = Arc::new(AtomicUsize::new(0));
    let sequential_max = Arc::new(AtomicUsize::new(0));
    let sequential_calls = Arc::new(AtomicUsize::new(0));
    let mut sequential_registry = ToolRegistry::new();
    sequential_registry.register(DelayTool::new(
        "safe_a",
        true,
        sequential_active.clone(),
        sequential_max.clone(),
        sequential_calls.clone(),
    ));
    sequential_registry.register(DelayTool::new(
        "safe_b",
        true,
        sequential_active,
        sequential_max.clone(),
        sequential_calls.clone(),
    ));
    let sequential = RuntimeToolExecutor::new(&sequential_registry).execute_tool_calls(
        vec![
            RuntimeToolCall::new("safe-a", "safe_a", json!({})),
            RuntimeToolCall::new("safe-b", "safe_b", json!({})),
        ],
        &ToolExecutionContext::default(),
    );
    if sequential.messages.len() != 2
        || sequential_max.load(Ordering::SeqCst) != 1
        || sequential_calls.load(Ordering::SeqCst) != 2
    {
        return Err(format!("sequential execution drifted: {sequential:?}").into());
    }

    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool::new(
        "safe_a",
        true,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "safe_b",
        true,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "unsafe_tool",
        false,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "safe_c",
        true,
        active,
        max_active.clone(),
        calls.clone(),
    ));

    let report = RuntimeToolExecutor::new(&registry).execute_tool_calls_concurrent(
        vec![
            RuntimeToolCall::new("safe-a", "safe_a", json!({})),
            RuntimeToolCall::new("safe-b", "safe_b", json!({})),
            RuntimeToolCall::new("unsafe", "unsafe_tool", json!({})),
            RuntimeToolCall::new("safe-c", "safe_c", json!({})),
        ],
        &ToolExecutionContext::default(),
    );
    let contents = report
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();
    if contents != ["safe_a", "safe_b", "unsafe_tool", "safe_c"]
        || max_active.load(Ordering::SeqCst) != 2
        || calls.load(Ordering::SeqCst) != 4
    {
        return Err(format!("concurrent batching drifted: {report:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_concurrent_execution_preserves_ask_user_interrupt_boundary() -> Result<(), Box<dyn Error>>
{
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let before_calls = Arc::new(AtomicUsize::new(0));
    let after_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool::new(
        "safe_before",
        true,
        active.clone(),
        max_active.clone(),
        before_calls.clone(),
    ));
    registry.register(AskUserTool::new());
    registry.register(DelayTool::new(
        "safe_after",
        true,
        active,
        max_active,
        after_calls.clone(),
    ));

    let report = RuntimeToolExecutor::new(&registry).execute_tool_calls_concurrent(
        vec![
            RuntimeToolCall::new("before", "safe_before", json!({})),
            RuntimeToolCall::new(
                "ask",
                "ask_user",
                json!({ "question": "Continue?", "options": ["Yes", "No"] }),
            ),
            RuntimeToolCall::new("after", "safe_after", json!({})),
        ],
        &ToolExecutionContext::default(),
    );
    if report.messages.len() != 1
        || report.messages[0].tool_call_id != "before"
        || before_calls.load(Ordering::SeqCst) != 1
        || after_calls.load(Ordering::SeqCst) != 0
        || report.skipped_tool_calls.len() != 1
        || report.skipped_tool_calls[0].id != "after"
    {
        return Err(format!("ask_user boundary drifted under concurrency: {report:?}").into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser { tool_call_id, .. }) if tool_call_id == "ask" => Ok(()),
        other => Err(format!("unexpected concurrent ask interrupt: {other:?}").into()),
    }
}

#[test]
fn runtime_preserves_ask_user_interrupt_and_skips_later_tools() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    registry.register(CountingTool {
        calls: calls.clone(),
    });

    let executor = RuntimeToolExecutor::new(&registry);
    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new(
                "ask",
                "ask_user",
                json!({ "question": "Continue?", "options": ["Yes", "No"] }),
            ),
            RuntimeToolCall::new("count", "count", json!({})),
        ],
        &ToolExecutionContext::default(),
    );

    if !report.messages.is_empty()
        || report.skipped_tool_calls.len() != 1
        || calls.load(Ordering::SeqCst) != 0
    {
        return Err(format!("ask_user did not stop later tools: {report:?}").into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser {
            tool_call_id,
            name,
            question,
            options,
        }) if tool_call_id == "ask"
            && name == "ask_user"
            && question == "Continue?"
            && options == ["Yes", "No"] =>
        {
            Ok(())
        }
        other => Err(format!("unexpected ask_user interrupt: {other:?}").into()),
    }
}

#[test]
fn bridge_dispatcher_search_and_describe_use_current_catalog() -> Result<(), Box<dyn Error>> {
    let registry = ToolRegistry::new();
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([
        (
            "mcp_github_search_repositories",
            "Find GitHub repositories",
            ["query"],
        ),
        ("mcp_slack_post_message", "Post Slack messages", ["channel"]),
    ]);

    let search = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "search-call",
            "tool_search",
            json!({ "query": "github", "limit": 1 }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let search_messages = search.messages();
    let search_message = search_messages.first().ok_or("missing search message")?;
    let search_content = parse_json_content(&search_message.content)?;
    if search_message.tool_call_id != "search-call"
        || search_message.name != "tool_search"
        || search_content["matches"].as_array().map(Vec::len) != Some(1)
        || search_content["matches"][0]["name"] != "mcp_github_search_repositories"
        || search_content["matches"][0].get("schema").is_some()
    {
        return Err(format!("unexpected search result: {search:?}").into());
    }

    let describe = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "describe-call",
            "tool_describe",
            json!({ "name": "mcp_github_search_repositories" }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let describe_messages = describe.messages();
    let describe_message = describe_messages
        .first()
        .ok_or("missing describe message")?;
    let describe_content = parse_json_content(&describe_message.content)?;
    if describe_message.tool_call_id != "describe-call"
        || describe_message.name != "tool_describe"
        || describe_content["name"] != "mcp_github_search_repositories"
        || describe_content["schema"]["function"]["parameters"]["properties"]
            .get("query")
            .is_none()
    {
        return Err(format!("unexpected describe result: {describe:?}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_fails_closed_without_catalog() -> Result<(), Box<dyn Error>> {
    let registry = ToolRegistry::new();
    let executor = RuntimeToolExecutor::new(&registry);
    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "missing-catalog",
            "tool_search",
            json!({ "query": "github" }),
        ),
        None,
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let messages = report.messages();
    let message = messages.first().ok_or("missing error message")?;
    if message.tool_call_id != "missing-catalog"
        || !message
            .content
            .contains("deferred tool catalog is not available")
        || !message
            .content
            .contains("[Analyze the error above and try a different approach.]")
    {
        return Err(format!("missing catalog was not fail-closed: {report:?}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_rejects_recursive_core_and_out_of_scope_calls() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let cases = [
        (
            "recursive",
            json!({ "name": "tool_search", "arguments": {} }),
            "recursive bridge tool call rejected",
        ),
        (
            "core",
            json!({ "name": "repeat", "arguments": {} }),
            "call it directly",
        ),
        (
            "unknown",
            json!({ "name": "mcp_missing", "arguments": {} }),
            "outside the current deferred tool catalog",
        ),
    ];

    for (call_id, arguments, expected) in cases {
        let report = dispatch_bridge_tool_call(
            RuntimeToolCall::new(call_id, "tool_call", arguments),
            Some(&catalog),
            &registry,
            &executor,
            &ToolExecutionContext::default(),
        );
        let messages = report.messages();
        let message = messages.first().ok_or("missing rejection message")?;
        if message.tool_call_id != call_id || !message.content.contains(expected) {
            return Err(format!("unexpected rejection for {call_id}: {report:?}").into());
        }
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_accepts_object_and_json_string_arguments() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("mcp_repeat"));
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "object-call",
                "tool_call",
                json!({ "name": "mcp_repeat", "arguments": { "text": "ha", "times": 2 } }),
            ),
            RuntimeToolCall::new(
                "string-call",
                "tool_call",
                json!({ "name": "mcp_repeat", "arguments": "{\"text\":\"yo\",\"times\":3}" }),
            ),
        ],
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
        false,
    );

    let messages = report.messages();
    let contents = messages
        .iter()
        .map(|message| {
            (
                message.tool_call_id.as_str(),
                message.name.as_str(),
                message.content.as_str(),
            )
        })
        .collect::<Vec<_>>();
    if contents
        != [
            ("object-call", "tool_call", "haha"),
            ("string-call", "tool_call", "yoyoyo"),
        ]
        || report.resolved_calls.len() != 2
        || report.resolved_calls[0].underlying_name != "mcp_repeat"
    {
        return Err(format!("bridge did not execute normalized arguments: {report:?}").into());
    }
    if report.permissioned_actions.len() != 2
        || report.permissioned_actions[0].tool_name != "mcp_repeat"
        || report.permissioned_actions[0].normalization_state != ActionNormalizationState::Ready
    {
        return Err(
            format!("bridge did not report deferred permissioned actions: {report:?}").into(),
        );
    }
    match &report.permissioned_actions[0].origin {
        PermissionedActionOrigin::DeferredBridge {
            bridge_name,
            parent_origin,
            ..
        } if bridge_name == "tool_call"
            && matches!(**parent_origin, PermissionedActionOrigin::UserTurn) => {}
        other => return Err(format!("bridge origin did not preserve parent: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn runtime_permission_actions_include_context_containment() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(RepeatTool);
    registry.register(NamedRepeatTool("mcp_repeat"));
    let executor = RuntimeToolExecutor::new(&registry);
    let containment = ContainmentSnapshotRef {
        contained: Some(true),
        digest: Some("container token=sk-container-secret".to_owned()),
        summary: Some("sandbox bearer sk-summary-secret".to_owned()),
    };
    let context = ToolExecutionContext {
        containment_snapshot: Some(containment),
        ..ToolExecutionContext::default()
    };

    let direct_report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "direct-repeat",
            "repeat",
            json!({ "text": "ok", "times": 1 }),
        )],
        &context,
    );
    let direct_action = direct_report
        .permissioned_actions
        .first()
        .ok_or("missing direct permissioned action")?;
    assert_safe_containment_snapshot(direct_action)?;

    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let bridge_report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "bridge-repeat",
            "tool_call",
            json!({ "name": "mcp_repeat", "arguments": { "text": "ha", "times": 2 } }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &context,
    );
    let bridge_action = bridge_report
        .permissioned_actions
        .first()
        .ok_or("missing bridge permissioned action")?;
    assert_safe_containment_snapshot(bridge_action)?;
    Ok(())
}

fn assert_safe_containment_snapshot(
    action: &shacs_core::runtime::PermissionedAction,
) -> Result<(), Box<dyn Error>> {
    let snapshot = action
        .containment_snapshot
        .as_ref()
        .ok_or("missing containment snapshot")?;
    let serialized = serde_json::to_string(action)?;
    if snapshot.contained != Some(true)
        || serialized.contains("sk-container-secret")
        || serialized.contains("sk-summary-secret")
        || snapshot
            .digest
            .as_deref()
            .is_some_and(|value| value.contains("sk-container-secret"))
        || snapshot
            .summary
            .as_deref()
            .is_some_and(|value| value.contains("sk-summary-secret"))
    {
        return Err(format!("unsafe containment snapshot in action: {serialized}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_rejects_invalid_arguments_before_execution() -> Result<(), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(CountingTool {
        calls: calls.clone(),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("count", "Count executions", ["unused"])]);
    let cases = [
        ("array-args", json!({ "name": "count", "arguments": [] })),
        ("bad-json", json!({ "name": "count", "arguments": "{" })),
        ("scalar-json", json!({ "name": "count", "arguments": "[]" })),
    ];

    for (call_id, arguments) in cases {
        let report = dispatch_bridge_tool_call(
            RuntimeToolCall::new(call_id, "tool_call", arguments),
            Some(&catalog),
            &registry,
            &executor,
            &ToolExecutionContext::default(),
        );
        let messages = report.messages();
        let message = messages.first().ok_or("missing invalid argument message")?;
        if !message.content.contains("Invalid bridge arguments") {
            return Err(format!("invalid arguments were not rejected: {report:?}").into());
        }
    }
    if calls.load(Ordering::SeqCst) != 0 {
        return Err("invalid bridge arguments reached underlying executor".into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_preserves_underlying_validation_error_shape() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(NamedRepeatTool("mcp_repeat"));
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([("mcp_repeat", "Repeat deferred text", ["text"])]);
    let report = dispatch_bridge_tool_call(
        RuntimeToolCall::new(
            "invalid-repeat",
            "tool_call",
            json!({ "name": "mcp_repeat", "arguments": {} }),
        ),
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
    );
    let messages = report.messages();
    let message = messages.first().ok_or("missing validation error message")?;
    if message.tool_call_id != "invalid-repeat"
        || message.name != "tool_call"
        || !message
            .content
            .contains("Error: Invalid parameters for tool 'mcp_repeat'")
        || !message.content.contains("missing required text")
        || !message
            .content
            .contains("[Analyze the error above and try a different approach.]")
    {
        return Err(format!("validation error shape drifted: {report:?}").into());
    }
    Ok(())
}

#[test]
fn bridge_dispatcher_propagates_ask_user_interrupt() -> Result<(), Box<dyn Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(DeferredAskTool);
    registry.register(CountingTool {
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([
        ("mcp_ask_user", "Ask deferred question", ["question"]),
        ("count", "Count executions", ["unused"]),
    ]);
    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "ask-bridge",
                "tool_call",
                json!({ "name": "mcp_ask_user", "arguments": { "question": "Continue?" } }),
            ),
            RuntimeToolCall::new(
                "after-bridge",
                "tool_call",
                json!({ "name": "count", "arguments": {} }),
            ),
        ],
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
        false,
    );
    if !report.messages().is_empty()
        || report.skipped_tool_calls.len() != 1
        || report.skipped_tool_calls[0].id != "after-bridge"
        || report.skipped_tool_calls[0].name != "tool_call"
    {
        return Err(format!("ask_user bridge boundary drifted: {report:?}").into());
    }
    match report.interrupt {
        Some(RuntimeInterrupt::AskUser {
            tool_call_id,
            name,
            question,
            options,
        }) if tool_call_id == "ask-bridge"
            && name == "mcp_ask_user"
            && question == "Continue?"
            && options == ["Yes", "No"] =>
        {
            Ok(())
        }
        other => Err(format!("unexpected bridge interrupt: {other:?}").into()),
    }
}

#[test]
fn bridge_dispatcher_concurrency_uses_underlying_tool_metadata() -> Result<(), Box<dyn Error>> {
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(DelayTool::new(
        "mcp_safe_a",
        true,
        active.clone(),
        max_active.clone(),
        calls.clone(),
    ));
    registry.register(DelayTool::new(
        "mcp_safe_b",
        true,
        active,
        max_active.clone(),
        calls.clone(),
    ));
    let executor = RuntimeToolExecutor::new(&registry);
    let catalog = bridge_catalog([
        ("mcp_safe_a", "Safe deferred A", ["unused"]),
        ("mcp_safe_b", "Safe deferred B", ["unused"]),
    ]);
    let report = dispatch_bridge_tool_calls(
        vec![
            RuntimeToolCall::new(
                "safe-a",
                "tool_call",
                json!({ "name": "mcp_safe_a", "arguments": {} }),
            ),
            RuntimeToolCall::new(
                "safe-b",
                "tool_call",
                json!({ "name": "mcp_safe_b", "arguments": {} }),
            ),
        ],
        Some(&catalog),
        &registry,
        &executor,
        &ToolExecutionContext::default(),
        true,
    );
    let contents = report
        .messages()
        .iter()
        .map(|message| {
            (
                message.tool_call_id.clone(),
                message.name.clone(),
                message.content.clone(),
            )
        })
        .collect::<Vec<_>>();
    if contents
        != [
            (
                "safe-a".to_owned(),
                "tool_call".to_owned(),
                "mcp_safe_a".to_owned(),
            ),
            (
                "safe-b".to_owned(),
                "tool_call".to_owned(),
                "mcp_safe_b".to_owned(),
            ),
        ]
        || max_active.load(Ordering::SeqCst) != 2
        || calls.load(Ordering::SeqCst) != 2
    {
        return Err(
            format!("bridge concurrency did not use underlying metadata: {report:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn runtime_applies_message_and_spawn_context() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let sent = Arc::new(Mutex::new(Vec::<OutboundMessage>::new()));
    let sent_capture = sent.clone();
    let message_tool = MessageTool::with_sender(
        workspace.path(),
        Arc::new(move |message: OutboundMessage| {
            sent_capture
                .lock()
                .map_err(|error| error.to_string())?
                .push(message);
            Ok(())
        }),
        "",
        "",
        None,
    );

    let spawned = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let spawned_capture = spawned.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        spawned_capture
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));

    let mut registry = ToolRegistry::new();
    registry.register(message_tool.clone());
    registry.register(spawn_tool.clone());
    let executor = RuntimeToolExecutor::with_context_tools(
        &registry,
        RuntimeContextTools::new()
            .with_message(message_tool.clone())
            .with_spawn(spawn_tool),
    );
    let context = ToolExecutionContext {
        channel: "telegram".to_owned(),
        chat_id: "chat-1".to_owned(),
        message_id: Some("msg-1".to_owned()),
        metadata: json!({ "thread": "alpha" }),
        session_key: Some("session-1".to_owned()),
        containment_snapshot: None,
        in_cron_context: false,
        record_channel_delivery: true,
    };

    let report = executor.execute_tool_calls(
        vec![
            RuntimeToolCall::new("message", "message", json!({ "content": "hello" })),
            RuntimeToolCall::new("spawn", "spawn", json!({ "task": "check status" })),
        ],
        &context,
    );
    if report.messages.len() != 2 || report.messages[1].content != "spawned" {
        return Err(format!("unexpected context tool report: {report:?}").into());
    }

    let sent = sent.lock().map_err(|error| error.to_string())?;
    let Some(message) = sent.first() else {
        return Err("message tool did not send outbound message".into());
    };
    if message.channel != "telegram"
        || message.chat_id != "chat-1"
        || message.metadata["message_id"] != "msg-1"
        || message.metadata["thread"] != "alpha"
        || message.metadata["_record_channel_delivery"] != true
        || !message_tool.sent_in_turn()
    {
        return Err(format!("message context was not applied: {message:?}").into());
    }

    {
        let spawned = spawned.lock().map_err(|error| error.to_string())?;
        let Some(request) = spawned.first() else {
            return Err("spawn tool did not capture request".into());
        };
        if request.origin_channel != "telegram"
            || request.origin_chat_id != "chat-1"
            || request.session_key != "session-1"
        {
            return Err(format!("spawn context was not applied: {request:?}").into());
        }
    }

    let plain_executor = RuntimeToolExecutor::new(&registry);
    plain_executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "spawn-default",
            "spawn",
            json!({ "task": "default context" }),
        )],
        &ToolExecutionContext::default(),
    );
    let spawned = spawned.lock().map_err(|error| error.to_string())?;
    let Some(default_request) = spawned.get(1) else {
        return Err("spawn tool did not capture default-context request".into());
    };
    if default_request.origin_channel != "cli"
        || default_request.origin_chat_id != "direct"
        || default_request.session_key != "cli:direct"
    {
        return Err(format!("spawn context leaked after execution: {default_request:?}").into());
    }
    Ok(())
}

#[test]
fn runtime_restores_cron_context_guard_after_execution() -> Result<(), Box<dyn Error>> {
    let cron_tool = CronTool::new(Arc::new(InMemoryCronService::new()));
    let mut registry = ToolRegistry::new();
    registry.register(cron_tool.clone());
    cron_tool.set_cron_context(true);

    let executor = RuntimeToolExecutor::with_context_tools(
        &registry,
        RuntimeContextTools::new().with_cron(cron_tool.clone()),
    );
    let report = executor.execute_tool_calls(
        vec![RuntimeToolCall::new(
            "cron",
            "cron",
            json!({ "action": "add", "message": "stretch", "every_seconds": 60 }),
        )],
        &ToolExecutionContext {
            channel: "cli".to_owned(),
            chat_id: "direct".to_owned(),
            session_key: Some("cli:direct".to_owned()),
            ..ToolExecutionContext::default()
        },
    );
    if report.messages.len() != 1 || !report.messages[0].content.contains("Created job") {
        return Err(
            format!("runtime cron execution did not temporarily clear guard: {report:?}").into(),
        );
    }

    let direct = cron_tool
        .execute(json_map(json!({
            "action": "add",
            "message": "should fail",
            "every_seconds": 60
        }))?)
        .into_text();
    cron_tool.reset_cron_context(false);
    if direct != "Error: cannot schedule new jobs from within a cron job execution" {
        return Err(format!("cron context guard was not restored: {direct}").into());
    }
    Ok(())
}

fn json_map(value: Value) -> Result<JsonMap, Box<dyn Error>> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(format!("expected object, got {other}").into()),
    }
}

fn parse_json_content(content: &str) -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(content).map_err(Into::into)
}

fn bridge_catalog<const COUNT: usize, const PARAMETER_COUNT: usize>(
    entries: [(&str, &str, [&str; PARAMETER_COUNT]); COUNT],
) -> DeferredToolCatalog {
    DeferredToolCatalog::new(
        entries
            .into_iter()
            .map(
                |(name, description, parameter_names)| DeferredToolCatalogEntry {
                    name: name.to_owned(),
                    description: description.to_owned(),
                    parameter_names: parameter_names.into_iter().map(str::to_owned).collect(),
                    full_schema: runtime_tool_schema(name, description, parameter_names),
                    source_kind: "mcp_tool".to_owned(),
                    source_name: "test".to_owned(),
                },
            )
            .collect(),
        2,
        3,
    )
}

fn runtime_tool_schema<const PARAMETER_COUNT: usize>(
    name: &str,
    description: &str,
    parameter_names: [&str; PARAMETER_COUNT],
) -> Value {
    let mut properties = serde_json::Map::new();
    for parameter_name in parameter_names {
        properties.insert(
            parameter_name.to_owned(),
            json!({ "type": "string", "description": parameter_name }),
        );
    }
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": []
            }
        }
    })
}
