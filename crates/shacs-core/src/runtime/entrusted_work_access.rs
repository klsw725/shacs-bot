use shacs_session::Session;

use super::entrusted_work_store::entrusted_works_from_session;
use super::goal::persistent_goal_result_from_session;
use super::{
    EntrustedWork, EntrustedWorkError, EntrustedWorkSnapshot, GoalObservedState, PersistentGoal,
    PersistentGoalStatus,
};

pub(super) fn bound_current_work(
    session: &Session,
    requester_target: &str,
    requested_work_id: &str,
) -> Result<(Vec<EntrustedWork>, usize, PersistentGoal), EntrustedWorkError> {
    let works = entrusted_works_from_session(session).map_err(EntrustedWorkError::Metadata)?;
    let work_index = works
        .iter()
        .position(|work| work.work_id == requested_work_id)
        .ok_or(EntrustedWorkError::WorkMissing)?;
    let work = works
        .get(work_index)
        .ok_or(EntrustedWorkError::WorkMissing)?;
    if !work.is_accessible_from(requester_target) {
        return Err(EntrustedWorkError::AccessDenied);
    }
    let goal = persistent_goal_result_from_session(session)
        .map_err(EntrustedWorkError::GoalMetadata)?
        .ok_or(EntrustedWorkError::GoalBindingMismatch)?;
    if goal.id != work.goal_id || goal.session_id != work.owner_session_id {
        return Err(EntrustedWorkError::GoalBindingMismatch);
    }
    if work.goal_transition_ids.last() != Some(&latest_transition_id(&goal)?) {
        return Err(EntrustedWorkError::GoalBindingMismatch);
    }
    Ok((works, work_index, goal))
}

pub(super) fn work_snapshot_from_session(
    session: &Session,
    requester_target: &str,
    requested_work_id: &str,
) -> Result<EntrustedWorkSnapshot, EntrustedWorkError> {
    let works = entrusted_works_from_session(session).map_err(EntrustedWorkError::Metadata)?;
    let work = works
        .into_iter()
        .find(|work| work.work_id == requested_work_id)
        .ok_or(EntrustedWorkError::WorkMissing)?;
    if !work.is_accessible_from(requester_target) {
        return Err(EntrustedWorkError::AccessDenied);
    }
    let goal = persistent_goal_result_from_session(session)
        .map_err(EntrustedWorkError::GoalMetadata)?
        .ok_or(EntrustedWorkError::GoalBindingMismatch)?;
    if goal.id == work.goal_id && goal.session_id == work.owner_session_id {
        return snapshot(work, &goal);
    }
    let latest_id = work
        .goal_transition_ids
        .last()
        .cloned()
        .ok_or(EntrustedWorkError::MissingGoalTransitionId)?;
    let transition = goal
        .transitions
        .iter()
        .find(|transition| transition.transition_id.as_ref() == Some(&latest_id))
        .ok_or(EntrustedWorkError::GoalBindingMismatch)?;
    let goal_status = match transition.current_state {
        GoalObservedState::Active => PersistentGoalStatus::Active,
        GoalObservedState::Paused => PersistentGoalStatus::Paused,
        GoalObservedState::Blocked => PersistentGoalStatus::Blocked,
        GoalObservedState::Done => PersistentGoalStatus::Done,
        GoalObservedState::Cleared => PersistentGoalStatus::Cleared,
        GoalObservedState::Unavailable => return Err(EntrustedWorkError::GoalBindingMismatch),
    };
    Ok(EntrustedWorkSnapshot {
        work,
        goal_status,
        latest_goal_transition_id: latest_id,
    })
}

pub(super) fn snapshot(
    work: EntrustedWork,
    goal: &PersistentGoal,
) -> Result<EntrustedWorkSnapshot, EntrustedWorkError> {
    let transition_id = latest_transition_id(goal)?;
    if work.goal_transition_ids.last() != Some(&transition_id) {
        return Err(EntrustedWorkError::GoalBindingMismatch);
    }
    Ok(EntrustedWorkSnapshot {
        latest_goal_transition_id: transition_id,
        work,
        goal_status: goal.status,
    })
}

pub(super) fn latest_transition_id(goal: &PersistentGoal) -> Result<String, EntrustedWorkError> {
    goal.last_transition
        .as_ref()
        .and_then(|transition| transition.transition_id.clone())
        .ok_or(EntrustedWorkError::MissingGoalTransitionId)
}
