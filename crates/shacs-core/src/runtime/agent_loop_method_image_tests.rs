use super::*;
use crate::runtime::agent_loop::{compile_read_only_workflow_plan, store_pending_workflow};
use crate::runtime::{
    AgentLoopConfig, ContextBuilder, MessageBus, PermissionModeSnapshot, SessionManager,
    WorkflowAdmissionInput, WorkflowHarnessPlan,
};
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderRequest};
use shacs_session::durable_event::{DurableEventPayload, DurableEventStore, WORKFLOW_FAILED};
use std::error::Error;

struct ImageTool;
impl crate::tools::Tool for ImageTool {
    fn name(&self) -> &str {
        "image_generate"
    }
    fn description(&self) -> &str {
        "Registered so the image method is available; never called here"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn execute(&self, _: crate::tools::JsonMap) -> crate::tools::ToolResult {
        "unused".into()
    }
}

/// Always claims an image, as a workflow child or the main run would.
struct ClaimProvider;
impl ProviderClient for ClaimProvider {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            content: Some("The blue cat is ready.".to_owned()),
            ..LlmResponse::default()
        })
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(shacs_providers::ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn admission() -> WorkflowAdmissionInput {
    WorkflowAdmissionInput {
        objective_complexity: 2,
        estimated_item_count: 2,
        requires_parallelism: false,
        requires_independent_verification: true,
        requires_adversarial_review: false,
        requires_large_context_partitioning: false,
        requires_write_isolation: false,
        requires_recurring_loop: false,
        risk_level: 1,
        user_requested_workflow: true,
        available_budget_tokens: Some(10_000),
        blocking_reasons: Vec::new(),
        missing_scope_questions: Vec::new(),
    }
}

fn plan() -> WorkflowHarnessPlan {
    compile_read_only_workflow_plan(
        "inspect workspace",
        "chat:room",
        "fixture-turn",
        &PermissionModeSnapshot::default(),
    )
}

fn image_agent<'a>(
    workspace: &std::path::Path,
    tools: &'a ToolRegistry,
    provider: &'a ClaimProvider,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.durable_event_root = Some(workspace.join("events"));
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        tools,
        provider,
        config,
    ))
}

fn assert_runtime_answer(agent: &mut AgentLoop<'_>, answer: &str) -> Result<(), Box<dyn Error>> {
    assert!(
        answer.starts_with("No image was generated for this request (image: not generated)"),
        "{answer}"
    );
    let session = agent.sessions.get_or_create("chat:room");
    let history = serde_json::to_string(&session.messages)?;
    for shown in [answer, &history] {
        assert!(!shown.contains("blue cat"), "{shown}");
        assert!(!shown.contains("Workflow"), "{shown}");
    }
    assert!(pending_workflow(&session).is_none());
    Ok(())
}

#[test]
fn an_image_request_with_workflow_metadata_stays_on_the_checked_image_path(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let mut tools = ToolRegistry::new();
    tools.register(ImageTool);
    let mut agent = image_agent(root.path(), &tools, &ClaimProvider)?;
    let mut message =
        InboundMessage::new("chat", "alice", "room", "Generate an image of a red square");
    message.metadata.insert(
        "workflow_admission".to_owned(),
        serde_json::to_value(admission())?,
    );
    message
        .metadata
        .insert("workflow_plan".to_owned(), serde_json::to_value(plan())?);
    // When
    let answer = agent
        .process_message(message)?
        .final_content
        .unwrap_or_default();
    // Then
    assert_runtime_answer(&mut agent, &answer)
}

#[test]
fn an_image_request_cancels_a_pending_workflow_instead_of_resuming_it() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path();
    let mut tools = ToolRegistry::new();
    tools.register(ImageTool);
    let mut agent = image_agent(workspace, &tools, &ClaimProvider)?;
    {
        let _guard = shacs_session::SessionMutationGuard::acquire(workspace, "chat:room")?;
        let mut manager = SessionManager::new(workspace)?;
        let mut session = manager.get_or_create("chat:room");
        store_pending_workflow(&mut session, "chat:room", &admission(), &plan(), None)?;
        manager.save(&session)?;
    }
    // When
    let answer = agent
        .process_message(InboundMessage::new(
            "chat",
            "alice",
            "room",
            "Generate an image of a red square",
        ))?
        .final_content
        .unwrap_or_default();
    // Then
    assert_runtime_answer(&mut agent, &answer)?;
    let records = DurableEventStore::open(workspace.join("events"))?
        .scan(usize::MAX)?
        .records;
    assert!(records.iter().any(|event| event.kind == WORKFLOW_FAILED && matches!(&event.payload, DurableEventPayload::Inline { data, .. } if data["state"] == "cancelled" && data["reason"] == "image_request")));
    Ok(())
}

/// A registered tool that is never called here.
struct Named(&'static str);
impl crate::tools::Tool for Named {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "Registered so a method part is available; never called here"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn execute(&self, _: crate::tools::JsonMap) -> crate::tools::ToolResult {
        "unused".into()
    }
}

#[test]
fn a_composed_request_cancels_a_pending_workflow_instead_of_resuming_it(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path();
    let mut tools = ToolRegistry::new();
    tools.register(Named("read_file"));
    tools.register(Named("exec"));
    let mut agent = image_agent(workspace, &tools, &ClaimProvider)?;
    {
        let _guard = shacs_session::SessionMutationGuard::acquire(workspace, "chat:room")?;
        let mut manager = SessionManager::new(workspace)?;
        let mut session = manager.get_or_create("chat:room");
        store_pending_workflow(&mut session, "chat:room", &admission(), &plan(), None)?;
        manager.save(&session)?;
    }
    // When
    let answer = agent
        .process_message(InboundMessage::new(
            "chat",
            "alice",
            "room",
            "Write a report with sources on the room and calculate the floor area in m^2",
        ))?
        .final_content
        .unwrap_or_default();
    // Then
    assert!(
        answer.contains("Method: composed [calculation + report]"),
        "{answer}"
    );
    assert!(!answer.contains("Workflow"), "{answer}");
    let session = agent.sessions.get_or_create("chat:room");
    assert!(pending_workflow(&session).is_none());
    let records = DurableEventStore::open(workspace.join("events"))?
        .scan(usize::MAX)?
        .records;
    assert!(records.iter().any(|event| event.kind == WORKFLOW_FAILED && matches!(&event.payload, DurableEventPayload::Inline { data, .. } if data["state"] == "cancelled" && data["reason"] == "composed_request")));
    Ok(())
}
