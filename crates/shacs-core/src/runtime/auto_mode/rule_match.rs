//! Applies the user's allow, ask and deny rules to one action (Spec 048 PRD
//! 000 scope 4): deny and ask need one matching part, allow needs every part.

use super::path_pattern::{PathAnchor, PathBases, PathMatch, RuleList};
use super::permission_rule::{
    PermissionRule, RuleEffect, RuleSpecifier, ToolSelector, UserPermissionRules,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathPair {
    pub(crate) requested: PathBuf,
    pub(crate) real: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileAccess {
    Read,
    Write,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommandPart {
    pub(crate) text_for_allow: String,
    pub(crate) text_for_deny: String,
    pub(crate) words: Vec<String>,
    pub(crate) reads: Vec<PathPair>,
    pub(crate) writes: Vec<PathPair>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommandSubject {
    pub(crate) analyzable: bool,
    pub(crate) parts: Vec<CommandPart>,
    /// Text searched letter by letter, including names whose folder is unknown.
    pub(crate) text_regions: Vec<String>,
    pub(crate) unanalyzed_text: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuleSubject<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) file: Option<(FileAccess, Vec<PathPair>)>,
    pub(crate) command: Option<CommandSubject>,
    pub(crate) host: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AllowOptions {
    /// Auto mode: broad rules and `spawn` allow rules are not used.
    pub(crate) suspend_broad: bool,
    pub(crate) classify_all_shell: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RuleMatches {
    pub(crate) deny: Option<String>,
    pub(crate) ask: Option<String>,
    pub(crate) allow: Option<String>,
}

pub(crate) fn match_rules(
    rules: &UserPermissionRules,
    subject: &RuleSubject<'_>,
    bases: PathBases<'_>,
    options: AllowOptions,
) -> RuleMatches {
    RuleMatches {
        deny: blocking_match(rules.list(RuleEffect::Deny), subject, bases, true),
        ask: blocking_match(rules.list(RuleEffect::Ask), subject, bases, false),
        allow: allow_match(rules.list(RuleEffect::Allow), subject, bases, options),
    }
}

fn blocking_match(
    rules: &[PermissionRule],
    subject: &RuleSubject<'_>,
    bases: PathBases<'_>,
    deny: bool,
) -> Option<String> {
    let tool_rule = rules
        .iter()
        .find(|rule| rule.specifier == RuleSpecifier::Any && selects_tool(&rule.tool, subject));
    if let Some(rule) = tool_rule {
        return Some(rule.text.clone());
    }
    if let (Some(host), true) = (&subject.host, subject.tool_name == "web_fetch") {
        if let Some(rule) = rules.iter().find(|rule| {
            matches!(&rule.specifier, RuleSpecifier::Domain(pattern) if pattern.matches(host))
        }) {
            return Some(rule.text.clone());
        }
    }
    let (reads, writes) = touched_paths(subject);
    let read_hit = reads
        .iter()
        .chain(if deny { writes.as_slice() } else { &[] })
        .find_map(|pair| path_list_match(rules, &ToolSelector::ReadTools, pair, bases));
    let edit_hit = writes
        .iter()
        .find_map(|pair| path_list_match(rules, &ToolSelector::EditTools, pair, bases));
    if let Some(rule) = read_hit.or(edit_hit) {
        return Some(rule);
    }
    let command = subject.command.as_ref()?;
    rules.iter().find_map(|rule| {
        let found = match &rule.specifier {
            RuleSpecifier::Exec(pattern) => {
                command
                    .parts
                    .iter()
                    .any(|part| pattern.matches(&part.text_for_deny))
                    || command
                        .unanalyzed_text
                        .iter()
                        .any(|text| pattern.appears_in(text))
            }
            RuleSpecifier::Path(pattern)
                if rule.tool == ToolSelector::ReadTools && !pattern.negated =>
            {
                pattern
                    .literal_name()
                    .is_some_and(|name| command.text_regions.iter().any(|text| text.contains(name)))
            }
            _ => false,
        };
        found.then(|| rule.text.clone())
    })
}

fn allow_match(
    rules: &[PermissionRule],
    subject: &RuleSubject<'_>,
    bases: PathBases<'_>,
    options: AllowOptions,
) -> Option<String> {
    let usable = |rule: &&PermissionRule| {
        !(options.suspend_broad
            && (matches!(&rule.tool, ToolSelector::Tool(name) if name == "spawn")
                || is_broad_exec(rule)))
    };
    if let Some(command) = &subject.command {
        if !command.analyzable || !command.unanalyzed_text.is_empty() || options.classify_all_shell
        {
            return None;
        }
        let mut first = None;
        for part in &command.parts {
            let words = part.words.iter().map(String::as_str).collect::<Vec<_>>();
            let rule = rules
                .iter()
                .filter(usable)
                .find(|rule| match &rule.specifier {
                    RuleSpecifier::Any => selects_tool(&rule.tool, subject),
                    RuleSpecifier::Exec(pattern) => {
                        pattern.matches(&part.text_for_allow) && !pattern.allow_excludes(&words)
                    }
                    RuleSpecifier::Path(_) | RuleSpecifier::Domain(_) => false,
                })?;
            first.get_or_insert_with(|| rule.text.clone());
        }
        return first;
    }
    if let Some((access, pairs)) = &subject.file {
        let covering = rules.iter().find(|rule| {
            pairs
                .iter()
                .all(|pair| rule_covers_path(rule, *access, pair, bases))
        });
        if let Some(rule) = covering {
            return Some(rule.text.clone());
        }
    }
    rules.iter().filter(usable).find_map(|rule| {
        let found = match &rule.specifier {
            RuleSpecifier::Any => {
                !matches!(rule.tool, ToolSelector::ReadTools | ToolSelector::EditTools)
                    && selects_tool(&rule.tool, subject)
            }
            RuleSpecifier::Domain(pattern) => {
                subject.tool_name == "web_fetch"
                    && subject
                        .host
                        .as_deref()
                        .is_some_and(|host| pattern.matches(host))
            }
            RuleSpecifier::Exec(_) | RuleSpecifier::Path(_) => false,
        };
        found.then(|| rule.text.clone())
    })
}

/// Whether an allow rule covers both spellings of `pair` for `access`, as a
/// redirect target needs when it lies outside the workspace.
pub(crate) fn allow_covers_path(
    rules: &UserPermissionRules,
    access: FileAccess,
    pair: &PathPair,
    bases: PathBases<'_>,
) -> bool {
    rules
        .list(RuleEffect::Allow)
        .iter()
        .any(|rule| rule_covers_path(rule, access, pair, bases))
}

fn rule_covers_path(
    rule: &PermissionRule,
    access: FileAccess,
    pair: &PathPair,
    bases: PathBases<'_>,
) -> bool {
    let selector = match access {
        FileAccess::Read => ToolSelector::ReadTools,
        FileAccess::Write => ToolSelector::EditTools,
    };
    rule.tool == selector
        && match &rule.specifier {
            RuleSpecifier::Any => true,
            RuleSpecifier::Path(pattern) => {
                pattern
                    .matches(&pair.requested, bases, RuleList::Allow)
                    .is_some()
                    && pattern
                        .matches(&pair.real, bases, RuleList::Allow)
                        .is_some()
            }
            RuleSpecifier::Exec(_) | RuleSpecifier::Domain(_) => false,
        }
}

fn is_broad_exec(rule: &PermissionRule) -> bool {
    match (&rule.tool, &rule.specifier) {
        (ToolSelector::Tool(name), RuleSpecifier::Any) => name == "exec",
        (_, RuleSpecifier::Exec(pattern)) => pattern.is_broad(),
        _ => false,
    }
}

fn selects_tool(selector: &ToolSelector, subject: &RuleSubject<'_>) -> bool {
    let tool = subject.tool_name;
    match selector {
        ToolSelector::Tool(name) => name == tool,
        ToolSelector::ToolGlob(pattern) => pattern.matches(tool),
        ToolSelector::ReadTools => matches!(subject.file, Some((FileAccess::Read, _))),
        ToolSelector::EditTools => matches!(subject.file, Some((FileAccess::Write, _))),
        ToolSelector::McpServer(server) => tool.starts_with(&format!("mcp_{server}_")),
        ToolSelector::McpServerTools { server, tools } => tool
            .strip_prefix(&format!("mcp_{server}_"))
            .is_some_and(|rest| tools.matches(rest)),
    }
}

fn touched_paths(subject: &RuleSubject<'_>) -> (Vec<PathPair>, Vec<PathPair>) {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    match &subject.file {
        Some((FileAccess::Read, pairs)) => reads.extend(pairs.iter().cloned()),
        Some((FileAccess::Write, pairs)) => writes.extend(pairs.iter().cloned()),
        None => {}
    }
    for part in subject.command.iter().flat_map(|command| &command.parts) {
        reads.extend(part.reads.iter().cloned());
        writes.extend(part.writes.iter().cloned());
    }
    (reads, writes)
}

/// Whether a `read` deny or ask rule covers `pair`; folder scans leave such
/// paths out.
pub(crate) fn read_rule_hides(
    rules: &UserPermissionRules,
    pair: &PathPair,
    bases: PathBases<'_>,
) -> bool {
    [RuleEffect::Deny, RuleEffect::Ask]
        .into_iter()
        .any(|effect| {
            path_list_match(rules.list(effect), &ToolSelector::ReadTools, pair, bases).is_some()
        })
}

/// The deny or ask rule of one kind that covers `pair`, after the `!` rules
/// listed later have carved their paths out of earlier workspace rules.
fn path_list_match(
    rules: &[PermissionRule],
    selector: &ToolSelector,
    pair: &PathPair,
    bases: PathBases<'_>,
) -> Option<String> {
    let mut hits: Vec<(&PermissionRule, PathMatch)> = Vec::new();
    for rule in rules.iter().filter(|rule| rule.tool == *selector) {
        let RuleSpecifier::Path(pattern) = &rule.specifier else {
            continue;
        };
        let found = pattern
            .matches(&pair.requested, bases, RuleList::DenyOrAsk)
            .or_else(|| pattern.matches(&pair.real, bases, RuleList::DenyOrAsk));
        match (pattern.negated, found) {
            (false, Some(how)) => hits.push((rule, how)),
            (true, Some(_)) => hits.retain(|(earlier, how)| !carvable(earlier, *how)),
            (_, None) => {}
        }
    }
    hits.first().map(|(rule, _)| rule.text.clone())
}

fn carvable(rule: &PermissionRule, how: PathMatch) -> bool {
    matches!(&rule.specifier, RuleSpecifier::Path(pattern)
        if pattern.anchor == PathAnchor::Workspace
            && !pattern.names_tree()
            && how == PathMatch::Exact)
}

#[cfg(test)]
#[path = "rule_match_tests.rs"]
mod tests;
