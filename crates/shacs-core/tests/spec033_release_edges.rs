#[path = "spec033_automation_results.rs"]
mod fixtures;
mod spec033_snapshot_replay_support;

use fixtures::{lifecycle_input, requirements, snapshot, source_event};
use shacs_core::runtime::{
    own_automation_lifecycle, replay_recorded_trajectory, AutomationExecutionRequirements,
    AutomationLifecycleInput, AutomationNoDispatchReason, ExecutionSnapshot,
    ExecutionSnapshotInput, RecordedTrajectoryReplayError, RecordedTrajectoryStore, SandboxMode,
};
use shacs_eval::evaluator::AutomationExecutionMode;
use shacs_projection::{CredentialStatus, HookDenialReason, SandboxFallback};
use spec033_snapshot_replay_support::{recorded_trajectory, write_trajectory};
use std::error::Error;

#[test]
fn trusted_hook_veto_blocks_dispatch() {
    // Given
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::ScriptOnly);

    // When
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        hook_denial: Some(HookDenialReason::ExtensionBlocked),
        ..lifecycle_input(&event, &snapshot)
    });

    // Then
    assert!(outcome.dispatch_request.is_none());
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::HookVeto)
    );
}

#[test]
fn sandbox_failure_blocks_dispatch() {
    // Given
    let snapshot = snapshot(SandboxMode::Failed, SandboxFallback::ExecutionDenied);
    let event = source_event(AutomationExecutionMode::ScriptOnly);

    // When
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        requirements: AutomationExecutionRequirements {
            sandbox_required: true,
            ..requirements()
        },
        hook_evidence: Some(&[shacs_core::runtime::PluginHookDispatchRecord::successful_noop()]),
        ..lifecycle_input(&event, &snapshot)
    });

    // Then
    assert!(outcome.dispatch_request.is_none());
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::SandboxFailed)
    );
}

#[test]
fn credential_failure_blocks_dispatch() {
    // Given
    let snapshot = snapshot_with_credential(CredentialStatus::Unavailable);
    let event = source_event(AutomationExecutionMode::ScriptOnly);

    // When
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        requirements: AutomationExecutionRequirements {
            credential_required: true,
            ..requirements()
        },
        hook_evidence: Some(&[shacs_core::runtime::PluginHookDispatchRecord::successful_noop()]),
        ..lifecycle_input(&event, &snapshot)
    });

    // Then
    assert!(outcome.dispatch_request.is_none());
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::CredentialUnavailable)
    );
}

#[test]
fn recorded_source_mutation_blocks_replay() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = RecordedTrajectoryStore::open(root.path())?;
    let mut trajectory = recorded_trajectory();
    trajectory.sources[0].bytes = b"different recorded source".to_vec();
    write_trajectory(&store, trajectory)?;

    // When
    let result = replay_recorded_trajectory(&store, "trajectory-004", "source-mutation");

    // Then
    assert_eq!(result, Err(RecordedTrajectoryReplayError::SourceMutation));
    Ok(())
}

#[test]
fn duplicate_automation_blocks_dispatch() {
    // Given
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::NoAgentCheck);
    let first = own_automation_lifecycle(lifecycle_input(&event, &snapshot));
    let existing = vec![first.lifecycle.run.expect("queued run")];

    // When
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        existing_runs: &existing,
        ..lifecycle_input(&event, &snapshot)
    });

    // Then
    assert!(outcome.dispatch_request.is_none());
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::Duplicate)
    );
}

#[test]
fn recursive_automation_blocks_dispatch() {
    // Given
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let mut event = source_event(AutomationExecutionMode::NoAgentCheck);
    event.recursion_guard.depth = event.recursion_guard.max_depth;

    // When
    let outcome = own_automation_lifecycle(lifecycle_input(&event, &snapshot));

    // Then
    assert!(outcome.dispatch_request.is_none());
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::RecursionGuard)
    );
}

fn snapshot_with_credential(status: CredentialStatus) -> ExecutionSnapshot {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    ExecutionSnapshot::create(ExecutionSnapshotInput {
        credential: shacs_core::runtime::CredentialSnapshotRef {
            status,
            ..snapshot.credential
        },
        snapshot_id: snapshot.snapshot_id,
        created_at_unix_ms: snapshot.created_at_unix_ms,
        config: snapshot.config,
        profiles: snapshot.profiles,
        trusted_runtime: snapshot.trusted_runtime,
        sandbox: snapshot.sandbox,
        context_sources: snapshot.context_sources,
        selected_tools: snapshot.selected_tools,
        selected_resources: snapshot.selected_resources,
        provider: snapshot.provider,
        token_budget: snapshot.token_budget,
        disclosure: snapshot.disclosure,
        replay: snapshot.replay,
    })
    .expect("valid credential snapshot")
}
