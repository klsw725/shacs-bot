#[path = "spec033_automation_results.rs"]
mod fixtures;

use fixtures::{requirements, snapshot, source_event};
use shacs_core::runtime::{
    build_spec033_snapshot_from, AutomationDeliveryResult, AutomationDispatchRequest,
    AutomationExecutionControl, AutomationExecutionReceipt, AutomationExecutionTerminalFact,
    AutomationExecutor, AutomationGateResolution, AutomationGateResolver, AutomationJobResult,
    AutomationOutcomePolicy, AutomationProcessCleanupFact, AutomationScheduleKind,
    AutomationWorkEnqueueInput, AutomationWorkEnvelope, DurableWorkDispatcher, MessageBus,
    PluginHookDispatchEffect, PluginHookDispatchRecord, PluginHookDispatchStatus, PluginHookEvent,
    SandboxMode,
};
use shacs_eval::evaluator::{AutomationExecutionMode, AutomationRunState};
use shacs_projection::HookDenialReason;
use shacs_projection::SandboxFallback;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, WORK_ENQUEUED,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::{
    evaluate_durable_work_recovery, ReplayWorkState, WorkPayloadRef, WorkTerminalKind,
    MAX_DURABLE_WORK_OPEN_ITEMS,
};
use std::error::Error;
use std::time::{Duration, Instant};

struct SuccessfulNoAgentExecutor {
    requests: Vec<AutomationDispatchRequest>,
}

struct TerminalControlExecutor {
    calls: usize,
}

struct DurableCancellationRaceExecutor {
    event_root: std::path::PathBuf,
    checkpoint_root: std::path::PathBuf,
    payload_root: std::path::PathBuf,
    bus: MessageBus,
}

impl AutomationExecutor for TerminalControlExecutor {
    fn execute(
        &mut self,
        request: AutomationDispatchRequest,
        control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        if request.work_id == "timed-out-work" {
            control.mark_deadline_elapsed();
        } else {
            control.cancel();
        }
        self.calls += 1;
        AutomationExecutionReceipt {
            job_result: AutomationJobResult::Succeeded {
                result_ref: "late-success".to_owned(),
            },
            terminal_fact: AutomationExecutionTerminalFact::Completed,
            delivery_result: AutomationDeliveryResult::NotRequested,
            process_receipt: None,
            process_cleanup: AutomationProcessCleanupFact::NotRequired,
            task_outcome: None,
        }
    }
}

impl AutomationExecutor for DurableCancellationRaceExecutor {
    fn execute(
        &mut self,
        request: AutomationDispatchRequest,
        _control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        let mut dispatcher = DurableWorkDispatcher::open(
            &self.event_root,
            &self.payload_root,
            self.bus.clone(),
            "cancel-race",
            100,
        )
        .expect("race dispatcher opens");
        let replay = evaluate_durable_recovery(&self.event_root, &self.checkpoint_root);
        let state = replay.state.expect("leased work replays");
        let item = state
            .work
            .items
            .get(&request.work_id)
            .expect("leased work exists");
        dispatcher
            .request_cancellation(item, "user_stop")
            .expect("durable cancellation records");
        AutomationExecutionReceipt {
            job_result: AutomationJobResult::Succeeded {
                result_ref: "late-success".to_owned(),
            },
            terminal_fact: AutomationExecutionTerminalFact::Completed,
            delivery_result: AutomationDeliveryResult::NotRequested,
            process_receipt: None,
            process_cleanup: AutomationProcessCleanupFact::NotRequired,
            task_outcome: None,
        }
    }
}

#[test]
fn execution_control_rejects_success_observed_after_deadline() {
    // Given
    let control =
        AutomationExecutionControl::with_timeout("test-timeout", Duration::from_millis(20));
    let started = Instant::now();

    // When
    std::thread::sleep(Duration::from_millis(25));
    let mut receipt = AutomationExecutionReceipt {
        job_result: AutomationJobResult::Succeeded {
            result_ref: "late-success".to_owned(),
        },
        terminal_fact: AutomationExecutionTerminalFact::Completed,
        delivery_result: AutomationDeliveryResult::NotRequested,
        process_receipt: None,
        process_cleanup: AutomationProcessCleanupFact::NotRequired,
        task_outcome: None,
    };
    if control.deadline_elapsed() {
        receipt.job_result = AutomationJobResult::TimedOut {
            timeout_ref: control.timeout_ref().to_owned(),
        };
        receipt.terminal_fact = AutomationExecutionTerminalFact::TimedOut {
            timeout_ref: control.timeout_ref().to_owned(),
        };
    }

    // Then
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(
        receipt.job_result,
        AutomationJobResult::TimedOut {
            timeout_ref: "test-timeout".to_owned()
        }
    );
    assert_eq!(
        receipt.terminal_fact,
        AutomationExecutionTerminalFact::TimedOut {
            timeout_ref: "test-timeout".to_owned()
        }
    );
}

#[test]
fn execution_control_exposes_cancellation_without_spawning_a_watchdog() {
    // Given
    let control = AutomationExecutionControl::with_timeout("test-timeout", Duration::from_secs(1));
    let started = Instant::now();

    // When
    control.cancel();

    // Then
    assert!(started.elapsed() < Duration::from_millis(250));
    assert!(control.is_cancelled());
    assert!(!control.deadline_elapsed());
}

#[test]
fn durable_cancellation_recorded_during_execution_prevents_terminal_success(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner", 100)?;
    let queued_snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: "cancel-race-work".to_owned(),
        envelope: AutomationWorkEnvelope {
            event: source_event(AutomationExecutionMode::NoAgentCheck),
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            enqueue_provenance_snapshot: Some(queued_snapshot.clone()),
            expected_current_facts_digest: queued_snapshot.semantic_compatibility_digest()?,
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("race cancellation".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Notify,
        },
        next_wake_at_ms: None,
    })?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing queued work")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 1);
    let mut resolver = FreshGateResolver {
        snapshot: refreshed_snapshot(queued_snapshot),
        calls: 0,
    };
    let mut executor = DurableCancellationRaceExecutor {
        event_root: event_root.clone(),
        checkpoint_root: checkpoint_root.clone(),
        payload_root: payload_root.clone(),
        bus,
    };

    // When
    dispatcher.dispatch_due_automation(&state.work, &admission, 1, &mut resolver, &mut executor)?;

    // Then
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let terminal = replay.state.ok_or("missing terminal work")?;
    let work = &terminal.work.items["cancel-race-work"];
    assert_eq!(work.terminal_kind, Some(WorkTerminalKind::Failed));
    assert_eq!(
        work.terminal_facts
            .as_ref()
            .map(|facts| &facts["job_result"]["kind"]),
        Some(&serde_json::json!("cancelled"))
    );
    Ok(())
}

struct FreshGateResolver {
    snapshot: shacs_core::runtime::ExecutionSnapshot,
    calls: usize,
}

struct FailedGateResolver {
    snapshot: shacs_core::runtime::ExecutionSnapshot,
}

struct UnsupportedGateResolver {
    snapshot: shacs_core::runtime::ExecutionSnapshot,
}

#[test]
fn automation_enqueue_fails_closed_without_orphaning_payload_at_event_log_quota(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let payloads = root.path().join("payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(events.join("events.log"))?
        .set_len(512 * 1024 * 1024)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);

    // When
    let error = dispatcher
        .enqueue_automation(AutomationWorkEnqueueInput {
            work_id: "automation-at-quota".to_owned(),
            envelope: AutomationWorkEnvelope {
                event: source_event(AutomationExecutionMode::NoAgentCheck),
                schedule: AutomationScheduleKind::OneShot,
                existing_runs: Vec::new(),
                enqueue_provenance_snapshot: Some(current.clone()),
                expected_current_facts_digest: current.semantic_compatibility_digest()?,
                hook_evidence: None,
                requirements: requirements(),
                instruction: Some("check quota".to_owned()),
                outcome_policy: AutomationOutcomePolicy::Notify,
            },
            next_wake_at_ms: None,
        })
        .expect_err("automation work must be rejected at the event-log quota");

    // Then
    assert!(error.to_string().contains("durable event log exceeds"));
    assert_eq!(std::fs::read_dir(&payloads)?.count(), 0);
    Ok(())
}

#[test]
fn automation_enqueue_rejects_open_work_limit_without_orphaning_payload(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let payloads = root.path().join("payloads");
    let mut event_store = DurableEventStore::open(&events)?;
    for index in 0..MAX_DURABLE_WORK_OPEN_ITEMS {
        event_store.append(DurableEventInput::new(
            "session-1",
            WORK_ENQUEUED,
            DurableEventPayload::inline(
                "durable_work",
                serde_json::json!({
                    "work_id": format!("seed-{index}"),
                    "work_kind": "test.seed",
                    "payload_ref": WorkPayloadRef::inline(
                        "test.seed.v1",
                        serde_json::json!({"index": index}),
                    )?,
                }),
            ),
        ))?;
    }
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);

    // When
    let error = dispatcher
        .enqueue_automation(AutomationWorkEnqueueInput {
            work_id: "automation-over-limit".to_owned(),
            envelope: AutomationWorkEnvelope {
                event: source_event(AutomationExecutionMode::NoAgentCheck),
                schedule: AutomationScheduleKind::OneShot,
                existing_runs: Vec::new(),
                enqueue_provenance_snapshot: Some(current.clone()),
                expected_current_facts_digest: current.semantic_compatibility_digest()?,
                hook_evidence: None,
                requirements: requirements(),
                instruction: Some("check open-work limit".to_owned()),
                outcome_policy: AutomationOutcomePolicy::Notify,
            },
            next_wake_at_ms: None,
        })
        .expect_err("automation work must be rejected at the open-work limit");

    // Then
    assert!(error.to_string().contains("open durable work limit"));
    assert_eq!(std::fs::read_dir(&payloads)?.count(), 0);
    let state = evaluate_durable_recovery(&events, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), MAX_DURABLE_WORK_OPEN_ITEMS);
    Ok(())
}

impl AutomationGateResolver for FreshGateResolver {
    fn resolve(&mut self, _request: &AutomationDispatchRequest) -> AutomationGateResolution {
        self.calls += 1;
        AutomationGateResolution {
            execution_snapshot: Some(self.snapshot.clone()),
            hook_evidence: vec![PluginHookDispatchRecord {
                plugin_id: "runtime:tool-before".to_owned(),
                event: PluginHookEvent::ToolBefore,
                status: PluginHookDispatchStatus::Succeeded,
                effect: Some(PluginHookDispatchEffect::Observed),
                output_evidence: None,
                error: None,
                timeout: None,
            }],
            hook_denial: None,
            adapter_supported: true,
            requirements: _request.requirements.clone(),
        }
    }
}

impl AutomationGateResolver for FailedGateResolver {
    fn resolve(&mut self, request: &AutomationDispatchRequest) -> AutomationGateResolution {
        AutomationGateResolution {
            execution_snapshot: Some(self.snapshot.clone()),
            hook_evidence: vec![PluginHookDispatchRecord::successful_noop()],
            hook_denial: Some(HookDenialReason::HookFailed),
            adapter_supported: true,
            requirements: request.requirements.clone(),
        }
    }
}

impl AutomationGateResolver for UnsupportedGateResolver {
    fn resolve(&mut self, request: &AutomationDispatchRequest) -> AutomationGateResolution {
        AutomationGateResolution {
            execution_snapshot: Some(self.snapshot.clone()),
            hook_evidence: vec![PluginHookDispatchRecord::successful_noop()],
            hook_denial: None,
            adapter_supported: false,
            requirements: request.requirements.clone(),
        }
    }
}

impl AutomationExecutor for SuccessfulNoAgentExecutor {
    fn execute(
        &mut self,
        request: AutomationDispatchRequest,
        _control: AutomationExecutionControl,
    ) -> AutomationExecutionReceipt {
        self.requests.push(request);
        AutomationExecutionReceipt {
            job_result: AutomationJobResult::Succeeded {
                result_ref: "result:no-agent:1".to_owned(),
            },
            terminal_fact: AutomationExecutionTerminalFact::Completed,
            delivery_result: AutomationDeliveryResult::NotRequested,
            process_receipt: None,
            process_cleanup: AutomationProcessCleanupFact::NotRequired,
            task_outcome: None,
        }
    }
}

#[test]
fn due_automation_is_normalized_consumed_and_persisted_by_durable_dispatcher(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let mut dispatcher = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        MessageBus::new(),
        "owner-1",
        100,
    )?;
    let queued_snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::NoAgentCheck);
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: "automation-work-1".to_owned(),
        envelope: AutomationWorkEnvelope {
            event,
            schedule: AutomationScheduleKind::Recurring,
            existing_runs: Vec::new(),
            enqueue_provenance_snapshot: Some(queued_snapshot.clone()),
            expected_current_facts_digest: queued_snapshot.semantic_compatibility_digest()?,
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("check runtime health".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Notify,
        },
        next_wake_at_ms: Some(100),
    })?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing queued automation")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 100);
    let mut executor = SuccessfulNoAgentExecutor {
        requests: Vec::new(),
    };
    let fresh_snapshot = refreshed_snapshot(queued_snapshot);
    let mut resolver = FreshGateResolver {
        snapshot: fresh_snapshot.clone(),
        calls: 0,
    };
    let summary = dispatcher.dispatch_due_automation(
        &state.work,
        &admission,
        100,
        &mut resolver,
        &mut executor,
    )?;

    assert_eq!(summary.consumed_work_ids, vec!["automation-work-1"]);
    assert!(summary.suppressed_work_ids.is_empty());
    assert_eq!(executor.requests.len(), 1);
    assert_eq!(resolver.calls, 1);

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let terminal = replay.state.ok_or("missing terminal automation")?;
    let work = &terminal.work.items["automation-work-1"];
    assert_eq!(work.state, ReplayWorkState::Terminal);
    assert_eq!(work.terminal_kind, Some(WorkTerminalKind::Succeeded));
    let facts = work
        .terminal_facts
        .as_ref()
        .ok_or("missing persisted automation facts")?;
    assert_eq!(facts["lifecycle"]["run"]["state"], "succeeded");
    assert_eq!(facts["lifecycle"]["gate"]["denial"], "terminal_result");
    assert_eq!(facts["delivery_result"]["kind"], "not_requested");
    assert_eq!(
        facts["lifecycle"]["gate"]["snapshot_digest"],
        fresh_snapshot.provenance_digest
    );
    Ok(())
}

fn refreshed_snapshot(
    snapshot: shacs_core::runtime::ExecutionSnapshot,
) -> shacs_core::runtime::ExecutionSnapshot {
    shacs_core::runtime::ExecutionSnapshot::create(shacs_core::runtime::ExecutionSnapshotInput {
        snapshot_id: "snapshot-fresh".to_owned(),
        created_at_unix_ms: 200,
        config: snapshot.config,
        profiles: snapshot.profiles,
        trusted_runtime: snapshot.trusted_runtime,
        sandbox: snapshot.sandbox,
        credential: snapshot.credential,
        context_sources: snapshot.context_sources,
        selected_tools: snapshot.selected_tools,
        selected_resources: snapshot.selected_resources,
        provider: snapshot.provider,
        token_budget: snapshot.token_budget,
        disclosure: snapshot.disclosure,
        replay: snapshot.replay,
    })
    .expect("valid refreshed snapshot fixture")
}

#[test]
fn missing_hook_evidence_blocks_sensitive_work_without_reporting_queued(
) -> Result<(), Box<dyn Error>> {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::ScriptOnly);
    let outcome = shacs_core::runtime::own_automation_lifecycle(
        shacs_core::runtime::AutomationLifecycleInput {
            event: &event,
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: &[],
            durable_work: None,
            execution_snapshot: Some(&snapshot),
            expected_snapshot_digest: &snapshot.semantic_compatibility_digest()?,
            hook_evidence: None,
            hook_denial: None,
            requirements: requirements(),
            job_result: AutomationJobResult::Pending,
            delivery_result: AutomationDeliveryResult::NotRequested,
        },
    );

    assert_eq!(
        outcome.no_dispatch_reason,
        Some(shacs_core::runtime::AutomationNoDispatchReason::MissingHookEvidence)
    );
    assert_eq!(
        outcome.lifecycle.run.map(|run| run.state),
        Some(AutomationRunState::Suppressed)
    );
    Ok(())
}

#[test]
fn failed_hook_denial_blocks_before_executor_and_persists_terminal_evidence(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let events = root.path().join("runtime/durable-events");
    let checkpoints = root.path().join("runtime/durable-checkpoints");
    let payloads = root.path().join("runtime/work-payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: "hook-failed".to_owned(),
        envelope: AutomationWorkEnvelope {
            event: source_event(AutomationExecutionMode::ScriptOnly),
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            enqueue_provenance_snapshot: Some(current.clone()),
            expected_current_facts_digest: current.semantic_compatibility_digest()?,
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("blocked".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Suppress,
        },
        next_wake_at_ms: None,
    })?;
    let state = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payloads, 100);
    let mut resolver = FailedGateResolver { snapshot: current };
    let mut executor = SuccessfulNoAgentExecutor {
        requests: Vec::new(),
    };

    dispatcher.dispatch_due_automation(
        &state.work,
        &admission,
        100,
        &mut resolver,
        &mut executor,
    )?;

    assert!(executor.requests.is_empty());
    let terminal = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing terminal")?;
    assert_eq!(
        terminal.work.items["hook-failed"]
            .terminal_facts
            .as_ref()
            .and_then(|facts| facts["lifecycle"]["gate"]["denial"].as_str()),
        Some("hook_failed")
    );
    let projection = build_spec033_snapshot_from(root.path(), root.path(), "session-1")?;
    assert_eq!(
        projection
            .automation
            .fact
            .ok_or("missing suppressed automation projection")?
            .job_status,
        shacs_projection::Spec033AutomationJobStatus::Suppressed
    );
    Ok(())
}

#[test]
fn unsupported_adapter_blocks_before_lease_and_executor() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let events = root.path().join("runtime/durable-events");
    let checkpoints = root.path().join("runtime/durable-checkpoints");
    let payloads = root.path().join("runtime/work-payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: "adapter-unsupported".to_owned(),
        envelope: AutomationWorkEnvelope {
            event: source_event(AutomationExecutionMode::ScriptOnly),
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            enqueue_provenance_snapshot: Some(current.clone()),
            expected_current_facts_digest: current.semantic_compatibility_digest()?,
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("must not run".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Suppress,
        },
        next_wake_at_ms: None,
    })?;
    let state = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payloads, 100);
    let mut resolver = UnsupportedGateResolver { snapshot: current };
    let mut executor = SuccessfulNoAgentExecutor {
        requests: Vec::new(),
    };

    dispatcher.dispatch_due_automation(
        &state.work,
        &admission,
        100,
        &mut resolver,
        &mut executor,
    )?;

    assert!(executor.requests.is_empty());
    let terminal = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing terminal")?;
    let work = &terminal.work.items["adapter-unsupported"];
    assert_eq!(work.state, ReplayWorkState::Terminal);
    assert_eq!(work.lease_id, None);
    assert_eq!(
        work.terminal_facts
            .as_ref()
            .and_then(|facts| facts["lifecycle"]["gate"]["denial"].as_str()),
        Some("adapter_unsupported")
    );
    Ok(())
}

#[test]
fn unknown_timeout_policy_fails_before_execution() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let checkpoints = root.path().join("checkpoints");
    let payloads = root.path().join("payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let mut event = source_event(AutomationExecutionMode::NoAgentCheck);
    event.timeout_policy_ref = "unknown-policy".to_owned();
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: "unknown-timeout-policy".to_owned(),
        envelope: AutomationWorkEnvelope {
            event,
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            enqueue_provenance_snapshot: Some(current.clone()),
            expected_current_facts_digest: current.semantic_compatibility_digest()?,
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("must not run".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Suppress,
        },
        next_wake_at_ms: None,
    })?;
    let state = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payloads, 100);
    let mut resolver = FreshGateResolver {
        snapshot: current,
        calls: 0,
    };
    let mut executor = SuccessfulNoAgentExecutor {
        requests: Vec::new(),
    };

    // When
    let error = dispatcher
        .dispatch_due_automation(&state.work, &admission, 100, &mut resolver, &mut executor)
        .expect_err("unknown timeout policy must fail closed");

    // Then
    assert!(error
        .to_string()
        .contains("unknown automation timeout policy"));
    assert!(executor.requests.is_empty());
    Ok(())
}

#[test]
fn timeout_and_cancellation_persist_typed_terminal_facts_without_process_receipts(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let checkpoints = root.path().join("checkpoints");
    let payloads = root.path().join("payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    for (work_id, observed_at) in [("timed-out-work", 1), ("cancelled-work", 2)] {
        let mut event = source_event(AutomationExecutionMode::NoAgentCheck);
        event.runtime_service_event_id = format!("event-{work_id}");
        event.job_id = format!("job-{work_id}");
        event.received_at_ms = observed_at;
        dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
            work_id: work_id.to_owned(),
            envelope: AutomationWorkEnvelope {
                event,
                schedule: AutomationScheduleKind::OneShot,
                existing_runs: Vec::new(),
                enqueue_provenance_snapshot: Some(current.clone()),
                expected_current_facts_digest: current.semantic_compatibility_digest()?,
                hook_evidence: None,
                requirements: requirements(),
                instruction: Some("controlled terminal".to_owned()),
                outcome_policy: AutomationOutcomePolicy::Suppress,
            },
            next_wake_at_ms: None,
        })?;
    }
    let state = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payloads, 100);
    let mut resolver = FreshGateResolver {
        snapshot: current,
        calls: 0,
    };
    let mut executor = TerminalControlExecutor { calls: 0 };

    // When
    dispatcher.dispatch_due_automation(
        &state.work,
        &admission,
        100,
        &mut resolver,
        &mut executor,
    )?;

    // Then
    let terminal = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing terminal state")?;
    let timed_out = terminal.work.items["timed-out-work"]
        .terminal_facts
        .as_ref()
        .ok_or("missing timeout facts")?;
    let cancelled = terminal.work.items["cancelled-work"]
        .terminal_facts
        .as_ref()
        .ok_or("missing cancellation facts")?;
    assert_eq!(timed_out["job_result"]["kind"], "timed_out");
    assert_eq!(timed_out["terminal_fact"]["kind"], "timed_out");
    assert!(timed_out["process_receipt"].is_null());
    assert_eq!(cancelled["job_result"]["kind"], "cancelled");
    assert_eq!(cancelled["terminal_fact"]["kind"], "cancelled");
    assert!(cancelled["process_receipt"].is_null());
    Ok(())
}

#[test]
fn execution_sensitivity_not_no_agent_mode_controls_hook_gate() {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::NoAgentCheck);
    let sensitive = shacs_core::runtime::own_automation_lifecycle(
        shacs_core::runtime::AutomationLifecycleInput {
            requirements: shacs_core::runtime::AutomationExecutionRequirements {
                execution_sensitive: true,
                ..requirements()
            },
            ..fixtures::lifecycle_input(&event, &snapshot)
        },
    );
    assert_eq!(
        sensitive.no_dispatch_reason,
        Some(shacs_core::runtime::AutomationNoDispatchReason::MissingHookEvidence)
    );

    let pure = shacs_core::runtime::own_automation_lifecycle(
        shacs_core::runtime::AutomationLifecycleInput {
            requirements: shacs_core::runtime::AutomationExecutionRequirements {
                execution_sensitive: false,
                ..requirements()
            },
            ..fixtures::lifecycle_input(&event, &snapshot)
        },
    );
    assert!(pure.dispatch_request.is_some());
}

#[test]
fn relevant_current_fact_mutation_blocks_dispatch() -> Result<(), Box<dyn Error>> {
    let queued = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let mut current = refreshed_snapshot(queued.clone());
    current.config.schema_version += 1;
    current = refreshed_snapshot(current);
    let event = source_event(AutomationExecutionMode::NoAgentCheck);
    let digest = queued.semantic_compatibility_digest()?;
    let outcome = shacs_core::runtime::own_automation_lifecycle(
        shacs_core::runtime::AutomationLifecycleInput {
            execution_snapshot: Some(&current),
            expected_snapshot_digest: &digest,
            hook_evidence: Some(&[PluginHookDispatchRecord::successful_noop()]),
            ..fixtures::lifecycle_input(&event, &queued)
        },
    );
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(shacs_core::runtime::AutomationNoDispatchReason::SnapshotMismatch)
    );
    Ok(())
}

#[test]
fn stale_automation_lease_is_not_automatically_requeued() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let events = root.path().join("events");
    let checkpoints = root.path().join("checkpoints");
    let payloads = root.path().join("payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&events, &payloads, MessageBus::new(), "owner", 100)?;
    let current = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    dispatcher.enqueue_automation(AutomationWorkEnqueueInput {
        work_id: "effectful-stale".to_owned(),
        envelope: AutomationWorkEnvelope {
            event: source_event(AutomationExecutionMode::SkillBackedAgent),
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: Vec::new(),
            enqueue_provenance_snapshot: Some(current.clone()),
            expected_current_facts_digest: current.semantic_compatibility_digest()?,
            hook_evidence: None,
            requirements: requirements(),
            instruction: Some("effectful".to_owned()),
            outcome_policy: AutomationOutcomePolicy::Notify,
        },
        next_wake_at_ms: None,
    })?;
    let queued = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing queued state")?;
    dispatcher.lease_work(&queued.work.items["effectful-stale"], 100)?;
    let leased = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing leased state")?;
    let stale = evaluate_durable_work_recovery(&leased.work, &payloads, 201);

    // When
    let recovery = dispatcher.requeue_stale(&leased.work, &stale)?;

    // Then
    assert!(recovery.requeued_work_ids.is_empty());
    let recovered = evaluate_durable_recovery(&events, &checkpoints)
        .state
        .ok_or("missing recovered state")?;
    assert_eq!(
        recovered.work.items["effectful-stale"].state,
        ReplayWorkState::Leased
    );
    Ok(())
}
