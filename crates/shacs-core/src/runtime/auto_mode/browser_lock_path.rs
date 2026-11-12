//! The browser's lock files and the folder that holds them (Spec 050 PRD
//! 020 scope 6). The browser and `browser forget` tell each other apart by
//! these paths, so a call that writes, removes, moves or replaces one is
//! refused in every mode, whether or not the files exist yet. Reading and
//! listing them is not refused. The list is kept apart from the markers
//! only the user changes: those are judged in auto mode alone.

use super::{real_path, shell_command, user_only_file};
use std::path::{Path, PathBuf};

/// The data folder's subfolder the browser keeps its files in.
pub const BROWSER_FOLDER: &str = "browser";
/// The lock files directly in [`BROWSER_FOLDER`].
pub const BROWSER_LOCK_FILES: [&str; 3] = ["active.lock", "forget.lock", "profile.lock"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowserLockPaths {
    /// The browser folder under its written and its real spelling.
    folders: Vec<PathBuf>,
}

impl BrowserLockPaths {
    pub fn new(data_folder: &Path) -> Self {
        let folder = data_folder.join(BROWSER_FOLDER);
        let mut folders = vec![shell_command::normalize(&folder)];
        let real = real_path::real_path(&folder);
        if !folders.contains(&real) {
            folders.push(real);
        }
        Self { folders }
    }

    /// Whether writing `path` changes a lock file or replaces the folder
    /// that holds them, under any spelling or through a symlink.
    pub fn written_by(&self, path: &Path) -> bool {
        let spellings = [shell_command::normalize(path), real_path::real_path(path)];
        spellings.iter().any(|spelling| {
            self.folders.iter().any(|folder| {
                let lock = spelling.parent() == Some(folder.as_path())
                    && spelling
                        .file_name()
                        .is_some_and(|name| BROWSER_LOCK_FILES.iter().any(|file| name == *file));
                spelling == folder || lock
            })
        })
    }

    /// Whether a write to a path written with a glob may reach a lock file
    /// or the folder that holds them.
    pub fn pattern_may_write(&self, pattern: &Path) -> bool {
        let targets = self
            .folders
            .iter()
            .flat_map(|folder| {
                std::iter::once(folder.clone())
                    .chain(BROWSER_LOCK_FILES.iter().map(|file| folder.join(file)))
            })
            .collect::<Vec<_>>();
        user_only_file::pattern_reaches(pattern, &targets)
    }

    /// Whether text that a command runs as code names a lock file by its
    /// absolute path or by its path relative to a working folder.
    pub(crate) fn named_in(&self, text: &str, working_folders: &[PathBuf]) -> bool {
        self.folders.iter().any(|folder| {
            BROWSER_LOCK_FILES.iter().any(|file| {
                let target = folder.join(file);
                let relative = working_folders.iter().any(|working| {
                    target.strip_prefix(working).is_ok_and(|relative| {
                        let relative = relative.display().to_string();
                        !relative.is_empty() && text.contains(&relative)
                    })
                });
                relative || text.contains(&target.display().to_string())
            })
        })
    }
}
