use super::*;
use crate::runtime::task_method_calc::RunStatus;
use crate::runtime::{AgentRunSpec, AgentRunner, CancellationToken};
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};

struct OneExec;
impl ProviderClient for OneExec {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![shacs_providers::ToolCallRequest::new(
                "late-exec",
                "exec",
                serde_json::Map::from_iter([(
                    "command".to_owned(),
                    serde_json::json!("python3 -c 'print(1)'"),
                )]),
            )],
            ..LlmResponse::default()
        })
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

#[test]
fn an_exec_cancelled_before_preparation_is_recorded_as_a_cancelled_run() {
    // Given
    let tools = ToolRegistry::new();
    let provider = OneExec;
    let token = CancellationToken::new();
    let observation = Arc::new(Mutex::new(MethodObservation::default()));
    let mut spec = AgentRunSpec::new(
        vec![serde_json::json!({"role": "user", "content": "calculate"})],
        &tools,
        &provider,
        "test-model",
    );
    spec.cancellation_token = Some(token.clone());
    spec.checkpoint_callback = Some(Arc::new(move |checkpoint: &Value| {
        if checkpoint.get("phase").and_then(Value::as_str) == Some("awaiting_tools") {
            token.cancel();
        }
    }));
    attach(&mut spec, observation.clone());
    // When
    let result = AgentRunner::new().run(spec);
    // Then
    assert!(result.is_ok());
    let observed = lock(&observation);
    assert_eq!(observed.calculation_runs.len(), 1);
    let run = &observed.calculation_runs[0];
    assert_eq!(run.status, RunStatus::Cancelled);
    assert_eq!(run.code, SKIPPED_EXEC);
}

struct ReadThenAnswer(Mutex<Vec<LlmResponse>>);
impl ProviderClient for ReadThenAnswer {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(lock(&self.0).pop().unwrap_or_default())
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct Reader;
impl crate::tools::Tool for Reader {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a file"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({"type":"object","properties":{"path":{"type":"string"}}})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: crate::tools::JsonMap) -> crate::tools::ToolResult {
        "1| text".into()
    }
}

struct BlockEverything;
impl crate::runtime::AgentHook for BlockEverything {
    fn block_tool_calls(
        &self,
        _: &crate::runtime::AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        calls
            .iter()
            .map(|call| RuntimeToolMessage {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: "Held back by the test policy.".to_owned(),
            })
            .collect()
    }
}

#[test]
fn a_read_blocked_by_a_hook_is_a_failed_source_whatever_its_message() {
    // Given
    let mut tools = ToolRegistry::new();
    tools.register(Reader);
    let read = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![shacs_providers::ToolCallRequest::new(
            "blocked-read",
            "read_file",
            serde_json::Map::from_iter([("path".to_owned(), serde_json::json!("notes.md"))]),
        )],
        ..LlmResponse::default()
    };
    let done = LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    };
    let provider = ReadThenAnswer(Mutex::new(vec![done, read]));
    let observation = Arc::new(Mutex::new(MethodObservation::default()));
    let mut spec = AgentRunSpec::new(
        vec![serde_json::json!({"role": "user", "content": "report"})],
        &tools,
        &provider,
        "test-model",
    );
    spec.agent_hook = Some(Arc::new(BlockEverything));
    attach(&mut spec, observation.clone());
    // When
    let result = AgentRunner::new().run(spec);
    // Then
    assert!(result.is_ok());
    let observed = lock(&observation);
    assert_eq!(observed.report_sources.len(), 1);
    let source = &observed.report_sources[0];
    assert_eq!(
        source.status,
        crate::runtime::task_method_report::SourceStatus::Failed
    );
    assert!(source.content_digest.is_none());
}

#[test]
fn a_read_whose_call_id_holds_an_address_is_recorded_as_read() {
    // Given: a read the provider names with an address, which progress events show by digest.
    let mut tools = ToolRegistry::new();
    tools.register(Reader);
    let read = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![shacs_providers::ToolCallRequest::new(
            "https://shop.example/item?coupon=secret-ref#checkout",
            "read_file",
            serde_json::Map::from_iter([("path".to_owned(), serde_json::json!("notes.md"))]),
        )],
        ..LlmResponse::default()
    };
    let done = LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    };
    let provider = ReadThenAnswer(Mutex::new(vec![done, read]));
    let observation = Arc::new(Mutex::new(MethodObservation::default()));
    let mut spec = AgentRunSpec::new(
        vec![serde_json::json!({"role": "user", "content": "report"})],
        &tools,
        &provider,
        "test-model",
    );
    attach(&mut spec, observation.clone());
    // When
    let result = AgentRunner::new().run(spec);
    // Then: the read is the method's source, named without the address's query.
    assert!(result.is_ok());
    let observed = lock(&observation);
    assert_eq!(observed.report_sources.len(), 1);
    let source = &observed.report_sources[0];
    assert_eq!(
        source.status,
        crate::runtime::task_method_report::SourceStatus::Read
    );
    assert!(!source.call_id.contains("secret-ref"), "{}", source.call_id);
}

#[test]
fn reads_whose_ids_look_alike_in_diagnostics_are_both_recorded() {
    // Given: a read under an address and a read under a plain id written like a short digest of
    // that address.
    let address = "https://shop.example/item?coupon=secret-ref#checkout";
    let digest = format!("{:x}", Sha256::digest(address.as_bytes()));
    let look_alike = format!("call-{}", &digest[..16]);
    let mut tools = ToolRegistry::new();
    tools.register(Reader);
    let read = |id: &str, path: &str| {
        shacs_providers::ToolCallRequest::new(
            id,
            "read_file",
            serde_json::Map::from_iter([("path".to_owned(), serde_json::json!(path))]),
        )
    };
    let reads = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![read(address, "notes.md"), read(&look_alike, "other.md")],
        ..LlmResponse::default()
    };
    let done = LlmResponse {
        content: Some("done".to_owned()),
        ..LlmResponse::default()
    };
    let provider = ReadThenAnswer(Mutex::new(vec![done, reads]));
    let observation = Arc::new(Mutex::new(MethodObservation::default()));
    let mut spec = AgentRunSpec::new(
        vec![serde_json::json!({"role": "user", "content": "report"})],
        &tools,
        &provider,
        "test-model",
    );
    attach(&mut spec, observation.clone());
    // When
    let result = AgentRunner::new().run(spec);
    // Then: both reads are sources the method read, under two names.
    assert!(result.is_ok());
    let observed = lock(&observation);
    let read_names = observed
        .report_sources
        .iter()
        .filter(|source| source.status == crate::runtime::task_method_report::SourceStatus::Read)
        .map(|source| source.call_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(observed.report_sources.len(), 2);
    assert_eq!(read_names.len(), 2, "{read_names:?}");
}

struct StrictReader;
impl crate::tools::Tool for StrictReader {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a file"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}},"required":["path"]})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: crate::tools::JsonMap) -> crate::tools::ToolResult {
        "1| text".into()
    }
}

fn read_call(arguments: serde_json::Value) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![shacs_providers::ToolCallRequest::new(
            "early-read",
            "read_file",
            arguments.as_object().cloned().unwrap_or_default(),
        )],
        ..LlmResponse::default()
    }
}

#[test]
fn reads_stopped_before_preparation_keep_the_requested_locator() {
    for cancel in [true, false] {
        // Given
        let mut tools = ToolRegistry::new();
        tools.register(StrictReader);
        let arguments = if cancel {
            serde_json::json!({"path": "notes.md"})
        } else {
            serde_json::json!({"path": "notes.md", "limit": "many"})
        };
        let done = LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        };
        let provider = ReadThenAnswer(Mutex::new(vec![done, read_call(arguments)]));
        let observation = Arc::new(Mutex::new(MethodObservation::default()));
        let token = CancellationToken::new();
        let mut spec = AgentRunSpec::new(
            vec![serde_json::json!({"role": "user", "content": "report"})],
            &tools,
            &provider,
            "test-model",
        );
        if cancel {
            spec.cancellation_token = Some(token.clone());
            spec.checkpoint_callback = Some(Arc::new(move |checkpoint: &Value| {
                if checkpoint.get("phase").and_then(Value::as_str) == Some("awaiting_tools") {
                    token.cancel();
                }
            }));
        }
        attach(&mut spec, observation.clone());
        // When
        let result = AgentRunner::new().run(spec);
        // Then
        assert!(result.is_ok());
        let observed = lock(&observation);
        assert_eq!(observed.report_sources.len(), 1, "cancel={cancel}");
        let source = &observed.report_sources[0];
        assert_eq!(source.locator, "notes.md", "cancel={cancel}");
        assert_eq!(
            source.status,
            crate::runtime::task_method_report::SourceStatus::Failed
        );
    }
}
