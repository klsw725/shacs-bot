//! The auth store of the active data folder (Spec 048 PRD 000 scope 5): kept
//! out of folder scans, asked about on every read, never written by the
//! assistant's tools.

use super::{real_path, shell_command};
use std::fs;
use std::path::{Component, Path, PathBuf};

const AUTH_FILE: &str = "auth.json";
const AUTH_TEMP_PREFIX: &str = ".auth.json.tmp-";
const AUTH_FOLDER: &str = "auth";
const MAX_SCANNED_FILES: usize = 4_096;
const MAX_SCAN_DEPTH: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretFiles {
    folders: Vec<PathBuf>,
}

impl SecretFiles {
    pub fn new(data_folder: &Path) -> Self {
        let mut folders = vec![shell_command::normalize(data_folder)];
        let real = real_path::real_path(data_folder);
        if !folders.contains(&real) {
            folders.push(real);
        }
        Self { folders }
    }

    /// Whether `path` is an auth store file under any spelling, a symlink to
    /// one, or a hard link to one.
    pub fn contains(&self, path: &Path) -> bool {
        let spellings = [shell_command::normalize(path), real_path::real_path(path)];
        spellings.iter().any(|spelling| {
            self.folders
                .iter()
                .any(|folder| is_auth_store_path(folder, spelling))
        }) || self.hard_link(path)
    }

    /// How a question names the file: its place in the data folder, or the
    /// file name of a link to it.
    pub(crate) fn display_name(&self, path: &Path) -> String {
        let spellings = [shell_command::normalize(path), real_path::real_path(path)];
        spellings
            .iter()
            .find_map(|spelling| {
                self.folders
                    .iter()
                    .find_map(|folder| spelling.strip_prefix(folder).ok())
                    .map(|relative| relative.display().to_string())
            })
            .or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| path.display().to_string())
    }

    /// Text that names a secret file: absolute paths, paths relative to the
    /// given working folders, and the dotted file names.
    pub(crate) fn names(&self, working_folders: &[PathBuf]) -> Vec<String> {
        let mut names = vec![AUTH_FILE.to_owned(), AUTH_TEMP_PREFIX.to_owned()];
        for folder in &self.folders {
            for target in [folder.join(AUTH_FILE), folder.join(AUTH_FOLDER)] {
                names.push(target.display().to_string());
                for working in working_folders {
                    if let Ok(relative) = target.strip_prefix(working) {
                        names.push(relative.display().to_string());
                    }
                }
            }
        }
        names.retain(|name| !name.is_empty());
        names.sort();
        names.dedup();
        names
    }

    /// The store is read again for each linked file, because saving the auth
    /// store replaces its files; a store that cannot be read whole within the
    /// limits counts as holding the file.
    #[cfg(unix)]
    fn hard_link(&self, path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if !metadata.is_file() || metadata.nlink() < 2 {
            return false;
        }
        let identity = (metadata.dev(), metadata.ino());
        self.folders
            .iter()
            .any(|folder| store_holds(folder, identity).unwrap_or(true))
    }

    #[cfg(not(unix))]
    fn hard_link(&self, _: &Path) -> bool {
        false
    }
}

fn is_auth_store_path(folder: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(folder) else {
        return false;
    };
    let mut parts = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        });
    match parts.next() {
        Some(AUTH_FOLDER) => true,
        Some(name) if name == AUTH_FILE || name.starts_with(AUTH_TEMP_PREFIX) => {
            parts.next().is_none()
        }
        _ => false,
    }
}

/// Whether the auth store in `folder` holds a file with `identity`; `None`
/// when the store could not be read within the limits.
#[cfg(unix)]
fn store_holds(folder: &Path, identity: (u64, u64)) -> Option<bool> {
    let mut budget = MAX_SCANNED_FILES;
    let Some(entries) = read_folder(folder)? else {
        return Some(false);
    };
    for entry in entries {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let found = if name == AUTH_FILE || name.starts_with(AUTH_TEMP_PREFIX) {
            is_identity(&entry.path(), identity)?
        } else if name == AUTH_FOLDER {
            tree_holds(&entry.path(), identity, 0, &mut budget)?
        } else {
            false
        };
        if found {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(unix)]
fn tree_holds(
    folder: &Path,
    identity: (u64, u64),
    depth: usize,
    budget: &mut usize,
) -> Option<bool> {
    if depth > MAX_SCAN_DEPTH {
        return None;
    }
    let Some(entries) = read_folder(folder)? else {
        return Some(false);
    };
    for entry in entries {
        let path = entry.ok()?.path();
        *budget = budget.checked_sub(1)?;
        let metadata = fs::symlink_metadata(&path).ok()?;
        let found = if metadata.is_dir() {
            tree_holds(&path, identity, depth + 1, budget)?
        } else {
            metadata.is_file() && is_identity(&path, identity)?
        };
        if found {
            return Some(true);
        }
    }
    Some(false)
}

/// The folder's entries; `Some(None)` when it does not exist, `None` when it
/// cannot be read.
#[cfg(unix)]
fn read_folder(folder: &Path) -> Option<Option<fs::ReadDir>> {
    match fs::read_dir(folder) {
        Ok(entries) => Some(Some(entries)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(None),
        Err(_) => None,
    }
}

#[cfg(unix)]
fn is_identity(path: &Path, identity: (u64, u64)) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    match fs::symlink_metadata(path) {
        Ok(metadata) => Some((metadata.dev(), metadata.ino()) == identity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    }
}

#[cfg(test)]
#[path = "secret_file_tests.rs"]
mod tests;
