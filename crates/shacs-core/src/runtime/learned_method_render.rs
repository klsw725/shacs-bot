use super::learned_method::{LearnedMethod, LearnedMethodVersion, OriginKind, SourceTrust};
use super::learned_method_recheck::Seen;
use super::learned_method_render_decisions::{basis_note, decisions, state_label, version_label};
use super::learned_method_render_evidence::{cited, evidence, since, statement, support};
use super::learned_method_store::LearnedMethodStoreError;
use std::collections::BTreeMap;

const EFFECT_NOTE: &str =
    "Effect not attributed: a good or accepted result does not show which step caused it.";
const GONE: &str = "not rechecked: its source is no longer in that session";
const CHANGED: &str = "not rechecked: its source changed after saving";

/// A saved method with what its session still shows (see
/// `learned_method_recheck`).
pub(super) struct Checked {
    pub method: LearnedMethod,
    pub origin_exists: bool,
    pub seen: BTreeMap<String, Seen>,
    /// What the session of each decision's check (by the decision's index)
    /// still shows of the evidence the check cites.
    pub checks: BTreeMap<usize, BTreeMap<String, Seen>>,
}

impl Checked {
    /// Why what rests on `ids` is not shown as rechecked, if it is not.
    pub(super) fn unchecked(&self, ids: &[String]) -> Option<&'static str> {
        let seen: Vec<Option<&Seen>> = ids.iter().map(|id| self.seen.get(id)).collect();
        if seen.contains(&None) {
            Some(GONE)
        } else if seen.contains(&Some(&Seen::Changed)) {
            Some(CHANGED)
        } else {
            None
        }
    }

    /// Whether all the current version and the check that confirmed it rest
    /// on was rechecked against its session.
    pub(super) fn rechecked(&self) -> bool {
        self.method
            .current()
            .is_some_and(|version| self.all_as_saved(version))
            && basis_note(self).is_empty()
    }

    fn all_as_saved(&self, version: &LearnedMethodVersion) -> bool {
        version
            .evidence
            .iter()
            .all(|record| self.seen.get(&record.id) == Some(&Seen::Present))
    }
}

pub(super) fn created(checked: &Checked, viewer_session_digest: &str) -> String {
    format!(
        "Saved method candidate {}. It is inactive: it is not used by default, and saving it granted no permission.\n\n{}",
        checked.method.method_id,
        show(checked, viewer_session_digest)
    )
}

pub(super) fn no_candidate(reason: &str) -> String {
    format!(
        "No method candidate was created: {reason}. This is a normal result; nothing was saved."
    )
}

pub(super) fn failed(reason: &str) -> String {
    format!("Method candidate generation did not finish: {reason}. Nothing was saved.")
}

pub(super) fn store_error(error: LearnedMethodStoreError) -> String {
    format!("Learned methods are unavailable: {error}.")
}

pub(super) fn list(methods: &[Checked]) -> String {
    if methods.is_empty() {
        return "No learned methods yet. Send /learn after a task to propose a candidate."
            .to_owned();
    }
    let mut lines =
        vec!["Learned methods (candidates are inactive and never used by default):".to_owned()];
    for checked in methods {
        let method = &checked.method;
        lines.push(match method.current() {
            Some(version) => format!(
                "- {} v{} {}{} · applies to: {} · {} step(s) · untrusted influence: {} · {}{}",
                method.method_id,
                version.version,
                state_label(method),
                basis_note(checked),
                version.applies_to.text,
                version.steps.len(),
                if untrusted_influence(version) {
                    "yes"
                } else {
                    "no"
                },
                version.purpose.text,
                if checked.all_as_saved(version) {
                    ""
                } else {
                    " · some sources changed or cleared since, not rechecked"
                }
            ),
            None => format!("- {} (no readable version)", method.method_id),
        });
    }
    lines.join("\n")
}

pub(super) fn show(checked: &Checked, viewer_session_digest: &str) -> String {
    show_version(
        checked,
        viewer_session_digest,
        checked.method.current_version,
    )
}

/// Version `number` of the method; every version keeps the evidence of the
/// first, so what its session shows applies to each.
pub(super) fn show_version(checked: &Checked, viewer_session_digest: &str, number: u32) -> String {
    let method = &checked.method;
    let Some(version) = method
        .versions
        .iter()
        .find(|version| version.version == number)
    else {
        return format!("{} has no v{number}.", method.method_id);
    };
    let standing = if number == method.current_version {
        format!("{}{}", state_label(method), basis_note(checked))
    } else {
        format!(
            "{} · not the current version (v{} is)",
            version_label(method, number),
            method.current_version
        )
    };
    let mut lines = vec![
        format!("{} v{} — {standing}", method.method_id, version.version),
        format!("Purpose: {}", cited(&version.purpose, checked)),
        format!("Applies to: {}", cited(&version.applies_to, checked)),
        "Premises:".to_owned(),
    ];
    let premises: Vec<String> = version
        .premises
        .iter()
        .map(|premise| cited(premise, checked))
        .collect();
    bullets(&mut lines, &premises, "none stated");
    lines.push("Steps:".to_owned());
    for (index, step) in version.steps.iter().enumerate() {
        lines.push(format!(
            "{}. {} — {}",
            index + 1,
            step.text,
            support(step, version, checked)
        ));
    }
    lines.push("Unverified:".to_owned());
    lines.push(format!("- {EFFECT_NOTE}"));
    bullets(&mut lines, &version.unverified, "");
    lines.push("Kept out of the method:".to_owned());
    let excluded: Vec<String> = version
        .excluded
        .iter()
        .map(|item| format!("{} ({})", item.evidence, statement(item.kind)))
        .collect();
    bullets(&mut lines, &excluded, "nothing");
    lines.push("Decisions:".to_owned());
    lines.extend(decisions(checked));
    lines.push("Evidence (digests and references only):".to_owned());
    lines.extend(version.evidence.iter().map(|record| {
        format!(
            "{}{}",
            evidence(record),
            since(checked.seen.get(&record.id))
        )
    }));
    let all_present = checked.all_as_saved(version);
    let same_session = version.origin.session == viewer_session_digest;
    let source = format!(
        "session {}{}",
        short_session(&version.origin.session),
        match (checked.origin_exists, same_session, all_present) {
            (false, _, _) => " (that session no longer exists)",
            (true, true, true) => " (this conversation)",
            (true, true, false) =>
                " (this session; some source messages changed or were cleared since)",
            (true, false, true) => "",
            (true, false, false) => " (some source messages changed or were cleared since)",
        }
    );
    let made = match (version.origin.kind, version.origin.edited_from) {
        (OriginKind::UserEdit, Some(from)) => format!(
            "Made by your edit of v{from} on {}; it keeps the evidence from {source}",
            version.created_at
        ),
        _ => format!(
            "Created {} from {source} by model calls without tools",
            version.created_at
        ),
    };
    lines.push(format!(
        "{made}; {} older evidence item(s) were outside the window.",
        version.origin.omitted_evidence
    ));
    lines.join("\n")
}

pub(super) fn untrusted_influence(version: &LearnedMethodVersion) -> bool {
    version.evidence.iter().any(|record| {
        record.trust == SourceTrust::Untrusted
            && version
                .steps
                .iter()
                .any(|step| step.evidence.contains(&record.id))
    })
}

fn short_session(digest: &str) -> &str {
    digest.get(7..19).unwrap_or(digest)
}

fn bullets(lines: &mut Vec<String>, items: &[String], empty: &str) {
    if items.is_empty() && !empty.is_empty() {
        lines.push(format!("- {empty}"));
    }
    lines.extend(items.iter().map(|item| format!("- {item}")));
}
