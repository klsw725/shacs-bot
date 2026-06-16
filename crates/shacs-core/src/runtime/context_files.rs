use super::context_safety::protected_context_path_reason;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFileDigest {
    pub sha256: String,
    pub byte_count: usize,
    pub token_estimate: usize,
}

pub fn discover_context_files(
    workspace_root: impl AsRef<Path>,
    options: ContextFileDiscoveryOptions,
) -> ContextFileDiscovery {
    let workspace_root = workspace_root.as_ref().to_path_buf();
    let workspace_canonical = canonicalize_or_self(&workspace_root);
    let mut entries = Vec::new();

    for (depth, directory) in
        discovery_directories(&workspace_canonical, options.current_dir.as_deref())
            .into_iter()
            .enumerate()
    {
        for filename in DEFAULT_CONTEXT_FILE_NAMES {
            let path = directory.join(filename);
            if !path.exists() {
                continue;
            }
            entries.push(read_context_file(
                entries.len(),
                path,
                filename.to_owned(),
                ContextFileSource::DefaultCandidate,
                depth,
                &workspace_canonical,
                options.max_bytes,
            ));
        }
    }

    let mut extras = options.extra_context_files;
    extras.sort();
    for extra in extras {
        let path = if extra.is_absolute() {
            extra
        } else {
            workspace_canonical.join(extra)
        };
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        entries.push(read_context_file(
            entries.len(),
            path,
            filename,
            ContextFileSource::ConfiguredExtra,
            usize::MAX,
            &workspace_canonical,
            options.max_bytes,
        ));
    }

    ContextFileDiscovery {
        workspace_root: workspace_canonical,
        entries,
    }
}

fn discovery_directories(workspace_root: &Path, current_dir: Option<&Path>) -> Vec<PathBuf> {
    let current = current_dir
        .map(canonicalize_or_self)
        .filter(|path| path.starts_with(workspace_root))
        .unwrap_or_else(|| workspace_root.to_path_buf());
    let relative = current
        .strip_prefix(workspace_root)
        .unwrap_or(Path::new(""));
    let mut directories = vec![workspace_root.to_path_buf()];
    let mut cursor = workspace_root.to_path_buf();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        directories.push(cursor.clone());
    }
    directories
}

fn read_context_file(
    order: usize,
    path: PathBuf,
    filename: String,
    source: ContextFileSource,
    source_directory_depth: usize,
    workspace_root: &Path,
    max_bytes: usize,
) -> ContextFileProjection {
    if !path.exists() {
        let mut entry = projection(
            order,
            path,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::SkippedMissing,
        );
        entry.reason = Some("context file candidate is missing".to_owned());
        return entry;
    }

    let Ok(canonical) = path.canonicalize() else {
        let mut entry = projection(
            order,
            path,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::ParseError,
        );
        entry.reason = Some("context file path could not be canonicalized".to_owned());
        return entry;
    };

    if !canonical.starts_with(workspace_root) {
        let mut entry = projection(
            order,
            path,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::DeniedBoundary,
        );
        entry.reason = Some("context file escapes workspace boundary".to_owned());
        return entry;
    }

    if let Some(reason) = protected_context_path_reason(&canonical) {
        let mut entry = projection(
            order,
            canonical,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::DeniedBoundary,
        );
        entry.reason = Some(reason.to_owned());
        return entry;
    }

    let Ok(metadata) = fs::metadata(&canonical) else {
        let mut entry = projection(
            order,
            canonical,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::ParseError,
        );
        entry.reason = Some("context file metadata could not be read".to_owned());
        return entry;
    };
    if !metadata.is_file() {
        let mut entry = projection(
            order,
            canonical,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::ParseError,
        );
        entry.reason = Some("context file candidate is not a regular file".to_owned());
        return entry;
    }

    let read_limit = max_bytes.saturating_add(1);
    let mut bytes = Vec::new();
    let read_result = File::open(&canonical).and_then(|mut file| {
        file.by_ref()
            .take(read_limit as u64)
            .read_to_end(&mut bytes)
    });
    if read_result.is_err() {
        let mut entry = projection(
            order,
            canonical,
            filename,
            source,
            source_directory_depth,
            ContextFileReadStatus::ParseError,
        );
        entry.reason = Some("context file content could not be read".to_owned());
        return entry;
    }

    let truncated = bytes.len() > max_bytes;
    if truncated {
        bytes.truncate(max_bytes);
    }
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let digest = ContextFileDigest {
        sha256: sha256_hex(&bytes),
        byte_count: bytes.len(),
        token_estimate: estimate_tokens(&content),
    };
    let mut entry = projection(
        order,
        canonical,
        filename,
        source,
        source_directory_depth,
        if truncated {
            ContextFileReadStatus::Truncated
        } else {
            ContextFileReadStatus::Included
        },
    );
    entry.reason = truncated.then(|| "context file exceeded max byte limit".to_owned());
    entry.digest = Some(digest);
    entry.content = Some(content);
    entry
}

fn projection(
    order: usize,
    path: PathBuf,
    filename: String,
    source: ContextFileSource,
    source_directory_depth: usize,
    status: ContextFileReadStatus,
) -> ContextFileProjection {
    ContextFileProjection {
        order,
        path,
        filename,
        source,
        source_directory_depth,
        status,
        reason: None,
        digest: None,
        content: None,
    }
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn estimate_tokens(content: &str) -> usize {
    content.split_whitespace().count().max(content.len() / 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn context_file_discovery_orders_nested_files_from_root_to_current(
    ) -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let nested = workspace.path().join("a/b");
        fs::create_dir_all(&nested)?;
        fs::write(workspace.path().join("AGENTS.md"), "root")?;
        fs::write(workspace.path().join("a/CLAUDE.md"), "middle")?;
        fs::write(nested.join(".shacs.md"), "leaf")?;

        let discovery = discover_context_files(
            workspace.path(),
            ContextFileDiscoveryOptions {
                current_dir: Some(nested),
                ..ContextFileDiscoveryOptions::default()
            },
        );

        let names = discovery
            .entries
            .iter()
            .map(|entry| entry.filename.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["AGENTS.md", "CLAUDE.md", ".shacs.md"]);
        assert!(discovery
            .entries
            .iter()
            .all(|entry| entry.status == ContextFileReadStatus::Included));
        Ok(())
    }

    #[test]
    fn context_file_discovery_keeps_duplicate_filenames_in_order() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let nested = workspace.path().join("a");
        fs::create_dir_all(&nested)?;
        fs::write(workspace.path().join("AGENTS.md"), "root")?;
        fs::write(nested.join("AGENTS.md"), "nested")?;

        let discovery = discover_context_files(
            workspace.path(),
            ContextFileDiscoveryOptions {
                current_dir: Some(nested),
                ..ContextFileDiscoveryOptions::default()
            },
        );

        assert_eq!(discovery.entries.len(), 2);
        assert_eq!(discovery.entries[0].source_directory_depth, 0);
        assert_eq!(discovery.entries[1].source_directory_depth, 1);
        assert_eq!(discovery.entries[0].filename, "AGENTS.md");
        assert_eq!(discovery.entries[1].filename, "AGENTS.md");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn context_file_discovery_denies_symlink_outside_workspace() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let outside_file = outside.path().join("AGENTS.md");
        fs::write(&outside_file, "outside")?;
        std::os::unix::fs::symlink(&outside_file, workspace.path().join("AGENTS.md"))?;

        let discovery =
            discover_context_files(workspace.path(), ContextFileDiscoveryOptions::default());

        assert_eq!(discovery.entries.len(), 1);
        assert_eq!(
            discovery.entries[0].status,
            ContextFileReadStatus::DeniedBoundary
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn context_file_discovery_denies_protected_symlink_target_inside_workspace(
    ) -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        let env_file = workspace.path().join(".env");
        fs::write(&env_file, "SECRET_TOKEN=raw")?;
        std::os::unix::fs::symlink(&env_file, workspace.path().join("AGENTS.md"))?;

        let discovery =
            discover_context_files(workspace.path(), ContextFileDiscoveryOptions::default());

        assert_eq!(discovery.entries.len(), 1);
        assert_eq!(
            discovery.entries[0].status,
            ContextFileReadStatus::DeniedBoundary
        );
        assert!(discovery.entries[0].content.is_none());
        assert!(discovery.entries[0]
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("protected"));
        Ok(())
    }

    #[test]
    fn context_file_discovery_truncates_oversized_files() -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join("AGENTS.md"), "0123456789")?;

        let discovery = discover_context_files(
            workspace.path(),
            ContextFileDiscoveryOptions {
                max_bytes: 4,
                ..ContextFileDiscoveryOptions::default()
            },
        );

        assert_eq!(discovery.entries.len(), 1);
        assert_eq!(
            discovery.entries[0].status,
            ContextFileReadStatus::Truncated
        );
        assert_eq!(discovery.entries[0].content.as_deref(), Some("0123"));
        assert_eq!(
            discovery.entries[0]
                .digest
                .as_ref()
                .map(|digest| digest.byte_count),
            Some(4)
        );
        Ok(())
    }

    #[test]
    fn context_file_discovery_orders_configured_extras_after_defaults_and_reports_missing(
    ) -> Result<(), Box<dyn Error>> {
        let workspace = tempfile::tempdir()?;
        fs::write(workspace.path().join("AGENTS.md"), "root")?;
        fs::write(workspace.path().join("z.md"), "z")?;
        fs::write(workspace.path().join("a.md"), "a")?;

        let discovery = discover_context_files(
            workspace.path(),
            ContextFileDiscoveryOptions {
                extra_context_files: vec![
                    PathBuf::from("z.md"),
                    PathBuf::from("missing.md"),
                    PathBuf::from("a.md"),
                ],
                ..ContextFileDiscoveryOptions::default()
            },
        );

        let names = discovery
            .entries
            .iter()
            .map(|entry| entry.filename.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["AGENTS.md", "a.md", "missing.md", "z.md"]);
        assert_eq!(
            discovery.entries[2].status,
            ContextFileReadStatus::SkippedMissing
        );
        Ok(())
    }
}
