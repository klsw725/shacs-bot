//! `rm` and `rmdir` of critical folders, asked about in every mode (Spec 048
//! PRD 000 scope 8).

use super::real_path;
use super::shell_command::{self, ShellCommand, Subcommand};
use super::shell_token::ShellWord;
use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

static RM_WORD: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(^|[^\w.-])(rm|rmdir)($|[^\w.-])").ok());

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CriticalRemoval {
    Folder(PathBuf),
    Unverifiable(&'static str),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CriticalFolders {
    folders: Vec<PathBuf>,
}

impl CriticalFolders {
    pub fn new(home: &Path, workspace: &Path, data_folder: &Path) -> Self {
        let mut folders = vec![PathBuf::from("/")];
        for path in [home, workspace, data_folder] {
            for spelling in [shell_command::normalize(path), real_path::real_path(path)] {
                folders.extend(spelling.ancestors().map(Path::to_path_buf));
            }
        }
        folders.sort();
        folders.dedup();
        Self { folders }
    }

    fn is_critical(&self, path: &Path) -> bool {
        self.folders.iter().any(|folder| folder == path)
            || (path.parent() == Some(Path::new("/")) && path.is_dir())
    }
}

pub(crate) fn critical_removal(
    command: &ShellCommand,
    critical: &CriticalFolders,
    home: &Path,
) -> Option<CriticalRemoval> {
    let unanalyzed_rm = command
        .unanalyzed_text
        .iter()
        .any(|text| RM_WORD.as_ref().is_some_and(|regex| regex.is_match(text)));
    if unanalyzed_rm {
        return Some(CriticalRemoval::Unverifiable(
            "the command could not be analyzed and contains rm",
        ));
    }
    command
        .subcommands
        .iter()
        .find_map(|subcommand| subcommand_removal(subcommand, critical, home))
}

fn subcommand_removal(
    subcommand: &Subcommand,
    critical: &CriticalFolders,
    home: &Path,
) -> Option<CriticalRemoval> {
    let words = without_elevation(&subcommand.words);
    let name = Path::new(&words.first()?.text).file_name()?.to_str()?;
    if name != "rm" && name != "rmdir" {
        return None;
    }
    let mut recursive = false;
    let mut operands = Vec::new();
    let mut options_ended = false;
    for word in &words[1..] {
        let text = word.text.as_str();
        if !options_ended && text == "--" {
            options_ended = true;
        } else if !options_ended && text.starts_with('-') && text.len() > 1 {
            recursive |=
                text == "--recursive" || (!text.starts_with("--") && text.contains(['r', 'R']));
        } else {
            operands.push(word);
        }
    }
    operands
        .into_iter()
        .find_map(|word| operand_removal(word, subcommand, critical, home, recursive))
}

fn without_elevation(words: &[ShellWord]) -> &[ShellWord] {
    match words.first().map(|word| word.text.as_str()) {
        Some("sudo" | "doas") => {
            let mut index = 1;
            while let Some(word) = words.get(index) {
                if !word.text.starts_with('-') {
                    break;
                }
                index += if matches!(word.text.as_str(), "-u" | "-g" | "-C") {
                    2
                } else {
                    1
                };
            }
            &words[index.min(words.len())..]
        }
        _ => words,
    }
}

fn operand_removal(
    word: &ShellWord,
    subcommand: &Subcommand,
    critical: &CriticalFolders,
    home: &Path,
    recursive: bool,
) -> Option<CriticalRemoval> {
    let text = if word.has_parameter {
        if word.has_positional && subcommand.positional_bound {
            return None;
        }
        word.without_parameters.as_str()
    } else {
        word.text.as_str()
    };
    if text.is_empty() {
        return None;
    }
    let relative = !(text.starts_with('/') || text == "~" || text.starts_with("~/"));
    if relative && subcommand.folders.unknown {
        return Some(CriticalRemoval::Unverifiable(
            "a cd before it could not be followed",
        ));
    }
    if relative && word.unquoted_glob && subcommand.folders.known.len() > 1 {
        return Some(CriticalRemoval::Unverifiable("a glob follows a cd"));
    }
    let bases = if relative {
        subcommand.folders.known.clone()
    } else {
        vec![PathBuf::from("/")]
    };
    bases.into_iter().find_map(|base| {
        let path = shell_command::join_word(&base, text, home);
        let target = if word.unquoted_glob {
            glob_removal(text, &path, critical)
        } else {
            critical
                .is_critical(&path)
                .then(|| CriticalRemoval::Folder(path.clone()))
        };
        target.or_else(|| {
            let folder = if text.ends_with("/*") {
                path.parent().map(Path::to_path_buf)
            } else {
                recursive.then(|| path.clone())
            };
            folder
                .filter(|folder| on_mounted_drive(folder))
                .map(CriticalRemoval::Folder)
        })
    })
}

fn glob_removal(text: &str, path: &Path, critical: &CriticalFolders) -> Option<CriticalRemoval> {
    let parent = path.parent()?;
    let last_has_glob = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(has_glob);
    if last_has_glob && critical.is_critical(parent) {
        return Some(CriticalRemoval::Folder(parent.to_path_buf()));
    }
    let parts = text.trim_end_matches('/').split('/').collect::<Vec<_>>();
    let inner_glob = parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|part| has_glob(part));
    (inner_glob || (text.ends_with('/') && parts.last().is_some_and(|part| has_glob(part))))
        .then_some(CriticalRemoval::Unverifiable(
            "a glob reaches folders it does not name",
        ))
}

fn has_glob(text: &str) -> bool {
    text.contains(['*', '?', '[', '{'])
}

fn on_mounted_drive(path: &Path) -> bool {
    let parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    match parts.as_slice() {
        ["Volumes", _] | ["Volumes", _, _] => true,
        ["System", "Volumes"] | ["System", "Volumes", _] | ["System", "Volumes", _, _] => true,
        ["media", _] | ["media", _, _] | ["media", _, _, _] => true,
        ["run", "media"] | ["run", "media", _] | ["run", "media", _, _] => true,
        ["run", "media", _, _, _] | ["mnt", _] => true,
        ["mnt", letter, _] => letter.len() == 1,
        _ => false,
    }
}

#[cfg(test)]
#[path = "critical_path_tests.rs"]
mod tests;
