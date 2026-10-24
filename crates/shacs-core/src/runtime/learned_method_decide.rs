use super::learned_method::{
    DecisionKind, LearnedMethod, LearnedMethodState, MethodDecision, TaskCheck,
};
use super::learned_method_decisions::MAX_DECISIONS;
use super::learned_method_evidence::digest;
use super::learned_method_tasks::refused;

/// The state the decisions about the current version lead to. A user's
/// confirmation makes it confirmed; so does one check of a later, different
/// task that redid every supported step, but only while no feedback of the
/// user has contradicted the version. A contradiction, or the user putting
/// it out of use, takes a confirmed method out of use until the user
/// confirms it again. A rejection is final.
pub(super) fn derived_state(method: &LearnedMethod) -> LearnedMethodState {
    replay(&method.decisions, method.current_version).state
}

/// The decision that put the current version in its state, if any.
pub(super) fn deciding(method: &LearnedMethod) -> Option<&MethodDecision> {
    replay(&method.decisions, method.current_version).by
}

pub(super) struct Replayed<'a> {
    pub state: LearnedMethodState,
    pub by: Option<&'a MethodDecision>,
    /// Whether the version was confirmed at any point.
    pub ever_confirmed: bool,
}

/// What `decisions` about `version` lead to, in order.
pub(super) fn replay(decisions: &[MethodDecision], version: u32) -> Replayed<'_> {
    let mut replayed = Replayed {
        state: LearnedMethodState::Candidate,
        by: None,
        ever_confirmed: false,
    };
    let mut contradicted = false;
    for decision in decisions
        .iter()
        .filter(|decision| decision.version == version)
    {
        let state = replayed.state;
        let next = match (decision.kind, state) {
            (_, LearnedMethodState::Rejected) | (DecisionKind::Rejected, _) => {
                LearnedMethodState::Rejected
            }
            (DecisionKind::Confirmed, _) => LearnedMethodState::Confirmed,
            (DecisionKind::Conflicted, current) => {
                contradicted = true;
                if current == LearnedMethodState::Confirmed {
                    LearnedMethodState::Inactive
                } else {
                    current
                }
            }
            (DecisionKind::Deactivated, LearnedMethodState::Confirmed) => {
                LearnedMethodState::Inactive
            }
            (DecisionKind::Verified, LearnedMethodState::Candidate) if !contradicted => {
                LearnedMethodState::Confirmed
            }
            (
                DecisionKind::Verified
                | DecisionKind::Repeated
                | DecisionKind::Edited
                | DecisionKind::Deactivated
                | DecisionKind::Restored,
                current,
            ) => current,
        };
        if next != state {
            replayed.by = Some(decision);
        }
        replayed.ever_confirmed |= next == LearnedMethodState::Confirmed;
        replayed.state = next;
    }
    replayed
}

/// The code shown with a review: it changes whenever the method does, so a
/// confirmation always confirms exactly what was reviewed.
pub(super) fn review_code(method: &LearnedMethod) -> String {
    let version = serde_json::to_string(&method.current()).unwrap_or_default();
    let reviewed = format!(
        "{}\n{}\n{}\n{version}",
        method.method_id, method.current_version, method.revision
    );
    digest(&reviewed)[7..15].to_owned()
}

pub(super) fn confirm(
    method: &mut LearnedMethod,
    version: u32,
    code: &str,
    (session, now): (&str, String),
) -> Result<(), String> {
    let id = method.method_id.clone();
    match method.state {
        LearnedMethodState::Rejected => {
            return Err(format!(
                "{id} was rejected and stays rejected; nothing was changed."
            ))
        }
        LearnedMethodState::Confirmed => {
            return Err(format!("{id} is already confirmed; nothing was changed."))
        }
        LearnedMethodState::Candidate | LearnedMethodState::Inactive => {}
    }
    if version != method.current_version || code != review_code(method) {
        return Err(format!(
            "That is not the review of {id} as it is now; send /learn confirm {id} to review it again. Nothing was changed."
        ));
    }
    room(method, "nothing was changed")?;
    decide(
        method,
        DecisionKind::Confirmed,
        Some(code.to_owned()),
        session,
        now,
    );
    Ok(())
}

/// Saves the check of a later task, if the method is still the version it
/// was made against and the check may follow what is recorded about it now.
pub(super) fn record_check(
    method: &mut LearnedMethod,
    (version, kind, check): (u32, DecisionKind, TaskCheck),
    (session, now): (&str, String),
) -> Result<(), String> {
    let id = method.method_id.clone();
    if method.state == LearnedMethodState::Rejected || version != method.current_version {
        return Err(format!(
            "{id} changed while this task was being checked; nothing was saved."
        ));
    }
    let current = method
        .current()
        .ok_or_else(|| format!("{id} has no readable version."))?;
    let earlier: Vec<&MethodDecision> = method
        .decisions
        .iter()
        .filter(|decision| decision.version == version)
        .collect();
    if let Some(reason) = refused(current, &earlier, kind, &check) {
        return Err(format!(
            "This task was not counted for {id}: {reason}. Nothing was saved."
        ));
    }
    room(method, "nothing was saved")?;
    decide(method, kind, None, session, now);
    if let Some(decision) = method.decisions.last_mut() {
        decision.check = Some(check);
    }
    method.state = derived_state(method);
    Ok(())
}

pub(super) fn reject(
    method: &mut LearnedMethod,
    version: u32,
    (session, now): (&str, String),
) -> Result<(), String> {
    let id = method.method_id.clone();
    if method.state == LearnedMethodState::Rejected {
        return Err(format!("{id} is already rejected; nothing was changed."));
    }
    if version != method.current_version {
        return Err(format!(
            "{id} is at v{}, not v{version}; nothing was changed.",
            method.current_version
        ));
    }
    room(method, "nothing was changed")?;
    decide(method, DecisionKind::Rejected, None, session, now);
    Ok(())
}

/// Whether `method` can keep one more decision.
pub(super) fn room(method: &LearnedMethod, outcome: &str) -> Result<(), String> {
    if method.decisions.len() < MAX_DECISIONS {
        Ok(())
    } else {
        Err(format!(
            "{} already keeps the most decisions a method can ({MAX_DECISIONS}); {outcome}.",
            method.method_id
        ))
    }
}

/// Records a decision about the current version and the state it leads to.
pub(super) fn decide(
    method: &mut LearnedMethod,
    kind: DecisionKind,
    review: Option<String>,
    session: &str,
    now: String,
) {
    method.decisions.push(MethodDecision {
        kind,
        version: method.current_version,
        at: now,
        session: session.to_owned(),
        review,
        check: None,
    });
    method.state = derived_state(method);
}
