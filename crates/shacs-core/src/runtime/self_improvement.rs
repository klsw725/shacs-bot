use shacs_utils::evaluator::{
    default_mcp_exposure_projection, failed_improvement_verification_next_action,
    improvement_proposal_can_affect_runtime, stable_sha256_digest,
    validate_improvement_apply_readiness, ApprovalDecisionKind, ApprovalRequestStatus,
    CheckpointGateDecision, DeniedOutcome, EvidenceRef, ImprovementApplyRecord,
    ImprovementApproval, ImprovementCheckpoint, ImprovementProposal, ImprovementProposalStatus,
    ImprovementRollbackFinalState, ImprovementRollbackRecord, ImprovementRollbackResult,
    ImprovementVerification, ImprovementVerificationNextAction, McpExposureProjection,
    OwnerPrimitiveRef,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SelfImprovementApplyReadiness {
    pub status: ImprovementProposalStatus,
    pub ready: bool,
    pub reason: String,
    pub denied_outcome: Option<Box<DeniedOutcome>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfImprovementRollbackProjection {
    pub status: ImprovementProposalStatus,
    pub rollback_record: Option<ImprovementRollbackRecord>,
    pub manual_recovery_hint: Option<String>,
}

pub fn runtime_improvement_proposal_behavior_inert(proposal: &ImprovementProposal) -> bool {
    !improvement_proposal_can_affect_runtime(proposal)
}

pub fn runtime_improvement_approved_scope_matches(
    proposal: &ImprovementProposal,
    approval: &ImprovementApproval,
) -> bool {
    if approval.proposal_id != proposal.proposal_id
        || approval.request_ref.status != ApprovalRequestStatus::Approved
        || approval.decision_ref.decision != ApprovalDecisionKind::Approved
    {
        return false;
    }

    approval.approved_scope.iter().any(|scope| {
        proposal
            .target_ref
            .as_ref()
            .map_or(scope == &proposal.target_kind, |target_ref| {
                scope == target_ref
            })
    })
}

pub fn runtime_improvement_apply_readiness(
    proposal: &ImprovementProposal,
    approval: Option<&ImprovementApproval>,
    checkpoint: Option<&ImprovementCheckpoint>,
    checkpoint_gate: Option<&CheckpointGateDecision>,
    now_ms: u64,
    evidence_ref: Option<EvidenceRef>,
) -> SelfImprovementApplyReadiness {
    let Some(approval) = approval else {
        return blocked_apply_readiness(
            ImprovementProposalStatus::BlockedApprovalRequired,
            "matching local-user approval is required before apply",
            None,
        );
    };

    if !runtime_improvement_approved_scope_matches(proposal, approval) {
        return blocked_apply_readiness(
            ImprovementProposalStatus::BlockedApprovalRequired,
            "approval must match the proposal and explicitly cover the target scope",
            None,
        );
    }

    if approval.expires_at_ms <= now_ms || approval.request_ref.expires_at_ms <= now_ms {
        return blocked_apply_readiness(
            ImprovementProposalStatus::BlockedApprovalRequired,
            "approval expired before apply readiness check",
            None,
        );
    }

    let (Some(checkpoint), Some(checkpoint_gate)) = (checkpoint, checkpoint_gate) else {
        return blocked_apply_readiness(
            ImprovementProposalStatus::BlockedCheckpointUnavailable,
            "owner checkpoint is unavailable; apply must not be attempted",
            None,
        );
    };

    if checkpoint.rollback_capability.is_none() {
        return blocked_apply_readiness(
            ImprovementProposalStatus::BlockedCheckpointUnavailable,
            "owner checkpoint does not provide rollback capability",
            None,
        );
    }

    match validate_improvement_apply_readiness(
        proposal,
        approval,
        checkpoint,
        checkpoint_gate,
        now_ms,
        evidence_ref,
    ) {
        Ok(()) => SelfImprovementApplyReadiness {
            status: ImprovementProposalStatus::Checkpointed,
            ready: true,
            reason: "approved checkpointed proposal is ready for owner primitive apply".to_owned(),
            denied_outcome: None,
        },
        Err(denied_outcome) => blocked_apply_readiness(
            ImprovementProposalStatus::BlockedCheckpointUnavailable,
            denied_outcome.message.clone(),
            Some(denied_outcome),
        ),
    }
}

pub fn runtime_improvement_apply_record(
    apply_id: impl Into<String>,
    proposal: &ImprovementProposal,
    owner_apply_ref: OwnerPrimitiveRef,
    input: &serde_json::Value,
    outcome_ref: EvidenceRef,
) -> Result<ImprovementApplyRecord, serde_json::Error> {
    Ok(ImprovementApplyRecord {
        apply_id: apply_id.into(),
        proposal_id: proposal.proposal_id.clone(),
        owner_spec: owner_apply_ref.owner_spec.clone(),
        action_ref: owner_apply_ref,
        input_digest: stable_sha256_digest(input)?,
        outcome_ref,
        correlation_id: proposal.correlation_id.clone(),
    })
}

pub fn runtime_improvement_status_after_apply_record() -> ImprovementProposalStatus {
    ImprovementProposalStatus::AppliedUnverified
}

pub fn runtime_improvement_verification_record(
    verification_id: impl Into<String>,
    proposal: &ImprovementProposal,
    expected_behavior: impl Into<String>,
    observed_result_ref: EvidenceRef,
    passed: bool,
    checkpoint: Option<&ImprovementCheckpoint>,
    owner_rollback_primitive_ready: bool,
) -> ImprovementVerification {
    let mut verification = ImprovementVerification {
        verification_id: verification_id.into(),
        expected_behavior: expected_behavior.into(),
        observed_result_ref,
        passed,
        next_action: ImprovementVerificationNextAction::ReportFailed,
        proposal_id: proposal.proposal_id.clone(),
        correlation_id: proposal.correlation_id.clone(),
    };
    verification.next_action = failed_improvement_verification_next_action(
        &verification,
        checkpoint,
        owner_rollback_primitive_ready,
    );
    verification
}

pub fn runtime_improvement_rollback_projection(
    rollback_id: impl Into<String>,
    proposal: &ImprovementProposal,
    checkpoint: Option<&ImprovementCheckpoint>,
    verification: &ImprovementVerification,
    owner_rollback_ref: Option<OwnerPrimitiveRef>,
    manual_recovery_hint: impl Into<String>,
) -> SelfImprovementRollbackProjection {
    if verification.passed {
        return SelfImprovementRollbackProjection {
            status: ImprovementProposalStatus::Verified,
            rollback_record: None,
            manual_recovery_hint: None,
        };
    }

    let manual_recovery_hint = manual_recovery_hint.into();
    let Some(checkpoint) = checkpoint else {
        return SelfImprovementRollbackProjection {
            status: ImprovementProposalStatus::BlockedCheckpointUnavailable,
            rollback_record: None,
            manual_recovery_hint: Some(manual_recovery_hint),
        };
    };

    let has_owner_rollback = owner_rollback_ref.is_some();
    let rollback_record = ImprovementRollbackRecord {
        rollback_id: rollback_id.into(),
        proposal_id: proposal.proposal_id.clone(),
        checkpoint_ref: checkpoint.checkpoint_ref.clone(),
        verify_failure_ref: verification.observed_result_ref.clone(),
        owner_rollback_ref,
        result: if has_owner_rollback {
            ImprovementRollbackResult::RolledBack
        } else {
            ImprovementRollbackResult::BlockedManualRecoveryRequired
        },
        manual_recovery_hint: if has_owner_rollback {
            None
        } else {
            Some(manual_recovery_hint.clone())
        },
        final_state: if has_owner_rollback {
            ImprovementRollbackFinalState::RestoredCheckpoint
        } else {
            ImprovementRollbackFinalState::ManualRecoveryRequired
        },
        correlation_id: proposal.correlation_id.clone(),
    };

    SelfImprovementRollbackProjection {
        status: if has_owner_rollback {
            ImprovementProposalStatus::RolledBack
        } else {
            ImprovementProposalStatus::BlockedRollbackUnavailable
        },
        rollback_record: Some(rollback_record),
        manual_recovery_hint: if has_owner_rollback {
            None
        } else {
            Some(manual_recovery_hint)
        },
    }
}

pub fn runtime_mcp_exposure_projection(
    tool_or_resource_id: impl Into<String>,
    requested_exposure: impl Into<String>,
    current_exposure: impl Into<String>,
    proposal: Option<&ImprovementProposal>,
    approval: Option<&ImprovementApproval>,
    now_ms: u64,
) -> McpExposureProjection {
    let tool_or_resource_id = tool_or_resource_id.into();
    let mut projection = default_mcp_exposure_projection(
        tool_or_resource_id.clone(),
        requested_exposure,
        current_exposure,
        "mcp exposure is default deny until explicit local approval covers this scope",
    );

    if let (Some(proposal), Some(approval)) = (proposal, approval) {
        projection.proposal_id = Some(proposal.proposal_id.clone());
        projection.correlation_id = Some(proposal.correlation_id.clone());
        if runtime_improvement_approved_scope_matches(proposal, approval)
            && approval.expires_at_ms > now_ms
            && approval.request_ref.expires_at_ms > now_ms
            && approval.approved_scope.iter().any(|scope| {
                scope == &tool_or_resource_id
                    || proposal.target_ref.as_ref().is_some_and(|target_ref| {
                        scope == target_ref && target_ref == &tool_or_resource_id
                    })
            })
        {
            projection.approval_ref = Some(approval.decision_ref.clone());
        }
    }

    projection
}

fn blocked_apply_readiness(
    status: ImprovementProposalStatus,
    reason: impl Into<String>,
    denied_outcome: Option<Box<DeniedOutcome>>,
) -> SelfImprovementApplyReadiness {
    SelfImprovementApplyReadiness {
        status,
        ready: false,
        reason: reason.into(),
        denied_outcome,
    }
}
