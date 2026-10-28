use super::learned_method::{
    DecisionKind, LearnedMethod, LearnedMethodState, MethodDecision, StepCheck, StepSupport,
};
use super::learned_method_decide::{deciding, replay};
use super::learned_method_render::Checked;
use super::learned_method_render_evidence::{evidence, since};

pub(super) const NO_GRANT: &str = "Confirming grants no permission: commands, Python, plugins and sending still ask or stay blocked as before.";

/// The state of a method, with the decision that put it there.
pub(super) fn state_label(method: &LearnedMethod) -> String {
    label(method.state, deciding(method))
}

/// The state version `number` of the method is in by its own decisions.
pub(super) fn version_label(method: &LearnedMethod, number: u32) -> String {
    let replayed = replay(&method.decisions, number);
    label(replayed.state, replayed.by)
}

fn label(state: LearnedMethodState, by: Option<&MethodDecision>) -> String {
    let at = by.map_or("", |decision| decision.at.as_str());
    match (state, by.map(|decision| decision.kind)) {
        (LearnedMethodState::Candidate, _) => "candidate (inactive; not used by default)".to_owned(),
        (LearnedMethodState::Confirmed, Some(DecisionKind::Verified)) => format!(
            "confirmed: redone in a second, different task on {at} (may be offered as the default for its tasks)"
        ),
        (LearnedMethodState::Confirmed, _) => {
            format!("confirmed by you on {at} (may be offered as the default for its tasks)")
        }
        (LearnedMethodState::Rejected, _) => format!("rejected by you on {at} (never offered)"),
        (LearnedMethodState::Inactive, Some(DecisionKind::Deactivated)) => format!(
            "inactive: you put it out of use on {at}; confirm it again to use it (not offered)"
        ),
        (LearnedMethodState::Inactive, _) => format!(
            "inactive: your feedback contradicted it on {at}; confirm it again to use it (not offered)"
        ),
    }
}

/// Every decision about the method, with the evidence of each check and how
/// its session stands now.
pub(super) fn decisions(checked: &Checked) -> Vec<String> {
    let method = &checked.method;
    if method.decisions.is_empty() {
        return vec!["- none yet".to_owned()];
    }
    let mut lines = Vec::new();
    for (index, decision) in method.decisions.iter().enumerate() {
        let session = decision.session.get(7..19).unwrap_or_default();
        let at = format!(
            "v{} on {} in session {session}",
            decision.version, decision.at
        );
        let steps = decision
            .check
            .as_ref()
            .map(|check| steps(&check.steps))
            .unwrap_or_default();
        let missed = decision
            .check
            .as_ref()
            .and_then(|check| not_redone(method, decision.version, &check.steps))
            .map(|missed| format!("; not redone: {missed}"))
            .unwrap_or_default();
        lines.push(match decision.kind {
            DecisionKind::Confirmed => format!(
                "- confirmed by you: {at} (review {})",
                decision.review.as_deref().unwrap_or_default()
            ),
            DecisionKind::Rejected => format!("- rejected by you: {at}"),
            DecisionKind::Verified => format!("- redone in a different task: {at}; {steps}"),
            DecisionKind::Conflicted => {
                format!("- contradicted by your feedback: {at}; {steps}")
            }
            DecisionKind::Repeated => format!(
                "- rerun of a task already recorded, kept as stability evidence only: {at}; {steps}{missed}"
            ),
            DecisionKind::Edited => format!(
                "- edited by you: {at}, made from v{}",
                method
                    .versions
                    .iter()
                    .find(|version| version.version == decision.version)
                    .and_then(|version| version.origin.edited_from)
                    .unwrap_or_default()
            ),
            DecisionKind::Deactivated => format!("- put out of use by you: {at}"),
            DecisionKind::Restored => format!(
                "- made current again by you: {at}; requests answered before keep the version they used, and what they did is not undone"
            ),
        });
        if let Some(check) = &decision.check {
            let seen = checked.checks.get(&index);
            lines.extend(check.evidence.iter().map(|record| {
                let now = since(seen.and_then(|seen| seen.get(&record.id)));
                format!("  {}{now}", evidence(record))
            }));
        }
    }
    lines
}

/// The supported steps of `version` that `redone` leaves out, if any.
fn not_redone(method: &LearnedMethod, version: u32, redone: &[StepCheck]) -> Option<String> {
    let steps = &method
        .versions
        .iter()
        .find(|candidate| candidate.version == version)?
        .steps;
    let missed: Vec<String> = steps
        .iter()
        .enumerate()
        .filter(|(index, step)| {
            step.support != StepSupport::Unverified
                && !redone.iter().any(|check| check.step == index + 1)
        })
        .map(|(index, _)| format!("step {}", index + 1))
        .collect();
    (!missed.is_empty()).then(|| missed.join(", "))
}

/// A note for a method confirmed by a later task whose evidence is no
/// longer in its session, so the confirmation cannot be rechecked.
pub(super) fn basis_note(checked: &Checked) -> &'static str {
    let method = &checked.method;
    let Some(decision) =
        deciding(method).filter(|decision| decision.kind == DecisionKind::Verified)
    else {
        return "";
    };
    let index = method
        .decisions
        .iter()
        .position(|other| std::ptr::eq(other, decision));
    let seen = index.and_then(|index| checked.checks.get(&index));
    let rechecked = decision.check.as_ref().is_none_or(|check| {
        check
            .evidence
            .iter()
            .all(|record| seen.is_some_and(|seen| seen.contains_key(&record.id)))
    });
    if rechecked {
        ""
    } else {
        " (the task it was redone in is no longer in its session, not rechecked)"
    }
}

fn steps(steps: &[StepCheck]) -> String {
    steps
        .iter()
        .map(|step| format!("step {} by {}", step.step, step.evidence.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The outcome of a check of this task that was saved.
pub(super) fn task_checked(method: &LearnedMethod, kind: DecisionKind, reason: &str) -> String {
    let id = &method.method_id;
    let v = method.current_version;
    let by_this = match (deciding(method), method.decisions.last()) {
        (Some(deciding), Some(last)) => std::ptr::eq(deciding, last),
        _ => false,
    };
    match kind {
        DecisionKind::Repeated => {
            let steps = method
                .decisions
                .last()
                .and_then(|decision| decision.check.as_ref())
                .map(|check| {
                    let missed = not_redone(method, v, &check.steps)
                        .map(|missed| format!("; not redone: {missed}"))
                        .unwrap_or_default();
                    format!("{}{missed}", steps(&check.steps))
                })
                .unwrap_or_default();
            format!(
                "This task is not counted as another task for {id} v{v}: {reason}. What it redid was kept as stability evidence only ({steps}). {id} stays {}.",
                state_label(method)
            )
        }
        DecisionKind::Conflicted => format!(
            "Your feedback in this task contradicts {id} v{v}; recorded. {}",
            if method.state == LearnedMethodState::Inactive {
                format!("{id} is now out of use until you confirm it again.")
            } else {
                format!("{id} will not be confirmed by later checks; you can still confirm it yourself.")
            }
        ),
        _ if by_this && method.state == LearnedMethodState::Confirmed => format!(
            "Checked {id} v{v} against this task: every step backed by your words or a run was redone here, and the model judged that the task fits ({reason}). This is a second, different task, so {id} v{v} is now confirmed: it may be offered as the default for its tasks. {NO_GRANT}"
        ),
        _ => format!(
            "Checked {id} v{v} against this task: every step backed by your words or a run was redone here, and the model judged that the task fits ({reason}). Recorded; {id} stays {}.",
            state_label(method)
        ),
    }
}

pub(super) fn not_counted(method_id: &str, reason: &str) -> String {
    format!("This task was not counted for {method_id}: {reason}. Nothing was saved.")
}

pub(super) fn check_failed(reason: &str) -> String {
    format!("Checking did not finish: {reason}. Nothing was saved.")
}
