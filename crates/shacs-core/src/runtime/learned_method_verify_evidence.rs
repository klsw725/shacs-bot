use super::learned_method::{
    EvidenceKind, LearnedMethodVersion, RunOutcome, StepCheck, StepSupport, TaskCheck, TaskIdentity,
};
use super::learned_method_decisions::{guidance, step_tools};
use super::learned_method_evidence::Evidence;
use super::learned_method_guard::quotes;
use super::learned_method_verify::StepReply;
use std::collections::BTreeSet;

/// The supported steps the model's `claims` show redone with evidence, and
/// the numbers of the supported steps they do not.
pub(super) fn redone_steps(
    version: &LearnedMethodVersion,
    claims: &[StepReply],
    items: &[Evidence],
) -> (Vec<StepCheck>, Vec<usize>) {
    let mut redone = Vec::new();
    let mut missing = Vec::new();
    for (index, step) in version.steps.iter().enumerate() {
        let number = index + 1;
        let claimed: Vec<String> = claims
            .iter()
            .filter(|claim| claim.step == number)
            .flat_map(|claim| claim.evidence.iter().cloned())
            .collect();
        let ids = match step.support {
            StepSupport::Unverified => continue,
            StepSupport::UserStated | StepSupport::UserEdited => cited(items, &claimed, |item| {
                guidance(&item.record) && quotes(&step.text, &item.text)
            }),
            StepSupport::ObservedRun => {
                let tools = step_tools(step, version);
                let ids = cited(items, &claimed, |item| {
                    item.record.kind == EvidenceKind::ToolCall
                        && item.record.outcome == Some(RunOutcome::Ok)
                        && item
                            .record
                            .tool
                            .as_deref()
                            .is_some_and(|tool| tools.contains(&tool))
                });
                let ran: BTreeSet<&str> = items
                    .iter()
                    .filter(|item| ids.contains(&item.record.id))
                    .filter_map(|item| item.record.tool.as_deref())
                    .collect();
                if tools.iter().all(|tool| ran.contains(tool)) {
                    ids
                } else {
                    Vec::new()
                }
            }
        };
        if ids.is_empty() {
            missing.push(number);
        } else {
            redone.push(StepCheck {
                step: number,
                evidence: ids,
            });
        }
    }
    (redone, missing)
}

/// The check of the task `task` that cites `steps`, with the evidence they
/// cite.
pub(super) fn with_evidence(
    task: TaskIdentity,
    steps: Vec<StepCheck>,
    items: &[Evidence],
) -> TaskCheck {
    let ids: BTreeSet<&String> = steps.iter().flat_map(|step| step.evidence.iter()).collect();
    TaskCheck {
        task,
        evidence: items
            .iter()
            .filter(|item| ids.contains(&item.record.id))
            .map(|item| item.record.clone())
            .collect(),
        steps,
    }
}

/// The ids among `claimed` of entries that exist and pass `accepts`.
pub(super) fn cited(
    items: &[Evidence],
    claimed: &[String],
    accepts: impl Fn(&Evidence) -> bool,
) -> Vec<String> {
    items
        .iter()
        .filter(|item| claimed.contains(&item.record.id) && accepts(item))
        .map(|item| item.record.id.clone())
        .collect()
}
