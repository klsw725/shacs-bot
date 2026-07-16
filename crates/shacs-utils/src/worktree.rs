use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeCreateRequest {
    pub repo_path: PathBuf,
    pub worktree_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch_name: String,
    pub base_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeCreateEvidence {
    pub repo_root: PathBuf,
    pub worktree_path: PathBuf,
    pub branch_name: String,
    pub base_ref: String,
    pub base_commit: String,
    pub worktree_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeDiffEvidence {
    pub worktree_path: PathBuf,
    pub branch_name: String,
    pub base_ref: String,
    pub status_porcelain: String,
    pub changed_files: Vec<String>,
    pub diff_stat: String,
    pub diff_digest: String,
    pub redaction_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeMergeHandoff {
    pub state: GitWorktreeMergeHandoffState,
    pub worktree_ref: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub base_ref: String,
    pub diff_digest: String,
    pub changed_files: Vec<String>,
    pub instructions: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitWorktreeMergeHandoffState {
    PendingParentReview,
    BlockedNoDiffEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeCleanupEvidence {
    pub worktree_path: PathBuf,
    pub diagnostics_recorded: bool,
    pub removed: bool,
    pub message: String,
}

pub fn create_git_worktree(
    request: &GitWorktreeCreateRequest,
) -> Result<GitWorktreeCreateEvidence, String> {
    validate_branch_name(&request.branch_name)?;
    let trusted_root = common_path_prefix(&request.repo_path, &request.worktree_root);
    reject_existing_symlink_components(&trusted_root, &request.worktree_root)?;
    fs::create_dir_all(&request.worktree_root).map_err(|error| error.to_string())?;
    reject_existing_symlink_components(&trusted_root, &request.worktree_root)?;
    ensure_child_path(&request.worktree_root, &request.worktree_path)?;
    if let Some(parent) = request.worktree_path.parent() {
        reject_existing_symlink_components(&request.worktree_root, parent)?;
    }
    let repo_root = git_stdout(&request.repo_path, &["rev-parse", "--show-toplevel"])?;
    let repo_root = PathBuf::from(repo_root.trim());
    let base_commit = git_stdout(&repo_root, &["rev-parse", "--verify", &request.base_ref])?
        .trim()
        .to_owned();
    if request.worktree_path.exists() {
        return Err("worktree path already exists".to_owned());
    }
    if let Some(parent) = request.worktree_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let worktree_path = path_string(&request.worktree_path)?;
    git_stdout(
        &repo_root,
        &[
            "worktree",
            "add",
            "-b",
            &request.branch_name,
            &worktree_path,
            &request.base_ref,
        ],
    )?;

    Ok(GitWorktreeCreateEvidence {
        repo_root,
        worktree_path: request.worktree_path.clone(),
        branch_name: request.branch_name.clone(),
        base_ref: request.base_ref.clone(),
        base_commit,
        worktree_ref: format!("worktree://{}", request.branch_name),
    })
}

pub fn collect_git_worktree_diff_evidence(
    worktree_path: impl AsRef<Path>,
    branch_name: &str,
    base_ref: &str,
) -> Result<GitWorktreeDiffEvidence, String> {
    let worktree_path = worktree_path.as_ref();
    validate_branch_name(branch_name)?;
    validate_ref_name(base_ref)?;
    let status_porcelain = git_stdout(worktree_path, &["status", "--porcelain=v1"])?;
    let changed_files = parse_status_files(&status_porcelain);
    let tracked_diff_stat = git_stdout(worktree_path, &["diff", "--stat", base_ref, "--"])?;
    let untracked_files = untracked_git_files(worktree_path)?;
    let diff_stat = append_untracked_diff_stat(tracked_diff_stat, &untracked_files);
    let diff = git_stdout(worktree_path, &["diff", "--no-ext-diff", base_ref, "--"])?;
    let untracked_manifest = untracked_content_manifest(worktree_path, &untracked_files)?;
    let diff_digest = sha256_hex(combined_diff_evidence(&diff, &untracked_manifest).as_bytes());

    Ok(GitWorktreeDiffEvidence {
        worktree_path: worktree_path.to_path_buf(),
        branch_name: branch_name.to_owned(),
        base_ref: base_ref.to_owned(),
        status_porcelain,
        changed_files,
        diff_stat,
        diff_digest,
        redaction_status: "already_safe".to_owned(),
    })
}

pub fn build_git_worktree_merge_handoff(
    diff_evidence: &GitWorktreeDiffEvidence,
) -> GitWorktreeMergeHandoff {
    let state =
        if diff_evidence.changed_files.is_empty() && diff_evidence.diff_stat.trim().is_empty() {
            GitWorktreeMergeHandoffState::BlockedNoDiffEvidence
        } else {
            GitWorktreeMergeHandoffState::PendingParentReview
        };
    GitWorktreeMergeHandoff {
        state,
        worktree_ref: format!("worktree://{}", diff_evidence.branch_name),
        branch_name: diff_evidence.branch_name.clone(),
        worktree_path: diff_evidence.worktree_path.clone(),
        base_ref: diff_evidence.base_ref.clone(),
        diff_digest: diff_evidence.diff_digest.clone(),
        changed_files: diff_evidence.changed_files.clone(),
        instructions: "review diff evidence and run verifier gates before any manual merge"
            .to_owned(),
    }
}

pub fn cleanup_git_worktree(
    repo_path: impl AsRef<Path>,
    worktree_path: impl AsRef<Path>,
    diagnostics_recorded: bool,
) -> Result<GitWorktreeCleanupEvidence, String> {
    let worktree_path = worktree_path.as_ref();
    if !diagnostics_recorded {
        return Ok(GitWorktreeCleanupEvidence {
            worktree_path: worktree_path.to_path_buf(),
            diagnostics_recorded,
            removed: false,
            message: "cleanup blocked until diagnostics are recorded".to_owned(),
        });
    }
    let status_porcelain = git_stdout(worktree_path, &["status", "--porcelain=v1"])?;
    if !status_porcelain.trim().is_empty() {
        return Ok(GitWorktreeCleanupEvidence {
            worktree_path: worktree_path.to_path_buf(),
            diagnostics_recorded,
            removed: false,
            message: "cleanup blocked because dirty worktree evidence still requires review"
                .to_owned(),
        });
    }
    let worktree_arg = path_string(worktree_path)?;
    let remove_args = vec!["worktree", "remove", &worktree_arg];
    git_stdout(repo_path.as_ref(), &remove_args)?;
    Ok(GitWorktreeCleanupEvidence {
        worktree_path: worktree_path.to_path_buf(),
        diagnostics_recorded,
        removed: true,
        message: "clean worktree removed after diagnostics were recorded".to_owned(),
    })
}

fn ensure_child_path(root: &Path, child: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("worktree root is not accessible: {error}"))?;
    if !child.is_absolute() {
        return Err("worktree path must be absolute".to_owned());
    }
    let child_parent = child
        .parent()
        .ok_or_else(|| "worktree path must have a parent".to_owned())?
        .canonicalize()
        .map_err(|error| format!("worktree parent is not accessible: {error}"))?;
    if child_parent != root && !child_parent.starts_with(&root) {
        return Err("worktree path escapes worktree root".to_owned());
    }
    let child_name = child
        .file_name()
        .ok_or_else(|| "worktree path must include a directory name".to_owned())?;
    let normalized_child = child_parent.join(child_name);
    if normalized_child == root
        || normalized_child
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("worktree path escapes worktree root".to_owned());
    }
    Ok(())
}

fn reject_existing_symlink_components(base: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| "worktree path escapes trusted root".to_owned())?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "worktree path contains symlink component: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "worktree path component is not accessible: {}: {error}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

fn common_path_prefix(left: &Path, right: &Path) -> PathBuf {
    let mut prefix = PathBuf::new();
    for (left, right) in left.components().zip(right.components()) {
        if left != right {
            break;
        }
        prefix.push(left.as_os_str());
    }
    prefix
}

fn validate_branch_name(branch_name: &str) -> Result<(), String> {
    if branch_name.trim().is_empty()
        || branch_name.starts_with('-')
        || branch_name.contains("..")
        || branch_name.contains('@')
        || branch_name.contains('\\')
        || branch_name.contains(' ')
        || branch_name.ends_with('/')
        || branch_name.ends_with(".lock")
    {
        return Err("unsafe worktree branch name".to_owned());
    }
    Ok(())
}

fn validate_ref_name(ref_name: &str) -> Result<(), String> {
    if ref_name.trim().is_empty()
        || ref_name.starts_with('-')
        || ref_name.contains(char::is_whitespace)
        || ref_name.chars().any(char::is_control)
    {
        return Err("unsafe worktree base ref".to_owned());
    }
    Ok(())
}

fn parse_status_files(status: &str) -> Vec<String> {
    let mut files = status
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.split(" -> ").last().unwrap_or(path).to_owned())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn git_stdout(workdir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workdir)
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

fn untracked_git_files(worktree_path: &Path) -> Result<Vec<String>, String> {
    let mut files = git_stdout(
        worktree_path,
        &["ls-files", "--others", "--exclude-standard"],
    )?
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(str::to_owned)
    .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn append_untracked_diff_stat(mut diff_stat: String, untracked_files: &[String]) -> String {
    if untracked_files.is_empty() {
        return diff_stat;
    }
    if !diff_stat.is_empty() && !diff_stat.ends_with('\n') {
        diff_stat.push('\n');
    }
    for file in untracked_files {
        diff_stat.push_str(&format!(" {file} | untracked\n"));
    }
    diff_stat
}

fn untracked_content_manifest(worktree_path: &Path, files: &[String]) -> Result<String, String> {
    let mut lines = Vec::new();
    for file in files {
        let object = git_stdout(worktree_path, &["hash-object", "--", file])?
            .trim()
            .to_owned();
        lines.push(format!("untracked {object} {file}"));
    }
    Ok(lines.join("\n"))
}

fn combined_diff_evidence(diff: &str, untracked_manifest: &str) -> String {
    if diff.is_empty() {
        untracked_manifest.to_owned()
    } else if untracked_manifest.is_empty() {
        diff.to_owned()
    } else {
        format!("{diff}\n{untracked_manifest}")
    }
}

fn path_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "path is not valid UTF-8".to_owned())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_path_escape_is_blocked_before_git_effect() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let request = GitWorktreeCreateRequest {
            repo_path: root.path().to_path_buf(),
            worktree_root: root.path().to_path_buf(),
            worktree_path: outside.path().join("child"),
            branch_name: "workflow/child-1".to_owned(),
            base_ref: "HEAD".to_owned(),
        };

        let error = create_git_worktree(&request).expect_err("escaped worktree path is rejected");
        assert!(error.contains("escapes worktree root"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn worktree_symlink_root_is_blocked_before_git_effect() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let symlink_root = root.path().join("workflow-worktrees");
        symlink(outside.path(), &symlink_root)?;
        let request = GitWorktreeCreateRequest {
            repo_path: root.path().to_path_buf(),
            worktree_root: symlink_root.clone(),
            worktree_path: symlink_root.join("child"),
            branch_name: "workflow/child-symlink".to_owned(),
            base_ref: "HEAD".to_owned(),
        };

        let error = create_git_worktree(&request).expect_err("symlinked worktree root is rejected");
        assert!(error.contains("symlink component"));
        assert!(!outside.path().join("child").exists());
        Ok(())
    }

    #[test]
    fn worktree_create_diff_handoff_and_cleanup_are_evidence_backed(
    ) -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("git").arg("--version").output().is_err() {
            return Ok(());
        }
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        let worktrees = root.path().join("worktrees");
        fs::create_dir_all(&repo)?;
        git_stdout(&repo, &["init"])?;
        fs::write(repo.join("README.md"), "base\n")?;
        git_stdout(&repo, &["add", "README.md"])?;
        git_stdout(&repo, &["commit", "-m", "initial"])?;

        let worktree_path = worktrees.join("child-1");
        let create = create_git_worktree(&GitWorktreeCreateRequest {
            repo_path: repo.clone(),
            worktree_root: worktrees.clone(),
            worktree_path: worktree_path.clone(),
            branch_name: "workflow/child-1".to_owned(),
            base_ref: "HEAD".to_owned(),
        })?;
        assert_eq!(create.worktree_ref, "worktree://workflow/child-1");
        assert!(worktrees.exists());
        fs::write(worktree_path.join("README.md"), "base\nchild\n")?;

        let diff = collect_git_worktree_diff_evidence(&worktree_path, "workflow/child-1", "HEAD")?;
        assert_eq!(diff.changed_files, vec!["README.md".to_owned()]);
        assert!(diff.diff_stat.contains("README.md"));
        assert_ne!(diff.diff_digest, sha256_hex(b""));

        let handoff = build_git_worktree_merge_handoff(&diff);
        assert_eq!(
            handoff.state,
            GitWorktreeMergeHandoffState::PendingParentReview
        );
        assert!(handoff.instructions.contains("before any manual merge"));
        assert_eq!(git_stdout(&repo, &["status", "--porcelain=v1"])?, "");

        let blocked = cleanup_git_worktree(&repo, &worktree_path, false)?;
        assert!(!blocked.removed);
        assert!(worktree_path.exists());
        fs::write(worktree_path.join("README.md"), "base\n")?;
        let cleanup = cleanup_git_worktree(&repo, &worktree_path, true)?;
        assert!(cleanup.removed);
        assert!(!worktree_path.exists());
        Ok(())
    }

    #[test]
    fn worktree_diff_evidence_includes_untracked_files() -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("git").arg("--version").output().is_err() {
            return Ok(());
        }
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        let worktrees = root.path().join("worktrees");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&worktrees)?;
        git_stdout(&repo, &["init"])?;
        fs::write(repo.join("README.md"), "base\n")?;
        git_stdout(&repo, &["add", "README.md"])?;
        git_stdout(&repo, &["commit", "-m", "initial"])?;

        let worktree_path = worktrees.join("child-untracked");
        create_git_worktree(&GitWorktreeCreateRequest {
            repo_path: repo.clone(),
            worktree_root: worktrees.clone(),
            worktree_path: worktree_path.clone(),
            branch_name: "workflow/child-untracked".to_owned(),
            base_ref: "HEAD".to_owned(),
        })?;
        fs::write(worktree_path.join("NEW.md"), "untracked evidence\n")?;

        let diff =
            collect_git_worktree_diff_evidence(&worktree_path, "workflow/child-untracked", "HEAD")?;

        assert_eq!(diff.changed_files, vec!["NEW.md".to_owned()]);
        assert!(diff.diff_stat.contains("NEW.md | untracked"));
        assert_ne!(diff.diff_digest, sha256_hex(b"\n"));
        let handoff = build_git_worktree_merge_handoff(&diff);
        assert_eq!(
            handoff.state,
            GitWorktreeMergeHandoffState::PendingParentReview
        );
        cleanup_git_worktree(&repo, &worktree_path, true)?;
        Ok(())
    }

    #[test]
    fn worktree_clean_diff_blocks_merge_handoff_without_evidence(
    ) -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("git").arg("--version").output().is_err() {
            return Ok(());
        }
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        let worktrees = root.path().join("worktrees");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&worktrees)?;
        git_stdout(&repo, &["init"])?;
        fs::write(repo.join("README.md"), "base\n")?;
        git_stdout(&repo, &["add", "README.md"])?;
        git_stdout(&repo, &["commit", "-m", "initial"])?;

        let worktree_path = worktrees.join("child-clean");
        create_git_worktree(&GitWorktreeCreateRequest {
            repo_path: repo.clone(),
            worktree_root: worktrees.clone(),
            worktree_path: worktree_path.clone(),
            branch_name: "workflow/child-clean".to_owned(),
            base_ref: "HEAD".to_owned(),
        })?;

        let diff =
            collect_git_worktree_diff_evidence(&worktree_path, "workflow/child-clean", "HEAD")?;
        let handoff = build_git_worktree_merge_handoff(&diff);

        assert_eq!(diff.changed_files, Vec::<String>::new());
        assert_eq!(
            handoff.state,
            GitWorktreeMergeHandoffState::BlockedNoDiffEvidence
        );
        cleanup_git_worktree(&repo, &worktree_path, true)?;
        Ok(())
    }

    #[test]
    fn worktree_cleanup_preserves_dirty_tree_after_diagnostics(
    ) -> Result<(), Box<dyn std::error::Error>> {
        if Command::new("git").arg("--version").output().is_err() {
            return Ok(());
        }
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        let worktrees = root.path().join("worktrees");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&worktrees)?;
        git_stdout(&repo, &["init"])?;
        fs::write(repo.join("README.md"), "base\n")?;
        git_stdout(&repo, &["add", "README.md"])?;
        git_stdout(&repo, &["commit", "-m", "initial"])?;

        let worktree_path = worktrees.join("child-dirty");
        create_git_worktree(&GitWorktreeCreateRequest {
            repo_path: repo.clone(),
            worktree_root: worktrees.clone(),
            worktree_path: worktree_path.clone(),
            branch_name: "workflow/child-dirty".to_owned(),
            base_ref: "HEAD".to_owned(),
        })?;
        fs::write(worktree_path.join("README.md"), "base\ndirty\n")?;

        let cleanup = cleanup_git_worktree(&repo, &worktree_path, true)?;

        assert!(!cleanup.removed);
        assert!(cleanup
            .message
            .contains("dirty worktree evidence still requires review"));
        assert!(worktree_path.exists());
        fs::write(worktree_path.join("README.md"), "base\n")?;
        cleanup_git_worktree(&repo, &worktree_path, true)?;
        Ok(())
    }
}
