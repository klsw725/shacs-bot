//! The user message an approval in `/permission recent` adds in auto mode,
//! and how the classifier request reads it back (Spec 048 PRD 020 scope 6).

use serde_json::Value;

const FIRST: &str =
    "[Approved in /permission recent] I approve one more try of this call that auto mode blocked: `";
const FIRST_END: &str = "`. Its arguments, as data:";
const LAST: &str = "Run it again if it is still needed. This approval covers only this exact call.";
const TIME_PREFIX: &str = "[Message Time: ";

/// `arguments` is one line of JSON: the call's arguments as shown.
pub(crate) fn message(tool: &str, arguments: &str) -> String {
    format!("{FIRST}{tool}{FIRST_END}\n{arguments}\n{LAST}")
}

/// The tool and arguments of a message that is exactly an approval, after
/// the time line session history may put first.
pub(crate) fn parse(text: &str) -> Option<(&str, Value)> {
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.len() == 4
        && lines
            .first()
            .is_some_and(|line| line.starts_with(TIME_PREFIX) && line.ends_with(']'))
    {
        lines.remove(0);
    }
    let [first, arguments, last] = lines.as_slice() else {
        return None;
    };
    let tool = first.strip_prefix(FIRST)?.strip_suffix(FIRST_END)?;
    if tool.is_empty() || tool.contains('`') || *last != LAST {
        return None;
    }
    let arguments = serde_json::from_str(arguments).ok()?;
    Some((tool, arguments))
}

#[cfg(test)]
#[path = "retry_approval_tests.rs"]
mod tests;
