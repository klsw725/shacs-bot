//! `read(<path>)` and `edit(<path>)` patterns: gitignore syntax with Claude
//! Code anchors and depth rules (Spec 048 PRD 000 scope 4).

use glob::{MatchOptions, Pattern};
use std::path::{Component, Path, PathBuf};

const MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAnchor {
    Absolute,
    Home,
    ConfigFolder,
    Workspace,
}

/// Which list a rule sits in. A single folder followed by `/**` matches at
/// any depth only for deny and ask rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuleList {
    Allow,
    DenyOrAsk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathMatch {
    Exact,
    /// The pattern matched a folder that holds the path.
    ViaFolder,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PathBases<'a> {
    pub(crate) home: &'a Path,
    pub(crate) config_folder: &'a Path,
    pub(crate) workspace: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathPattern {
    pub(crate) negated: bool,
    pub(crate) anchor: PathAnchor,
    body: String,
    usable: bool,
}

impl PathPattern {
    pub(crate) fn parse(raw: &str) -> Self {
        let (negated, rest) = match raw.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let (anchor, body) = if negated {
            (
                PathAnchor::Workspace,
                rest.strip_prefix("./").unwrap_or(rest),
            )
        } else if let Some(body) = rest.strip_prefix("//") {
            (PathAnchor::Absolute, body)
        } else if let Some(body) = rest.strip_prefix("~/") {
            (PathAnchor::Home, body)
        } else if let Some(body) = rest.strip_prefix('/') {
            (PathAnchor::ConfigFolder, body)
        } else {
            (
                PathAnchor::Workspace,
                rest.strip_prefix("./").unwrap_or(rest),
            )
        };
        let body = body.trim_end_matches('/').to_owned();
        let usable = Pattern::new(&body).is_ok();
        Self {
            negated,
            anchor,
            body,
            usable,
        }
    }

    pub(crate) fn usable(&self) -> bool {
        self.usable
    }

    /// The pattern names a whole folder tree (`secrets/**`), so a `!` rule
    /// cannot reopen paths inside it.
    pub(crate) fn names_tree(&self) -> bool {
        self.body.ends_with("/**") || self.body == "**"
    }

    /// The last name of a glob-free pattern, searched for in text.
    pub(crate) fn literal_name(&self) -> Option<&str> {
        let name = self.body.rsplit('/').next()?;
        (!name.is_empty() && !name.contains(['*', '?', '['])).then_some(name)
    }

    pub(crate) fn matches(
        &self,
        path: &Path,
        bases: PathBases<'_>,
        list: RuleList,
    ) -> Option<PathMatch> {
        let base = self.base(bases);
        let found = self.match_under(path, &base, &self.body, list);
        if found.is_some() || list == RuleList::Allow || self.anchor == PathAnchor::Workspace {
            return found;
        }
        let (fixed, rest) = split_fixed_prefix(&self.body);
        let written = base.join(&fixed);
        let real = std::fs::canonicalize(&written).ok()?;
        (real != written)
            .then(|| self.match_under(path, &real, &rest, list))
            .flatten()
    }

    fn base(&self, bases: PathBases<'_>) -> PathBuf {
        match self.anchor {
            PathAnchor::Absolute => PathBuf::from("/"),
            PathAnchor::Home => bases.home.to_path_buf(),
            PathAnchor::ConfigFolder => bases.config_folder.to_path_buf(),
            PathAnchor::Workspace => bases.workspace.to_path_buf(),
        }
    }

    fn match_under(
        &self,
        path: &Path,
        base: &Path,
        body: &str,
        list: RuleList,
    ) -> Option<PathMatch> {
        let relative = path.strip_prefix(base).ok()?;
        let parts = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !self.usable {
            let literal = body
                .split('/')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            return literal_match(&parts, &literal);
        }
        if body.is_empty() || body == "**" {
            return Some(PathMatch::Exact);
        }
        let any_depth = !body.contains('/')
            || (list == RuleList::DenyOrAsk
                && body
                    .strip_suffix("/**")
                    .is_some_and(|folder| !folder.contains('/')));
        let pattern = if any_depth && !body.starts_with("**/") {
            Pattern::new(&format!("**/{body}")).ok()?
        } else {
            Pattern::new(body).ok()?
        };
        (1..=parts.len()).find_map(|length| {
            let candidate = parts[..length].join("/");
            pattern
                .matches_with(&candidate, MATCH_OPTIONS)
                .then_some(if length == parts.len() {
                    PathMatch::Exact
                } else {
                    PathMatch::ViaFolder
                })
        })
    }
}

fn literal_match(parts: &[&str], literal: &[&str]) -> Option<PathMatch> {
    if parts.len() < literal.len() || parts[..literal.len()] != *literal {
        return None;
    }
    Some(if parts.len() == literal.len() {
        PathMatch::Exact
    } else {
        PathMatch::ViaFolder
    })
}

/// Splits `etc/**/*.conf` into the folder written without glob letters
/// (`etc`) and the rest (`**/*.conf`).
fn split_fixed_prefix(body: &str) -> (PathBuf, String) {
    let parts = body.split('/').collect::<Vec<_>>();
    let fixed_length = parts
        .iter()
        .take_while(|part| !part.contains(['*', '?', '[']))
        .count()
        .min(parts.len().saturating_sub(1));
    (
        parts[..fixed_length].iter().collect(),
        parts[fixed_length..].join("/"),
    )
}

#[cfg(test)]
#[path = "path_pattern_tests.rs"]
mod tests;
