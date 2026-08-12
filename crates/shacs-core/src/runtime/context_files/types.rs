use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_CONTEXT_FILE_NAMES: [&str; 5] = [
    "AGENTS.md",
    "CLAUDE.md",
    ".cursorrules",
    ".shacs.md",
    ".shacs-bot.md",
];
pub const DEFAULT_CONTEXT_FILE_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFileDiscoveryOptions {
    pub current_dir: Option<PathBuf>,
    pub extra_context_files: Vec<PathBuf>,
    pub max_bytes: usize,
}

impl Default for ContextFileDiscoveryOptions {
    fn default() -> Self {
        Self {
            current_dir: None,
            extra_context_files: Vec::new(),
            max_bytes: DEFAULT_CONTEXT_FILE_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileDiscovery {
    pub workspace_root: PathBuf,
    pub entries: Vec<ContextFileProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileProjection {
    pub order: usize,
    pub path: PathBuf,
    pub filename: String,
    pub source: ContextFileSource,
    pub source_directory_depth: usize,
    pub status: ContextFileReadStatus,
    pub reason: Option<String>,
    pub digest: Option<ContextFileDigest>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFileSource {
    DefaultCandidate,
    ConfiguredExtra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFileReadStatus {
    Included,
    SkippedMissing,
    DeniedBoundary,
    Truncated,
    ParseError,
    SkippedDuplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileDigest {
    pub sha256: String,
    pub byte_count: usize,
    pub token_estimate: usize,
}
