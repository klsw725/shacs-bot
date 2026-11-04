//! Reads a tokenized shell command into subcommands (Spec 048 PRD 000
//! scope 6): leading assignments and wrappers are set apart, `cd` targets are
//! tracked, and shell `-c` scripts are read once more.

use super::shell_token::{self, RedirectKind, ShellToken, ShellWord};
use std::path::{Path, PathBuf};

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh"];
const RESERVED_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "select", "function",
];
const UNFOLLOWED_COMMANDS: &[&str] = &["eval", "source", ".", "exec"];
const MAX_WORKING_FOLDERS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellCommand {
    pub(crate) analyzable: bool,
    pub(crate) subcommands: Vec<Subcommand>,
    /// Text searched letter by letter for secret files and deny or ask paths.
    pub(crate) text_regions: Vec<String>,
    /// Text that could not be analyzed, searched for `exec` deny or ask
    /// patterns and for `rm` words.
    pub(crate) unanalyzed_text: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Subcommand {
    pub(crate) assignments: Vec<String>,
    /// The command word and its arguments, wrappers removed.
    pub(crate) words: Vec<ShellWord>,
    pub(crate) redirects: Vec<(RedirectKind, ShellWord)>,
    pub(crate) folders: WorkingFolders,
    /// The words come from a `-c` script that was given arguments, so its
    /// positional parameters have values.
    pub(crate) positional_bound: bool,
}

/// The folders a subcommand's relative paths may resolve against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkingFolders {
    pub(crate) known: Vec<PathBuf>,
    pub(crate) unknown: bool,
}

impl Subcommand {
    pub(crate) fn command_word(&self) -> Option<&str> {
        self.words.first().map(|word| word.text.as_str())
    }

    /// The words joined by one space, for `exec` pattern matching. Leading
    /// assignments are kept only for allow rules, so an assignment blocks them.
    pub(crate) fn text(&self, keep_assignments: bool) -> String {
        let words = self.words.iter().map(|word| word.text.as_str());
        if keep_assignments {
            self.assignments
                .iter()
                .map(String::as_str)
                .chain(words)
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            words.collect::<Vec<_>>().join(" ")
        }
    }
}

pub(crate) fn analyze(command: &str, working_folder: &Path, home: &Path) -> ShellCommand {
    let folders = WorkingFolders {
        known: vec![working_folder.to_path_buf()],
        unknown: false,
    };
    let mut analysis = ShellCommand {
        analyzable: true,
        subcommands: Vec::new(),
        text_regions: Vec::new(),
        unanalyzed_text: Vec::new(),
    };
    if read_into(command, folders, home, false, 0, &mut analysis).is_err() {
        analysis = ShellCommand {
            analyzable: false,
            subcommands: Vec::new(),
            text_regions: vec![command.to_owned()],
            unanalyzed_text: vec![command.to_owned()],
        };
    }
    analysis
}

fn read_into(
    command: &str,
    mut folders: WorkingFolders,
    home: &Path,
    positional_bound: bool,
    depth: usize,
    analysis: &mut ShellCommand,
) -> Result<(), shell_token::Unanalyzable> {
    let tokens = shell_token::tokenize(command)?;
    for segment in tokens.split(|token| matches!(token, ShellToken::Separator(_))) {
        let mut subcommand = Subcommand {
            assignments: Vec::new(),
            words: Vec::new(),
            redirects: Vec::new(),
            folders: folders.clone(),
            positional_bound,
        };
        let mut bodies = Vec::new();
        for token in segment {
            match token {
                ShellToken::Word(word) => subcommand.words.push(word.clone()),
                ShellToken::Redirect {
                    kind,
                    target: Some(target),
                } => subcommand.redirects.push((*kind, target.clone())),
                ShellToken::HereDoc { body } => bodies.push(body.clone()),
                ShellToken::HereString { word } => bodies.push(word.text.clone()),
                ShellToken::Redirect { target: None, .. } | ShellToken::Separator(_) => {}
            }
        }
        split_assignments(&mut subcommand);
        strip_wrappers(&mut subcommand.words);
        let command_word = subcommand.command_word().unwrap_or_default().to_owned();
        if RESERVED_WORDS.contains(&command_word.as_str())
            || UNFOLLOWED_COMMANDS.contains(&command_word.as_str())
        {
            return Err(shell_token::Unanalyzable);
        }
        if matches!(command_word.as_str(), "cd" | "pushd" | "popd") {
            folders = folders_after_cd(&folders, &subcommand.words, home);
        }
        let shell = SHELLS.contains(&command_word.as_str());
        if !shell {
            analysis.text_regions.extend(bodies.iter().cloned());
            analysis
                .text_regions
                .extend(code_arguments(&subcommand.words));
        }
        if !subcommand.words.is_empty() || !subcommand.redirects.is_empty() {
            analysis.subcommands.push(subcommand.clone());
        }
        if shell {
            read_shell_scripts(&subcommand, &bodies, home, depth, analysis);
        }
    }
    Ok(())
}

fn split_assignments(subcommand: &mut Subcommand) {
    let count = subcommand
        .words
        .iter()
        .take_while(|word| is_assignment(&word.text))
        .count();
    subcommand.assignments = subcommand
        .words
        .drain(..count)
        .map(|word| word.text)
        .collect();
}

fn is_assignment(text: &str) -> bool {
    let Some((name, _)) = text.split_once('=') else {
        return false;
    };
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn strip_wrappers(words: &mut Vec<ShellWord>) {
    loop {
        let Some(first) = words.first() else {
            return;
        };
        let second_is_option = words.get(1).is_some_and(|word| word.text.starts_with('-'));
        let skip = match first.text.as_str() {
            "timeout" => leading_options(words, &["-s", "-k", "--signal", "--kill-after"]) + 1,
            "time" => leading_options(words, &["-o", "--output"]),
            "nice" => leading_options(words, &["-n", "--adjustment"]),
            "stdbuf" => leading_options(words, &["-i", "-o", "-e"]),
            "command"
                if !matches!(
                    words.get(1).map(|word| word.text.as_str()),
                    Some("-v" | "-V")
                ) =>
            {
                leading_options(words, &[])
            }
            "nohup" | "builtin" | "noglob" | "!" => 1,
            "xargs" if !second_is_option => 1,
            _ => return,
        };
        if skip >= words.len() {
            return;
        }
        words.drain(..skip);
    }
}

/// Counts the wrapper word and the options after it. Options listed in
/// `with_value` take the next word as their value.
fn leading_options(words: &[ShellWord], with_value: &[&str]) -> usize {
    let mut index = 1;
    while let Some(word) = words.get(index) {
        let text = word.text.as_str();
        if text == "--" {
            return index + 1;
        }
        if !text.starts_with('-') || text == "-" {
            break;
        }
        index += if with_value.contains(&text) { 2 } else { 1 };
    }
    index
}

fn read_shell_scripts(
    subcommand: &Subcommand,
    bodies: &[String],
    home: &Path,
    depth: usize,
    analysis: &mut ShellCommand,
) {
    let mut scripts = bodies
        .iter()
        .map(|body| (body.clone(), false))
        .collect::<Vec<_>>();
    if let Some(index) = script_argument_index(&subcommand.words) {
        let bound = index + 1 < subcommand.words.len();
        scripts.push((subcommand.words[index].text.clone(), bound));
    }
    for (script, bound) in scripts {
        let mut inner = ShellCommand {
            analyzable: true,
            subcommands: Vec::new(),
            text_regions: Vec::new(),
            unanalyzed_text: Vec::new(),
        };
        let nested_shell = |command: &ShellCommand| {
            command.subcommands.iter().any(|sub| {
                sub.command_word()
                    .is_some_and(|word| SHELLS.contains(&word))
            })
        };
        let read = read_into(
            &script,
            subcommand.folders.clone(),
            home,
            bound,
            depth + 1,
            &mut inner,
        );
        if depth > 0 || read.is_err() || nested_shell(&inner) {
            analysis.text_regions.push(script.clone());
            analysis.unanalyzed_text.push(script);
        } else {
            analysis.subcommands.extend(inner.subcommands);
            analysis.text_regions.extend(inner.text_regions);
            analysis.unanalyzed_text.extend(inner.unanalyzed_text);
        }
    }
}

fn script_argument_index(words: &[ShellWord]) -> Option<usize> {
    let mut index = 1;
    while let Some(word) = words.get(index) {
        let text = word.text.as_str();
        if matches!(text, "-o" | "+o" | "-O" | "+O") {
            index += 2;
            continue;
        }
        if !text.starts_with('-') || text.starts_with("--") {
            return None;
        }
        if text[1..].contains('c') {
            return (index + 1 < words.len()).then_some(index + 1);
        }
        index += 1;
    }
    None
}

/// The quoted code after `-c` or `-e`, as in `python3 -c "..."` or `node -e '...'`.
fn code_arguments(words: &[ShellWord]) -> Vec<String> {
    words
        .windows(2)
        .filter(|pair| matches!(pair[0].text.as_str(), "-c" | "-e") && pair[1].quoted)
        .map(|pair| pair[1].text.clone())
        .collect()
}

fn folders_after_cd(folders: &WorkingFolders, words: &[ShellWord], home: &Path) -> WorkingFolders {
    let target = words
        .iter()
        .skip(1)
        .find(|word| !word.text.starts_with('-'));
    let mut next = folders.clone();
    match target {
        _ if words.first().is_some_and(|word| word.text == "popd") => next.unknown = true,
        None => next.known.push(home.to_path_buf()),
        Some(word) if word.has_parameter || word.unquoted_glob || word.text == "-" => {
            next.unknown = true;
        }
        Some(word) => {
            let targets = folders
                .known
                .iter()
                .map(|folder| join_word(folder, &word.text, home))
                .collect::<Vec<_>>();
            next.known.extend(targets);
        }
    }
    next.known.dedup();
    if next.known.len() > MAX_WORKING_FOLDERS {
        next.known.truncate(MAX_WORKING_FOLDERS);
        next.unknown = true;
    }
    next
}

/// Resolves a word written as a path against `folder`, expanding `~`.
pub(crate) fn join_word(folder: &Path, text: &str, home: &Path) -> PathBuf {
    let joined = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        folder.join(text)
    };
    normalize(&joined)
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

#[cfg(test)]
#[path = "shell_command_tests.rs"]
mod tests;
