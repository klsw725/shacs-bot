use crate::{error_response, json_response, ApiError, ApiHttpResponse, ChatCompletionAdapter};
use serde_json::json;
use shacs_projection::{
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031ConstructionError,
    Spec031Count, Spec031Envelope, Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage,
    Spec031ProjectionKind, Spec031ReadinessCapability, Spec031Reason, Spec031ReasonCode,
    Spec031SafeSummary, Spec031SchemaVersion, Spec031Severity, Spec031Source, Spec031SourceOwner,
    Spec031SubagentCapability, Spec031SubjectRef, Spec031ToolCapability,
};

pub const SUBAGENTS_PATH: &str = "/v1/subagents";
pub const TOOLS_PATH: &str = "/v1/tools";
pub const READINESS_PATH: &str = "/v1/readiness";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031ApiProjection {
    Diagnostics,
    Subagent,
    Tool,
    Readiness,
}

pub fn handle_spec031_projection_request(
    path: &str,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> Option<ApiHttpResponse> {
    match spec031_projection_from_path(path) {
        Ok(Some(projection)) => Some(spec031_projection_response(projection, adapter)),
        Ok(None) => None,
        Err(error) => Some(error_response(error)),
    }
}

pub fn spec031_projection_path(path: &str) -> &str {
    path.split_once('?').map_or(path, |(path, _)| path)
}

fn spec031_projection_from_path(path: &str) -> Result<Option<Spec031ApiProjection>, ApiError> {
    let (path, query) = split_query(path);
    let projection = match path {
        crate::DIAGNOSTICS_PATH => Some(Spec031ApiProjection::Diagnostics),
        SUBAGENTS_PATH => Some(Spec031ApiProjection::Subagent),
        TOOLS_PATH => Some(Spec031ApiProjection::Tool),
        READINESS_PATH => Some(Spec031ApiProjection::Readiness),
        _ => None,
    };
    if projection.is_some() && query.is_some_and(|query| query != "schema_version=1") {
        return Err(ApiError::invalid_request(
            "unsupported Spec031 schema version selector",
        ));
    }
    Ok(projection)
}

fn spec031_projection_response(
    projection: Spec031ApiProjection,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    if projection == Spec031ApiProjection::Readiness {
        if let Some(readiness) = adapter.readiness_projection() {
            return json_response(200, readiness);
        }
    }
    match adapter.spec031_projection(projection) {
        Ok(Some(envelope)) => json_response(200, json!(envelope)),
        Ok(None) if projection == Spec031ApiProjection::Diagnostics => {
            json_response(200, adapter.diagnostics_projection())
        }
        Ok(None) => match unavailable_projection(projection) {
            Ok(envelope) => json_response(200, json!(envelope)),
            Err(error) => error_response(spec031_error(error)),
        },
        Err(error) => error_response(error),
    }
}

fn unavailable_projection(
    projection: Spec031ApiProjection,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    let (kind, subject, action, capability) = match projection {
        Spec031ApiProjection::Subagent => (
            Spec031ProjectionKind::Subagent,
            "subject:api:subagents-unavailable",
            "action:api:subagents-unavailable",
            Spec031Capability::Subagent(Spec031SubagentCapability { child_count: None }),
        ),
        Spec031ApiProjection::Tool => (
            Spec031ProjectionKind::Tool,
            "subject:api:tools-unavailable",
            "action:api:tools-unavailable",
            Spec031Capability::Tool(Spec031ToolCapability {
                attempt_count: None,
            }),
        ),
        Spec031ApiProjection::Readiness => (
            Spec031ProjectionKind::Readiness,
            "subject:api:readiness-unavailable",
            "action:api:readiness-unavailable",
            Spec031Capability::Readiness(Spec031ReadinessCapability {
                availability: Spec031Availability::Unavailable,
                component_count: None,
                queue_depth: None,
                queue_capacity: None,
                remediation: None,
            }),
        ),
        Spec031ApiProjection::Diagnostics => (
            Spec031ProjectionKind::Diagnostics,
            "subject:api:diagnostics-unavailable",
            "action:api:diagnostics-unavailable",
            Spec031Capability::Diagnostics(shacs_projection::Spec031DiagnosticsCapability {
                component_count: Some(Spec031Count::new(0)),
            }),
        ),
    };
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind,
        state: Spec031Availability::Unavailable,
        severity: Spec031Severity::Warning,
        reason: Spec031Reason {
            code: Spec031ReasonCode::Unsupported,
            safe_summary: Spec031SafeSummary::try_new("projection adapter is not configured")?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(subject)?,
            parent_ref: None,
            action_ref: Some(Spec031ActionRef::try_new(action)?),
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Spec031,
            observed_at_unix_ms: None,
            freshness: Spec031Freshness::Unavailable,
        },
        capability,
        children: Vec::new(),
    })
}

fn split_query(path: &str) -> (&str, Option<&str>) {
    path.split_once('?')
        .map_or((path, None), |(path, query)| (path, Some(query)))
}

fn spec031_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("Spec031 projection could not be built: {error}"))
}
