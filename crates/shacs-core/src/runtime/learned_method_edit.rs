use super::learned_method::{
    CitedText, DecisionKind, LearnedMethod, LearnedMethodState, MethodStep, OriginKind, StepSupport,
};
use super::learned_method_build::FAILED_RUN_NOTE;
use super::learned_method_decide::{decide, replay, room};
use super::learned_method_store::MAX_VERSIONS;
use shacs_command::MethodPart;

const MAX_EDIT_CHARS: usize = 400;
const MAX_PREMISES: usize = 8;
const MAX_STEPS: usize = 12;

/// Makes a new version from `version`, which must be the current one, with
/// `part` set to the user's `text` (redacted) or removed. The new version
/// keeps the evidence of the one it came from, becomes current, and starts
/// as a candidate: it is used only once the user confirms it.
pub(super) fn edit(
    method: &mut LearnedMethod,
    (version, part, text): (u32, MethodPart, Option<&str>),
    (session, now): (&str, String),
) -> Result<(), String> {
    let id = method.method_id.clone();
    current_is(method, version)?;
    if method.versions.len() >= MAX_VERSIONS {
        return Err(format!(
            "{id} already keeps the most versions a method can ({MAX_VERSIONS}); nothing was changed."
        ));
    }
    room(method, "nothing was changed")?;
    let mut next = method
        .current()
        .cloned()
        .ok_or_else(|| format!("{id} has no readable version."))?;
    let number = method
        .versions
        .iter()
        .map(|version| version.version)
        .max()
        .unwrap_or(version)
        + 1;
    let text = text.map(|text| written(text, number)).transpose()?;
    let unchanged = next.clone();
    apply(&mut next, part, text)?;
    if next == unchanged {
        return Err(format!(
            "That is what {id} v{version} already says; nothing was changed."
        ));
    }
    next.version = number;
    next.created_at = now.clone();
    next.origin.kind = OriginKind::UserEdit;
    next.origin.edited_from = Some(version);
    method.versions.push(next);
    method.current_version = number;
    decide(method, DecisionKind::Edited, None, session, now);
    Ok(())
}

/// Puts the current, confirmed version out of use until the user confirms
/// it again.
pub(super) fn deactivate(
    method: &mut LearnedMethod,
    version: u32,
    (session, now): (&str, String),
) -> Result<(), String> {
    let id = method.method_id.clone();
    current_is(method, version)?;
    if method.state != LearnedMethodState::Confirmed {
        return Err(format!(
            "{id} v{version} is not in use (only a confirmed method is); nothing was changed."
        ));
    }
    room(method, "nothing was changed")?;
    decide(method, DecisionKind::Deactivated, None, session, now);
    Ok(())
}

/// Makes an earlier version that was confirmed, and is not rejected,
/// current again; its state is what its own decisions lead to.
pub(super) fn restore(
    method: &mut LearnedMethod,
    version: u32,
    (session, now): (&str, String),
) -> Result<(), String> {
    let id = method.method_id.clone();
    if version == method.current_version {
        return Err(format!(
            "{id} v{version} is already the current version; nothing was changed."
        ));
    }
    if !method
        .versions
        .iter()
        .any(|candidate| candidate.version == version)
    {
        return Err(format!("{id} has no v{version}; nothing was changed."));
    }
    let replayed = replay(&method.decisions, version);
    if !replayed.ever_confirmed || replayed.state == LearnedMethodState::Rejected {
        return Err(format!(
            "Only a version that was confirmed and not rejected can be made current again; {id} v{version} is not one. Nothing was changed."
        ));
    }
    room(method, "nothing was changed")?;
    method.current_version = version;
    decide(method, DecisionKind::Restored, None, session, now);
    Ok(())
}

fn current_is(method: &LearnedMethod, version: u32) -> Result<(), String> {
    if version == method.current_version {
        Ok(())
    } else {
        Err(format!(
            "{id} is at v{current}, not v{version}; nothing was changed. Send /learn show {id} to see it as it is now.",
            id = method.method_id,
            current = method.current_version
        ))
    }
}

/// The user's text as saved: trimmed, redacted, within the limit.
fn written(text: &str, version: u32) -> Result<(String, u32), String> {
    let text = shacs_redaction::redact_string(text.trim());
    if text.is_empty() {
        return Err("The new text is empty; nothing was changed.".to_owned());
    }
    if text.chars().count() > MAX_EDIT_CHARS {
        return Err(format!(
            "Keep the new text to {MAX_EDIT_CHARS} characters; nothing was changed."
        ));
    }
    Ok((text, version))
}

fn apply(
    next: &mut super::learned_method::LearnedMethodVersion,
    part: MethodPart,
    text: Option<(String, u32)>,
) -> Result<(), String> {
    let cited = |(text, version): (String, u32)| CitedText {
        text,
        evidence: Vec::new(),
        edited_in: Some(version),
    };
    let step = |(text, version): (String, u32)| MethodStep {
        text,
        evidence: Vec::new(),
        support: StepSupport::UserEdited,
        model_note: None,
        edited_in: Some(version),
    };
    match (part, text) {
        (MethodPart::Purpose, Some(text)) => next.purpose = cited(text),
        (MethodPart::AppliesTo, Some(text)) => next.applies_to = cited(text),
        (MethodPart::Purpose | MethodPart::AppliesTo, None) => {
            return Err("The purpose and the tasks it applies to can be changed, not removed; nothing was changed.".to_owned())
        }
        (MethodPart::Premise(number), text) => {
            place(&mut next.premises, number, text.map(cited), MAX_PREMISES, "premise")?;
        }
        (MethodPart::Step(number), text) => {
            let removed = text.is_none();
            place(&mut next.steps, number, text.map(step), MAX_STEPS, "step")?;
            if next.steps.is_empty() {
                return Err("A method keeps at least one step; nothing was changed.".to_owned());
            }
            renumber_notes(&mut next.unverified, number, removed);
        }
    }
    Ok(())
}

/// Replaces item `number` (from 1) with `item`, adds it as the next one, or
/// removes it when there is no item.
fn place<T>(
    items: &mut Vec<T>,
    number: usize,
    item: Option<T>,
    limit: usize,
    kind: &str,
) -> Result<(), String> {
    let count = items.len();
    let existing = (1..=count).contains(&number);
    match item {
        Some(item) if existing => items[number - 1] = item,
        Some(item) if number == count + 1 && count < limit => items.push(item),
        None if existing => {
            items.remove(number - 1);
        }
        _ => {
            return Err(format!(
                "There is no {kind} {number} to change (this version has {count}; at most {limit}, and a new one is number {}); nothing was changed.",
                count + 1
            ))
        }
    }
    Ok(())
}

/// Keeps the runtime's notes about failed runs in step with an edit of step
/// `number`: its note goes, and when it was removed, later steps move up.
fn renumber_notes(unverified: &mut Vec<String>, number: usize, removed: bool) {
    unverified.retain_mut(|item| {
        let step = item
            .strip_prefix("Step ")
            .and_then(|rest| rest.strip_suffix(FAILED_RUN_NOTE))
            .and_then(|digits| digits.parse::<usize>().ok());
        match step {
            Some(step) if step == number => false,
            Some(step) if removed && step > number => {
                *item = format!("Step {}{FAILED_RUN_NOTE}", step - 1);
                true
            }
            _ => true,
        }
    });
}
