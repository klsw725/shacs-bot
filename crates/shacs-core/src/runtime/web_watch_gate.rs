use shacs_session::Session;

use super::entrusted_work_access::{bound_current_work, latest_transition_id};
use super::web_watch_store::web_watches;
use super::{ObservationBlock, PersistentGoalStatus, WebWatchEntry, WebWatchWake};

/// The wake the current owner session allows for `work_id` and its stored watch.
/// Uses the same goal, consent and lineage gate as 036 local observations.
pub(super) fn active_web_wake(
    session: &Session,
    work_id: &str,
) -> Result<Option<(WebWatchWake, WebWatchEntry)>, ObservationBlock> {
    let (works, index, goal) = bound_current_work(session, &session.key, work_id)
        .map_err(|_| ObservationBlock::StaleLineage)?;
    let work = works.get(index).ok_or(ObservationBlock::StaleLineage)?;
    if work.consent.evidence_ref.trim().is_empty() || work.consent.recorded_at.trim().is_empty() {
        return Err(ObservationBlock::ConsentMissing);
    }
    if goal.status != PersistentGoalStatus::Active {
        return Err(ObservationBlock::GoalInactive);
    }
    let revision = work
        .revisions
        .iter()
        .find(|revision| revision.revision == work.current_revision)
        .ok_or(ObservationBlock::StaleLineage)?;
    let Some(entry) = web_watches(session)
        .map_err(|_| ObservationBlock::StaleLineage)?
        .into_iter()
        .find(|entry| entry.work_id == work.work_id)
    else {
        return Ok(None);
    };
    // A new lineage (resume, revised conditions) starts checking when it starts.
    let lineage_start = goal
        .last_transition
        .iter()
        .map(|transition| transition.observed_at.as_str())
        .chain([revision.changed_at.as_str()])
        .filter_map(parse_ms)
        .max()
        .unwrap_or_default();
    let wake = WebWatchWake {
        entrusted_work_id: work.work_id.clone(),
        owner_session_id: work.owner_session_id.clone(),
        goal_id: goal.id.clone(),
        condition_revision: work.current_revision,
        goal_transition_id: latest_transition_id(&goal)
            .map_err(|_| ObservationBlock::StaleLineage)?,
        plan_digest: entry.plan.digest(),
        due_at_ms: entry.plan.first_wake_at_ms.max(lineage_start),
    };
    Ok(Some((wake, entry)))
}

/// The stored watch when `wake` still matches the current lineage and plan.
pub(super) fn check_web_wake(
    session: &Session,
    wake: &WebWatchWake,
) -> Result<WebWatchEntry, ObservationBlock> {
    let (mut current, entry) =
        active_web_wake(session, &wake.entrusted_work_id)?.ok_or(ObservationBlock::StaleLineage)?;
    current.due_at_ms = wake.due_at_ms;
    if current == *wake {
        Ok(entry)
    } else {
        Err(ObservationBlock::StaleLineage)
    }
}

fn parse_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    value.parse::<u64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .and_then(|time| u64::try_from(time.timestamp_millis()).ok())
    })
}
