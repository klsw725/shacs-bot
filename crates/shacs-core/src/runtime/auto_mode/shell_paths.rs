//! The paths a subcommand reads and writes (Spec 048 PRD 000 scope 6).

use super::shell_command::{self, Subcommand};
use super::shell_token::{RedirectKind, ShellWord};
use std::path::{Path, PathBuf};

const DEVICE_PATHS: &[&str] = &["/dev/null", "/dev/stdin", "/dev/stdout", "/dev/stderr"];
const OPERAND_WRITERS: &[&str] = &[
    "rm", "rmdir", "mv", "touch", "truncate", "chmod", "chown", "ln",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathSource {
    Argument,
    InputRedirect,
    OutputRedirect,
    Tee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathWord {
    pub(crate) word: ShellWord,
    pub(crate) source: PathSource,
    /// Where the word points, one entry per working folder it may resolve
    /// against. Empty when the word holds a parameter.
    pub(crate) paths: Vec<PathBuf>,
    /// The word is relative and follows a `cd` whose target is unknown.
    pub(crate) unknown_base: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SubcommandPaths {
    pub(crate) reads: Vec<PathWord>,
    pub(crate) writes: Vec<PathWord>,
}

pub(crate) fn subcommand_paths(subcommand: &Subcommand, home: &Path) -> SubcommandPaths {
    let mut writes = write_words(&subcommand.words);
    let mut reads = subcommand
        .words
        .iter()
        .skip(1)
        .filter(|word| !writes.iter().any(|(written, _)| written == *word))
        .flat_map(|word| {
            let mut words = vec![(word.clone(), PathSource::Argument)];
            words.extend(option_value(word).map(|value| (value, PathSource::Argument)));
            words
        })
        .collect::<Vec<_>>();
    for (kind, target) in &subcommand.redirects {
        match kind {
            RedirectKind::Input => reads.push((target.clone(), PathSource::InputRedirect)),
            RedirectKind::Output => writes.push((target.clone(), PathSource::OutputRedirect)),
        }
    }
    let resolve_all = |words: Vec<(ShellWord, PathSource)>| {
        words
            .into_iter()
            .filter_map(|(word, source)| resolve(word, source, subcommand, home))
            .collect::<Vec<_>>()
    };
    SubcommandPaths {
        reads: resolve_all(reads),
        writes: resolve_all(writes),
    }
}

fn write_words(words: &[ShellWord]) -> Vec<(ShellWord, PathSource)> {
    let Some(command) = words.first() else {
        return Vec::new();
    };
    let arguments = &words[1..];
    let operands = operands(arguments);
    let as_arguments = |words: Vec<ShellWord>| {
        words
            .into_iter()
            .map(|word| (word, PathSource::Argument))
            .collect::<Vec<_>>()
    };
    match command.text.as_str() {
        name if OPERAND_WRITERS.contains(&name) => as_arguments(operands),
        "cp" => as_arguments(operands.last().cloned().into_iter().collect()),
        "tee" => operands
            .into_iter()
            .map(|word| (word, PathSource::Tee))
            .collect(),
        "sed" if arguments.iter().any(|word| is_in_place_option(&word.text)) => {
            as_arguments(operands)
        }
        "dd" => as_arguments(
            arguments
                .iter()
                .filter_map(|word| word_after_prefix(word, "of="))
                .collect(),
        ),
        _ => Vec::new(),
    }
}

fn operands(arguments: &[ShellWord]) -> Vec<ShellWord> {
    let mut operands = Vec::new();
    let mut options_ended = false;
    for word in arguments {
        if !options_ended && word.text == "--" {
            options_ended = true;
        } else if options_ended || !word.text.starts_with('-') || word.text == "-" {
            operands.push(word.clone());
        }
    }
    operands
}

fn is_in_place_option(text: &str) -> bool {
    text.starts_with("--in-place") || (text.starts_with("-i") && !text.starts_with("--"))
}

/// The value of `--option=value`, or of `if=` for `dd`.
fn option_value(word: &ShellWord) -> Option<ShellWord> {
    if word.text.starts_with("--") {
        let (_, value) = word.text.split_once('=')?;
        return word_after_prefix(word, &word.text[..word.text.len() - value.len()]);
    }
    word_after_prefix(word, "if=")
}

fn word_after_prefix(word: &ShellWord, prefix: &str) -> Option<ShellWord> {
    let text = word.text.strip_prefix(prefix)?;
    let without_parameters = word
        .without_parameters
        .strip_prefix(prefix)
        .unwrap_or_default();
    Some(ShellWord {
        text: text.to_owned(),
        without_parameters: without_parameters.to_owned(),
        ..word.clone()
    })
}

fn resolve(
    word: ShellWord,
    source: PathSource,
    subcommand: &Subcommand,
    home: &Path,
) -> Option<PathWord> {
    let text = word.text.as_str();
    if text.is_empty() || DEVICE_PATHS.contains(&text) || text.starts_with("/dev/fd/") {
        return None;
    }
    if word.has_parameter {
        return Some(PathWord {
            word,
            source,
            paths: Vec::new(),
            unknown_base: false,
        });
    }
    let absolute = text.starts_with('/') || text == "~" || text.starts_with("~/");
    let paths = if absolute {
        vec![shell_command::join_word(Path::new("/"), text, home)]
    } else {
        subcommand
            .folders
            .known
            .iter()
            .map(|folder| shell_command::join_word(folder, text, home))
            .collect()
    };
    let unknown_base = !absolute && subcommand.folders.unknown;
    Some(PathWord {
        word,
        source,
        paths,
        unknown_base,
    })
}

#[cfg(test)]
#[path = "shell_paths_tests.rs"]
mod tests;
