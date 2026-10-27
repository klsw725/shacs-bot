use std::path::Path;

use shacs_session::{Session, SessionManager, SessionMutationGuard};

use super::entrusted_work_access::{
    bound_current_work, latest_transition_id, snapshot, work_snapshot_from_session,
};
use super::entrusted_work_store::{entrusted_works_from_session, store_entrusted_works, work_id};
use super::goal::persistent_goal_result_from_session;
use super::{
    clear_goal, create_persistent_goal, mark_goal_done, pause_goal, resume_goal,
    store_persistent_goal, EntrustedWork, EntrustedWorkConditions, EntrustedWorkCreateRequest,
    EntrustedWorkError, EntrustedWorkLifecycleAction, EntrustedWorkLifecycleRequest,
    EntrustedWorkReadRequest, EntrustedWorkReviseRequest, EntrustedWorkRevision,
    EntrustedWorkSnapshot, PersistentGoal,
};

pub fn create_entrusted_work(
    workspace: &Path,
    request: EntrustedWorkCreateRequest,
) -> Result<EntrustedWorkSnapshot, EntrustedWorkError> {
    validate_create(&request)?;
    let _guard = SessionMutationGuard::acquire(workspace, &request.owner_session_id)?;
    let (mut manager, mut session) = load_owner_session(workspace, &request.owner_session_id)?;
    let (work, goal) = create_entrusted_work_in(&mut session, request)?;
    manager.save_with_fsync(&session)?;
    snapshot(work, &goal)
}

/// Adds a new work and its goal to the loaded owner `session` without saving it, so a caller
/// can save it once together with other settings.
pub(super) fn create_entrusted_work_in(
    session: &mut Session,
    request: EntrustedWorkCreateRequest,
) -> Result<(EntrustedWork, PersistentGoal), EntrustedWorkError> {
    validate_create(&request)?;
    let previous_goal =
        persistent_goal_result_from_session(session).map_err(EntrustedWorkError::GoalMetadata)?;
    if previous_goal
        .as_ref()
        .is_some_and(|goal| !goal.is_terminal())
    {
        return Err(EntrustedWorkError::GoalAlreadyActive);
    }
    let mut goal = create_persistent_goal(
        &request.owner_session_id,
        &request.goal_text,
        &request.observed_at,
        request.turn_budget,
    );
    if let Some(previous_goal) = previous_goal {
        let first_transition = goal.last_transition.iter().cloned();
        goal.transitions = previous_goal.transitions;
        goal.transitions.extend(first_transition);
    }
    let transition_id = latest_transition_id(&goal)?;
    let work = EntrustedWork {
        work_id: work_id(&goal.id),
        owner_session_id: request.owner_session_id,
        linked_targets: request.linked_targets.clone(),
        goal_id: goal.id.clone(),
        consent: request.consent,
        current_revision: 1,
        revisions: vec![EntrustedWorkRevision {
            revision: 1,
            conditions: request.conditions,
            linked_targets: request.linked_targets,
            changed_at: request.observed_at,
        }],
        goal_transition_ids: vec![transition_id],
        observation_plan: None,
    };
    let mut works = entrusted_works_from_session(session).map_err(EntrustedWorkError::Metadata)?;
    if works.iter().any(|stored| stored.work_id == work.work_id) {
        return Err(EntrustedWorkError::WorkAlreadyExists);
    }
    works.push(work.clone());
    store_persistent_goal(session, &goal).map_err(EntrustedWorkError::GoalMetadata)?;
    store_entrusted_works(session, &works).map_err(EntrustedWorkError::Metadata)?;
    Ok((work, goal))
}

pub fn read_entrusted_work(
    workspace: &Path,
    request: &EntrustedWorkReadRequest,
) -> Result<EntrustedWorkSnapshot, EntrustedWorkError> {
    let (_, session) = load_owner_session(workspace, &request.owner_session_id)?;
    work_snapshot_from_session(&session, &request.requester_target, &request.work_id)
}

pub fn revise_entrusted_work(
    workspace: &Path,
    request: EntrustedWorkReviseRequest,
) -> Result<EntrustedWorkSnapshot, EntrustedWorkError> {
    validate_conditions(&request.conditions)?;
    if request
        .linked_targets
        .as_ref()
        .is_some_and(|targets| targets.iter().any(|target| target.trim().is_empty()))
    {
        return Err(EntrustedWorkError::InvalidInput("linked target"));
    }
    let _guard = SessionMutationGuard::acquire(workspace, &request.owner_session_id)?;
    let (mut manager, mut session) = load_owner_session(workspace, &request.owner_session_id)?;
    let (mut works, work_index, goal) =
        bound_current_work(&session, &request.requester_target, &request.work_id)?;
    let work = works
        .get_mut(work_index)
        .ok_or(EntrustedWorkError::WorkMissing)?;
    if work.current_revision != request.expected_revision {
        return Err(EntrustedWorkError::RevisionConflict {
            expected: request.expected_revision,
            actual: work.current_revision,
        });
    }
    let revision = work
        .current_revision
        .checked_add(1)
        .ok_or(EntrustedWorkError::InvalidInput("revision"))?;
    work.current_revision = revision;
    if let Some(targets) = request.linked_targets {
        work.linked_targets = targets;
    }
    work.revisions.push(EntrustedWorkRevision {
        revision,
        conditions: request.conditions,
        linked_targets: work.linked_targets.clone(),
        changed_at: request.changed_at,
    });
    let snapshot_work = work.clone();
    store_entrusted_works(&mut session, &works).map_err(EntrustedWorkError::Metadata)?;
    manager.save_with_fsync(&session)?;
    snapshot(snapshot_work, &goal)
}

pub fn transition_entrusted_work(
    workspace: &Path,
    request: EntrustedWorkLifecycleRequest,
) -> Result<EntrustedWorkSnapshot, EntrustedWorkError> {
    let _guard = SessionMutationGuard::acquire(workspace, &request.owner_session_id)?;
    let (mut manager, mut session) = load_owner_session(workspace, &request.owner_session_id)?;
    let (mut works, work_index, goal) =
        bound_current_work(&session, &request.requester_target, &request.work_id)?;
    let next_goal = match request.action {
        EntrustedWorkLifecycleAction::Pause => pause_goal(&goal, &request.observed_at),
        EntrustedWorkLifecycleAction::Resume => resume_goal(&goal, &request.observed_at),
        EntrustedWorkLifecycleAction::Done => mark_goal_done(&goal, &request.observed_at),
        EntrustedWorkLifecycleAction::Clear => clear_goal(&goal, &request.observed_at),
    }
    .map_err(EntrustedWorkError::GoalTransition)?;
    let work = works
        .get_mut(work_index)
        .ok_or(EntrustedWorkError::WorkMissing)?;
    work.goal_transition_ids
        .push(latest_transition_id(&next_goal)?);
    let snapshot_work = work.clone();
    persist(&mut manager, &mut session, &next_goal, &works)?;
    snapshot(snapshot_work, &next_goal)
}

pub(super) fn load_owner_session(
    workspace: &Path,
    owner_session_id: &str,
) -> Result<(SessionManager, Session), EntrustedWorkError> {
    let manager =
        SessionManager::open_existing(workspace)?.ok_or(EntrustedWorkError::SessionMissing)?;
    let session = manager
        .load_existing(owner_session_id)
        .ok_or(EntrustedWorkError::SessionMissing)?;
    Ok((manager, session))
}

fn persist(
    manager: &mut SessionManager,
    session: &mut Session,
    goal: &PersistentGoal,
    works: &[EntrustedWork],
) -> Result<(), EntrustedWorkError> {
    store_persistent_goal(session, goal).map_err(EntrustedWorkError::GoalMetadata)?;
    store_entrusted_works(session, works).map_err(EntrustedWorkError::Metadata)?;
    manager.save_with_fsync(session)?;
    Ok(())
}

fn validate_create(request: &EntrustedWorkCreateRequest) -> Result<(), EntrustedWorkError> {
    if request.owner_session_id.trim().is_empty() {
        return Err(EntrustedWorkError::InvalidInput("owner session"));
    }
    if request.goal_text.trim().is_empty() {
        return Err(EntrustedWorkError::InvalidInput("goal text"));
    }
    if request.consent.evidence_ref.trim().is_empty() {
        return Err(EntrustedWorkError::InvalidInput("consent evidence"));
    }
    if request.consent.recorded_at.trim().is_empty() || request.observed_at.trim().is_empty() {
        return Err(EntrustedWorkError::InvalidInput("timestamp"));
    }
    if request
        .linked_targets
        .iter()
        .any(|target| target.trim().is_empty())
    {
        return Err(EntrustedWorkError::InvalidInput("linked target"));
    }
    validate_conditions(&request.conditions)
}

fn validate_conditions(conditions: &EntrustedWorkConditions) -> Result<(), EntrustedWorkError> {
    if conditions.instructions.trim().is_empty() {
        return Err(EntrustedWorkError::InvalidInput("conditions"));
    }
    Ok(())
}
