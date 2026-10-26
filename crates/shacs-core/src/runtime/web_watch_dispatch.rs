use std::path::Path;

use shacs_security::NetworkGuard;
use shacs_session::durable_work::{
    evaluate_durable_work_recovery_for_owner, DurableWorkEnqueueJsonInput, ReplayWorkItem,
    ReplayWorkState,
};
use shacs_session::{SessionManager, SessionMutationGuard};

use super::entrusted_work_surface::load_owner_session;
use super::web_watch_decision::build_record;
use super::web_watch_fetch::{check_plan, display_url, WebFetchLimits};
use super::web_watch_gate::{active_web_wake, check_web_wake};
use super::web_watch_store::{web_notify_settings, web_watches, WebWatchLedger};
use super::{
    DurableWorkDispatcher, EntrustedWorkError, ObservationBlock, ObservationError,
    ObservationNextWake, WebCheckResult, WebWatchEntry, WebWatchWake, WEB_WATCH_WORK_KIND,
};

pub struct WebWatchRuntime<'a> {
    pub workspace: &'a Path,
    pub data_dir: &'a Path,
    pub now_ms: u64,
    pub guard: &'a NetworkGuard,
    pub limits: WebFetchLimits,
}

/// Owner checks before a watcher read: execution snapshot provenance and `tool:before` hooks.
/// The confirmed watch authorizes the read; it is not a tool permission grant.
pub trait WebWatchExecutor {
    fn prepare(&self, wake: &WebWatchWake, display_url: &str) -> Result<String, ObservationBlock>;
}

impl DurableWorkDispatcher {
    pub fn process_web_watches(
        &mut self,
        runtime: &WebWatchRuntime<'_>,
        executor: &dyn WebWatchExecutor,
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
        for item in state
            .items
            .values()
            .filter(|item| item.work_kind == WEB_WATCH_WORK_KIND && pending(&item.state))
        {
            let wake: WebWatchWake = self.read_work_payload(item)?;
            let Some(entry) = self.admit(runtime.workspace, item, &wake, None)? else {
                continue;
            };
            if !admission.due_work_ids.contains(&item.work_id) || wake.due_at_ms > runtime.now_ms {
                continue;
            }
            let preparation = executor.prepare(&wake, &display_url(&entry.plan.url));
            let Some(entry) = self.admit(runtime.workspace, item, &wake, Some(runtime.now_ms))?
            else {
                continue;
            };
            let (result, snapshot_ref) = match preparation {
                Ok(snapshot_ref) => (
                    check_plan(&entry.plan, runtime.guard, &runtime.limits, runtime.now_ms),
                    Some(snapshot_ref),
                ),
                Err(reason) => (WebCheckResult::Blocked { reason }, None),
            };
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &item.session_key)?;
            let entry = match self.denial(runtime.workspace, item, &wake)? {
                Ok(entry) => entry,
                Err(reason) => {
                    self.cancel_web_watch(item, reason)?;
                    continue;
                }
            };
            let ledger = WebWatchLedger::read(
                &runtime.data_dir.join("runtime/durable-events"),
                &wake.owner_session_id,
                &wake.entrusted_work_id,
            )?;
            let (_, session) = load_owner_session(runtime.workspace, &wake.owner_session_id)?;
            let notify_revision = web_notify_settings(&session, &wake.entrusted_work_id)
                .map_err(EntrustedWorkError::Metadata)?
                .filter(|settings| settings.target.is_some())
                .map(|settings| settings.revision);
            let record = build_record(
                wake,
                &entry,
                result,
                &ledger,
                runtime.now_ms,
                (snapshot_ref, notify_revision),
            );
            self.record_web_watch_terminal(item, &record)?;
        }
        self.enqueue_web_watches(runtime)
    }

    pub fn enqueue_web_watches(
        &mut self,
        runtime: &WebWatchRuntime<'_>,
    ) -> Result<(), ObservationError> {
        let Some(manager) = SessionManager::open_existing(runtime.workspace)? else {
            return Ok(());
        };
        let root = runtime.data_dir.join("runtime/durable-events");
        for owner in manager.list_sessions()? {
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &owner.key)?;
            let (_, session) = load_owner_session(runtime.workspace, &owner.key)?;
            for entry in web_watches(&session).map_err(EntrustedWorkError::Metadata)? {
                let Ok(Some((mut wake, _))) = active_web_wake(&session, &entry.work_id) else {
                    continue;
                };
                let ledger = WebWatchLedger::read(&root, &owner.key, &entry.work_id)?;
                let ObservationNextWake::AwaitingEnqueue { work_id, due_at_ms } =
                    ledger.next(&wake)
                else {
                    continue;
                };
                wake.due_at_ms = due_at_ms;
                self.enqueue_json_work(
                    "shacs.web_watch.v1",
                    &wake,
                    DurableWorkEnqueueJsonInput {
                        work_id: work_id.clone(),
                        work_kind: WEB_WATCH_WORK_KIND.to_owned(),
                        session_key: owner.key.clone(),
                        turn_id: None,
                        effect_id: Some(wake.goal_id.clone()),
                        dedupe_hint: Some(work_id),
                        next_wake_at_ms: Some(due_at_ms),
                    },
                )?;
            }
        }
        Ok(())
    }

    /// Gate under the owner session lock: still pending and current (then leased when
    /// `lease_at` is set), or cancelled without a new observation.
    fn admit(
        &mut self,
        workspace: &Path,
        item: &ReplayWorkItem,
        wake: &WebWatchWake,
        lease_at: Option<u64>,
    ) -> Result<Option<WebWatchEntry>, ObservationError> {
        let _guard = SessionMutationGuard::acquire(workspace, &item.session_key)?;
        if !self
            .replay_work_state()?
            .items
            .get(&item.work_id)
            .is_some_and(|current| pending(&current.state))
        {
            return Ok(None);
        }
        let entry = match self.denial(workspace, item, wake)? {
            Ok(entry) => entry,
            Err(reason) => {
                self.cancel_web_watch(item, reason)?;
                return Ok(None);
            }
        };
        if let Some(now_ms) = lease_at {
            self.lease_work(item, now_ms)?;
        }
        Ok(Some(entry))
    }

    fn denial(
        &self,
        workspace: &Path,
        item: &ReplayWorkItem,
        wake: &WebWatchWake,
    ) -> Result<Result<WebWatchEntry, ObservationBlock>, ObservationError> {
        if self.cancellation_requested(item)? {
            return Ok(Err(ObservationBlock::Cancelled));
        }
        if wake.durable_work_id() != item.work_id
            || wake.owner_session_id != item.session_key
            || item.effect_id.as_deref() != Some(wake.goal_id.as_str())
            || item.dedupe_hint.as_deref() != Some(item.work_id.as_str())
        {
            return Ok(Err(ObservationBlock::StaleLineage));
        }
        Ok(
            match load_owner_session(workspace, &wake.owner_session_id) {
                Ok((_, session)) => check_web_wake(&session, wake),
                Err(_) => Err(ObservationBlock::StaleLineage),
            },
        )
    }

    fn cancel_web_watch(
        &mut self,
        item: &ReplayWorkItem,
        reason: ObservationBlock,
    ) -> Result<(), ObservationError> {
        let reason = format!("web_watch:{reason:?}");
        if !self.cancellation_requested(item)? {
            self.request_cancellation(item, &reason)?;
        }
        self.record_cancelled(item, reason)?;
        Ok(())
    }
}

fn pending(state: &ReplayWorkState) -> bool {
    matches!(
        state,
        ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
    )
}
