use std::path::Path;

use shacs_session::durable_event::{DurableEventPayload, DurableEventStore, WORK_TERMINAL};
use shacs_session::durable_work::{
    evaluate_durable_work_recovery_for_owner, DurableWorkEnqueueJsonInput, ReplayWorkItem,
    ReplayWorkState, WorkTerminal,
};
use shacs_session::{SessionManager, SessionMutationGuard};

use super::entrusted_work_surface::load_owner_session;
use super::web_watch_fetch::{display_url, sha256_ref};
use super::web_watch_gate::{active_deadline, ActiveDeadline};
use super::web_watch_redact::{safe_text, MAX_PHRASE_CHARS};
use super::web_watch_store::{web_watches, WebWatchLedger};
use super::{
    DurableDispatchError, DurableWorkDispatcher, EntrustedWorkError, ObservationBlock,
    ObservationError, WebDeadlineRecord, WebDeadlineWake, WebEvidence, WebValueKind,
    WebWatchRuntime, WEB_DEADLINE_OUTCOME, WEB_DEADLINE_WORK_KIND,
};

const MAX_GOAL_CHARS: usize = 240;

impl DurableWorkDispatcher {
    /// Keeps one reminder wake for the current deadline of each active goal lineage, cancels
    /// wakes that no longer match it, and records the due ones that still match. A reminder
    /// time already past when the lineage started, or a passed deadline, gets no wake.
    pub fn process_web_deadlines(
        &mut self,
        runtime: &WebWatchRuntime<'_>,
    ) -> Result<(), ObservationError> {
        let state = self.replay_work_state()?;
        let admission = evaluate_durable_work_recovery_for_owner(
            &state,
            runtime.data_dir.join("runtime/work-payloads"),
            runtime.now_ms,
            Some(self.lease_owner_ref()),
        );
        if !admission.writable {
            return Err(ObservationError::Blocked(ObservationBlock::RecoveryNeeded));
        }
        let root = runtime.data_dir.join("runtime/durable-events");
        for item in state
            .items
            .values()
            .filter(|item| item.work_kind == WEB_DEADLINE_WORK_KIND && pending(&item.state))
        {
            let wake: WebDeadlineWake = self.read_work_payload(item)?;
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &item.session_key)?;
            if !self
                .replay_work_state()?
                .items
                .get(&item.work_id)
                .is_some_and(|current| pending(&current.state))
            {
                continue;
            }
            let Some(active) = self.matching(runtime, item, &wake)? else {
                let reason = "web_deadline:stale";
                if !self.cancellation_requested(item)? {
                    self.request_cancellation(item, reason)?;
                }
                self.record_cancelled(item, reason)?;
                continue;
            };
            if !admission.due_work_ids.contains(&item.work_id) || wake.due_at_ms > runtime.now_ms {
                continue;
            }
            self.lease_work(item, runtime.now_ms)?;
            let ledger =
                WebWatchLedger::read(&root, &wake.owner_session_id, &wake.entrusted_work_id)?;
            let record = WebDeadlineRecord {
                latest: latest_read(&ledger, &active),
                wake,
                reached_at_ms: runtime.now_ms,
                deadline: active.deadline,
                goal: safe_text(&active.goal, MAX_GOAL_CHARS),
                url: display_url(&active.entry.plan.url),
                url_digest: sha256_ref(&active.entry.plan.url),
                condition_revision: active.web_wake.condition_revision,
                condition: active.entry.plan.condition.clone(),
                phrase: match &active.entry.plan.extraction.value {
                    WebValueKind::Text { phrase } => Some(safe_text(phrase, MAX_PHRASE_CHARS)),
                    WebValueKind::Number { .. } => None,
                },
            };
            self.record_web_deadline_terminal(item, &record)?;
        }
        let Some(manager) = SessionManager::open_existing(runtime.workspace)? else {
            return Ok(());
        };
        let state = self.replay_work_state()?;
        for owner in manager.list_sessions()? {
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &owner.key)?;
            let (_, session) = load_owner_session(runtime.workspace, &owner.key)?;
            for watch in web_watches(&session).map_err(EntrustedWorkError::Metadata)? {
                let Some(active) = active_deadline(&session, &watch.work_id) else {
                    continue;
                };
                let work_id = active.wake.durable_work_id();
                if active.deadline.remind_at_ms < active.lineage_started_at_ms
                    || runtime.now_ms >= active.deadline.at_ms
                    || state.items.contains_key(&work_id)
                {
                    continue;
                }
                self.enqueue_json_work(
                    "shacs.web_deadline.v1",
                    &active.wake,
                    DurableWorkEnqueueJsonInput {
                        work_id: work_id.clone(),
                        work_kind: WEB_DEADLINE_WORK_KIND.to_owned(),
                        session_key: owner.key.clone(),
                        turn_id: None,
                        effect_id: Some(active.wake.goal_id.clone()),
                        dedupe_hint: Some(work_id),
                        next_wake_at_ms: Some(active.wake.due_at_ms),
                    },
                )?;
            }
        }
        Ok(())
    }

    /// The current deadline when `item` is still its wake: same work item, deadline revision,
    /// reminder time and active goal lineage, before the deadline and not cancelled.
    fn matching(
        &self,
        runtime: &WebWatchRuntime<'_>,
        item: &ReplayWorkItem,
        wake: &WebDeadlineWake,
    ) -> Result<Option<ActiveDeadline>, ObservationError> {
        if self.cancellation_requested(item)?
            || wake.durable_work_id() != item.work_id
            || wake.owner_session_id != item.session_key
            || item.effect_id.as_deref() != Some(wake.goal_id.as_str())
            || item.dedupe_hint.as_deref() != Some(item.work_id.as_str())
        {
            return Ok(None);
        }
        Ok(
            load_owner_session(runtime.workspace, &wake.owner_session_id)
                .ok()
                .and_then(|(_, session)| active_deadline(&session, &wake.entrusted_work_id))
                .filter(|active| active.wake == *wake && runtime.now_ms < active.deadline.at_ms),
        )
    }
}

/// The latest read of the current plan: its newest successful check or its confirmation read.
fn latest_read(ledger: &WebWatchLedger, active: &ActiveDeadline) -> Option<WebEvidence> {
    let plan = active.entry.plan.digest();
    ledger
        .last_success
        .as_ref()
        .filter(|record| record.wake.plan_digest == plan)
        .and_then(|record| record.decision.current.clone())
        .into_iter()
        .chain([active.entry.baseline.clone()])
        .max_by_key(|evidence| evidence.observed_at_ms)
}

/// Deadline records of one work in the owner session, oldest first.
pub(super) fn read_web_deadline_records(
    root: &Path,
    owner: &str,
    work_id: &str,
) -> Result<Vec<WebDeadlineRecord>, ObservationError> {
    let mut records = Vec::new();
    if !root.exists() {
        return Ok(records);
    }
    let store = DurableEventStore::open(root).map_err(DurableDispatchError::from)?;
    let mut failure = None;
    let summary = store
        .visit_from_sequence(0, |event| {
            if failure.is_some() || event.session_id != owner || event.kind != WORK_TERMINAL {
                return;
            }
            let DurableEventPayload::Inline { data, .. } = &event.payload else {
                return;
            };
            let parsed =
                serde_json::from_value::<WorkTerminal>(data.clone()).and_then(|terminal| {
                    if terminal.outcome_ref != WEB_DEADLINE_OUTCOME {
                        return Ok(None);
                    }
                    terminal
                        .facts
                        .map(serde_json::from_value::<WebDeadlineRecord>)
                        .transpose()
                });
            match parsed {
                Ok(Some(record)) if record.wake.entrusted_work_id == work_id => {
                    records.push(record)
                }
                Ok(_) => {}
                Err(error) => failure = Some(DurableDispatchError::Serialization(error)),
            }
        })
        .map_err(DurableDispatchError::from)?;
    if let Some(error) = failure {
        return Err(error.into());
    }
    if summary.incomplete_tail || !summary.compatibility.is_current() {
        return Err(ObservationError::Blocked(ObservationBlock::RecoveryNeeded));
    }
    Ok(records)
}

fn pending(state: &ReplayWorkState) -> bool {
    matches!(
        state,
        ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
    )
}
