use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    AutomationOwnerEffect, AutomationRouteEvidence, AutomationTaskOutcomeInput,
    DurableWorkDispatcher, MessageBus,
};
use shacs_eval::completion_boundary::EvaluatorRoute;
use shacs_eval::evaluator::ProjectionSurface;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::ReplayWorkItem;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const LEASE_MS: u64 = 30_000;
pub(super) const ROUTE_WORK_KIND: &str = "automation.owner_request";
pub(super) const ROUTE_PAYLOAD_TYPE: &str = "shacs.automation_owner_request.v1";

pub(super) fn open_dispatcher(data_dir: &Path) -> Result<DurableWorkDispatcher, String> {
    DurableWorkDispatcher::open(
        data_dir.join("runtime/durable-events"),
        data_dir.join("runtime/work-payloads"),
        MessageBus::new(),
        "automation-route-owner",
        LEASE_MS,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn route_item(data_dir: &Path, work_id: &str) -> Result<Option<ReplayWorkItem>, String> {
    let recovery = evaluate_durable_recovery(
        data_dir.join("runtime/durable-events"),
        data_dir.join("runtime/durable-checkpoints"),
    );
    if !recovery.writable {
        return Err("automation route durable store is not writable".to_owned());
    }
    Ok(recovery
        .state
        .and_then(|state| state.work.items.get(work_id).cloned()))
}

pub(super) fn delivery_target<'a>(
    target: &ProjectionSurface,
    session_key: &'a str,
) -> Result<(&'a str, &'a str), String> {
    let (channel, chat_id) = session_key
        .split_once(':')
        .ok_or_else(|| "automation route target surface is unsupported".to_owned())?;
    match target {
        ProjectionSurface::Channel if channel != "api" => Ok((channel, chat_id)),
        ProjectionSurface::LocalApi if channel == "api" => Ok((channel, chat_id)),
        ProjectionSurface::Cli
        | ProjectionSurface::Tui
        | ProjectionSurface::Channel
        | ProjectionSurface::LocalApi => {
            Err("automation route target surface is unsupported".to_owned())
        }
    }
}

pub(super) fn evidence_ref(route: EvaluatorRoute, input: &AutomationTaskOutcomeInput) -> String {
    format!(
        "automation-owner:sha256:{:x}",
        Sha256::digest(format!("{route:?}:{}:{}", input.work_id, input.result_ref).as_bytes())
    )
}

pub(super) fn route_work_id(route: EvaluatorRoute, input: &AutomationTaskOutcomeInput) -> String {
    format!(
        "automation-route-{}-{:x}",
        route_name(route),
        Sha256::digest(format!("{}:{}", input.work_id, input.result_ref).as_bytes())
    )
}

const fn route_name(route: EvaluatorRoute) -> &'static str {
    match route {
        EvaluatorRoute::Notify => "notify",
        EvaluatorRoute::Suppress => "suppress",
        EvaluatorRoute::Continue => "continue",
        EvaluatorRoute::Escalate => "escalate",
        EvaluatorRoute::Verify => "verify",
        EvaluatorRoute::RollbackCandidate => "rollback-candidate",
    }
}

pub(super) fn route_evidence(
    route: EvaluatorRoute,
    owner_evidence_ref: String,
) -> AutomationRouteEvidence {
    let effect = match route {
        EvaluatorRoute::Notify => AutomationOwnerEffect::DeliveryRequest,
        EvaluatorRoute::Suppress => AutomationOwnerEffect::NoNotification,
        EvaluatorRoute::Continue => AutomationOwnerEffect::ContinuationEnqueue,
        EvaluatorRoute::Escalate => AutomationOwnerEffect::DurableEscalation,
        EvaluatorRoute::Verify => AutomationOwnerEffect::VerificationRequest,
        EvaluatorRoute::RollbackCandidate => AutomationOwnerEffect::RollbackCandidateRecord,
    };
    AutomationRouteEvidence {
        owner_evidence_ref,
        effect,
    }
}

pub(super) fn observed_at() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("unix-ms:{millis}")
}
