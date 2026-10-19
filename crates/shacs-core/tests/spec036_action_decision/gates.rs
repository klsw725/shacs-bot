use super::*;

struct UnusedExecutor;

impl PluginHookCommandExecutor for UnusedExecutor {
    fn execute(&self, _invocation: &PluginHookCommandInvocation) -> PluginHookCallbackResult {
        panic!("no external plugin commands in this fixture")
    }
}

struct ConfirmSend(Arc<AtomicUsize>);

impl ToolBeforeHandler for ConfirmSend {
    fn hook_ref(&self) -> &str {
        "test:confirm-send"
    }
    fn order_key(&self) -> ToolBeforeOrderKey {
        ToolBeforeOrderKey::new(self.hook_ref())
    }
    fn evaluate(&self, context: &ToolBeforeContext<'_>) -> ToolBeforeDecision {
        if context.call().name != "message" {
            return ToolBeforeDecision::Allow;
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        match context.confirm("confirm send independently of action intent") {
            ToolBeforeConfirmation::Confirmed => ToolBeforeDecision::Allow,
            ToolBeforeConfirmation::Denied | ToolBeforeConfirmation::HeadlessDenied => {
                ToolBeforeDecision::Block {
                    reason: "confirmation required".to_owned(),
                }
            }
        }
    }
}

#[test]
fn accepted_action_still_traverses_spec030_hook_confirmation() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let counter = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(vec![
        response(vec![proposal(&work.work.work_id)]),
        response(vec![send_call()]),
        LlmResponse {
            content: Some("blocked by confirmation".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let hook = PluginRuntimeHookAgentHook::with_executor(
        PluginRuntimeSnapshot::default(),
        PluginHookDispatchMode::LiveDiagnostics,
        Arc::new(UnusedExecutor),
    )
    .with_trusted_handlers(vec![Arc::new(ConfirmSend(hook_calls.clone()))]);
    let mut agent = runtime(workspace.path(), &tools, &provider)?.with_agent_hook(Arc::new(hook));
    let id = proposal_id(&agent.process_message(inbound("suggest action"))?)?;

    agent.process_message(inbound(&format!("/action accept {id}")))?;

    assert_eq!(hook_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn accepted_intent_cannot_bypass_tool_denial_or_a_later_action_decline(
) -> Result<(), Box<dyn Error>> {
    for decline_action in [false, true] {
        let workspace = tempfile::tempdir()?;
        let work = setup(workspace.path())?;
        let counter = Arc::new(AtomicUsize::new(0));
        let tools = registry(&counter);
        let provider = Provider::new(vec![
            response(vec![proposal(&work.work.work_id)]),
            response(vec![send_call()]),
        ]);
        let mut agent = runtime(workspace.path(), &tools, &provider)?;
        let id = proposal_id(&agent.process_message(inbound("suggest action"))?)?;
        agent.process_message(inbound(&format!("/action accept {id}")))?;

        agent.process_message(inbound(&if decline_action {
            format!("/action decline {id}")
        } else {
            "deny".to_owned()
        }))?;

        assert_eq!(provider.calls(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        let saved = agent
            .session_manager()
            .read_session_file("chat:room")
            .ok_or("missing session")?;
        assert!(saved["metadata"]
            .get("pending_permission_approval")
            .is_none());
        assert!(saved["metadata"].get("entrusted_action_decision").is_none());
    }
    Ok(())
}

#[test]
fn action_commands_wait_for_their_own_turn_instead_of_provider_injection(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    let provider = Provider::new(vec![LlmResponse {
        content: Some("ordinary answer".to_owned()),
        ..LlmResponse::default()
    }]);
    let bus = MessageBus::new();
    bus.publish_inbound(inbound("/action accept forged"));
    let mut agent = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &tools,
        &provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = agent.process_message(inbound("hello"))?;

    assert!(!result.had_injections);
    assert_eq!(
        bus.consume_inbound()
            .ok_or("missing queued command")?
            .content,
        "/action accept forged"
    );
    assert_eq!(provider.calls(), 1);
    Ok(())
}
