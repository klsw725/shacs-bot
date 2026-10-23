use super::*;
use crate::runtime::task_method::RequestState;
use crate::runtime::task_method_store::MethodStore;
use shacs_providers::{LlmResponse, ProviderRequest};

struct NoProvider;
impl ProviderClient for NoProvider {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        panic!("method commands must not call the provider")
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(shacs_providers::ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

struct TextProvider;
impl ProviderClient for TextProvider {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            content: Some("ok".to_owned()),
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

fn admission(requires_write_isolation: bool) -> WorkflowAdmissionInput {
    WorkflowAdmissionInput {
        objective_complexity: 2,
        estimated_item_count: 2,
        requires_parallelism: false,
        requires_independent_verification: true,
        requires_adversarial_review: false,
        requires_large_context_partitioning: false,
        requires_write_isolation,
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

#[test]
fn pending_workflow_cancellation_is_durable_before_markers_are_removed(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path();
    let tools = ToolRegistry::new();
    let provider = NoProvider;
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.durable_event_root = Some(workspace.join("events"));
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        &tools,
        &provider,
        config,
    );
    agent.process_message(InboundMessage::new(
        "chat",
        "alice",
        "room",
        "Calculate 2 plus 3",
    ))?;
    {
        let _guard = shacs_session::SessionMutationGuard::acquire(workspace, "chat:room")?;
        let mut manager = SessionManager::new(workspace)?;
        let mut session = manager.get_or_create("chat:room");
        store_pending_workflow(&mut session, "chat:room", &admission(false), &plan(), None)?;
        manager.save(&session)?;
    }
    // When
    agent.process_message(InboundMessage::new(
        "chat",
        "alice",
        "room",
        "/method use general",
    ))?;
    // Then
    let records = DurableEventStore::open(workspace.join("events"))?
        .scan(usize::MAX)?
        .records;
    assert!(records.iter().any(|event| event.kind == WORKFLOW_FAILED && matches!(&event.payload, DurableEventPayload::Inline { data, .. } if data["state"] == "cancelled" && data["reason"] == "method_changed")));
    let session = agent.sessions.get_or_create("chat:room");
    assert!(pending_workflow(&session).is_none());
    assert_eq!(session.metadata["runtime_workflow"]["state"], "cancelled");
    Ok(())
}

#[test]
fn non_user_turn_ending_a_bound_workflow_closes_its_method_request() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path();
    let tools = ToolRegistry::new();
    let provider = TextProvider;
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.durable_event_root = Some(workspace.join("events"));
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        &tools,
        &provider,
        config,
    );
    agent.process_message(InboundMessage::new(
        "chat",
        "alice",
        "room",
        "Calculate 2 plus 3",
    ))?;
    agent.process_message(InboundMessage::new(
        "chat",
        "alice",
        "room",
        "/method use general",
    ))?;
    let request_id = {
        let _guard = shacs_session::SessionMutationGuard::acquire(workspace, "chat:room")?;
        let mut manager = SessionManager::new(workspace)?;
        let mut session = manager.get_or_create("chat:room");
        let mut store = MethodStore::load(&session)?;
        let active = store.active.as_mut().ok_or("missing active request")?;
        active.state = RequestState::InProgress;
        let request_id = active.request_id.clone();
        store.save(&mut session)?;
        let mut plan = plan();
        method_workflow::bind(&session, &mut plan)?;
        store_pending_workflow(&mut session, "chat:room", &admission(true), &plan, None)?;
        manager.save(&session)?;
        request_id
    };
    // When
    agent.process_message(
        InboundMessage::new("system", "subagent", "room", "subagent finished")
            .with_session_key_override("chat:room"),
    )?;
    agent.process_message(InboundMessage::new("chat", "alice", "room", "hello"))?;
    // Then
    let session = agent.sessions.get_or_create("chat:room");
    assert!(pending_workflow(&session).is_none());
    let store = MethodStore::load(&session)?;
    let bound = store
        .closed
        .iter()
        .find(|request| request.request_id == request_id)
        .ok_or("bound request was not closed")?;
    assert_eq!(bound.state, RequestState::Failed);
    let latest = store.closed.last().ok_or("missing new request")?;
    assert_ne!(latest.request_id, request_id);
    assert_eq!(latest.goal_preview, "hello");
    Ok(())
}
