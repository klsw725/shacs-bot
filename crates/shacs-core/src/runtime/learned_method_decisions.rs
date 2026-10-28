use super::learned_method::{
    DecisionKind, EvidenceKind, LearnedMethod, LearnedMethodVersion, MethodDecision,
    MethodEvidence, MethodStep, RunOutcome, SourceTrust, StepCheck, StepSupport, TaskCheck,
    UserStatement,
};
use super::learned_method_decide::derived_state;
use super::learned_method_history::history_holds;
use super::learned_method_tasks::refused;
use super::learned_method_validate::{
    digest_form, identity_holds, record_holds, tiny, MAX_ENTRIES,
};
use std::collections::BTreeSet;

pub(super) const MAX_DECISIONS: usize = 64;

/// The decisions are well formed, tell one story of versions (see
/// `learned_method_history`), every check may follow the decisions before
/// it (see `learned_method_tasks::refused`), and the saved state is the one
/// they lead to.
pub(super) fn decisions_hold(method: &LearnedMethod) -> bool {
    method.decisions.len() <= MAX_DECISIONS
        && history_holds(method)
        && method.decisions.iter().all(|decision| {
            method
                .versions
                .iter()
                .find(|version| version.version == decision.version)
                .is_some_and(|version| decision_holds(decision, version))
        })
        && method.versions.iter().all(|version| {
            let decisions: Vec<&MethodDecision> = method
                .decisions
                .iter()
                .filter(|decision| decision.version == version.version)
                .collect();
            decisions.iter().enumerate().all(|(index, decision)| {
                decision.check.as_ref().is_none_or(|check| {
                    refused(version, &decisions[..index], decision.kind, check).is_none()
                })
            })
        })
        && method.state == derived_state(method)
}

fn decision_holds(decision: &MethodDecision, version: &LearnedMethodVersion) -> bool {
    let code = |code: &str| {
        code.len() == 8 && code.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    tiny(&decision.at)
        && digest_form(&decision.session)
        && match (decision.kind, &decision.review, &decision.check) {
            (DecisionKind::Confirmed, Some(review), None) => code(review),
            (
                DecisionKind::Rejected
                | DecisionKind::Edited
                | DecisionKind::Deactivated
                | DecisionKind::Restored,
                None,
                None,
            ) => true,
            (
                DecisionKind::Verified | DecisionKind::Conflicted | DecisionKind::Repeated,
                None,
                Some(check),
            ) => check_holds(check, decision, version),
            _ => false,
        }
}

fn check_holds(
    check: &TaskCheck,
    decision: &MethodDecision,
    version: &LearnedMethodVersion,
) -> bool {
    let ids: BTreeSet<&str> = check
        .evidence
        .iter()
        .map(|record| record.id.as_str())
        .collect();
    let numbers: BTreeSet<usize> = check.steps.iter().map(|step| step.step).collect();
    let cited = |ids: &[String]| -> Vec<&MethodEvidence> {
        check
            .evidence
            .iter()
            .filter(|record| ids.contains(&record.id))
            .collect()
    };
    !check.task.requests.is_empty()
        && identity_holds(&check.task)
        && check.evidence.len() <= MAX_ENTRIES
        && ids.len() == check.evidence.len()
        && check
            .evidence
            .iter()
            .all(|record| record_holds(record, &decision.session))
        && !check.steps.is_empty()
        && numbers.len() == check.steps.len()
        && check.steps.iter().all(|step| {
            (1..=version.steps.len()).contains(&step.step)
                && !step.evidence.is_empty()
                && step.evidence.iter().all(|id| ids.contains(id.as_str()))
        })
        && match decision.kind {
            DecisionKind::Verified => {
                check
                    .steps
                    .iter()
                    .all(|redone| redid(redone, version, &cited))
                    && version.steps.iter().enumerate().all(|(index, step)| {
                        step.support == StepSupport::Unverified || numbers.contains(&(index + 1))
                    })
            }
            DecisionKind::Repeated => check
                .steps
                .iter()
                .all(|redone| redid(redone, version, &cited)),
            DecisionKind::Conflicted => check
                .steps
                .iter()
                .all(|step| cited(&step.evidence).iter().all(|record| guidance(record))),
            DecisionKind::Confirmed
            | DecisionKind::Rejected
            | DecisionKind::Edited
            | DecisionKind::Deactivated
            | DecisionKind::Restored => false,
        }
}

/// Whether the evidence `redone` cites redoes its step: the user's own
/// words for a step in them, an ok run of each of its tools for a run step.
fn redid<'a>(
    redone: &StepCheck,
    version: &LearnedMethodVersion,
    cited: &dyn Fn(&[String]) -> Vec<&'a MethodEvidence>,
) -> bool {
    let Some(step) = redone
        .step
        .checked_sub(1)
        .and_then(|index| version.steps.get(index))
    else {
        return false;
    };
    let records = cited(&redone.evidence);
    match step.support {
        StepSupport::Unverified => false,
        StepSupport::UserStated | StepSupport::UserEdited => {
            records.iter().all(|record| guidance(record))
        }
        StepSupport::ObservedRun => {
            let tools = step_tools(step, version);
            records.iter().all(|record| {
                record.outcome == Some(RunOutcome::Ok)
                    && record
                        .tool
                        .as_deref()
                        .is_some_and(|tool| tools.contains(&tool))
            }) && tools.iter().all(|tool| {
                records
                    .iter()
                    .any(|record| record.tool.as_deref() == Some(*tool))
            })
        }
    }
}

/// The user's own feedback or procedure: the only words that redo or
/// contradict a step.
pub(super) fn guidance(record: &MethodEvidence) -> bool {
    record.kind == EvidenceKind::UserMessage
        && record.trust == SourceTrust::Owner
        && matches!(
            record.classified_as,
            Some(UserStatement::Feedback | UserStatement::Procedure)
        )
}

/// The tools a run step ran, as its "Run …" text names them.
pub(super) fn step_tools<'a>(step: &MethodStep, version: &'a LearnedMethodVersion) -> Vec<&'a str> {
    step.evidence
        .iter()
        .filter_map(|id| version.evidence.iter().find(|record| &record.id == id))
        .filter_map(|record| record.tool.as_deref())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
