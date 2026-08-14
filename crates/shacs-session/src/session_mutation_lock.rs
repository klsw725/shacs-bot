use fs4::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

#[derive(Debug)]
pub struct SessionMutationGuard {
    file: File,
}

impl SessionMutationGuard {
    pub fn acquire(workspace: &Path, session_key: &str) -> std::io::Result<Self> {
        let locks_dir = workspace.join(".session-mutation-locks");
        std::fs::create_dir_all(&locks_dir)?;
        let path = locks_dir.join(format!(
            "{}.lock",
            crate::SessionManager::safe_key(session_key)
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        FileExt::lock(&file)?;
        Ok(Self { file })
    }
}

impl Drop for SessionMutationGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}
