use shacs_projection::Spec033Snapshot;
use shacs_tui::state::{ApprovalLineage, ApprovalStatus, RuntimeSession, SessionKey};

pub fn fixture_session(key: &str, lineage: &str, progress: u64, outcomes: u64) -> RuntimeSession {
    RuntimeSession {
        key: SessionKey::new(key)
            .unwrap_or_else(|error| panic!("fixture session key failed: {error:?}")),
        updated_at: Some("2026-08-02T00:00:00Z".to_owned()),
        message_count: 2,
        recovery_markers: Vec::new(),
        checkpoint_phase: None,
        diagnostics_ref_count: 0,
        spec033: Spec033Snapshot::unavailable(key),
        workflow: Some(shacs_session::SessionRuntimeWorkflowProjection {
            schema_label: Some("024WorkflowProjection".to_owned()),
            schema_version: Some("024WorkflowProjection.v1".to_owned()),
            workflow_id: Some("wf-1".to_owned()),
            pattern: Some("workflow_sequence".to_owned()),
            state: Some("running".to_owned()),
            progress_count: Some(progress),
            active_child_count: Some(1),
            pending_barrier_count: Some(0),
            verifier_status: Some("pending".to_owned()),
            budget_usage: None,
            worktree_ref_count: 0,
            evidence_ref_count: 0,
            blocked_reason: None,
            next_action: None,
            resume_available: false,
        }),
        execution: Some(shacs_session::SessionRuntimeExecutionProjection {
            pending_count: 1,
            outcome_count: outcomes,
            pending_by_domain: shacs_session::SessionRuntimeExecutionDomainCounts::default(),
            outcomes_by_domain: shacs_session::SessionRuntimeExecutionDomainCounts::default(),
            decisions: shacs_session::SessionRuntimeExecutionDecisionCounts::default(),
            artifact_ref_count: 0,
            safe_artifact_ref_count: 0,
            recent_outcomes: Vec::new(),
        }),
        pending_approval: Some(shacs_tui::state::PendingApproval {
            lineage: ApprovalLineage::new(lineage)
                .unwrap_or_else(|error| panic!("fixture lineage failed: {error:?}")),
            tool_name: "exec".to_owned(),
            status: ApprovalStatus::Pending,
            expires_at_unix_ms: Some(9_999),
            action: shacs_tui::state::ApprovalActionState::Actionable {
                target_owner_id: "owner-fixture".to_owned(),
            },
        }),
        media: shacs_tui::media_view::MediaProjectionView::unavailable(),
        tasks: None,
    }
}
