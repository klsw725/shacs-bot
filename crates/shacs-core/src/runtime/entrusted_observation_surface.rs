use std::path::Path;

use shacs_session::{Session, SessionMutationGuard};

use super::entrusted_observation_store::ObservationLedger;
use super::entrusted_work_access::{bound_current_work, latest_transition_id, snapshot};
use super::entrusted_work_store::{entrusted_works_from_session, store_entrusted_works};
use super::entrusted_work_surface::load_owner_session;
use super::{
    ConfigureObservationRequest, EntrustedObservationSnapshot, EntrustedWorkError,
    EntrustedWorkReadRequest, EntrustedWorkRevision, EntrustedWorkSnapshot, ObservationBlock,
    ObservationError, ObservationNextWake, ObservationWake, PersistentGoalStatus,
};

pub fn configure_entrusted_observation(
    workspace: &Path,
    request: ConfigureObservationRequest,
) -> Result<EntrustedWorkSnapshot, ObservationError> {
    request.plan.validate()?;
    let owner = &request.work.owner_session_id;
    let _guard = SessionMutationGuard::acquire(workspace, owner)?;
    let (mut manager, mut session) = load_owner_session(workspace, owner)?;
    let (mut works, index, goal) = bound_current_work(
        &session,
        &request.work.requester_target,
        &request.work.work_id,
    )?;
    if goal.status != PersistentGoalStatus::Active {
        return Err(ObservationError::Blocked(ObservationBlock::GoalInactive));
    }
    if super::web_watch_store::web_watches(&session)
        .map_err(EntrustedWorkError::Metadata)?
        .iter()
        .any(|entry| entry.work_id == request.work.work_id)
    {
        return Err(ObservationError::InvalidInput(
            "work already watches a web page",
        ));
    }
    let work = &mut works[index];
    if work.current_revision != request.expected_revision {
        return Err(EntrustedWorkError::RevisionConflict {
            expected: request.expected_revision,
            actual: work.current_revision,
        }
        .into());
    }
    if work.consent.evidence_ref.trim().is_empty() || work.consent.recorded_at.trim().is_empty() {
        return Err(ObservationError::Blocked(ObservationBlock::ConsentMissing));
    }
    let conditions = work
        .current_conditions()
        .cloned()
        .ok_or(ObservationError::Blocked(ObservationBlock::StaleLineage))?;
    work.current_revision = work
        .current_revision
        .checked_add(1)
        .ok_or(ObservationError::InvalidInput("revision overflow"))?;
    work.observation_plan = Some(request.plan);
    work.revisions.push(EntrustedWorkRevision {
        revision: work.current_revision,
        conditions,
        linked_targets: work.linked_targets.clone(),
        changed_at: request.changed_at,
    });
    let result = snapshot(work.clone(), &goal)?;
    store_entrusted_works(&mut session, &works).map_err(EntrustedWorkError::Metadata)?;
    manager.save_with_fsync(&session)?;
    Ok(result)
}

pub fn read_entrusted_observation(
    workspace: &Path,
    data_dir: &Path,
    request: &EntrustedWorkReadRequest,
) -> Result<EntrustedObservationSnapshot, ObservationError> {
    let _guard = SessionMutationGuard::acquire(workspace, &request.owner_session_id)?;
    let (_, session) = load_owner_session(workspace, &request.owner_session_id)?;
    let works = entrusted_works_from_session(&session).map_err(EntrustedWorkError::Metadata)?;
    let work = works
        .iter()
        .find(|work| work.work_id == request.work_id)
        .ok_or(EntrustedWorkError::WorkMissing)?;
    if !work.is_accessible_from(&request.requester_target) {
        return Err(EntrustedWorkError::AccessDenied.into());
    }
    let current_transition_id = super::goal::persistent_goal_result_from_session(&session)
        .map_err(EntrustedWorkError::GoalMetadata)?
        .filter(|goal| goal.id == work.goal_id && goal.session_id == work.owner_session_id)
        .and_then(|goal| {
            goal.last_transition
                .and_then(|transition| transition.transition_id)
        });
    let current_lineage = |revision: u64, transition_id: &str| {
        revision == work.current_revision && current_transition_id.as_deref() == Some(transition_id)
    };
    let ledger = ObservationLedger::read(&data_dir.join("runtime/durable-events"), request)?;
    let next_wake = match active_wake(&session, &request.work_id) {
        Ok(Some(wake)) => ledger.next(&wake),
        Ok(None) => ObservationNextWake::NotConfigured,
        Err(reason) => ObservationNextWake::Stopped { reason },
    };
    let current_observation = ledger
        .history
        .iter()
        .rev()
        .find(|record| {
            current_lineage(
                record.wake.condition_revision,
                &record.wake.goal_transition_id,
            ) && work.observation_plan.as_ref() == Some(&record.wake.plan)
        })
        .cloned();
    Ok(EntrustedObservationSnapshot {
        work_id: work.work_id.clone(),
        condition_revision: work.current_revision,
        plan: work.observation_plan.clone(),
        current_observation,
        history: ledger.history,
        next_wake,
        current_decision: ledger
            .decisions
            .iter()
            .rev()
            .find(|decision| {
                current_lineage(decision.condition_revision, &decision.goal_transition_id)
                    && work.observation_plan.as_ref().is_some_and(|plan| {
                        decision.source == plan.source && decision.policy == plan.comparison
                    })
            })
            .cloned(),
        comparison_baseline: ledger
            .baseline
            .filter(|record| {
                current_lineage(
                    record.wake.condition_revision,
                    &record.wake.goal_transition_id,
                ) && work.observation_plan.as_ref() == Some(&record.wake.plan)
            })
            .and_then(|record| record.decision)
            .and_then(|decision| decision.current),
        decisions: ledger.decisions,
        notifications: super::entrusted_notification_store::read_notifications(
            &data_dir.join("runtime/durable-events"),
        )?
        .into_values()
        .map(|entry| entry.snapshot)
        .filter(|snapshot| {
            snapshot.intent.owner_session_id == request.owner_session_id
                && snapshot.intent.entrusted_work_id == request.work_id
        })
        .collect(),
    })
}

pub(super) fn active_wake(
    session: &Session,
    work_id: &str,
) -> Result<Option<ObservationWake>, ObservationBlock> {
    let (works, index, goal) = bound_current_work(session, &session.key, work_id)
        .map_err(|_| ObservationBlock::StaleLineage)?;
    let work = &works[index];
    if work.consent.evidence_ref.trim().is_empty() || work.consent.recorded_at.trim().is_empty() {
        return Err(ObservationBlock::ConsentMissing);
    }
    if goal.status != PersistentGoalStatus::Active {
        return Err(ObservationBlock::GoalInactive);
    }
    if work.current_conditions().is_none() {
        return Err(ObservationBlock::StaleLineage);
    }
    let Some(plan) = work.observation_plan.clone() else {
        return Ok(None);
    };
    plan.validate()
        .map_err(|_| ObservationBlock::UnsupportedSource)?;
    Ok(Some(ObservationWake {
        entrusted_work_id: work.work_id.clone(),
        owner_session_id: work.owner_session_id.clone(),
        goal_id: goal.id.clone(),
        condition_revision: work.current_revision,
        goal_transition_id: latest_transition_id(&goal)
            .map_err(|_| ObservationBlock::StaleLineage)?,
        due_at_ms: plan.first_wake_at_ms,
        plan,
    }))
}

pub(super) fn check_wake(
    session: &Session,
    wake: &ObservationWake,
) -> Result<(), ObservationBlock> {
    let mut current =
        active_wake(session, &wake.entrusted_work_id)?.ok_or(ObservationBlock::StaleLineage)?;
    current.due_at_ms = wake.due_at_ms;
    if &current == wake {
        Ok(())
    } else {
        Err(ObservationBlock::StaleLineage)
    }
}
