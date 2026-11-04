use super::shell_command;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_LINKS: usize = 16;

/// The path with symlinks followed (Spec 048 PRD 000 scope 4). A path that
/// does not exist yet keeps its missing tail on top of the nearest existing
/// folder, and a dangling symlink on the way is followed to where a write
/// would create its target.
pub(crate) fn real_path(path: &Path) -> PathBuf {
    resolve(path, 0)
}

fn resolve(path: &Path, links: usize) -> PathBuf {
    let normalized = shell_command::normalize(path);
    let mut tail: Vec<OsString> = Vec::new();
    let mut current = normalized.clone();
    loop {
        if let Ok(mut real) = fs::canonicalize(&current) {
            real.extend(tail.iter().rev());
            return real;
        }
        if links < MAX_LINKS {
            if let Ok(target) = fs::read_link(&current) {
                let mut next = current.parent().unwrap_or(Path::new("/")).join(target);
                next.extend(tail.iter().rev());
                return resolve(&next, links + 1);
            }
        }
        let Some(name) = current.file_name() else {
            return normalized;
        };
        tail.push(name.to_owned());
        if !current.pop() {
            return normalized;
        }
    }
}
