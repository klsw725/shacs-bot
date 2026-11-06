//! The marker files only the user changes in auto mode (Spec 048 PRD 050
//! scope 8): the one that turns the setup hint off and the one that records
//! the first notice. An auto mode call that writes them, or removes or
//! moves the folder that holds them, is refused at row 1. So is a write
//! whose glob may match them and a command whose code text names them.

use super::{real_path, shell_command};
use std::path::{Path, PathBuf};

/// The data folder's subfolder that holds the markers.
pub const MARKER_FOLDER: &str = "runtime";
/// Present once the first notice that auto mode is on by default was sent.
pub const DEFAULT_NOTICE_MARKER: &str = "auto-mode-default-notice";
/// Present while the user keeps the setup hint off.
pub const SETUP_HINT_OFF_MARKER: &str = "auto-mode-setup-hint-off";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserOnlyFiles {
    /// The marker folder under its written and its real spelling.
    folders: Vec<PathBuf>,
}

impl UserOnlyFiles {
    pub fn new(data_folder: &Path) -> Self {
        let folder = data_folder.join(MARKER_FOLDER);
        let mut folders = vec![shell_command::normalize(&folder)];
        let real = real_path::real_path(&folder);
        if !folders.contains(&real) {
            folders.push(real);
        }
        Self { folders }
    }

    /// Whether writing `path` changes a marker or replaces the folder that
    /// holds them, under any spelling or through a symlink.
    pub fn written_by(&self, path: &Path) -> bool {
        let spellings = [shell_command::normalize(path), real_path::real_path(path)];
        spellings.iter().any(|spelling| {
            self.folders.iter().any(|folder| {
                let marker = spelling.parent() == Some(folder.as_path())
                    && spelling.file_name().is_some_and(|name| {
                        name == DEFAULT_NOTICE_MARKER || name == SETUP_HINT_OFF_MARKER
                    });
                spelling == folder || marker
            })
        })
    }

    /// Whether a write to a path written with a glob may reach a marker or
    /// the folder that holds them. A bracket or brace part may match
    /// anything.
    pub fn pattern_may_write(&self, pattern: &Path) -> bool {
        let pattern = shell_command::normalize(pattern);
        let parts = pattern
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let literal = parts.iter().take_while(|part| !is_glob(part)).count();
        let real_prefix = real_path::real_path(&parts[..literal].iter().collect::<PathBuf>());
        let real = real_prefix
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .chain(parts[literal..].iter().cloned())
            .collect::<Vec<_>>();
        self.folders.iter().any(|folder| {
            let targets = [
                folder.clone(),
                folder.join(DEFAULT_NOTICE_MARKER),
                folder.join(SETUP_HINT_OFF_MARKER),
            ];
            targets
                .iter()
                .any(|target| matches_path(&parts, target) || matches_path(&real, target))
        })
    }
}

/// Whether text that a command runs as code names a marker.
pub fn names_a_marker(text: &str) -> bool {
    text.contains(DEFAULT_NOTICE_MARKER) || text.contains(SETUP_HINT_OFF_MARKER)
}

fn is_glob(part: &str) -> bool {
    part.contains(['*', '?', '[', '{'])
}

fn matches_path(pattern: &[String], target: &Path) -> bool {
    let target = target
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    pattern.len() == target.len()
        && pattern
            .iter()
            .zip(&target)
            .all(|(pattern, name)| matches_name(pattern, name))
}

/// `*` and `?` as the shell matches them within one name; a part with a
/// bracket or brace counts as matching.
fn matches_name(pattern: &str, name: &str) -> bool {
    if !is_glob(pattern) {
        return pattern == name;
    }
    if pattern.contains(['[', '{']) {
        return true;
    }
    let pattern = pattern.chars().collect::<Vec<_>>();
    let name = name.chars().collect::<Vec<_>>();
    let mut reachable = vec![false; name.len() + 1];
    reachable[0] = true;
    for symbol in pattern {
        let mut next = vec![false; name.len() + 1];
        for (index, &reached) in reachable.iter().enumerate() {
            if !reached {
                continue;
            }
            match symbol {
                '*' => {
                    for later in next.iter_mut().skip(index) {
                        *later = true;
                    }
                }
                '?' if index < name.len() => next[index + 1] = true,
                literal if index < name.len() && name[index] == literal => next[index + 1] = true,
                _ => {}
            }
        }
        reachable = next;
    }
    reachable[name.len()]
}

/// Where the data folder keeps one marker.
pub fn marker_path(data_folder: &Path, marker: &str) -> PathBuf {
    data_folder.join(MARKER_FOLDER).join(marker)
}
