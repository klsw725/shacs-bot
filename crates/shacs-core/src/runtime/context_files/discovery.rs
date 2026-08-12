use super::reader::read_context_file;
use super::types::{
    ContextFileDiscovery, ContextFileDiscoveryOptions, ContextFileReadStatus, ContextFileSource,
    DEFAULT_CONTEXT_FILE_NAMES,
};
use std::path::{Path, PathBuf};

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
            if path.exists() {
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
        let mut entry = read_context_file(
            entries.len(),
            path,
            filename,
            ContextFileSource::ConfiguredExtra,
            usize::MAX,
            &workspace_canonical,
            options.max_bytes,
        );
        let duplicate = entries.iter().find(|existing| {
            existing.path == entry.path
                || existing
                    .digest
                    .as_ref()
                    .zip(entry.digest.as_ref())
                    .is_some_and(|(left, right)| left.sha256 == right.sha256)
        });
        if let Some(existing) = duplicate {
            entry.status = ContextFileReadStatus::SkippedDuplicate;
            entry.reason = Some(format!("duplicate of {}", existing.path.display()));
            entry.content = None;
        }
        entries.push(entry);
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

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
