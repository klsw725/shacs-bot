use super::{
    clear_goal, create_persistent_goal, mark_goal_blocked, mark_goal_done, pause_goal,
    persistent_goal_from_session, resume_goal, store_persistent_goal, GoalCompletionVerdict,
    GoalMetadataError, GoalTransitionError, PersistentGoal, PersistentGoalStatus,
    GOAL_EVALUATOR_BOUNDARY_METADATA_KEY,
};
use shacs_projection::{
    Spec033Availability, Spec033EvidenceLineage, Spec033EvidenceSource, Spec033GoalFact,
    Spec033GoalOwner, Spec033GoalStatus, Spec033GoalUsageSummary, Spec033GoalVerdict, Spec033Owner,
    Spec033OwnerFact, Spec033Snapshot,
};
use shacs_session::{Session, SessionManager, SessionMutationGuard};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalSurfaceAction {
    Set { text: String, turn_budget: u32 },
    Pause,
    Resume,
    Clear,
    Done,
    Blocked { reason: String },
}

#[derive(Debug)]
pub enum GoalSurfaceError {
    Io(std::io::Error),
    SessionMissing,
    GoalMissing,
    GoalAlreadyActive,
    Metadata(GoalMetadataError),
    Transition(GoalTransitionError),
}

impl std::fmt::Display for GoalSurfaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "goal session I/O failed: {error}"),
            Self::SessionMissing => write!(formatter, "goal session does not exist"),
            Self::GoalMissing => write!(formatter, "persistent goal is not set"),
            Self::GoalAlreadyActive => write!(formatter, "a persistent goal is already active"),
            Self::Metadata(error) => write!(formatter, "goal metadata failed: {error}"),
            Self::Transition(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for GoalSurfaceError {}

impl From<std::io::Error> for GoalSurfaceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn build_spec033_snapshot(
    workspace: &Path,
    session_id: &str,
) -> Result<Spec033Snapshot, GoalSurfaceError> {
    build_spec033_snapshot_from(workspace, workspace, session_id)
}

pub fn build_spec033_snapshot_from(
    workspace: &Path,
    data_dir: &Path,
    session_id: &str,
) -> Result<Spec033Snapshot, GoalSurfaceError> {
    let session = SessionManager::open_existing(workspace)?
        .and_then(|manager| manager.load_existing(session_id));
    let mut snapshot = session.map_or_else(
        || Spec033Snapshot::unavailable(session_id),
        |session| snapshot_from_session(&session),
    );
    super::spec033_projection::populate_durable_facts(
        &mut snapshot,
        workspace,
        data_dir,
        session_id,
    )?;
    Ok(snapshot)
}

pub fn apply_goal_surface_action(
    workspace: &Path,
    session_id: &str,
    action: GoalSurfaceAction,
    observed_at: &str,
) -> Result<Spec033Snapshot, GoalSurfaceError> {
    let _mutation_guard = SessionMutationGuard::acquire(workspace, session_id)?;
    let mut manager =
        SessionManager::open_existing(workspace)?.ok_or(GoalSurfaceError::SessionMissing)?;
    let mut session = manager
        .load_existing(session_id)
        .ok_or(GoalSurfaceError::SessionMissing)?;
    let next = transition_goal(&session, action, observed_at)?;
    store_persistent_goal(&mut session, &next).map_err(GoalSurfaceError::Metadata)?;
    manager.save(&session)?;
    build_spec033_snapshot(workspace, session_id)
}

fn transition_goal(
    session: &Session,
    action: GoalSurfaceAction,
    observed_at: &str,
) -> Result<PersistentGoal, GoalSurfaceError> {
    match action {
        GoalSurfaceAction::Set { text, turn_budget } => {
            if persistent_goal_from_session(session).is_some_and(|goal| !goal.is_terminal()) {
                return Err(GoalSurfaceError::GoalAlreadyActive);
            }
            let mut next = create_persistent_goal(&session.key, text, observed_at, turn_budget);
            if let Some(previous) = persistent_goal_from_session(session) {
                next.transitions = previous.transitions;
                next.transitions
                    .extend(next.last_transition.iter().cloned());
            }
            Ok(next)
        }
        GoalSurfaceAction::Pause => with_goal(session, |goal| pause_goal(goal, observed_at)),
        GoalSurfaceAction::Resume => with_goal(session, |goal| resume_goal(goal, observed_at)),
        GoalSurfaceAction::Clear => with_goal(session, |goal| clear_goal(goal, observed_at)),
        GoalSurfaceAction::Done => with_goal(session, |goal| mark_goal_done(goal, observed_at)),
        GoalSurfaceAction::Blocked { reason } => {
            with_goal(session, |goal| mark_goal_blocked(goal, reason, observed_at))
        }
    }
}

fn with_goal(
    session: &Session,
    transition: impl FnOnce(&PersistentGoal) -> Result<PersistentGoal, GoalTransitionError>,
) -> Result<PersistentGoal, GoalSurfaceError> {
    let goal = persistent_goal_from_session(session).ok_or(GoalSurfaceError::GoalMissing)?;
    transition(&goal).map_err(GoalSurfaceError::Transition)
}

fn snapshot_from_session(session: &Session) -> Spec033Snapshot {
    let mut snapshot = Spec033Snapshot::unavailable(&session.key);
    if let Some(goal) = persistent_goal_from_session(session) {
        snapshot.goal = goal_owner(goal);
        if let Some(goal) = snapshot.goal.fact.as_ref() {
            snapshot.diagnostics.goal_id =
                shacs_projection::Spec033DiagnosticLink::available(&goal.goal_id);
        }
    }
    if session
        .metadata
        .get(GOAL_EVALUATOR_BOUNDARY_METADATA_KEY)
        .and_then(serde_json::Value::as_array)
        .is_some_and(|facts| !facts.is_empty())
    {
        if let Some(fact) = super::spec033_projection::latest_evaluator_fact(session) {
            snapshot.evaluator = Spec033OwnerFact::available(
                Spec033Owner::Evaluator,
                Spec033EvidenceSource::SessionMetadata,
                fact,
                vec![format!(
                    "session_metadata:{GOAL_EVALUATOR_BOUNDARY_METADATA_KEY}"
                )],
            );
            if let Some(request_id) =
                super::spec033_projection::latest_evaluator_request_id(session)
            {
                snapshot.diagnostics.evaluator_request_id =
                    shacs_projection::Spec033DiagnosticLink::available(request_id);
            }
        }
    }
    snapshot
}

fn goal_owner(goal: PersistentGoal) -> Spec033GoalOwner {
    let remaining_turns = goal.turn_budget.saturating_sub(goal.turns_used);
    let latest_transition = goal.last_transition.as_ref().map(|transition| {
        shacs_projection::Spec033GoalTransitionFact {
            goal_id: transition.goal_id.clone(),
            prior_state: observed_state(transition.prior_state),
            current_state: observed_state(transition.current_state),
            stop_reason: stop_reason(transition.stop_reason),
            budget: shacs_projection::Spec033GoalBudgetFact {
                turn_budget: transition.budget.turn_budget,
                turns_used: transition.budget.turns_used,
                remaining_turns: transition.budget.remaining_turns,
            },
            user_interrupted: transition.user_interrupted,
            observed_at: transition.observed_at.clone(),
        }
    });
    Spec033GoalOwner {
        availability: Spec033Availability::Available,
        fact: Some(Spec033GoalFact {
            goal_id: goal.id,
            session_id: goal.session_id,
            status: goal_status(goal.status),
            turn_budget: goal.turn_budget,
            turns_used: goal.turns_used,
            last_verdict: goal.last_verdict.map(goal_verdict),
            blocked: goal.blocked_reason.is_some(),
            stop_reason: latest_transition
                .as_ref()
                .map(|transition| transition.stop_reason.clone()),
            budget: latest_transition.as_ref().map_or_else(
                || shacs_projection::Spec033GoalBudgetFact {
                    turn_budget: goal.turn_budget,
                    turns_used: goal.turns_used,
                    remaining_turns: goal.turn_budget.saturating_sub(goal.turns_used),
                },
                |transition| transition.budget.clone(),
            ),
            usage: Spec033GoalUsageSummary {
                turn_limit: goal.turn_budget,
                turns_used: goal.turns_used,
                remaining_turns,
                exhausted: remaining_turns == 0,
            },
            user_interrupted: latest_transition
                .as_ref()
                .is_some_and(|transition| transition.user_interrupted),
            latest_transition,
        }),
        lineage: Spec033EvidenceLineage::new(
            Spec033Owner::Goal,
            Spec033EvidenceSource::SessionMetadata,
            vec!["session_metadata:persistent_goal".to_owned()],
        ),
    }
}

fn observed_state(state: super::GoalObservedState) -> Spec033GoalStatus {
    match state {
        super::GoalObservedState::Unavailable => Spec033GoalStatus::Unavailable,
        super::GoalObservedState::Active => Spec033GoalStatus::Active,
        super::GoalObservedState::Paused => Spec033GoalStatus::Paused,
        super::GoalObservedState::Blocked => Spec033GoalStatus::Blocked,
        super::GoalObservedState::Done => Spec033GoalStatus::Done,
        super::GoalObservedState::Cleared => Spec033GoalStatus::Cleared,
    }
}

fn stop_reason(reason: super::GoalStopReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn goal_status(status: PersistentGoalStatus) -> Spec033GoalStatus {
    match status {
        PersistentGoalStatus::Active => Spec033GoalStatus::Active,
        PersistentGoalStatus::Paused => Spec033GoalStatus::Paused,
        PersistentGoalStatus::Blocked => Spec033GoalStatus::Blocked,
        PersistentGoalStatus::Done => Spec033GoalStatus::Done,
        PersistentGoalStatus::Cleared => Spec033GoalStatus::Cleared,
    }
}

fn goal_verdict(verdict: GoalCompletionVerdict) -> Spec033GoalVerdict {
    match verdict {
        GoalCompletionVerdict::Done => Spec033GoalVerdict::Done,
        GoalCompletionVerdict::Continue => Spec033GoalVerdict::Continue,
        GoalCompletionVerdict::Blocked => Spec033GoalVerdict::Blocked,
    }
}
