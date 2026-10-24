use super::learned_method::{
    CitedText, EvidenceKind, ExcludedEvidence, LearnedMethodVersion, MethodEvidence, MethodStep,
    OriginKind, RunOutcome, StepSupport, UserStatement, VersionOrigin,
};
use super::learned_method_check::Checker;
use super::learned_method_draft::{CitedDraft, Draft};
use super::learned_method_evidence::{bounded, Evidence, EvidenceSet};
use super::learned_method_generate::{shown_text, Proposal};
use super::learned_method_guard::quotes;
use super::learned_method_tasks::task_identity;
use std::collections::BTreeSet;

const MAX_STEPS: usize = 12;
const MAX_LIST_ITEMS: usize = 8;
const STEP_CHARS: usize = 400;
const FIELD_CHARS: usize = 300;
const PREMISE_NOTE: &str = "Premise not in your own words: ";
pub(super) const FAILED_RUN_NOTE: &str = " comes from a run that failed or did not finish.";
const UNKNOWN_NOTE: &str = " cited evidence id(s) did not exist and were ignored.";
const DROPPED_NOTE: &str = " proposed text(s) were dropped: they repeated or cited material kept from the model, or copied your request.";

/// Support for every line is computed from what it cites, never taken from
/// the model.
pub(super) fn build(draft: Draft, evidence: EvidenceSet, now: String) -> Proposal {
    let Some(candidate) = draft.candidate else {
        let reason = bounded(draft.reason.trim(), FIELD_CHARS);
        return Proposal::NoCandidate(if reason.is_empty() {
            "the evidence shows no reusable steps".to_owned()
        } else {
            reason
        });
    };
    let mut checker = Checker::new(&evidence.items);
    let mut cited_ids = BTreeSet::new();
    let mut framing = |draft: &CitedDraft, checker: &mut Checker<'_>| {
        let (text, cited) = checker.check(draft, FIELD_CHARS)?;
        let cited: Vec<String> = cited
            .iter()
            .filter(|item| {
                shown_text(item).is_some() && item.record.kind == EvidenceKind::UserMessage
            })
            .map(|item| item.record.id.clone())
            .collect();
        (!cited.is_empty()).then(|| {
            cited_ids.extend(cited.iter().cloned());
            CitedText {
                text,
                evidence: cited,
                edited_in: None,
            }
        })
    };
    let (Some(purpose), Some(applies_to)) = (
        framing(&candidate.purpose, &mut checker),
        framing(&candidate.applies_to, &mut checker),
    ) else {
        return Proposal::Failed(
            "the draft's purpose or the tasks it applies to were missing, not tied to your own messages, or copied text it may not keep"
                .to_owned(),
        );
    };
    let mut unverified = Vec::new();
    let mut premises = Vec::new();
    for draft in candidate.premises.iter().take(MAX_LIST_ITEMS) {
        let Some((text, cited)) = checker.check(draft, FIELD_CHARS) else {
            continue;
        };
        let quoted: Vec<String> = cited
            .iter()
            .filter(|item| guidance(item) && quotes(&text, &item.text))
            .map(|item| item.record.id.clone())
            .collect();
        if quoted.is_empty() {
            unverified.push(format!("{PREMISE_NOTE}{text}"));
        } else {
            cited_ids.extend(quoted.iter().cloned());
            premises.push(CitedText {
                text,
                evidence: quoted,
                edited_in: None,
            });
        }
    }
    let mut steps = Vec::new();
    for draft in candidate.steps.iter().take(MAX_STEPS) {
        let Some((text, cited)) = checker.check(draft, STEP_CHARS) else {
            continue;
        };
        let ids = |items: Vec<&&Evidence>| -> Vec<String> {
            items.iter().map(|item| item.record.id.clone()).collect()
        };
        let quoted = ids(cited
            .iter()
            .filter(|item| guidance(item) && quotes(&text, &item.text))
            .collect());
        let runs: Vec<&&Evidence> = cited
            .iter()
            .filter(|item| item.record.outcome == Some(RunOutcome::Ok))
            .collect();
        // A supported step keeps only the evidence that supports it, so its
        // label never vouches for the other entries the model cited.
        let step = if !quoted.is_empty() {
            MethodStep {
                text,
                evidence: quoted,
                support: StepSupport::UserStated,
                model_note: None,
                edited_in: None,
            }
        } else if !runs.is_empty() {
            MethodStep {
                text: run_label(runs.iter().map(|item| &item.record)),
                evidence: ids(runs),
                support: StepSupport::ObservedRun,
                model_note: Some(text),
                edited_in: None,
            }
        } else {
            if cited.iter().any(|item| {
                matches!(
                    item.record.outcome,
                    Some(RunOutcome::Error | RunOutcome::Incomplete)
                )
            }) {
                unverified.push(format!("Step {}{FAILED_RUN_NOTE}", steps.len() + 1));
            }
            MethodStep {
                text,
                evidence: ids(cited.iter().collect()),
                support: StepSupport::Unverified,
                model_note: None,
                edited_in: None,
            }
        };
        cited_ids.extend(step.evidence.iter().cloned());
        steps.push(step);
    }
    if steps
        .iter()
        .all(|step| step.support == StepSupport::Unverified)
    {
        return Proposal::NoCandidate(
            "no proposed step repeats your own words or reuses a run that went through".to_owned(),
        );
    }
    for item in candidate.unverified.iter().take(MAX_LIST_ITEMS) {
        if let Some((text, _)) = checker.check(
            &CitedDraft {
                text: item.clone(),
                evidence: Vec::new(),
            },
            FIELD_CHARS,
        ) {
            unverified.push(text);
        }
    }
    if checker.unknown_citations > 0 {
        unverified.push(format!("{}{UNKNOWN_NOTE}", checker.unknown_citations));
    }
    if checker.kept_out > 0 {
        unverified.push(format!("{}{DROPPED_NOTE}", checker.kept_out));
    }
    let excluded = evidence
        .items
        .iter()
        .filter_map(|item| {
            let kind = item
                .record
                .classified_as
                .filter(|statement| statement.excluded())?;
            Some(ExcludedEvidence {
                evidence: item.record.id.clone(),
                kind,
            })
        })
        .collect();
    let task = task_identity(&evidence.items);
    let records: Vec<MethodEvidence> = evidence
        .items
        .into_iter()
        .map(|item| item.record)
        .filter(|record| {
            cited_ids.contains(&record.id)
                || record
                    .classified_as
                    .is_some_and(|statement| statement != UserStatement::Other)
        })
        .collect();
    Proposal::Candidate(Box::new(LearnedMethodVersion {
        version: 1,
        created_at: now,
        origin: VersionOrigin {
            kind: OriginKind::ToolLessGeneration,
            session: evidence.session,
            omitted_evidence: evidence.omitted,
            task,
            edited_from: None,
        },
        purpose,
        applies_to,
        premises,
        steps,
        unverified,
        excluded,
        evidence: records,
    }))
}

/// The model's words in an `unverified` entry; the runtime's own notes have
/// none.
pub(super) fn model_words(item: &str) -> Option<&str> {
    if let Some(text) = item.strip_prefix(PREMISE_NOTE) {
        return Some(text);
    }
    let count = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let note = item
        .strip_prefix("Step ")
        .and_then(|rest| rest.strip_suffix(FAILED_RUN_NOTE))
        .is_some_and(count)
        || [UNKNOWN_NOTE, DROPPED_NOTE]
            .iter()
            .any(|tail| item.strip_suffix(tail).is_some_and(count));
    (!note).then_some(item)
}

/// The text of a step backed by `runs`: the runtime's, each tool once.
pub(super) fn run_label<'a>(runs: impl IntoIterator<Item = &'a MethodEvidence>) -> String {
    let tools: BTreeSet<&str> = runs
        .into_iter()
        .filter_map(|record| record.tool.as_deref())
        .collect();
    bounded(
        &format!("Run {}", tools.into_iter().collect::<Vec<_>>().join(", ")),
        STEP_CHARS,
    )
}

/// The user's feedback and procedures: the only words a step or premise may
/// quote as the user's. A request is task-specific and never quoted.
fn guidance(item: &Evidence) -> bool {
    shown_text(item).is_some()
        && matches!(
            item.record.classified_as,
            Some(UserStatement::Feedback | UserStatement::Procedure)
        )
}
