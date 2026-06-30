use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub summary: String,
    pub timestamp: String,
    pub files_changed: Vec<String>,
}

impl CommitInfo {
    pub fn format(&self) -> String {
        let files = if self.files_changed.is_empty() {
            "no files".to_owned()
        } else {
            self.files_changed.join(", ")
        };
        format!(
            "{} {} — {} ({files})",
            self.sha, self.timestamp, self.summary
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineAge {
    pub age_days: u64,
}

pub trait GitStore {
    fn is_initialized(&self) -> bool;
    fn init(&self) -> Result<bool, String>;
    fn auto_commit(&self, message: &str) -> Result<Option<String>, String>;
    fn log(&self, max_entries: usize) -> Result<Vec<CommitInfo>, String>;
    fn line_ages(&self, relative_path: &str) -> Result<Option<Vec<LineAge>>, String>;
    fn diff_commits(&self, from: &str, to: &str) -> Result<Option<String>, String>;
    fn show_commit_diff(&self, sha: &str) -> Result<Option<String>, String>;
    fn revert(&self, sha: &str) -> Result<Option<String>, String>;
}

#[derive(Debug, Clone)]
pub struct GitCliStore {
    workspace: PathBuf,
    tracked_files: Vec<String>,
}

impl GitCliStore {
    pub fn new(
        workspace: impl Into<PathBuf>,
        tracked_files: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            tracked_files: tracked_files.into_iter().collect(),
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn tracked_files(&self) -> &[String] {
        &self.tracked_files
    }

    pub fn find_commit(
        &self,
        short_sha: &str,
        max_entries: usize,
    ) -> Result<Option<CommitInfo>, String> {
        Ok(self
            .log(max_entries)?
            .into_iter()
            .find(|commit| commit.sha.starts_with(short_sha)))
    }

    pub fn show_commit_with_diff(
        &self,
        short_sha: &str,
        max_entries: usize,
    ) -> Result<Option<(CommitInfo, String)>, String> {
        let commits = self.log(max_entries)?;
        for (index, commit) in commits.iter().enumerate() {
            if commit.sha.starts_with(short_sha) {
                let diff = if let Some(parent) = commits.get(index + 1) {
                    self.diff_commits(&parent.sha, &commit.sha)?
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                return Ok(Some((commit.clone(), diff)));
            }
        }
        Ok(None)
    }

    fn run_git(&self, args: &[&str]) -> Result<String, String> {
        run_git_command(&self.workspace, args)
    }

    fn run_git_status(&self, args: &[&str]) -> Result<Output, String> {
        Command::new("git")
            .args(args)
            .current_dir(&self.workspace)
            .env("GIT_AUTHOR_NAME", "shacs-bot")
            .env("GIT_AUTHOR_EMAIL", "shacs-bot@local")
            .env("GIT_COMMITTER_NAME", "shacs-bot")
            .env("GIT_COMMITTER_EMAIL", "shacs-bot@local")
            .output()
            .map_err(|error| error.to_string())
    }

    fn gitignore_content(&self) -> String {
        let mut dirs = self
            .tracked_files
            .iter()
            .filter_map(|file| Path::new(file).parent())
            .filter_map(|parent| parent.to_str())
            .filter(|parent| !parent.is_empty() && *parent != ".")
            .map(str::to_owned)
            .collect::<Vec<_>>();
        dirs.sort();
        dirs.dedup();

        let mut lines = vec!["/*".to_owned()];
        lines.extend(dirs.into_iter().map(|dir| format!("!{dir}/")));
        lines.extend(self.tracked_files.iter().map(|file| format!("!{file}")));
        lines.push("!.gitignore".to_owned());
        format!("{}\n", lines.join("\n"))
    }

    fn merge_gitignore(&self) -> Result<(), String> {
        let path = self.workspace.join(".gitignore");
        let desired = self.gitignore_content();
        if path.exists() {
            let existing = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let existing_lines = existing.lines().collect::<BTreeSet<_>>();
            let new_lines = desired
                .lines()
                .filter(|line| !existing_lines.contains(line))
                .collect::<Vec<_>>();
            if !new_lines.is_empty() {
                fs::write(
                    &path,
                    format!("{}\n{}\n", existing.trim_end(), new_lines.join("\n")),
                )
                .map_err(|error| error.to_string())?;
            }
        } else {
            fs::write(&path, desired).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn ensure_tracked_files_exist(&self) -> Result<(), String> {
        for file in &self.tracked_files {
            let path = self.workspace.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            if !path.exists() {
                fs::write(&path, "").map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn stage_tracked_files(&self, include_gitignore: bool) -> Result<(), String> {
        if !include_gitignore && self.tracked_files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["add", "--"];
        if include_gitignore {
            args.push(".gitignore");
        }
        args.extend(self.tracked_files.iter().map(String::as_str));
        self.run_git(&args).map(|_| ())
    }

    fn has_staged_tracked_changes(&self) -> Result<bool, String> {
        let mut args = vec!["status", "--porcelain", "--"];
        args.push(".gitignore");
        args.extend(self.tracked_files.iter().map(String::as_str));
        Ok(!self.run_git(&args)?.trim().is_empty())
    }

    fn resolve_sha(&self, short_sha: &str) -> Option<String> {
        self.run_git(&["rev-parse", "--verify", short_sha])
            .ok()
            .map(|sha| sha.trim().to_owned())
            .filter(|sha| !sha.is_empty())
    }

    fn files_changed_for_commit(&self, sha: &str) -> Result<Vec<String>, String> {
        let output = self.run_git(&["show", "--name-only", "--pretty=format:", sha])?;
        Ok(output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

impl GitStore for GitCliStore {
    fn is_initialized(&self) -> bool {
        self.workspace.join(".git").is_dir() || self.workspace.join(".git").is_file()
    }

    fn init(&self) -> Result<bool, String> {
        if self.is_initialized() {
            return Ok(false);
        }
        if is_inside_parent_git_repo(&self.workspace) {
            return Ok(false);
        }
        fs::create_dir_all(&self.workspace).map_err(|error| error.to_string())?;
        self.run_git(&["init", "--template="])?;
        self.merge_gitignore()?;
        self.ensure_tracked_files_exist()?;
        self.stage_tracked_files(true)?;
        self.run_git(&["commit", "-m", "init: shacs-bot memory store"])?;
        Ok(true)
    }

    fn auto_commit(&self, message: &str) -> Result<Option<String>, String> {
        if !self.is_initialized() {
            return Ok(None);
        }
        self.stage_tracked_files(false)?;
        if !self.has_staged_tracked_changes()? {
            return Ok(None);
        }
        self.run_git(&["commit", "-m", message])?;
        Ok(Some(
            self.run_git(&["rev-parse", "--short=8", "HEAD"])?
                .trim()
                .to_owned(),
        ))
    }

    fn log(&self, max_entries: usize) -> Result<Vec<CommitInfo>, String> {
        if !self.is_initialized() || max_entries == 0 {
            return Ok(Vec::new());
        }
        let count = max_entries.to_string();
        let output = self.run_git(&[
            "log",
            "--abbrev=8",
            "--date=format:%Y-%m-%d %H:%M",
            "--pretty=format:%h%x1f%s%x1f%ad",
            "-n",
            &count,
        ])?;
        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut parts = line.split('\x1f');
                let sha = parts.next().unwrap_or_default().to_owned();
                let summary = parts.next().unwrap_or_default().to_owned();
                let timestamp = parts.next().unwrap_or_default().to_owned();
                let files_changed = self.files_changed_for_commit(&sha)?;
                Ok(CommitInfo {
                    sha,
                    summary,
                    timestamp,
                    files_changed,
                })
            })
            .collect()
    }

    fn line_ages(&self, relative_path: &str) -> Result<Option<Vec<LineAge>>, String> {
        if !self.is_initialized() || !self.workspace.join(relative_path).is_file() {
            return Ok(None);
        }
        let output = self.run_git_status(&["blame", "--line-porcelain", relative_path])?;
        if !output.status.success() {
            return Ok(None);
        }
        let now_days = current_unix_days();
        let ages = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("author-time "))
            .filter_map(|raw_time| raw_time.parse::<u64>().ok())
            .map(|seconds| LineAge {
                age_days: now_days.saturating_sub(seconds / 86_400),
            })
            .collect::<Vec<_>>();
        Ok((!ages.is_empty()).then_some(ages))
    }

    fn diff_commits(&self, from: &str, to: &str) -> Result<Option<String>, String> {
        if !self.is_initialized() {
            return Ok(None);
        }
        let Some(from) = self.resolve_sha(from) else {
            return Ok(None);
        };
        let Some(to) = self.resolve_sha(to) else {
            return Ok(None);
        };
        Ok(Some(self.run_git(&["diff", &from, &to, "--"])?))
    }

    fn show_commit_diff(&self, sha: &str) -> Result<Option<String>, String> {
        if !self.is_initialized() || self.resolve_sha(sha).is_none() {
            return Ok(None);
        }
        Ok(Some(self.run_git(&["show", "--format=", sha, "--"])?))
    }

    fn revert(&self, sha: &str) -> Result<Option<String>, String> {
        if !self.is_initialized() {
            return Ok(None);
        }
        let Some(full_sha) = self.resolve_sha(sha) else {
            return Ok(None);
        };
        let parent = format!("{full_sha}^");
        if self.resolve_sha(&parent).is_none() {
            return Ok(None);
        }
        let mut args = vec!["checkout", parent.as_str(), "--"];
        args.extend(self.tracked_files.iter().map(String::as_str));
        self.run_git(&args)?;
        self.auto_commit(&format!("revert: undo {sha}"))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoGitStore;

impl GitStore for NoGitStore {
    fn is_initialized(&self) -> bool {
        false
    }

    fn init(&self) -> Result<bool, String> {
        Ok(false)
    }

    fn auto_commit(&self, _message: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn log(&self, _max_entries: usize) -> Result<Vec<CommitInfo>, String> {
        Ok(Vec::new())
    }

    fn line_ages(&self, _relative_path: &str) -> Result<Option<Vec<LineAge>>, String> {
        Ok(None)
    }

    fn diff_commits(&self, _from: &str, _to: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn show_commit_diff(&self, _sha: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn revert(&self, _sha: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
}

pub fn is_nested_git_path(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
}

fn is_inside_parent_git_repo(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .any(|ancestor| ancestor.join(".git").exists())
}

fn run_git_command(workspace: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .env("GIT_AUTHOR_NAME", "shacs-bot")
        .env("GIT_AUTHOR_EMAIL", "shacs-bot@local")
        .env("GIT_COMMITTER_NAME", "shacs-bot")
        .env("GIT_COMMITTER_EMAIL", "shacs-bot@local")
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        Err(message)
    }
}

fn current_unix_days() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() / 86_400)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn no_git_store_is_safe_noop() {
        let git = NoGitStore;
        assert!(!git.is_initialized());
        assert_eq!(git.auto_commit("msg"), Ok(None));
        assert_eq!(git.log(10), Ok(Vec::new()));
        assert_eq!(git.line_ages("memory/MEMORY.md"), Ok(None));
    }

    #[test]
    fn commit_info_formats_summary() {
        let info = CommitInfo {
            sha: "abc123".to_owned(),
            summary: "dream: update".to_owned(),
            timestamp: "2026-05-03".to_owned(),
            files_changed: vec!["memory/MEMORY.md".to_owned()],
        };
        assert!(info.format().contains("abc123 2026-05-03"));
        assert!(info.format().contains("memory/MEMORY.md"));
    }

    #[test]
    fn git_cli_store_initializes_commits_logs_blames_and_reverts() -> Result<(), String> {
        if Command::new("git").arg("--version").output().is_err() {
            return Ok(());
        }
        let workspace = temporary_workspace()?;
        let store = GitCliStore::new(
            &workspace,
            [
                "SOUL.md".to_owned(),
                "USER.md".to_owned(),
                "memory/MEMORY.md".to_owned(),
            ],
        );

        assert!(store.init()?);
        assert!(!store.init()?);
        assert!(store.is_initialized());
        let gitignore =
            fs::read_to_string(workspace.join(".gitignore")).map_err(|error| error.to_string())?;
        assert!(gitignore.contains("/*"));
        assert!(gitignore.contains("!memory/"));

        fs::write(workspace.join("memory/MEMORY.md"), "first\nsecond\n")
            .map_err(|error| error.to_string())?;
        let first_sha = store
            .auto_commit("dream: update memory")?
            .ok_or_else(|| "missing first commit".to_owned())?;
        let log = store.log(10)?;
        assert!(log.iter().any(|commit| commit.sha == first_sha));
        let ages = store
            .line_ages("memory/MEMORY.md")?
            .ok_or_else(|| "missing line ages".to_owned())?;
        assert_eq!(ages.len(), 2);
        assert!(store
            .show_commit_diff(&first_sha)?
            .unwrap_or_default()
            .contains("first"));

        fs::write(workspace.join("memory/MEMORY.md"), "third\n")
            .map_err(|error| error.to_string())?;
        let second_sha = store
            .auto_commit("dream: second")?
            .ok_or_else(|| "missing second commit".to_owned())?;
        assert!(store
            .diff_commits(&first_sha, &second_sha)?
            .unwrap_or_default()
            .contains("third"));
        let revert_sha = store
            .revert(&second_sha)?
            .ok_or_else(|| "missing revert commit".to_owned())?;
        assert_ne!(revert_sha, second_sha);
        let restored = fs::read_to_string(workspace.join("memory/MEMORY.md"))
            .map_err(|error| error.to_string())?;
        assert!(restored.contains("first"));
        Ok(())
    }

    #[test]
    fn git_cli_store_adds_dash_prefixed_paths_as_pathspecs() -> Result<(), String> {
        if Command::new("git").arg("--version").output().is_err() {
            return Ok(());
        }
        let workspace = temporary_workspace()?;
        let store = GitCliStore::new(&workspace, ["-memory.md".to_owned()]);

        assert!(store.init()?);
        fs::write(workspace.join("-memory.md"), "dash path\n")
            .map_err(|error| error.to_string())?;
        assert!(store.auto_commit("dream: dash path")?.is_some());
        assert!(store
            .log(3)?
            .iter()
            .any(|commit| commit.files_changed.iter().any(|file| file == "-memory.md")));
        Ok(())
    }

    fn temporary_workspace() -> Result<PathBuf, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "shacs-utils-git-{}-{nanos}-{counter}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|error| error.to_string())?;
        }
        fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }
}
