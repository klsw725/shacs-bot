//! The secret files of the active data folder: the auth store (Spec 048 PRD
//! 000 scope 5), `.env.json` and the config migrations' backups and secrets
//! marker (PRD 010 scope 9). They are kept out of folder scans, asked about
//! on every read and never written by the assistant's tools.

use super::{real_path, shell_command};
use std::fs;
use std::path::{Component, Path, PathBuf};

const AUTH_FILE: &str = "auth.json";
const AUTH_FOLDER: &str = "auth";
const MAX_SCANNED_FILES: usize = 4_096;
const MAX_SCAN_DEPTH: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretFiles {
    folders: Vec<PathBuf>,
    /// The secret file names directly in the data folder; each one's atomic
    /// save writes `.<name>.tmp-*` beside it.
    files: Vec<String>,
}

impl SecretFiles {
    pub fn new(data_folder: &Path, config_file: &Path) -> Self {
        let mut folders = vec![shell_command::normalize(data_folder)];
        let real = real_path::real_path(data_folder);
        if !folders.contains(&real) {
            folders.push(real);
        }
        let [config_backup, env_file_backup] =
            shacs_config::secrets_migration_backup_paths(config_file);
        let mut files = vec![AUTH_FILE.to_owned(), shacs_config::ENV_FILE_NAME.to_owned()];
        files.extend(
            [
                shacs_config::config_migration_backup_path(config_file),
                shacs_config::secrets_migration_marker_path(config_file),
                config_backup,
                env_file_backup,
            ]
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned()),
        );
        Self { folders, files }
    }

    /// Whether `path` is an auth store file under any spelling, a symlink to
    /// one, or a hard link to one.
    pub fn contains(&self, path: &Path) -> bool {
        let spellings = [shell_command::normalize(path), real_path::real_path(path)];
        spellings.iter().any(|spelling| {
            self.folders
                .iter()
                .any(|folder| self.is_secret_path(folder, spelling))
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
        let mut names = Vec::new();
        for file in &self.files {
            names.push(file.clone());
            names.push(temp_prefix(file));
        }
        for folder in &self.folders {
            let targets = self
                .files
                .iter()
                .map(|file| folder.join(file))
                .chain([folder.join(AUTH_FOLDER)]);
            for target in targets {
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
            .any(|folder| self.store_holds(folder, identity).unwrap_or(true))
    }

    #[cfg(not(unix))]
    fn hard_link(&self, _: &Path) -> bool {
        false
    }
}

impl SecretFiles {
    fn is_secret_name(&self, name: &str) -> bool {
        self.files
            .iter()
            .any(|file| name == file || name.starts_with(&temp_prefix(file)))
    }

    fn is_secret_path(&self, folder: &Path, path: &Path) -> bool {
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
            Some(name) if self.is_secret_name(name) => parts.next().is_none(),
            _ => false,
        }
    }

    /// Whether the secret files in `folder` include one with `identity`;
    /// `None` when they could not be read within the limits.
    #[cfg(unix)]
    fn store_holds(&self, folder: &Path, identity: (u64, u64)) -> Option<bool> {
        let mut budget = MAX_SCANNED_FILES;
        let Some(entries) = read_folder(folder)? else {
            return Some(false);
        };
        for entry in entries {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let found = if self.is_secret_name(&name) {
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
}

fn temp_prefix(file: &str) -> String {
    format!(".{file}.tmp-")
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
