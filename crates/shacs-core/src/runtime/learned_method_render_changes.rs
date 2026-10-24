use super::learned_method::LearnedMethod;
use super::learned_method_render::untrusted_influence;
use super::learned_method_render_decisions::{state_label, NO_GRANT};

/// What confirming means for the version shown, with the code that
/// confirms exactly it.
pub(super) fn review(shown: &str, method: &LearnedMethod, code: &str) -> String {
    let id = &method.method_id;
    let Some(version) = method.current() else {
        return format!("{id} has no readable version.");
    };
    let v = version.version;
    let mut lines = vec![
        format!("Review {id} v{v} before confirming it."),
        String::new(),
        shown.to_owned(),
        String::new(),
        format!(
            "Confirming means: this exact version may be offered as the default method for tasks like \"{}\". Nothing runs because of it now.",
            version.applies_to.text
        ),
    ];
    if untrusted_influence(version) {
        lines.push("Untrusted material influenced this method: steps marked \"influenced by untrusted material\" above come from it. Confirm only after checking them.".to_owned());
    }
    lines.push(NO_GRANT.to_owned());
    lines.push(format!(
        "To confirm this version, send: /learn confirm {id} v{v} {code}"
    ));
    lines.push(format!("To reject it, send: /learn reject {id} v{v}"));
    lines.join("\n")
}

pub(super) fn confirmed(method: &LearnedMethod) -> String {
    let scope = method
        .current()
        .map_or("", |version| version.applies_to.text.as_str());
    format!(
        "Confirmed {} v{}. It may be offered as the default for tasks like \"{scope}\"; nothing runs because of it now. {NO_GRANT}",
        method.method_id, method.current_version
    )
}

pub(super) fn edited(method: &LearnedMethod, review: &str) -> String {
    let id = &method.method_id;
    let from = method
        .current()
        .and_then(|version| version.origin.edited_from)
        .unwrap_or_default();
    format!(
        "Saved your edit as {id} v{}, made from v{from}. It is now the current version and a candidate: it is not used until you confirm it. If v{from} was confirmed, /learn restore {id} v{from} makes it current again.\n\n{review}",
        method.current_version
    )
}

pub(super) fn deactivated(method: &LearnedMethod) -> String {
    format!(
        "Put {id} v{v} out of use. It is not offered until you confirm it again (/learn confirm {id}); nothing else changed.",
        id = method.method_id,
        v = method.current_version
    )
}

pub(super) fn restored(method: &LearnedMethod) -> String {
    format!(
        "Made {id} v{v} current again; it is {}. Requests from now on use v{v}. Requests already answered keep the version they used, and what they did (files written, messages sent) is not undone.",
        state_label(method),
        id = method.method_id,
        v = method.current_version
    )
}

pub(super) fn rejected(method: &LearnedMethod) -> String {
    format!(
        "Rejected {} v{}. It will never be offered; nothing else changed.",
        method.method_id, method.current_version
    )
}
