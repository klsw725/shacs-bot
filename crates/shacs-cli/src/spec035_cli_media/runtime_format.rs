mod header;

use self::header::runtime_inspect_header;
use super::super::*;

pub(crate) fn format_runtime_inspect(report: RuntimeInspectReport) -> String {
    let state = runtime_inspect_header(&report);
    let mut lines = state.lines;
    if let Some(marker) = &report.lifecycle.ownership.marker {
        lines.push(format!(
            "Owner: ref={} mode={} renewed_at_ms={} expires_at_ms={}",
            opaque_ref("owner", &marker.owner_id),
            marker.mode,
            marker.updated_at_ms,
            marker.expires_at_ms
        ));
    }
    lines.push(format!(
        "Supervision: schema=v{} owner={} components={} shutdown=reason={:?} phase={:?}",
        report.supervision.schema_version,
        report
            .supervision
            .owner
            .as_ref()
            .map(|owner| opaque_ref("owner", &owner.owner_id))
            .unwrap_or_else(|| "none".to_owned()),
        report.supervision.components.len(),
        report.supervision.shutdown.reason,
        report.supervision.shutdown.phase
    ));
    for component in &report.supervision.components {
        lines.push(format!(
            "Supervision component {}: {} ({})",
            component.name,
            component.state,
            opaque_ref("supervision-detail", &component.detail)
        ));
    }
    if let Some(request) = &report.lifecycle.stop_request {
        lines.push(format!(
            "Stop request: {} request_id={} requested_at_ms={} owner_ref={} target_owner_ref={} event_sequence={}",
            request.request,
            request.request_id,
            request.requested_at_ms,
            request.owner_pid.map(|pid| opaque_ref("pid", &pid.to_string())).unwrap_or_else(|| "unknown".to_owned()),
            request.target_owner_id.as_deref().map(|owner| opaque_ref("owner", owner)).unwrap_or_else(|| "unknown".to_owned()),
            request.event_sequence.map(|sequence| sequence.to_string()).unwrap_or_else(|| "unknown".to_owned())
        ));
    } else {
        lines.push("Stop request: none".to_owned());
    }
    match report.lifecycle.update_marker {
        Some(marker) => lines.push(format!(
            "Update marker: {} {} -> {} (migration_required={})",
            marker.phase, marker.from_version, marker.target_version, marker.migration_required
        )),
        None => lines.push("Update marker: none".to_owned()),
    }
    for entry in &report.lifecycle.migration_plan.entries {
        if entry.action != DurableMigrationAction::NoOp {
            lines.push(format!(
                "Stored-data migration plan {}: {:?} {} -> {} detail_ref={}",
                entry.family,
                entry.action,
                entry.source_version,
                entry.target_version,
                entry.detail_ref
            ));
        }
    }
    for issue in &report.lifecycle.durable_recovery.issues {
        lines.push(format!(
            "Durable recovery issue: {} detail_ref={}",
            issue.kind.as_str(),
            opaque_ref("recovery-detail", &issue.detail)
        ));
    }
    for hint in &report.lifecycle.durable_recovery.recovery_hints {
        lines.push(format!("Durable recovery hint: {}", hint.as_str()));
    }
    for issue in &report.lifecycle.durable_work.issues {
        lines.push(format!(
            "Durable work issue: {} work_ref={} detail={}",
            issue.kind.as_str(),
            opaque_ref("work", &issue.work_id),
            redact_string(&issue.detail)
        ));
    }
    for child_ref in &report.lifecycle.durable_children.active_child_refs {
        lines.push(format!("Durable child active: {child_ref}"));
    }
    if let Some(latest_key) = report.sessions.latest_key {
        let updated = report
            .sessions
            .latest_updated_at
            .unwrap_or_else(|| "unknown".to_owned());
        lines.push(format!("Latest session: {latest_key} ({updated})"));
    }
    lines.push(format!(
        "Runtime containment: contained={} backend={} snapshot_digest={}",
        optional_bool_label(report.containment.contained),
        report.containment.backend.as_deref().unwrap_or("none"),
        report.containment.digest.as_deref().unwrap_or("none")
    ));
    lines.push(format!(
        "Generated image artifacts: {}",
        report.generated_media.len()
    ));
    for artifact in &report.generated_media {
        lines.push(format!(
            "  - {}: {} {} bytes redacted={}",
            artifact.artifact_id, artifact.mime_type, artifact.byte_len, artifact.redacted
        ));
    }
    if report.media_projections.is_empty() {
        lines.push(
            "Spec035 media projections: unavailable (canonical records unavailable)".to_owned(),
        );
    } else {
        lines.push(format!(
            "Spec035 media projections: {}",
            report.media_projections.len()
        ));
        for projection in &report.media_projections {
            match super::present_media_projection(projection) {
                Ok(presentation) => {
                    lines.push(presentation.human);
                    lines.push(format!("Spec035 media JSON: {}", presentation.machine_json));
                }
                Err(error) => lines.push(format!(
                    "Spec035 media: state=unavailable reason=unavailable detail={}",
                    redact_string(&error.to_string())
                )),
            }
        }
    }
    lines.push(format!(
        "Channel restart states: {} (hint projection; not session truth)",
        report.channel_restart.len()
    ));
    for restart_state in &report.channel_restart {
        lines.push(format_channel_restart_state_line(restart_state));
    }
    if report.providers.is_empty() {
        lines.push("Configured providers: none".to_owned());
    } else {
        lines.push("Configured providers:".to_owned());
        for provider in &report.providers {
            lines.push(format!(
                "  - {}: api_key={}, api_base={}",
                provider.name,
                configured_label(provider.has_api_key),
                configured_label(provider.has_api_base)
            ));
        }
    }
    lines.push("Runtime capabilities:".to_owned());
    for capability in &report.capabilities {
        lines.push(format!(
            "  - {}: {} ({})",
            capability.component,
            runtime_capability_label(&capability.status),
            capability.reason
        ));
    }
    lines.extend(state.readiness_lines);
    spec031_cli::push(
        &mut lines,
        &[
            spec031_cli::Projection::Diagnostics {
                component_count: state.diagnostics_component_count,
                blocked: state.diagnostics_blocked,
            },
            spec031_cli::Projection::Readiness {
                available: state.readiness_available,
            },
            spec031_cli::Projection::Subagent {
                child_count: state.subagent_child_count,
            },
            spec031_cli::Projection::Tool {
                attempt_count: state.tool_attempt_count,
            },
            spec031_cli::Projection::Context { included: true },
            spec031_cli::Projection::App {
                total_count: state.app_total_count,
            },
            spec031_cli::Projection::Media {
                artifact_count: state.media_artifact_count,
            },
        ],
    );
    lines.join("\n")
}
