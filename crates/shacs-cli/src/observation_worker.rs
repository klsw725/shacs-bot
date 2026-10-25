use super::AgentLoopChatCompletionAdapter;
use shacs_core::runtime::{
    AgentHookContext, BoundedObservationTool, EntrustedObservationExecutor, ObservationBlock,
    ObservationResult, ObservationSource, ObservationWake, PluginRuntimeHookAgentHook,
    RuntimeToolCall, RuntimeToolExecutor, ToolExecutionContext, WebWatchExecutor, WebWatchWake,
};
use shacs_core::tools::ToolRegistry;

fn read_call(wake: &ObservationWake) -> RuntimeToolCall {
    let ObservationSource::LocalFile { relative_path } = &wake.plan.source;
    RuntimeToolCall::new(
        wake.durable_work_id(),
        "read_file",
        serde_json::json!({"path": relative_path}),
    )
}

impl AgentLoopChatCompletionAdapter {
    /// 031 snapshot provenance and 030 `tool:before` hooks before a bounded watcher read.
    fn watcher_preflight(
        &self,
        purpose: &str,
        call: &RuntimeToolCall,
    ) -> Result<String, ObservationBlock> {
        let snapshot = self
            .automation_snapshot(purpose)
            .map_err(|_| ObservationBlock::SnapshotUnavailable)?;
        snapshot
            .validate_provenance()
            .map_err(|_| ObservationBlock::SnapshotUnavailable)?;
        #[cfg(not(test))]
        let hook = PluginRuntimeHookAgentHook::new(self.plugin_runtime_snapshot.clone())
            .with_trusted_handlers(self.trusted_tool_before_handlers.clone())
            .with_spec030_fact_store(self.spec030_provider.fact_store());
        #[cfg(test)]
        let hook = PluginRuntimeHookAgentHook::new(self.plugin_runtime_snapshot.clone());
        let diagnostics_before = hook.hook_runtime_projection().diagnostics.len();
        hook.dispatch_tool_before(
            &AgentHookContext {
                iteration: 0,
                messages: Vec::new(),
            },
            std::slice::from_ref(call),
        );
        let facts = hook.hook_runtime_projection();
        if facts.diagnostics.len() > diagnostics_before
            || facts
                .recent_denials
                .iter()
                .any(|denial| denial.call_ref == call.id)
        {
            return Err(ObservationBlock::HookDenied);
        }
        Ok(format!(
            "{}:{}",
            snapshot.snapshot_id, snapshot.provenance_digest
        ))
    }
}

impl WebWatchExecutor for AgentLoopChatCompletionAdapter {
    fn prepare(&self, wake: &WebWatchWake, display_url: &str) -> Result<String, ObservationBlock> {
        let call = RuntimeToolCall::new(
            wake.durable_work_id(),
            "web_watch_fetch",
            serde_json::json!({"url": display_url}),
        );
        self.watcher_preflight("entrusted web observation", &call)
    }
}

impl EntrustedObservationExecutor for AgentLoopChatCompletionAdapter {
    fn prepare(&self, wake: &ObservationWake) -> Result<String, ObservationBlock> {
        self.watcher_preflight("entrusted local observation", &read_call(wake))
    }

    fn observe(&self, wake: &ObservationWake) -> ObservationResult {
        let config = self.loop_config();
        let mut registry = ToolRegistry::new();
        registry.register(BoundedObservationTool::new(&self.workspace, wake));
        let context = ToolExecutionContext {
            session_key: Some(wake.owner_session_id.clone()),
            active_workspace: Some(self.workspace.clone()),
            containment_snapshot: config.containment_snapshot,
            permission_mode_snapshot: config.permission_mode_snapshot,
            permission_rule_input: config.permission_rule_input,
            permission_auto_approval: config.permission_auto_approval,
            permission_ceiling_snapshot: config.permission_ceiling_snapshot,
            project_permission_store: config.project_permission_store,
            permission_interactive: false,
            ..ToolExecutionContext::default()
        };
        let report =
            RuntimeToolExecutor::new(&registry).execute_tool_calls(vec![read_call(wake)], &context);
        report
            .messages
            .first()
            .and_then(|message| serde_json::from_str(&message.content).ok())
            .unwrap_or(ObservationResult::Blocked {
                reason: ObservationBlock::PermissionDenied,
            })
    }
}

#[cfg(test)]
mod tests {
    use shacs_core::runtime::{
        DurableWorkDispatcher, DurableWorkEnqueueInput, MessageBus, ENTRUSTED_OBSERVATION_WORK_KIND,
    };
    use shacs_session::durable_work::WorkPayloadRef;

    #[test]
    fn observation_wake_is_not_a_channel_outbound_request() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let events = root.path().join("events");
        let mut dispatcher = DurableWorkDispatcher::open(
            &events,
            root.path().join("payloads"),
            MessageBus::new(),
            "owner",
            100,
        )?;
        for (id, kind) in [
            ("observation", ENTRUSTED_OBSERVATION_WORK_KIND),
            ("inbound", "agent.inbound_turn"),
        ] {
            dispatcher.enqueue_work(DurableWorkEnqueueInput {
                work_id: id.to_owned(),
                work_kind: kind.to_owned(),
                session_key: "telegram:chat".to_owned(),
                turn_id: None,
                effect_id: None,
                payload_ref: WorkPayloadRef::inline(
                    "fixture",
                    serde_json::json!({"channel":"telegram"}),
                )?,
                dedupe_hint: None,
                next_wake_at_ms: Some(100),
            })?;
        }
        let state = shacs_session::durable_replay::evaluate_durable_recovery(
            &events,
            root.path().join("checkpoints"),
        )
        .state
        .ok_or("replay")?;
        let mut channels = std::collections::BTreeMap::new();

        super::super::link_channel_restart_work_refs(&mut channels, &state.work);

        let telegram = channels.get("telegram").ok_or("inbound channel")?;
        assert_eq!(telegram.pending_inbound_refs.len(), 1);
        assert!(telegram.pending_outbound_refs.is_empty());
        Ok(())
    }
}
