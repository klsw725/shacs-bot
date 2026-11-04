//! The facts about one action that the decision order reads (Spec 048 PRD
//! 000 scope 2). They travel with the action so the tool execution decision
//! and the process gate decide the same way.

use super::critical_path::{self, CriticalRemoval};
use super::permission_environment::PermissionEnvironment;
use super::rule_match::{
    self, AllowOptions, CommandPart, CommandSubject, FileAccess, PathPair, RuleSubject,
};
use super::shell_paths::{self, PathSource, PathWord};
use super::{real_path, shell_command};
use crate::runtime::PermissionedAction;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

const READ_TOOLS: &[&str] = &["read_file", "list_dir", "glob", "grep"];
const WRITE_TOOLS: &[&str] = &["write_file", "edit_file", "notebook_edit"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionMaterials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_rule: Option<String>,
    /// The allow rule, remembered allow or session approval that approves the
    /// action in auto mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_allow: Option<AutoAllow>,
    #[serde(default)]
    pub secret_read: bool,
    #[serde(default)]
    pub secret_read_by_text: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_name: Option<String>,
    /// The user approved this very call in an approval question. A session
    /// approval does not count (secret reads, critical paths, ask rules).
    #[serde(default)]
    pub this_call_approved: bool,
    #[serde(default)]
    pub secret_write: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_removal: Option<String>,
    #[serde(default)]
    pub protected_write: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlinked_protected_write: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlinked_outside_write: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_refusal: Option<String>,
    /// A file tool write inside the workspace, away from protected paths,
    /// while `allowWorkspaceEdits` is on (row 15).
    #[serde(default)]
    pub workspace_edit: bool,
    /// No allow rule, remembered allow or session approval may approve the
    /// action in auto mode (row 12 conditions).
    #[serde(default)]
    pub auto_allow_blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum AutoAllow {
    Rule { rule: String },
    Remembered { rule_ref: String },
}

pub(crate) struct MaterialsInput<'a> {
    pub(crate) action: &'a PermissionedAction,
    pub(crate) environment: &'a PermissionEnvironment,
    pub(crate) auto_mode: bool,
    pub(crate) allow_workspace_edits: bool,
    pub(crate) preflight_refusal: Option<String>,
}

struct RedirectTarget {
    pairs: Vec<PathPair>,
    source: PathSource,
    plain: bool,
}

pub(crate) fn permission_materials(input: MaterialsInput<'_>) -> PermissionMaterials {
    let environment = input.environment;
    let action = input.action;
    let arguments = &action.redacted_arguments;
    let file = file_access(action, environment);
    let mut redirect_targets = Vec::new();
    let mut critical_removal = None;
    let command = (action.tool_name == "exec")
        .then(|| arguments.get("command").and_then(Value::as_str))
        .flatten()
        .map(|text| {
            let folder = arguments
                .get("working_dir")
                .and_then(Value::as_str)
                .map_or_else(
                    || environment.workspace.clone(),
                    |dir| shell_command::join_word(&environment.workspace, dir, &environment.home),
                );
            let analysis = shell_command::analyze(text, &folder, &environment.home);
            critical_removal = critical_path::critical_removal(
                &analysis,
                &environment.critical_folders,
                &environment.home,
            );
            command_subject(&analysis, &environment.home, &mut redirect_targets)
        });
    let subject = RuleSubject {
        tool_name: &action.tool_name,
        file,
        command,
        host: arguments
            .get("url")
            .and_then(Value::as_str)
            .and_then(url_host),
    };
    let matches = rule_match::match_rules(
        &environment.user_rules,
        &subject,
        environment.bases(),
        AllowOptions {
            suspend_broad: input.auto_mode,
            classify_all_shell: input.auto_mode && environment.classify_all_shell,
        },
    );
    let (reads, writes) = touched(&subject);
    let secrets = &environment.secret_files;
    let text_name = subject.command.as_ref().and_then(|command| {
        let names = secrets.names(std::slice::from_ref(&environment.workspace));
        names.into_iter().find(|name| {
            command
                .text_regions
                .iter()
                .any(|text| contains_name(text, name))
        })
    });
    let secret_path = reads
        .iter()
        .find(|pair| secrets.contains(&pair.requested))
        .map(|pair| secrets.display_name(&pair.requested));
    let protected = &environment.protected_paths;
    let protected_write = writes
        .iter()
        .any(|pair| protected.contains(&pair.requested) || protected.contains(&pair.real));
    let targets_allowed = redirect_targets
        .iter()
        .all(|target| target_allowed(target, environment));
    let is_exec = action.tool_name == "exec";
    PermissionMaterials {
        deny_rule: matches.deny,
        ask_rule: matches.ask,
        allow_rule: matches.allow,
        auto_allow: None,
        secret_read: text_name.is_some() || secret_path.is_some(),
        secret_read_by_text: secret_path.is_none() && text_name.is_some(),
        secret_name: secret_path.or(text_name),
        this_call_approved: false,
        secret_write: writes.iter().any(|pair| secrets.contains(&pair.requested)),
        critical_removal: critical_removal.map(|removal| match removal {
            CriticalRemoval::Folder(path) => path.display().to_string(),
            CriticalRemoval::Unverifiable(reason) => reason.to_owned(),
        }),
        protected_write,
        symlinked_protected_write: writes
            .iter()
            .find(|pair| !protected.contains(&pair.requested) && protected.contains(&pair.real))
            .map(|pair| pair.real.display().to_string()),
        symlinked_outside_write: writes
            .iter()
            .find(|pair| {
                inside_written_workspace(&pair.requested, environment)
                    && !pair.real.starts_with(&environment.real_workspace)
            })
            .map(|pair| pair.real.display().to_string()),
        preflight_refusal: input.preflight_refusal,
        workspace_edit: input.allow_workspace_edits
            && !protected_write
            && matches!(&subject.file, Some((FileAccess::Write, pairs))
                if !pairs.is_empty()
                    && pairs.iter().all(|pair| environment.inside_workspace(&pair.requested, &pair.real))),
        auto_allow_blocked: protected_write
            || action.tool_name == "spawn"
            || (is_exec && (!targets_allowed || environment.classify_all_shell)),
    }
}

fn file_access(
    action: &PermissionedAction,
    environment: &PermissionEnvironment,
) -> Option<(FileAccess, Vec<PathPair>)> {
    let tool = action.tool_name.as_str();
    let access = if READ_TOOLS.contains(&tool) {
        FileAccess::Read
    } else if WRITE_TOOLS.contains(&tool) {
        FileAccess::Write
    } else {
        return None;
    };
    let text = action
        .redacted_arguments
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".");
    let requested = shell_command::join_word(&environment.workspace, text, &environment.home);
    Some((access, vec![pair(requested)]))
}

fn pair(requested: PathBuf) -> PathPair {
    let real = real_path::real_path(&requested);
    PathPair { requested, real }
}

fn pairs(words: &[PathWord]) -> Vec<PathPair> {
    words
        .iter()
        .flat_map(|word| word.paths.iter().cloned().map(pair))
        .collect()
}

fn command_subject(
    analysis: &shell_command::ShellCommand,
    home: &Path,
    redirect_targets: &mut Vec<RedirectTarget>,
) -> CommandSubject {
    let mut text_regions = analysis.text_regions.clone();
    let parts = analysis
        .subcommands
        .iter()
        .map(|subcommand| {
            let paths = shell_paths::subcommand_paths(subcommand, home);
            for word in paths.reads.iter().chain(&paths.writes) {
                if word.unknown_base {
                    text_regions.push(word.word.text.clone());
                }
                if word.source != PathSource::Argument {
                    redirect_targets.push(RedirectTarget {
                        pairs: pairs(std::slice::from_ref(word)),
                        source: word.source,
                        plain: !word.word.has_parameter
                            && !word.word.unquoted_glob
                            && !word.unknown_base
                            && !(word.source != PathSource::InputRedirect
                                && word.word.text.starts_with('~')),
                    });
                }
            }
            CommandPart {
                text_for_allow: subcommand.text(true),
                text_for_deny: subcommand.text(false),
                words: subcommand
                    .words
                    .iter()
                    .map(|word| word.text.clone())
                    .collect(),
                reads: pairs(&paths.reads),
                writes: pairs(&paths.writes),
            }
        })
        .collect();
    CommandSubject {
        analyzable: analysis.analyzable,
        parts,
        text_regions,
        unanalyzed_text: analysis.unanalyzed_text.clone(),
    }
}

fn target_allowed(target: &RedirectTarget, environment: &PermissionEnvironment) -> bool {
    let access = if target.source == PathSource::InputRedirect {
        FileAccess::Read
    } else {
        FileAccess::Write
    };
    target.plain
        && !target.pairs.is_empty()
        && target.pairs.iter().all(|pair| {
            environment.inside_workspace(&pair.requested, &pair.real)
                || rule_match::allow_covers_path(
                    &environment.user_rules,
                    access,
                    pair,
                    environment.bases(),
                )
        })
}

fn touched(subject: &RuleSubject<'_>) -> (Vec<PathPair>, Vec<PathPair>) {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    match &subject.file {
        Some((FileAccess::Read, found)) => reads.extend(found.iter().cloned()),
        Some((FileAccess::Write, found)) => writes.extend(found.iter().cloned()),
        None => {}
    }
    for part in subject.command.iter().flat_map(|command| &command.parts) {
        reads.extend(part.reads.iter().cloned());
        writes.extend(part.writes.iter().cloned());
    }
    (reads, writes)
}

fn inside_written_workspace(requested: &Path, environment: &PermissionEnvironment) -> bool {
    requested.starts_with(&environment.workspace)
        || requested.starts_with(&environment.real_workspace)
}

/// Finds `name` in `text` where it is not part of a longer name.
fn contains_name(text: &str, name: &str) -> bool {
    let inside_name =
        |character: char| character.is_alphanumeric() || matches!(character, '_' | '-');
    text.match_indices(name).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + name.len()..].chars().next();
        !before.is_some_and(|character| inside_name(character) || character == '.')
            && !after.is_some_and(inside_name)
    })
}

fn url_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next()?
    } else {
        host_port.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_owned())
}

#[cfg(test)]
#[path = "permission_materials_tests.rs"]
mod tests;
