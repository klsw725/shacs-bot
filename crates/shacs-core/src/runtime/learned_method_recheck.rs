use super::learned_method::{
    DecisionKind, EvidenceKind, LearnedMethodVersion, MethodEvidence, RunOutcome, SourceTrust,
    StepSupport, TaskCheck,
};
use super::learned_method_build::model_words;
use super::learned_method_check::Checker;
use super::learned_method_draft::CitedDraft;
use super::learned_method_evidence::{shown_form, Evidence};
use super::learned_method_guard::quotes;
use super::learned_method_source::{content, ref_index};
pub(super) use super::learned_method_sources::Sources;
use std::collections::BTreeMap;

const EDITED: &str = "a method does not match the messages it cites";
const UNREGISTERED: &str = "unregistered tool";
/// Longer than any saved text, so a recheck never shortens one.
const RECHECK_CHARS: usize = 1_000;

/// How a saved evidence record compares with its session now; a record
/// that is not there at all has no entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Seen {
    /// Still there at the saved position, and it rechecks.
    Present,
    /// Still there and it rechecks, but not at the saved position (the
    /// session was trimmed since, or the saved position is wrong).
    Moved,
    /// A run not saved as one that went through, whose result changed since
    /// (an approval or answer came later): not rechecked.
    Changed,
}

/// Rechecks a saved version against the messages it cites that are still in
/// its session (`None` once the session is gone). Each fact the runtime
/// derived must derive the same way again, every premise and step shown as
/// the user's words must still be in each message it cites, and no line the
/// model wrote may copy the request or a message kept out, or cite one kept
/// out. A version that disagrees with a message still there was edited
/// outside `/learn`.
pub(super) fn recheck(
    version: &LearnedMethodVersion,
    sources: Option<&mut Sources<'_>>,
) -> Result<BTreeMap<String, Seen>, &'static str> {
    let Some(sources) = sources else {
        return Ok(BTreeMap::new());
    };
    let found = records(&version.evidence, &version.origin.session, sources)?;
    if quotes_hold(version, &found.texts) && guards_hold(version, &found.items) {
        Ok(found.seen)
    } else {
        Err(EDITED)
    }
}

/// Rechecks the evidence a saved check of a later task cites against that
/// task's session, `session` (`None` once it is gone): the same rules as
/// for a method's own evidence, and every step credited to the user's words
/// must still be in each message cited for it.
pub(super) fn recheck_check(
    check: &TaskCheck,
    version: &LearnedMethodVersion,
    (kind, session): (DecisionKind, &str),
    sources: Option<&mut Sources<'_>>,
) -> Result<BTreeMap<String, Seen>, &'static str> {
    let Some(sources) = sources else {
        return Ok(BTreeMap::new());
    };
    let found = records(&check.evidence, session, sources)?;
    let quoted = kind == DecisionKind::Conflicted
        || check.steps.iter().all(|redone| {
            let step = version.steps.get(redone.step.saturating_sub(1));
            let words = step.filter(|step| {
                matches!(
                    step.support,
                    StepSupport::UserStated | StepSupport::UserEdited
                )
            });
            words.map_or(true, |step| {
                redone.evidence.iter().all(|id| {
                    found
                        .texts
                        .get(id.as_str())
                        .map_or(true, |source| quotes(&step.text, &shown_form(source)))
                })
            })
        });
    if quoted {
        Ok(found.seen)
    } else {
        Err(EDITED)
    }
}

/// What a session still shows for saved `records` of the session whose key
/// digest is `session`.
struct Found<'v, 'm> {
    seen: BTreeMap<String, Seen>,
    texts: BTreeMap<&'v str, &'m str>,
    /// The user's own messages still there, for the generation-time checks.
    items: Vec<Evidence>,
}

fn records<'v, 'm>(
    records: &'v [MethodEvidence],
    session: &str,
    sources: &mut Sources<'m>,
) -> Result<Found<'v, 'm>, &'static str> {
    let messages = sources.messages;
    let mut found = Found {
        seen: BTreeMap::new(),
        texts: BTreeMap::new(),
        items: Vec::new(),
    };
    for record in records {
        let Some((index, call)) = sources.find(record) else {
            continue;
        };
        let now = sources.derive(index, call, record.kind);
        if !stable(record, &now.record) {
            return Err(EDITED);
        }
        if (record.outcome, &record.material_digest)
            != (now.record.outcome, &now.record.material_digest)
        {
            // A later result can only turn up for a run not saved as one that
            // went through, and such a run backs no claim.
            if record.kind == EvidenceKind::ToolCall && record.outcome != Some(RunOutcome::Ok) {
                found.seen.insert(record.id.clone(), Seen::Changed);
                continue;
            }
            return Err(EDITED);
        }
        let at = ref_index(&record.source_ref, session, call.is_some());
        let place = if at == Some(index) {
            Seen::Present
        } else {
            Seen::Moved
        };
        found.seen.insert(record.id.clone(), place);
        found
            .texts
            .insert(record.id.as_str(), content(&messages[index]));
        if record.kind == EvidenceKind::UserMessage && now.record.trust == SourceTrust::Owner {
            found.items.push(Evidence {
                record: MethodEvidence {
                    trust: now.record.trust,
                    ..record.clone()
                },
                text: now.text.clone(),
                material: None,
            });
        }
    }
    Ok(found)
}

/// Whether a user message is the user's own depends on the message alone,
/// so it must recheck exactly. What the model wrote may recheck as more
/// trusted than saved (when untrusted material before it was trimmed
/// since), never less.
fn stable(saved: &MethodEvidence, now: &MethodEvidence) -> bool {
    let trust = if saved.kind == EvidenceKind::UserMessage {
        saved.trust == now.trust
    } else {
        rank(saved.trust) <= rank(now.trust)
    };
    trust && (saved.tool == now.tool || saved.tool.as_deref() == Some(UNREGISTERED))
}

fn rank(trust: SourceTrust) -> u8 {
    match trust {
        SourceTrust::Untrusted => 0,
        SourceTrust::Model => 1,
        SourceTrust::Owner => 2,
    }
}

fn quotes_hold(version: &LearnedMethodVersion, texts: &BTreeMap<&str, &str>) -> bool {
    let quoted = |text: &str, ids: &[String]| {
        ids.iter().all(|id| {
            texts
                .get(id.as_str())
                .map_or(true, |source| quotes(text, &shown_form(source)))
        })
    };
    version
        .premises
        .iter()
        .filter(|premise| premise.edited_in.is_none())
        .all(|premise| quoted(&premise.text, &premise.evidence))
        && version
            .steps
            .iter()
            .filter(|step| step.support == StepSupport::UserStated)
            .all(|step| quoted(&step.text, &step.evidence))
}

/// Checks the model's lines (not the runtime's "Run …" texts and notes, nor
/// the user's edits) against `items`, the user's own messages still
/// present: a line may not copy the request or a message kept out, nor cite
/// one kept out.
fn guards_hold(version: &LearnedMethodVersion, items: &[Evidence]) -> bool {
    let mut lines: Vec<(&str, &[String])> = [&version.purpose, &version.applies_to]
        .into_iter()
        .chain(&version.premises)
        .filter(|text| text.edited_in.is_none())
        .map(|text| (text.text.as_str(), text.evidence.as_slice()))
        .collect();
    for step in &version.steps {
        let text = match step.support {
            StepSupport::ObservedRun => step.model_note.as_deref(),
            StepSupport::UserStated | StepSupport::Unverified => Some(step.text.as_str()),
            StepSupport::UserEdited => None,
        };
        lines.extend(text.map(|text| (text, step.evidence.as_slice())));
    }
    lines.extend(
        version
            .unverified
            .iter()
            .filter_map(|item| model_words(item))
            .map(|text| (text, &[][..])),
    );
    let mut checker = Checker::new(items);
    lines.into_iter().all(|(text, evidence)| {
        let line = CitedDraft {
            text: text.to_owned(),
            evidence: evidence.to_vec(),
        };
        checker
            .check(&line, RECHECK_CHARS)
            .is_some_and(|(checked, _)| checked == line.text)
    })
}
