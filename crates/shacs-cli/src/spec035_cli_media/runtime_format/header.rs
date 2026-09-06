use super::super::super::*;

pub(super) struct RuntimeInspectFormatState {
    pub(super) diagnostics_blocked: bool,
    pub(super) diagnostics_component_count: usize,
    pub(super) readiness_available: bool,
    pub(super) subagent_child_count: usize,
    pub(super) tool_attempt_count: usize,
    pub(super) app_total_count: usize,
    pub(super) media_artifact_count: usize,
    pub(super) readiness_lines: Vec<String>,
    pub(super) lines: Vec<String>,
}

pub(super) fn runtime_inspect_header(report: &RuntimeInspectReport) -> RuntimeInspectFormatState {
    let diagnostics_blocked = report.lifecycle.durable_diagnostics.missing
        || report.lifecycle.durable_diagnostics.corrupt_tail
        || !report.lifecycle.durable_recovery.writable
        || !report.lifecycle.durable_work.writable
        || report.lifecycle.durable_children.recovery_needed_count > 0;
    let diagnostics_component_count =
        report.supervision.components.len() + report.capabilities.len();
    let readiness_available = matches!(
        report.lifecycle.compatibility,
        RuntimeCompatibility::FullyCompatible
    ) && !report.lifecycle.migration_plan.blocked
        && !matches!(
            report.lifecycle.ownership.state,
            RuntimeOwnershipState::Stale
        );
    let readiness_lines = spec031_cli::readiness::lines(report).unwrap_or_else(|error| {
        vec![format!(
            "Spec031 readiness: kind=readiness state=unavailable severity=error reason=missing lineage=subject:cli:readiness detail={}",
            redact_string(&error)
        )]
    });
    let lines = vec![
        "shacs-bot runtime inspect".to_owned(),
        format!(
            "Config: {} ({})",
            display_path(&report.config_path),
            exists_label(report.config_exists)
        ),
        format!(
            "Workspace: {} ({})",
            display_path(&report.workspace),
            exists_label(report.workspace_exists)
        ),
        format!("Data dir: {}", display_path(&report.data_dir)),
        format!("Provider: {}", report.provider),
        format!("Model: {}", report.model),
        format!("Binary version: {}", report.lifecycle.binary_version),
        format!(
            "Data schema: {} (min {})",
            report.lifecycle.data_schema_version, report.lifecycle.data_schema_min_version
        ),
        format!("Compatibility: {}", report.lifecycle.compatibility.as_str()),
        format!(
            "Stored-data migration: blocked={} transforms={} families={} ledger_phase={} manual_recovery={}",
            report.lifecycle.migration_plan.blocked,
            report.lifecycle.migration_plan.entries.iter().filter(|entry| entry.action == DurableMigrationAction::Transform).count(),
            report.lifecycle.migration_plan.entries.len(),
            report.lifecycle.migration_ledger.phase.as_deref().unwrap_or("none"),
            report.lifecycle.migration_ledger.manual_recovery_required,
        ),
        format!(
            "Durable recovery: {} (writable={}, checkpoint={}, replayed_events={})",
            report.lifecycle.durable_recovery.status.as_str(),
            report.lifecycle.durable_recovery.writable,
            report.lifecycle.durable_recovery.checkpoint_used.as_deref().unwrap_or("none"),
            report.lifecycle.durable_recovery.replayed_event_count
        ),
        format!(
            "Durable work: {} (writable={}, pending={}, leased={}, retry_waiting={}, cancel_requested={}, terminal={}, evicted={})",
            report.lifecycle.durable_work.status.as_str(),
            report.lifecycle.durable_work.writable,
            report.lifecycle.durable_work.pending_count,
            report.lifecycle.durable_work.leased_count,
            report.lifecycle.durable_work.waiting_retry_count,
            report.lifecycle.durable_work.cancellation_requested_count,
            report.lifecycle.durable_work.terminal_count,
            report.lifecycle.durable_work.terminal_evicted_count,
        ),
        format!(
            "Durable children: spawned={} recovery_needed={} cancel_requested={} terminal={} stale={} duplicate={} late={} evicted={}/{}",
            report.lifecycle.durable_children.spawned_count,
            report.lifecycle.durable_children.recovery_needed_count,
            report.lifecycle.durable_children.cancellation_requested_count,
            report.lifecycle.durable_children.terminal_count,
            report.lifecycle.durable_children.stale_decision_count,
            report.lifecycle.durable_children.duplicate_decision_count,
            report.lifecycle.durable_children.late_decision_count,
            report.lifecycle.durable_children.terminal_evicted_count,
            report.lifecycle.durable_children.decision_evicted_count,
        ),
        format!(
            "Durable diagnostics evidence: schema={}.v{} missing={} corrupt_tail={} evidence={} active_recovery={} terminal={} latest_event_sequence={} refs={}",
            report.lifecycle.durable_diagnostics.schema_family,
            report.lifecycle.durable_diagnostics.schema_version,
            report.lifecycle.durable_diagnostics.missing,
            report.lifecycle.durable_diagnostics.corrupt_tail,
            report.lifecycle.durable_diagnostics.evidence_count,
            report.lifecycle.durable_diagnostics.active_recovery_count,
            report.lifecycle.durable_diagnostics.terminal_count,
            report.lifecycle.durable_diagnostics.latest_event_sequence.map(|value| value.to_string()).unwrap_or_else(|| "none".to_owned()),
            report.lifecycle.durable_diagnostics.recent_trace_refs.len(),
        ),
        format!(
            "Ownership: {} ({})",
            report.lifecycle.ownership.state.as_str(),
            report.lifecycle.ownership.reason
        ),
        format!("Sessions: {}", report.sessions.count),
        format!("Workflow recipes: {}", report.workflow_recipes.len()),
    ];
    RuntimeInspectFormatState {
        diagnostics_blocked,
        diagnostics_component_count,
        readiness_available,
        subagent_child_count: report.lifecycle.durable_children.spawned_count,
        tool_attempt_count: report.lifecycle.durable_work.pending_count
            + report.lifecycle.durable_work.leased_count
            + report.lifecycle.durable_work.terminal_count,
        app_total_count: usize::from(report.lifecycle.ownership.marker.is_some()),
        media_artifact_count: report.generated_media.len(),
        readiness_lines,
        lines,
    }
}
