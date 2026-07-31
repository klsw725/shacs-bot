pub mod attachments;
pub mod diagnostics;
mod diagnostics_sanitizer;
pub mod document;
pub mod gitstore;
pub mod media_decode;
pub mod path;
pub mod progress_events;
pub mod prompt_templates;
pub mod restart;
pub mod runtime;
pub mod searchusage;
pub mod text;
pub mod tool_hints;
pub mod tool_results;
pub mod worktree;

pub use path::abbreviate_path;
pub use text::ensure_dir;
pub use worktree::{
    build_git_worktree_merge_handoff, cleanup_git_worktree, collect_git_worktree_diff_evidence,
    create_git_worktree, GitWorktreeCleanupEvidence, GitWorktreeCreateEvidence,
    GitWorktreeCreateRequest, GitWorktreeDiffEvidence, GitWorktreeMergeHandoff,
    GitWorktreeMergeHandoffState,
};
