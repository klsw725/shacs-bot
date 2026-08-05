use shacs_projection::*;
use std::error::Error;

fn safe_lineage(action: &str) -> Result<Spec031LifecycleLineage, Box<dyn Error>> {
    Ok(Spec031LifecycleLineage {
        subject_ref: Spec031SubjectRef::try_new("subject:lifecycle:session")?,
        parent_ref: Some(Spec031ParentRef::try_new("parent:lifecycle:turn")?),
        action_ref: Some(Spec031ActionRef::try_new(action)?),
        digest: Some(Spec031Digest::try_new("sha256:lifecycle0001")?),
    })
}

#[test]
fn spec031_lifecycle_projects_approval_states_with_same_safe_lineage() -> Result<(), Box<dyn Error>>
{
    for state in [
        Spec031ApprovalState::Denied,
        Spec031ApprovalState::Expired,
        Spec031ApprovalState::Skipped,
        Spec031ApprovalState::RetryConsumed,
    ] {
        let envelopes = spec031_project_lifecycle(Spec031LifecycleInput {
            lineage: safe_lineage("action:approval:stable")?,
            facts: vec![Spec031LifecycleFact::Approval { state }],
            observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
        })?;

        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].kind(), Spec031ProjectionKind::Approval);
        assert_eq!(
            envelopes[0]
                .lineage()
                .action_ref
                .as_ref()
                .map(Spec031ActionRef::as_str),
            Some("action:approval:stable")
        );
        assert!(matches!(
            envelopes[0].capability(),
            Spec031Capability::Approval(capability) if capability.state == state
        ));
    }
    Ok(())
}

#[test]
fn spec031_lifecycle_retry_consumed_is_typed_non_success_reason() -> Result<(), Box<dyn Error>> {
    let envelopes = spec031_project_lifecycle(Spec031LifecycleInput {
        lineage: safe_lineage("action:approval:retry-consumed")?,
        facts: vec![Spec031LifecycleFact::Approval {
            state: Spec031ApprovalState::RetryConsumed,
        }],
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(37)),
    })?;

    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].kind(), Spec031ProjectionKind::Approval);
    assert_eq!(envelopes[0].state(), Spec031Availability::Blocked);
    assert_eq!(envelopes[0].reason().code, Spec031ReasonCode::RetryConsumed);
    assert_ne!(envelopes[0].reason().code, Spec031ReasonCode::Completed);
    assert!(matches!(
        envelopes[0].capability(),
        Spec031Capability::Approval(capability)
            if capability.state == Spec031ApprovalState::RetryConsumed
    ));
    Ok(())
}

#[test]
fn spec031_lifecycle_keeps_runtime_control_requested_distinct_from_completed(
) -> Result<(), Box<dyn Error>> {
    let envelopes = spec031_project_lifecycle(Spec031LifecycleInput {
        lineage: safe_lineage("action:runtime:restart")?,
        facts: vec![
            Spec031LifecycleFact::RuntimeControl {
                kind: Spec031RuntimeControlKind::Restart,
                state: Spec031RuntimeControlState::Requested,
            },
            Spec031LifecycleFact::RuntimeControl {
                kind: Spec031RuntimeControlKind::Restart,
                state: Spec031RuntimeControlState::Completed,
            },
        ],
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(32)),
    })?;

    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[0].reason().code, Spec031ReasonCode::Requested);
    assert_eq!(envelopes[1].reason().code, Spec031ReasonCode::Completed);
    assert_ne!(envelopes[0].reason().code, envelopes[1].reason().code);
    Ok(())
}

#[test]
fn spec031_lifecycle_progress_is_non_terminal_until_owner_final() -> Result<(), Box<dyn Error>> {
    let envelopes = spec031_project_lifecycle(Spec031LifecycleInput {
        lineage: safe_lineage("action:progress:turn")?,
        facts: vec![
            Spec031LifecycleFact::Progress {
                delivery: Spec031ProgressDelivery::Live,
            },
            Spec031LifecycleFact::Terminal {
                outcome: Spec031TerminalOutcome::Failed,
            },
        ],
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(33)),
    })?;

    assert_eq!(envelopes.len(), 2);
    assert_eq!(envelopes[0].kind(), Spec031ProjectionKind::Progress);
    assert_eq!(envelopes[0].reason().code, Spec031ReasonCode::Progress);
    assert_eq!(envelopes[0].state(), Spec031Availability::Blocked);
    assert_eq!(envelopes[1].reason().code, Spec031ReasonCode::Final);
    assert_eq!(envelopes[1].state(), Spec031Availability::Blocked);
    Ok(())
}

#[test]
fn spec031_lifecycle_rejects_stale_lineage_and_duplicate_terminal() -> Result<(), Box<dyn Error>> {
    let completed_without_request = spec031_project_lifecycle(Spec031LifecycleInput {
        lineage: safe_lineage("action:runtime:orphan-complete")?,
        facts: vec![Spec031LifecycleFact::RuntimeControl {
            kind: Spec031RuntimeControlKind::Stop,
            state: Spec031RuntimeControlState::Completed,
        }],
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(34)),
    });
    assert_eq!(
        completed_without_request,
        Err(Spec031LifecycleError::StaleLineage)
    );

    let duplicate_terminal = spec031_project_lifecycle(Spec031LifecycleInput {
        lineage: safe_lineage("action:terminal:duplicate")?,
        facts: vec![
            Spec031LifecycleFact::Terminal {
                outcome: Spec031TerminalOutcome::Cancelled,
            },
            Spec031LifecycleFact::Terminal {
                outcome: Spec031TerminalOutcome::Failed,
            },
        ],
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(35)),
    });
    assert_eq!(
        duplicate_terminal,
        Err(Spec031LifecycleError::DuplicateTerminal)
    );
    Ok(())
}

#[test]
fn spec031_lifecycle_repeated_recovery_and_pending_follow_up_are_non_success(
) -> Result<(), Box<dyn Error>> {
    let envelopes = spec031_project_lifecycle(Spec031LifecycleInput {
        lineage: safe_lineage("action:recovery:turn")?,
        facts: vec![
            Spec031LifecycleFact::Recovery {
                state: Spec031RecoveryState::Interrupted,
            },
            Spec031LifecycleFact::Recovery {
                state: Spec031RecoveryState::Interrupted,
            },
            Spec031LifecycleFact::PendingFollowUp,
        ],
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(36)),
    })?;

    assert_eq!(envelopes.len(), 3);
    assert_eq!(
        envelopes[1].reason().code,
        Spec031ReasonCode::RepeatedInterruption
    );
    assert_eq!(envelopes[1].state(), Spec031Availability::Blocked);
    assert_eq!(
        envelopes[2].reason().code,
        Spec031ReasonCode::PendingFollowUp
    );
    assert_ne!(envelopes[2].state(), Spec031Availability::Ready);
    Ok(())
}
