use std::collections::VecDeque;
use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};
use shacs_core::runtime::*;
use shacs_core::tools::{AskUserTool, JsonMap, Tool, ToolRegistry, ToolResult};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};

#[path = "spec036_action_decision/decisions.rs"]
mod decisions;
#[path = "spec036_action_decision/gates.rs"]
mod gates;
#[path = "spec036_action_decision/lineage.rs"]
mod lineage;
#[path = "spec036_action_decision/manual.rs"]
mod manual;
#[path = "spec036_action_decision/recovery.rs"]
mod recovery;

struct Provider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl ProviderClient for Provider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests.lock().unwrap().push(request);
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider continuation"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _callback: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct SideEffect(Arc<AtomicUsize>);

impl Tool for SideEffect {
    fn name(&self) -> &str {
        "message"
    }
    fn description(&self) -> &str {
        "Count external sends without contacting a transport."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"content":{"type":"string"}},"required":["content"]})
    }
    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        "sent".into()
    }
}

fn setup(workspace: &Path) -> Result<EntrustedWorkSnapshot, Box<dyn Error>> {
    let mut manager = SessionManager::new(workspace)?;
    let session = manager.get_or_create("chat:room");
    manager.save(&session)?;
    Ok(create_entrusted_work(
        workspace,
        EntrustedWorkCreateRequest {
            owner_session_id: "chat:room".to_owned(),
            goal_text: "monitor offer".to_owned(),
            turn_budget: 100,
            consent: EntrustedWorkConsentProof {
                kind: EntrustedWorkConsentKind::ExplicitRequest,
                evidence_ref: "user:request".to_owned(),
                recorded_at: "2026-10-19T00:00:00Z".to_owned(),
            },
            conditions: EntrustedWorkConditions {
                instructions: "notify below 100".to_owned(),
                budget: None,
                prior_decisions: Vec::new(),
                remaining_questions: Vec::new(),
            },
            linked_targets: Vec::new(),
            observed_at: "2026-10-19T00:00:00Z".to_owned(),
        },
    )?)
}

fn proposal(work_id: &str) -> ToolCallRequest {
    ToolCallRequest::new(
        "decision-call",
        "ask_user",
        Map::from_iter([
            ("question".to_owned(), json!("Send the prepared request?")),
            (
                "external_action".to_owned(),
                json!({"work_id":work_id,"category":"send","summary":"Send the prepared inquiry","reference":"draft:inquiry-1"}),
            ),
        ]),
    )
}

fn send_call() -> ToolCallRequest {
    ToolCallRequest::new(
        "send-call",
        "message",
        Map::from_iter([("content".to_owned(), json!("prepared inquiry"))]),
    )
}

fn response(calls: Vec<ToolCallRequest>) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: calls,
        ..LlmResponse::default()
    }
}

fn registry(counter: &Arc<AtomicUsize>) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    tools.register(SideEffect(counter.clone()));
    tools
}

fn runtime<'a>(
    workspace: &Path,
    tools: &'a ToolRegistry,
    provider: &'a Provider,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.permission_interactive = true;
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        tools,
        provider,
        config,
    )
    .with_goal_completion_evaluator(Arc::new(ConservativeGoalCompletionEvaluator)))
}

fn inbound(content: &str) -> InboundMessage {
    InboundMessage::new("chat", "alice", "room", content)
}

fn proposal_id(result: &AgentLoopTurnResult) -> Result<String, Box<dyn Error>> {
    result
        .final_content
        .as_deref()
        .and_then(|text| text.split("/action accept ").nth(1))
        .and_then(|text| text.split('`').next())
        .map(str::to_owned)
        .ok_or_else(|| "missing action proposal command".into())
}

#[test]
fn free_text_never_resumes_an_external_action_question() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(vec![
        response(vec![proposal(&work.work.work_id)]),
        response(vec![send_call()]),
    ]);
    let mut agent = runtime(workspace.path(), &tools, &provider)?;
    agent.process_message(inbound("suggest next action"))?;

    agent.process_message(inbound("yes"))?;

    assert_eq!(provider.calls(), 1, "free text must not resume provider");
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn action_question_stops_a_cobatched_send_before_any_permission_or_execution(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(vec![response(vec![
        send_call(),
        proposal(&work.work.work_id),
    ])]);
    let mut agent = runtime(workspace.path(), &tools, &provider)?;

    let result = agent.process_message(inbound("suggest next action"))?;

    assert!(proposal_id(&result).is_ok());
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    let saved = agent
        .session_manager()
        .read_session_file("chat:room")
        .ok_or("missing session")?;
    assert!(saved["metadata"]
        .get("pending_permission_approval")
        .is_none());
    Ok(())
}
