#[path = "spec033_automation_results.rs"]
mod fixtures;
use fixtures::{lifecycle_input, requirements, snapshot, source_event};
use shacs_core::runtime::AutomationDeliveryResult;
use shacs_core::runtime::SandboxMode;
use shacs_core::runtime::{
    own_automation_lifecycle, AutomationExecutionRequirements, AutomationJobResult,
    AutomationLifecycleInput, AutomationNoDispatchReason, AutomationScheduleKind,
};
use shacs_core::runtime::{ExecutionSnapshot, ExecutionSnapshotInput};
use shacs_eval::evaluator::AutomationExecutionMode;
use shacs_projection::CredentialStatus;
use shacs_projection::SandboxFallback;

#[test]
fn duplicate_superseded_and_recursion_fail_closed() {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::NoAgentCheck);
    let first = own_automation_lifecycle(lifecycle_input(&event, &snapshot));
    let existing = vec![first.lifecycle.run.expect("queued run record")];
    let duplicate = own_automation_lifecycle(AutomationLifecycleInput {
        existing_runs: &existing,
        ..lifecycle_input(&event, &snapshot)
    });
    assert_eq!(
        duplicate.no_dispatch_reason,
        Some(AutomationNoDispatchReason::Duplicate)
    );

    let mut recursive_event = event;
    recursive_event.recursion_guard.depth = 3;
    let recursive = own_automation_lifecycle(lifecycle_input(&recursive_event, &snapshot));
    assert_eq!(
        recursive.no_dispatch_reason,
        Some(AutomationNoDispatchReason::RecursionGuard)
    );
}

#[test]
fn snapshot_sandbox_and_credential_failures_are_explicit() {
    let active = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let unsupported = snapshot(
        SandboxMode::Unsupported,
        SandboxFallback::TrustedNativeFallback,
    );
    let failed = snapshot(SandboxMode::Failed, SandboxFallback::ExecutionDenied);
    let event = source_event(AutomationExecutionMode::ScriptOnly);
    let hook_evidence = vec![shacs_core::runtime::PluginHookDispatchRecord {
        plugin_id: "hook-owner-1".to_owned(),
        event: shacs_core::runtime::PluginHookEvent::ToolBefore,
        status: shacs_core::runtime::PluginHookDispatchStatus::Succeeded,
        effect: Some(shacs_core::runtime::PluginHookDispatchEffect::Observed),
        output_evidence: None,
        error: None,
        timeout: None,
    }];
    let cases = [
        (
            None,
            "sha256:missing",
            requirements(),
            AutomationNoDispatchReason::SnapshotMissing,
        ),
        (
            Some(&active),
            "sha256:stale",
            requirements(),
            AutomationNoDispatchReason::SnapshotMismatch,
        ),
        (
            Some(&unsupported),
            &unsupported
                .semantic_compatibility_digest()
                .expect("semantic digest"),
            AutomationExecutionRequirements {
                sandbox_required: true,
                ..requirements()
            },
            AutomationNoDispatchReason::SandboxUnsupported,
        ),
        (
            Some(&failed),
            &failed
                .semantic_compatibility_digest()
                .expect("semantic digest"),
            AutomationExecutionRequirements {
                sandbox_required: true,
                ..requirements()
            },
            AutomationNoDispatchReason::SandboxFailed,
        ),
    ];
    for (execution_snapshot, expected_digest, requirements, expected) in cases {
        let outcome = own_automation_lifecycle(AutomationLifecycleInput {
            event: &event,
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: &[],
            durable_work: None,
            execution_snapshot,
            expected_snapshot_digest: expected_digest,
            hook_evidence: Some(&hook_evidence),
            hook_denial: None,
            requirements,
            job_result: AutomationJobResult::Pending,
            delivery_result: AutomationDeliveryResult::NotRequested,
        });
        assert!(outcome.dispatch_request.is_none());
        assert_eq!(outcome.no_dispatch_reason, Some(expected));
    }

    let unavailable_credential = snapshot_with_credential(CredentialStatus::Unavailable);
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        hook_evidence: Some(&hook_evidence),
        requirements: AutomationExecutionRequirements {
            credential_required: true,
            ..requirements()
        },
        ..lifecycle_input(&event, &unavailable_credential)
    });
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::CredentialUnavailable)
    );
}

#[test]
fn missing_execution_snapshot_blocks_dispatch() {
    // Given
    let event = source_event(AutomationExecutionMode::ScriptOnly);
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);

    // When
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        execution_snapshot: None,
        expected_snapshot_digest: &snapshot.semantic_compatibility_digest().expect("digest"),
        ..lifecycle_input(&event, &snapshot)
    });

    // Then
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::SnapshotMissing)
    );
}

#[test]
fn unsupported_required_sandbox_blocks_dispatch() {
    // Given
    let event = source_event(AutomationExecutionMode::ScriptOnly);
    let snapshot = snapshot(
        SandboxMode::Unsupported,
        SandboxFallback::TrustedNativeFallback,
    );
    let hook_evidence = [shacs_core::runtime::PluginHookDispatchRecord::successful_noop()];

    // When
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        hook_evidence: Some(&hook_evidence),
        requirements: AutomationExecutionRequirements {
            sandbox_required: true,
            ..requirements()
        },
        ..lifecycle_input(&event, &snapshot)
    });

    // Then
    assert_eq!(
        outcome.no_dispatch_reason,
        Some(AutomationNoDispatchReason::SandboxUnsupported)
    );
}

fn snapshot_with_credential(status: CredentialStatus) -> ExecutionSnapshot {
    let mut input = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    input.credential.status = status;
    ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: input.snapshot_id,
        created_at_unix_ms: input.created_at_unix_ms,
        config: input.config,
        profiles: input.profiles,
        trusted_runtime: input.trusted_runtime,
        sandbox: input.sandbox,
        credential: input.credential,
        context_sources: input.context_sources,
        selected_tools: input.selected_tools,
        selected_resources: input.selected_resources,
        provider: input.provider,
        token_budget: input.token_budget,
        disclosure: input.disclosure,
        replay: input.replay,
    })
    .expect("valid credential snapshot")
}
