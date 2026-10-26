use std::path::Path;

use serde_json::{Map, Value};
use shacs_session::SessionMutationGuard;

use super::entrusted_work_access::work_snapshot_from_session;
use super::entrusted_work_surface::load_owner_session;
use super::web_watch_deadline_surface::deadline_view;
use super::web_watch_fetch::{display_url, sha256_ref};
use super::web_watch_gate::active_web_wake;
use super::web_watch_notify_store::read_web_notifications;
use super::web_watch_notify_surface::settings_view;
use super::web_watch_redact::safe_extraction;
use super::web_watch_store::{
    web_deadline_settings, web_notify_settings, web_watches, WebWatchLedger,
};
use super::{
    EntrustedWorkError, EntrustedWorkReadRequest, ObservationBlock, ObservationNextWake,
    WebExtraction, WebIntervalCloseReason, WebIntervalClosure, WebWatchError, WebWatchPlan,
    WebWatchPlanView, WebWatchSnapshot, WEB_WATCH_METADATA_KEY,
};

/// Status of a web watch with query and fragment removed from every URL.
pub fn read_web_watch(
    workspace: &Path,
    data_dir: &Path,
    request: &EntrustedWorkReadRequest,
) -> Result<WebWatchSnapshot, WebWatchError> {
    let _guard = SessionMutationGuard::acquire(workspace, &request.owner_session_id)?;
    let (_, session) = load_owner_session(workspace, &request.owner_session_id)?;
    let work = work_snapshot_from_session(&session, &request.requester_target, &request.work_id)?;
    let entry = web_watches(&session)
        .map_err(EntrustedWorkError::Metadata)?
        .into_iter()
        .find(|entry| entry.work_id == request.work_id);
    let ledger = WebWatchLedger::read(
        &data_dir.join("runtime/durable-events"),
        &request.owner_session_id,
        &request.work_id,
    )?;
    let current = active_web_wake(&session, &request.work_id);
    let next_wake = match &current {
        Ok(Some((wake, _))) => ledger.next(wake),
        Ok(None) => ObservationNextWake::NotConfigured,
        Err(reason) => ObservationNextWake::Stopped { reason: *reason },
    };
    let current_wake = current
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|(wake, _)| wake);
    let current_decision = current_wake.and_then(|wake| {
        ledger
            .history
            .iter()
            .rev()
            .find(|record| {
                record.wake.lineage_prefix() == wake.lineage_prefix()
                    && record.wake.plan_digest == wake.plan_digest
            })
            .map(|record| record.decision.clone())
    });
    let plan_digest = entry.as_ref().map(|entry| entry.plan.digest());
    let last_success_at_ms = ledger
        .last_success
        .as_ref()
        .filter(|record| Some(&record.wake.plan_digest) == plan_digest.as_ref())
        .map(|record| record.observed_at_ms)
        .into_iter()
        .chain(entry.as_ref().map(|entry| entry.baseline.observed_at_ms))
        .max();
    let mut intervals = ledger.intervals;
    if let Some(open) = intervals
        .last_mut()
        .filter(|interval| interval.closed.is_none())
    {
        let reason = match (&current, current_wake) {
            (Err(ObservationBlock::GoalInactive), _) => Some(WebIntervalCloseReason::GoalInactive),
            (_, Some(wake))
                if wake.condition_revision == open.condition_revision
                    && wake.goal_transition_id == open.goal_transition_id =>
            {
                None
            }
            _ => Some(WebIntervalCloseReason::LineageChanged),
        };
        open.closed = reason.map(|reason| WebIntervalClosure {
            reason,
            at_ms: None,
        });
    }
    let notify = web_notify_settings(&session, &request.work_id)
        .map_err(EntrustedWorkError::Metadata)?
        .map(|settings| {
            settings_view(
                &settings,
                entry.as_ref().map_or(0, |entry| entry.plan.cadence_seconds),
            )
        });
    let deadline = web_deadline_settings(&session, &request.work_id)
        .map_err(EntrustedWorkError::Metadata)?
        .map(|settings| deadline_view(&settings));
    let mut notifications: Vec<_> =
        read_web_notifications(&data_dir.join("runtime/durable-events"))?
            .into_values()
            .filter(|entry| {
                entry.intent.owner_session_id == request.owner_session_id
                    && entry.intent.entrusted_work_id == request.work_id
            })
            .map(|entry| (entry.first_fact_index, entry.snapshot))
            .collect();
    notifications.sort_by_key(|(index, _)| *index);
    Ok(WebWatchSnapshot {
        work_id: request.work_id.clone(),
        condition_revision: work.work.current_revision,
        goal_status: work.goal_status,
        plan: entry.as_ref().map(|entry| plan_view(&entry.plan)),
        baseline: entry.map(|entry| entry.baseline),
        last_success_at_ms,
        current_decision,
        failure_interval: intervals
            .last()
            .filter(|interval| interval.closed.is_none())
            .cloned(),
        failure_intervals: intervals,
        runtime_gaps: ledger.gaps,
        next_wake,
        history: ledger.history,
        notify,
        deadline,
        notifications: notifications
            .into_iter()
            .map(|(_, snapshot)| snapshot)
            .collect(),
    })
}

/// Session metadata for output outside the session file: every stored web watch URL
/// without query and fragment, next to the full URL's digest, and every extraction as
/// `safe_extraction` shows it (one that cannot be read is left out).
pub fn mask_web_watch_metadata(metadata: &mut Map<String, Value>) {
    let plans = metadata
        .get_mut(WEB_WATCH_METADATA_KEY)
        .and_then(|store| store.get_mut("watches"))
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|watch| watch.get_mut("plan").and_then(Value::as_object_mut));
    for plan in plans {
        if let Some(url) = plan.get("url").and_then(Value::as_str).map(str::to_owned) {
            plan.insert("url".to_owned(), Value::String(display_url(&url)));
            plan.insert("url_digest".to_owned(), Value::String(sha256_ref(&url)));
        }
        let shown = plan
            .remove("extraction")
            .and_then(|extraction| serde_json::from_value::<WebExtraction>(extraction).ok())
            .and_then(|extraction| serde_json::to_value(safe_extraction(&extraction)).ok());
        if let Some(shown) = shown {
            plan.insert("extraction".to_owned(), shown);
        }
    }
}

pub(super) fn plan_view(plan: &WebWatchPlan) -> WebWatchPlanView {
    WebWatchPlanView {
        url: display_url(&plan.url),
        url_digest: sha256_ref(&plan.url),
        extraction: safe_extraction(&plan.extraction),
        condition: plan.condition.clone(),
        cadence_seconds: plan.cadence_seconds,
    }
}
