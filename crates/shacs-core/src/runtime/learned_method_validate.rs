use super::learned_method::{
    CitedText, EvidenceKind, LearnedMethodVersion, MethodEvidence, RunOutcome, SourceTrust,
    StepSupport, TaskIdentity, UserStatement,
};
use super::learned_method_build::run_label;
use super::learned_method_source::ref_index;
use std::collections::BTreeSet;

const MAX_STEPS: usize = 12;
const MAX_PREMISES: usize = 8;
pub(super) const MAX_ENTRIES: usize = 40;
const MAX_TEXT_CHARS: usize = 1_000;
const MAX_REF_CHARS: usize = 200;

const SHOWN: [UserStatement; 3] = [
    UserStatement::Request,
    UserStatement::Feedback,
    UserStatement::Procedure,
];
const GUIDANCE: [UserStatement; 2] = [UserStatement::Feedback, UserStatement::Procedure];

/// Whether every citation points at evidence of the kind its label claims,
/// as the generator binds them, so an edited store cannot dress up evidence.
/// Text the user wrote with `/learn edit` cites nothing and belongs to a
/// later version.
pub(super) fn consistent(version: &LearnedMethodVersion) -> bool {
    let find = |id: &String| version.evidence.iter().find(|record| &record.id == id);
    let cites = |ids: &[String], allowed: &dyn Fn(&MethodEvidence) -> bool| {
        ids.iter().all(|id| find(id).is_some_and(allowed))
    };
    let said = |statements: &'static [UserStatement]| {
        move |record: &MethodEvidence| {
            record.kind == EvidenceKind::UserMessage
                && record.trust == SourceTrust::Owner
                && record
                    .classified_as
                    .is_some_and(|statement| statements.contains(&statement))
        }
    };
    let edit_holds = |edited_in: Option<u32>, evidence: &[String]| {
        edited_in.map(|made| evidence.is_empty() && (2..=version.version).contains(&made))
    };
    let framing = [&version.purpose, &version.applies_to];
    framing.iter().all(|text| {
        edit_holds(text.edited_in, &text.evidence)
            .unwrap_or_else(|| !text.evidence.is_empty() && cites(&text.evidence, &said(&SHOWN)))
    }) && version.premises.iter().all(|premise| {
        edit_holds(premise.edited_in, &premise.evidence).unwrap_or_else(|| {
            !premise.evidence.is_empty() && cites(&premise.evidence, &said(&GUIDANCE))
        })
    }) && version.steps.iter().all(|step| {
        let edited = edit_holds(step.edited_in, &step.evidence);
        match step.support {
            StepSupport::UserEdited => edited == Some(true) && step.model_note.is_none(),
            _ if edited.is_some() => false,
            StepSupport::UserStated => {
                !step.evidence.is_empty()
                    && step.model_note.is_none()
                    && cites(&step.evidence, &said(&GUIDANCE))
            }
            StepSupport::ObservedRun => {
                !step.evidence.is_empty()
                    && cites(&step.evidence, &|record| {
                        record.kind == EvidenceKind::ToolCall
                            && record.outcome == Some(RunOutcome::Ok)
                    })
                    && step.text == run_label(step.evidence.iter().filter_map(find))
            }
            StepSupport::Unverified => {
                step.model_note.is_none() && cites(&step.evidence, &|_| true)
            }
        }
    }) && version.excluded.iter().all(|item| {
        item.kind.excluded()
            && find(&item.evidence).is_some_and(|record| {
                record.kind == EvidenceKind::UserMessage && record.classified_as == Some(item.kind)
            })
    }) && version.evidence.iter().all(|record| {
        let kept_out = record.classified_as.is_some_and(UserStatement::excluded);
        record_holds(record, &version.origin.session)
            && kept_out
                == version
                    .excluded
                    .iter()
                    .any(|item| item.evidence == record.id)
    }) && digest_form(&version.origin.session)
}

/// The shape of every saved evidence record from the session whose key
/// digest is `session`, as `learned_method_evidence::collect` writes it.
pub(super) fn record_holds(record: &MethodEvidence, session: &str) -> bool {
    let call = record.kind == EvidenceKind::ToolCall;
    let user = record.kind == EvidenceKind::UserMessage;
    let trust_fits = match record.trust {
        SourceTrust::Owner => user,
        SourceTrust::Model => !user,
        SourceTrust::Untrusted => true,
    };
    let fields_fit = match record.kind {
        EvidenceKind::ToolCall => {
            record.tool.is_some() && record.outcome.is_some() && record.classified_as.is_none()
        }
        EvidenceKind::UserMessage => record.tool.is_none() && record.outcome.is_none(),
        EvidenceKind::Answer => {
            record.tool.is_none() && record.outcome.is_none() && record.classified_as.is_none()
        }
    };
    trust_fits
        && fields_fit
        && id_form(&record.id)
        && digest_form(&record.digest)
        && record.material_digest.as_deref().map_or(true, digest_form)
        && ref_index(&record.source_ref, session, call).is_some()
        && [&record.id, &record.source_ref, &record.digest]
            .iter()
            .all(|field| tiny(field))
        && [&record.at, &record.tool, &record.material_digest]
            .iter()
            .all(|field| field.as_deref().map_or(true, tiny))
}

/// Digest lists of a task's identity, as `learned_method_tasks::task_identity`
/// writes them.
pub(super) fn identity_holds(task: &TaskIdentity) -> bool {
    [
        &task.requests,
        &task.messages,
        &task.materials,
        &task.sources,
    ]
    .iter()
    .all(|list| list.len() <= MAX_ENTRIES && list.iter().all(|item| digest_form(item)))
}

/// `sha256:` and 64 lowercase hex digits, as `learned_method_evidence::digest`
/// writes them.
pub(super) fn digest_form(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn id_form(id: &str) -> bool {
    id.strip_prefix('e').is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Every saved text was redacted before it was saved, so redacting it again
/// changes nothing; a secret written in by hand would change.
fn clean(text: &str) -> bool {
    shacs_redaction::redact_string(text) == text
}

pub(super) fn short(text: &str) -> bool {
    text.chars().count() <= MAX_TEXT_CHARS && clean(text)
}

pub(super) fn tiny(text: &str) -> bool {
    text.chars().count() <= MAX_REF_CHARS && clean(text)
}

fn citations(ids: &[String]) -> bool {
    ids.len() <= MAX_ENTRIES && ids.iter().all(|id| tiny(id))
}

/// The limits the generator writes within, checked again on read so an
/// edited file cannot slip in more.
pub(super) fn within_limits(version: &LearnedMethodVersion) -> bool {
    let cited = |text: &CitedText| short(&text.text) && citations(&text.evidence);
    let evidence_ids: BTreeSet<&str> = version
        .evidence
        .iter()
        .map(|record| record.id.as_str())
        .collect();
    (1..=MAX_STEPS).contains(&version.steps.len())
        && version.premises.len() <= MAX_PREMISES
        && [
            version.unverified.len(),
            version.excluded.len(),
            version.evidence.len(),
        ]
        .iter()
        .all(|len| *len <= MAX_ENTRIES)
        && evidence_ids.len() == version.evidence.len()
        && tiny(&version.created_at)
        && tiny(&version.origin.session)
        && identity_holds(&version.origin.task)
        && cited(&version.purpose)
        && cited(&version.applies_to)
        && version.premises.iter().all(cited)
        && version.steps.iter().all(|step| {
            short(&step.text)
                && citations(&step.evidence)
                && step.model_note.as_deref().map_or(true, short)
        })
        && version.unverified.iter().all(|item| short(item))
        && version.excluded.iter().all(|item| tiny(&item.evidence))
}
