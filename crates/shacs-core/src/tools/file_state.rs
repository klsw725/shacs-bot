use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReadState {
    pub modified_at: SystemTime,
    pub offset: u64,
    pub limit: Option<usize>,
    pub content_hash: Option<String>,
    pub can_dedup: bool,
}

#[derive(Debug, Default)]
pub struct FileState {
    reads: HashMap<PathBuf, FileReadState>,
}

impl FileState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_read(&mut self, path: impl AsRef<Path>, offset: u64, limit: Option<usize>) {
        let path = normalize_path(path);
        let Ok(metadata) = fs::metadata(&path) else {
            return;
        };
        let Ok(modified_at) = metadata.modified() else {
            return;
        };
        self.reads.insert(
            path.clone(),
            FileReadState {
                modified_at,
                offset,
                limit,
                content_hash: hash_file(&path),
                can_dedup: true,
            },
        );
    }

    pub fn record_write(&mut self, path: impl AsRef<Path>) {
        let path = normalize_path(path);
        let Ok(metadata) = fs::metadata(&path) else {
            self.reads.remove(&path);
            return;
        };
        let Ok(modified_at) = metadata.modified() else {
            self.reads.remove(&path);
            return;
        };
        self.reads.insert(
            path.clone(),
            FileReadState {
                modified_at,
                offset: 1,
                limit: None,
                content_hash: hash_file(&path),
                can_dedup: false,
            },
        );
    }

    pub fn check_read(&mut self, path: impl AsRef<Path>) -> Option<String> {
        let path = normalize_path(path);
        let Some(entry) = self.reads.get_mut(&path) else {
            return Some(
                "Warning: file has not been read yet. Read it first to verify content before editing."
                    .to_owned(),
            );
        };
        let Ok(metadata) = fs::metadata(&path) else {
            return None;
        };
        let Ok(modified_at) = metadata.modified() else {
            return None;
        };

        if modified_at != entry.modified_at {
            if entry.content_hash.is_some() && hash_file(&path) == entry.content_hash {
                entry.modified_at = modified_at;
                return None;
            }
            return Some(
                "Warning: file has been modified since last read. Re-read to verify content before editing."
                    .to_owned(),
            );
        }

        if entry.content_hash.is_some() && hash_file(&path) != entry.content_hash {
            return Some(
                "Warning: file has been modified since last read. Re-read to verify content before editing."
                    .to_owned(),
            );
        }
        None
    }

    pub fn is_unchanged(
        &mut self,
        path: impl AsRef<Path>,
        offset: u64,
        limit: Option<usize>,
    ) -> bool {
        let path = normalize_path(path);
        let Some(entry) = self.reads.get_mut(&path) else {
            return false;
        };
        if !entry.can_dedup || entry.offset != offset || entry.limit != limit {
            return false;
        }
        let Ok(metadata) = fs::metadata(&path) else {
            return false;
        };
        let Ok(modified_at) = metadata.modified() else {
            return false;
        };
        if modified_at != entry.modified_at {
            let current_hash = hash_file(&path);
            if current_hash != entry.content_hash {
                entry.can_dedup = false;
                return false;
            }
            entry.can_dedup = false;
            return true;
        }
        if let Some(recorded_hash) = &entry.content_hash {
            if hash_file(&path).as_ref() == Some(recorded_hash) {
                return true;
            }
            entry.can_dedup = false;
            return false;
        }

        entry.can_dedup = false;
        false
    }

    pub fn clear(&mut self) {
        self.reads.clear();
    }
}

fn normalize_path(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_path_buf())
}

fn hash_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let digest = Sha256::digest(bytes);
    Some(format!("{digest:x}"))
}
