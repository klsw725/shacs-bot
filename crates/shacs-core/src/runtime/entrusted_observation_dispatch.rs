use std::path::Path;

use shacs_session::durable_work::{
    evaluate_durable_work_recovery_for_owner, DurableWorkEnqueueJsonInput, ReplayWorkItem,
    ReplayWorkState,
};
use shacs_session::{SessionManager, SessionMutationGuard};

use super::entrusted_observation_store::ObservationLedger;
use super::entrusted_observation_surface::{active_wake, check_wake};
use super::entrusted_work_store::entrusted_works_from_session;
use super::entrusted_work_surface::load_owner_session;
use super::{
    DurableWorkDispatcher, EntrustedWorkError, EntrustedWorkReadRequest, ObservationBlock,
    ObservationError, ObservationNextWake, ObservationRecord, ObservationResult, ObservationWake,
    ENTRUSTED_OBSERVATION_WORK_KIND,
};

pub struct ObservationRuntime<'a> {
    pub workspace: &'a Path,
    pub data_dir: &'a Path,
    pub now_ms: u64,
}

pub trait EntrustedObservationExecutor {
    fn prepare(&self, wake: &ObservationWake) -> Result<String, ObservationBlock>;
    fn observe(&self, wake: &ObservationWake) -> ObservationResult;
}

impl DurableWorkDispatcher {
    pub fn process_entrusted_observations(
        &mut self,
        runtime: &ObservationRuntime<'_>,
        executor: &dyn EntrustedObservationExecutor,
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
        for item in state.items.values().filter(|item| {
            item.work_kind == ENTRUSTED_OBSERVATION_WORK_KIND
                && matches!(
                    item.state,
                    ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
                )
        }) {
            let wake: ObservationWake = self.read_work_payload(item)?;
            {
                let _guard = SessionMutationGuard::acquire(runtime.workspace, &item.session_key)?;
                if !self
                    .replay_work_state()?
                    .items
                    .get(&item.work_id)
                    .is_some_and(|current| {
                        matches!(
                            current.state,
                            ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
                        )
                    })
                {
                    continue;
                }
                if let Some(reason) = self.observation_denial(runtime.workspace, item, &wake)? {
                    self.cancel_observation(item, reason)?;
                    continue;
                }
            }
            if !admission.due_work_ids.contains(&item.work_id) || wake.due_at_ms > runtime.now_ms {
                continue;
            }
            let preparation = executor.prepare(&wake);
            {
                let _guard = SessionMutationGuard::acquire(runtime.workspace, &item.session_key)?;
                if !self
                    .replay_work_state()?
                    .items
                    .get(&item.work_id)
                    .is_some_and(|current| {
                        matches!(
                            current.state,
                            ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
                        )
                    })
                {
                    continue;
                }
                if let Some(reason) = self.observation_denial(runtime.workspace, item, &wake)? {
                    self.cancel_observation(item, reason)?;
                    continue;
                }
                self.lease_work(item, runtime.now_ms)?;
            }
            let (result, snapshot_ref) = match preparation {
                Ok(snapshot_ref) => (executor.observe(&wake), Some(snapshot_ref)),
                Err(reason) => (ObservationResult::Blocked { reason }, None),
            };
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &item.session_key)?;
            if let Some(reason) = self.observation_denial(runtime.workspace, item, &wake)? {
                self.cancel_observation(item, reason)?;
                continue;
            }
            let next_wake_at_ms = match result {
                ObservationResult::Observed { .. } => runtime
                    .now_ms
                    .max(wake.due_at_ms)
                    .checked_add(wake.plan.cadence_seconds.saturating_mul(1000)),
                ObservationResult::Unavailable { .. } | ObservationResult::Blocked { .. } => None,
            };
            let ledger = ObservationLedger::read(
                &runtime.data_dir.join("runtime/durable-events"),
                &EntrustedWorkReadRequest {
                    owner_session_id: wake.owner_session_id.clone(),
                    requester_target: wake.owner_session_id.clone(),
                    work_id: wake.entrusted_work_id.clone(),
                },
            )?;
            let mut record = ObservationRecord {
                wake,
                observed_at_ms: runtime.now_ms,
                result,
                execution_snapshot_ref: snapshot_ref,
                next_wake_at_ms,
                decision: None,
            };
            record.decision = Some(super::entrusted_comparison_evaluate::compare(
                &record,
                ledger.baseline.as_ref(),
                ledger.history.last(),
            ));
            self.record_observation_terminal(item, &record)?;
        }
        self.enqueue_entrusted_observations(runtime)
    }

    pub fn enqueue_entrusted_observations(
        &mut self,
        runtime: &ObservationRuntime<'_>,
    ) -> Result<(), ObservationError> {
        let Some(manager) = SessionManager::open_existing(runtime.workspace)? else {
            return Ok(());
        };
        for owner in manager.list_sessions()? {
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &owner.key)?;
            let (_, session) = load_owner_session(runtime.workspace, &owner.key)?;
            let works =
                entrusted_works_from_session(&session).map_err(EntrustedWorkError::Metadata)?;
            for work in works {
                let Ok(Some(mut wake)) = active_wake(&session, &work.work_id) else {
                    continue;
                };
                let ledger = ObservationLedger::read(
                    &runtime.data_dir.join("runtime/durable-events"),
                    &EntrustedWorkReadRequest {
                        owner_session_id: owner.key.clone(),
                        requester_target: owner.key.clone(),
                        work_id: work.work_id,
                    },
                )?;
                let ObservationNextWake::AwaitingEnqueue { work_id, due_at_ms } =
                    ledger.next(&wake)
                else {
                    continue;
                };
                wake.due_at_ms = due_at_ms;
                self.enqueue_json_work(
                    "shacs.entrusted_observation.v1",
                    &wake,
                    DurableWorkEnqueueJsonInput {
                        work_id: work_id.clone(),
                        work_kind: ENTRUSTED_OBSERVATION_WORK_KIND.to_owned(),
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

    fn observation_denial(
        &self,
        workspace: &Path,
        item: &ReplayWorkItem,
        wake: &ObservationWake,
    ) -> Result<Option<ObservationBlock>, ObservationError> {
        if self.cancellation_requested(item)? {
            return Ok(Some(ObservationBlock::Cancelled));
        }
        if wake.durable_work_id() != item.work_id
            || wake.owner_session_id != item.session_key
            || item.effect_id.as_deref() != Some(wake.goal_id.as_str())
            || item.dedupe_hint.as_deref() != Some(item.work_id.as_str())
        {
            return Ok(Some(ObservationBlock::StaleLineage));
        }
        Ok(
            match load_owner_session(workspace, &wake.owner_session_id) {
                Ok((_, session)) => check_wake(&session, wake).err(),
                Err(_) => Some(ObservationBlock::StaleLineage),
            },
        )
    }

    fn cancel_observation(
        &mut self,
        item: &ReplayWorkItem,
        reason: ObservationBlock,
    ) -> Result<(), ObservationError> {
        let reason = format!("observation:{reason:?}");
        if !self.cancellation_requested(item)? {
            self.request_cancellation(item, &reason)?;
        }
        self.record_cancelled(item, reason)?;
        Ok(())
    }
}
