use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_staging_directory(root: &Path) -> PathBuf {
    let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
    root.join(format!("{}-{id}", std::process::id()))
}

pub(super) struct AnalyzerStagingLease {
    root: PathBuf,
    path: PathBuf,
}

impl AnalyzerStagingLease {
    pub(super) fn create(root: PathBuf, path: PathBuf) -> Result<Self, ()> {
        if fs::symlink_metadata(&root)
            .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(());
        }
        fs::create_dir_all(&root).map_err(|_| ())?;
        fs::create_dir(&path).map_err(|_| ())?;
        Ok(Self { root, path })
    }
}

impl Drop for AnalyzerStagingLease {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        let _ = fs::remove_dir(&self.root);
    }
}
