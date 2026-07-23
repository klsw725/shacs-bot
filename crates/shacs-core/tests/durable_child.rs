use shacs_core::runtime::{
    CancellationToken, ChildResultEnvelope, ChildResultStatus, MergeDecision, SpawnEnvelope,
    SubagentExecutionConfig, SubagentRuntime,
};
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use shacs_session::durable_child::{
    ChildSpawned, DurableChildRecorder, ReplayChildTaskState, CHILD_RESULT_REENTRY_PAYLOAD_TYPE,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::{ReplayWorkState, WorkTerminalKind};
use std::error::Error;

struct UnusedProvider;

impl ProviderClient for UnusedProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        unreachable!("cancelled child must not invoke provider")
    }

    fn chat_stream(
        &self,
        _request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        unreachable!("cancelled child must not invoke provider")
    }
}

#[test]
fn subagent_runtime_persists_spawn_running_and_accepted_result() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let checkpoints = tempfile::tempdir()?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open(root.path())?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-1", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("child did not enter running state")?;
    let result = ChildResultEnvelope::from_spawn(
        &spawn,
        ChildResultStatus::Completed,
        "raw child result must not be durable inline",
    );
    let (decision, _) = runtime.finish_child(result);
    assert_eq!(decision, MergeDecision::AcceptSummaryOnly);

    let recovery = evaluate_durable_recovery(root.path(), checkpoints.path());
    assert!(recovery.writable, "{:?}", recovery.issues);
    let state = recovery.state.ok_or("missing replay state")?;
    let child = state.children.items.get("child-1").ok_or("missing child")?;
    assert_eq!(child.state, ReplayChildTaskState::Completed);
    assert!(child
        .result_ref
        .as_deref()
        .is_some_and(|value| value.starts_with("child-result:")));
    let events = std::fs::read_to_string(root.path().join("events.log"))?;
    assert!(!events.contains("raw child result must not be durable inline"));
    let traces = std::fs::read_to_string(
        root.path()
            .join("durable-diagnostics")
            .join("diagnostics.log"),
    )?;
    assert!(!traces.contains("child-1"), "{traces}");
    assert!(!traces.contains(&spawn.spawn_effect_id));
    Ok(())
}

#[test]
fn subagent_runtime_persists_stale_result_without_finishing_active_child(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let checkpoints = tempfile::tempdir()?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open(root.path())?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-1", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    let mut stale =
        ChildResultEnvelope::from_spawn(&spawn, ChildResultStatus::Completed, "stale raw result");
    stale.parent_turn_id = "turn:wrong".to_owned();
    let (decision, message) = runtime.finish_child(stale);
    assert!(matches!(decision, MergeDecision::DiscardAsStale { .. }));
    assert!(message.is_none());
    assert_eq!(runtime.running_count(), 1);

    let recovery = evaluate_durable_recovery(root.path(), checkpoints.path());
    let state = recovery.state.ok_or("missing replay state")?;
    assert_eq!(
        state.children.items["child-1"].state,
        ReplayChildTaskState::Spawned
    );
    assert_eq!(state.children.decisions.len(), 1);
    Ok(())
}

#[test]
fn subagent_runtime_restart_discards_late_success_after_durable_cancellation(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-cancelled", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    let cancelled =
        ChildResultEnvelope::from_spawn(&spawn, ChildResultStatus::Cancelled, "cancelled");
    assert_eq!(
        runtime.finish_child(cancelled).0,
        MergeDecision::AcceptCancellationFact
    );

    let restarted = SubagentRuntime::new()
        .attach_durable_recorder(recorder)
        .map_err(std::io::Error::other)?;
    let late_success =
        ChildResultEnvelope::from_spawn(&spawn, ChildResultStatus::Completed, "late raw success");
    let (decision, message) = restarted.finish_child(late_success);
    assert!(matches!(decision, MergeDecision::DiscardAsLate { .. }));
    assert!(message.is_none());
    let events = std::fs::read_to_string(root.path().join("events.log"))?;
    assert!(!events.contains("late raw success"));
    Ok(())
}

#[test]
fn subagent_runtime_rejects_identity_mismatch_without_poisoning_replay(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let checkpoints = tempfile::tempdir()?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open(root.path())?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-identity", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    let mut forged =
        ChildResultEnvelope::from_spawn(&spawn, ChildResultStatus::Completed, "forged result");
    forged.correlation_id = Some("correlation:forged".to_owned());
    let (decision, message) = runtime.finish_child(forged);
    assert!(matches!(decision, MergeDecision::DiscardAsStale { .. }));
    assert!(message.is_none());

    let recovery = evaluate_durable_recovery(root.path(), checkpoints.path());
    assert!(recovery.writable);
    assert_eq!(
        recovery.state.ok_or("missing replay")?.children.items["child-identity"].state,
        ReplayChildTaskState::Spawned
    );
    Ok(())
}

#[test]
fn child_cancel_request_rejects_late_success_and_token_cancellation_is_durable(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let checkpoints = tempfile::tempdir()?;
    let workspace = tempfile::tempdir()?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open(root.path())?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-token", "summarize");
    let token = CancellationToken::new();
    runtime
        .register_spawn_with_cancellation(spawn.clone(), token.clone())
        .map_err(std::io::Error::other)?;
    token.cancel();
    let result = runtime.run_spawn(
        spawn.clone(),
        &UnusedProvider,
        SubagentExecutionConfig::new(workspace.path(), "test-model"),
    );
    assert_eq!(result.status, ChildResultStatus::Cancelled);

    let late_success =
        ChildResultEnvelope::from_spawn(&spawn, ChildResultStatus::Completed, "late success");
    assert!(matches!(
        runtime.finish_child(late_success).0,
        MergeDecision::DiscardAsLate { .. } | MergeDecision::DiscardAsDuplicate { .. }
    ));
    let recovery = evaluate_durable_recovery(root.path(), checkpoints.path());
    assert!(recovery.writable, "{:?}", recovery.issues);
    let child = &recovery.state.ok_or("missing replay")?.children.items["child-token"];
    assert_eq!(child.state, ReplayChildTaskState::Cancelled);
    assert!(child.cancellation_requested_sequence < child.terminal_sequence);
    Ok(())
}

#[test]
fn concurrent_duplicate_finish_keeps_durable_replay_writable() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let checkpoints = tempfile::tempdir()?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open(root.path())?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-race", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("child did not enter running state")?;
    let result = ChildResultEnvelope::from_spawn(&spawn, ChildResultStatus::Completed, "result");
    let left_runtime = runtime.clone();
    let left_result = result.clone();
    let right_runtime = runtime.clone();
    let left = std::thread::spawn(move || left_runtime.finish_child(left_result).0);
    let right = std::thread::spawn(move || right_runtime.finish_child(result).0);
    let decisions = [
        left.join().map_err(|_| "left finish panicked")?,
        right.join().map_err(|_| "right finish panicked")?,
    ];
    assert!(decisions.contains(&MergeDecision::AcceptSummaryOnly));
    assert!(decisions
        .iter()
        .any(|decision| matches!(decision, MergeDecision::DiscardAsDuplicate { .. })));
    assert!(evaluate_durable_recovery(root.path(), checkpoints.path()).writable);
    Ok(())
}

#[test]
fn late_success_after_cancellation_keeps_child_until_terminal_cancel() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open(root.path())?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-cancel-race", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    assert_eq!(runtime.cancel_by_session("session"), 1);

    let late = ChildResultEnvelope::from_spawn(
        &spawn,
        ChildResultStatus::Completed,
        "late success must not finish cancellation",
    );
    assert!(matches!(
        runtime.finish_child(late).0,
        MergeDecision::DiscardAsLate { .. }
    ));
    assert_eq!(
        runtime.running_count(),
        1,
        "late success removed a cancellation-requested child before terminal cancel"
    );
    Ok(())
}

#[test]
fn restart_reregistration_of_same_spawn_is_idempotent() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let checkpoints = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    let spawn = SpawnEnvelope::new("session", "child-restart", "summarize");
    SubagentRuntime::new()
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;

    let restarted = SubagentRuntime::new()
        .attach_durable_recorder(recorder)
        .map_err(std::io::Error::other)?;
    restarted
        .register_spawn(spawn)
        .map_err(std::io::Error::other)?;

    let recovery = evaluate_durable_recovery(root.path(), checkpoints.path());
    assert!(
        recovery.writable,
        "idempotent restart registration poisoned durable replay: {:?}",
        recovery.issues
    );
    assert_eq!(restarted.running_count(), 1);
    Ok(())
}

#[test]
fn accepted_result_ref_resolves_to_reentry_artifact_and_enqueues_parent_after_event(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = shacs_core::runtime::MessageBus::new();
    let recorder = DurableChildRecorder::open_with_payload_root(&event_root, &payload_root)?;
    let runtime = SubagentRuntime::with_bus(bus.clone())
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-reentry", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("child did not enter running state")?;
    let result = ChildResultEnvelope::from_spawn(
        &spawn,
        ChildResultStatus::Completed,
        "raw child result must live only in the payload artifact",
    );

    let decision = runtime.publish_child_result(result.clone());
    assert_eq!(decision, MergeDecision::AcceptSummaryOnly);
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing replay")?;
    let child = state
        .children
        .items
        .get("child-reentry")
        .ok_or("missing child")?;
    let result_ref = child.result_ref.as_deref().ok_or("missing result ref")?;
    let artifact = recorder.read_result_artifact(result_ref)?;
    assert_eq!(artifact["payload_type"], CHILD_RESULT_REENTRY_PAYLOAD_TYPE);
    assert_eq!(artifact["result"]["child_task_id"], "child-reentry");
    let result_sequence = child.terminal_sequence.ok_or("missing result sequence")?;
    let parent_work = state
        .work
        .items
        .values()
        .find(|item| {
            item.work_kind == "agent.inbound_turn"
                && item.effect_id.as_deref() == Some(&spawn.spawn_effect_id)
        })
        .ok_or("missing parent reentry work")?;
    assert!(result_sequence < parent_work.enqueued_sequence);
    let events = std::fs::read_to_string(event_root.join("events.log"))?;
    assert!(!events.contains("raw child result must live only in the payload artifact"));
    assert!(
        bus.consume_inbound().is_none(),
        "durable child reentry must be delivered only through its durable inbound work"
    );
    Ok(())
}

#[test]
fn spawned_child_has_resolvable_run_artifact_and_idempotent_child_run_work(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let recorder = DurableChildRecorder::open_with_payload_root(&event_root, &payload_root)?;
    let spawn = SpawnEnvelope::new("session", "child-run-ticket", "summarize secret-token");
    SubagentRuntime::new()
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;

    let restarted = SubagentRuntime::new()
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?;
    restarted
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing replay")?;
    let child = state
        .children
        .items
        .get("child-run-ticket")
        .ok_or("missing child")?;
    let run_ref = child.run_ref.as_deref().ok_or("missing run ref")?;
    let run_artifact = recorder.read_run_artifact(run_ref)?;
    assert_eq!(run_artifact["child_task_id"], "child-run-ticket");
    let child_work = state
        .work
        .items
        .values()
        .find(|item| item.work_kind == "subagent.child_run")
        .ok_or("missing child run work")?;
    assert!(
        child.spawned_sequence < child_work.enqueued_sequence,
        "child run work was enqueued before the authoritative child spawn"
    );
    assert_eq!(
        state
            .children
            .items
            .values()
            .filter(|item| item.child_task_id == "child-run-ticket")
            .count(),
        1
    );
    let events = std::fs::read_to_string(event_root.join("events.log"))?;
    assert!(!events.contains("secret-token"));
    Ok(())
}

#[test]
fn child_run_work_is_leased_before_running_and_terminal_after_result() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(DurableChildRecorder::open_with_payload_root(
            &event_root,
            &payload_root,
        )?)
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-work-lifecycle", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("child did not enter running state")?;

    let running = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing running replay")?;
    let child_work = running
        .work
        .items
        .values()
        .find(|item| item.work_kind == "subagent.child_run")
        .ok_or("missing running child work")?;
    assert_eq!(child_work.state, ReplayWorkState::Leased);
    assert!(
        child_work.updated_sequence
            < running.children.items["child-work-lifecycle"]
                .started_sequence
                .ok_or("missing child running sequence")?
    );

    runtime.finish_child(ChildResultEnvelope::from_spawn(
        &spawn,
        ChildResultStatus::Completed,
        "complete",
    ));
    let completed = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing completed replay")?;
    let child_work = completed
        .work
        .items
        .values()
        .find(|item| item.work_kind == "subagent.child_run")
        .ok_or("missing completed child work")?;
    assert_eq!(child_work.state, ReplayWorkState::Terminal);
    assert_eq!(child_work.terminal_kind, Some(WorkTerminalKind::Succeeded));
    assert!(
        completed.children.items["child-work-lifecycle"].terminal_sequence
            < child_work.terminal_sequence
    );
    Ok(())
}

#[test]
fn restart_repairs_spawned_child_missing_run_work() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let recorder = DurableChildRecorder::open_with_payload_root(&event_root, &payload_root)?;
    let spawn = SpawnEnvelope::new("session", "child-missing-work", "summarize");
    let spawn_value = serde_json::to_value(&spawn)?;
    let run_ref = recorder.write_child_run_artifact(&spawn_value)?;
    recorder.record_spawned(
        &spawn.session_id,
        &ChildSpawned {
            child_task_id: spawn.child_task_id.clone(),
            parent_turn_id: spawn.parent_turn_id.clone(),
            spawn_effect_id: spawn.spawn_effect_id.clone(),
            correlation_id: spawn.correlation_id.clone().ok_or("missing correlation")?,
            idempotency_key: spawn.idempotency_key.clone().ok_or("missing idempotency")?,
            run_ref: Some(run_ref),
            attempt: 1,
            spawned_at_ms: u64::try_from(spawn.issued_at_ms)?,
        },
    )?;

    SubagentRuntime::new()
        .attach_durable_recorder(recorder)
        .map_err(std::io::Error::other)?;
    let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing repaired replay")?;
    assert!(state
        .work
        .items
        .values()
        .any(|item| item.work_kind == "subagent.child_run"));
    Ok(())
}

#[test]
fn recovery_repairs_terminal_child_with_nonterminal_run_work() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let recorder = DurableChildRecorder::open_with_payload_root(&event_root, &payload_root)?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-result-crash", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    runtime
        .mark_running(&spawn.child_task_id)
        .ok_or("child did not enter running state")?;
    let artifact = serde_json::json!({
        "payload_type": CHILD_RESULT_REENTRY_PAYLOAD_TYPE,
        "result": {"child_task_id": spawn.child_task_id, "status": "completed"},
        "decision": "accept_summary_only",
        "reentry_message": null,
    });
    let result_ref = recorder.write_result_artifact(&artifact)?;
    recorder.record_result(
        &spawn.session_id,
        &shacs_session::durable_child::ChildResultRecorded {
            child_task_id: spawn.child_task_id.clone(),
            parent_turn_id: spawn.parent_turn_id.clone(),
            spawn_effect_id: spawn.spawn_effect_id.clone(),
            correlation_id: spawn.correlation_id.clone().ok_or("missing correlation")?,
            idempotency_key: spawn.idempotency_key.clone().ok_or("missing idempotency")?,
            decision: shacs_session::durable_child::ChildResultDecisionKind::Accepted,
            terminal_state: Some(ReplayChildTaskState::Completed),
            result_ref,
            finished_at_ms: 30,
        },
    )?;

    recorder.repair_incomplete_lifecycle(40)?;
    let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing repaired replay")?;
    let work = state
        .work
        .items
        .values()
        .find(|item| item.work_kind == "subagent.child_run")
        .ok_or("missing child work")?;
    assert_eq!(work.state, ReplayWorkState::Terminal);
    assert_eq!(work.terminal_kind, Some(WorkTerminalKind::Succeeded));
    Ok(())
}

#[test]
fn recovery_finishes_spawned_cancellation_request_before_reregistration(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let recorder = DurableChildRecorder::open_with_payload_root(&event_root, &payload_root)?;
    let runtime = SubagentRuntime::new()
        .attach_durable_recorder(recorder.clone())
        .map_err(std::io::Error::other)?;
    let spawn = SpawnEnvelope::new("session", "child-cancel-crash", "summarize");
    runtime
        .register_spawn(spawn.clone())
        .map_err(std::io::Error::other)?;
    recorder.record_cancel_requested(
        &spawn.session_id,
        &spawn.parent_turn_id,
        &spawn.spawn_effect_id,
        spawn
            .correlation_id
            .as_deref()
            .ok_or("missing correlation")?,
        &shacs_session::durable_child::ChildCancelRequested {
            child_task_id: spawn.child_task_id.clone(),
            requested_at_ms: 20,
        },
    )?;

    let restarted = SubagentRuntime::new()
        .attach_durable_recorder(recorder)
        .map_err(std::io::Error::other)?;
    assert!(restarted.register_spawn(spawn).is_err());
    let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing cancellation repair replay")?;
    assert_eq!(
        state.children.items["child-cancel-crash"].state,
        ReplayChildTaskState::Cancelled
    );
    assert_eq!(
        state
            .work
            .items
            .values()
            .find(|item| item.work_kind == "subagent.child_run")
            .ok_or("missing child work")?
            .state,
        ReplayWorkState::Cancelled
    );
    Ok(())
}
