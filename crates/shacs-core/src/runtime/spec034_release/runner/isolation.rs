use super::super::model::{CleanupReceipt, Spec034ReleaseArtifactError};
use super::super::tools::control::{acquire_control, ControlLease};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static PROCESS_CACHE_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
type CleanupHook = Box<dyn FnOnce(&Path)>;
#[cfg(test)]
thread_local! {
    static INJECT_CLEANUP_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECT_MONITOR_UNCERTAINTY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CLEANUP_HOOK: std::cell::RefCell<Option<CleanupHook>> = const { std::cell::RefCell::new(None) };
}

mod cache_root;
mod locking;
mod root;
mod root_identity;
mod root_monitor;
use locking::{canonical_missing_path, lock_cache, lock_target};
use root::RetainedRoot;

#[cfg(test)]
#[path = "isolation_cache_test.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "isolation_test.rs"]
mod tests;

#[cfg(test)]
#[path = "isolation/root_race_test.rs"]
mod root_race_tests;

pub(super) struct RunnerIsolation {
    _control: ControlLease,
    _lock: File,
    _process_cache_lock: MutexGuard<'static, ()>,
    _cache_lock: File,
    root: Option<RetainedRoot>,
    source_parent: PathBuf,
    home: PathBuf,
    cargo_home: PathBuf,
    target: PathBuf,
    tools: PathBuf,
    cache_tools: PathBuf,
    #[cfg(test)]
    owner: u32,
    #[cfg(test)]
    test_cache_root: Option<PathBuf>,
}

pub(super) struct CompletedIsolationCleanup {
    binding_digest: String,
    raw_evidence_cleaned: bool,
    leak_count: u8,
    leak_summary: Vec<String>,
}

impl CompletedIsolationCleanup {
    pub(super) fn receipt(&self, run_id: &str) -> CleanupReceipt {
        CleanupReceipt {
            schema: "spec034.cleanup.v2".to_owned(),
            run_id: run_id.to_owned(),
            raw_evidence_cleaned: self.raw_evidence_cleaned,
            leak_count: self.leak_count,
            leak_summary: self.leak_summary.clone(),
            cleanup_binding_digest: self.binding_digest.clone(),
        }
    }

    pub(super) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    pub(super) fn verify_receipt(
        &self,
        receipt: &CleanupReceipt,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        (receipt.schema == "spec034.cleanup.v2"
            && receipt.raw_evidence_cleaned == self.raw_evidence_cleaned
            && receipt.leak_count == self.leak_count
            && receipt.leak_summary == self.leak_summary
            && receipt.cleanup_binding_digest == self.binding_digest)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)
    }
}

impl RunnerIsolation {
    #[cfg(test)]
    pub(super) fn inject_next_cleanup_failure() {
        INJECT_CLEANUP_FAILURE.set(true);
    }

    #[cfg(test)]
    pub(super) fn inject_next_monitor_uncertainty() {
        INJECT_MONITOR_UNCERTAINTY.set(true);
    }

    #[cfg(test)]
    pub(super) fn inject_next_cleanup_hook(hook: impl FnOnce(&Path) + 'static) {
        CLEANUP_HOOK.with(|slot| slot.replace(Some(Box::new(hook))));
    }

    #[cfg(test)]
    pub(super) fn inject_next_pre_unlink_hook(hook: impl FnOnce(&Path) + 'static) {
        RetainedRoot::inject_next_pre_unlink_hook(hook);
    }

    pub(super) fn prepare(
        repo_root: &Path,
        evidence_root: &Path,
        cache_root: Option<&Path>,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let repo = repo_root
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let parent = repo
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        let lock = lock_target(&repo, evidence_root)?;
        let control = acquire_control(parent)?;
        let process_cache_lock = PROCESS_CACHE_LOCK
            .lock()
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let root = tempfile::Builder::new()
            .prefix(".shacs-spec034-run-")
            .tempdir_in(parent)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let cache_key = super::super::tools::tool_cache_key()?;
        let cache_base = cache_root::resolve(&repo, cache_root)?;
        let cache_lock = lock_cache(&cache_base, &cache_key)?;
        let cache = cache_base.join("objects").join(cache_key);
        std::fs::create_dir_all(&cache).map_err(Spec034ReleaseArtifactError::Io)?;
        let home = root.path().join("home");
        let cargo_home = root.path().join("cargo-home");
        let source_parent = root.path().join("source");
        let target = root.path().join("target");
        let tools = root.path().join("toolchain/tools");
        let cache_tools = cache.join("toolchain/tools");
        for path in [&home, &cargo_home, &source_parent, &target, &tools, &cache_tools] {
            std::fs::create_dir_all(path).map_err(Spec034ReleaseArtifactError::Io)?;
        }
        let retained_root = RetainedRoot::capture(root.path())?;
        #[cfg(test)]
        let owner = retained_root.owner();
        let _ = root.keep();
        Ok(Self {
            _control: control,
            _lock: lock,
            _process_cache_lock: process_cache_lock,
            _cache_lock: cache_lock,
            root: Some(retained_root),
            source_parent,
            home,
            cargo_home,
            target,
            tools,
            cache_tools,
            #[cfg(test)]
            owner,
            #[cfg(test)]
            test_cache_root: cache_root.map(Path::to_path_buf),
        })
    }

    pub(super) fn source_parent(&self) -> &Path {
        &self.source_parent
    }

    pub(super) fn home(&self) -> &Path {
        &self.home
    }

    pub(super) fn cargo_home(&self) -> &Path {
        &self.cargo_home
    }

    pub(super) fn target(&self) -> &Path {
        &self.target
    }

    pub(super) fn tools(&self) -> &Path {
        &self.tools
    }

    pub(super) fn cache_tools(&self) -> &Path {
        &self.cache_tools
    }

    pub(super) fn cleanup(
        mut self,
    ) -> Result<CompletedIsolationCleanup, Spec034ReleaseArtifactError> {
        #[cfg(test)]
        if INJECT_CLEANUP_FAILURE.replace(false) {
            return Err(Spec034ReleaseArtifactError::CleanupFailed(Box::new(
                Spec034ReleaseArtifactError::Io(std::io::Error::other(
                    "injected isolation cleanup failure",
                )),
            )));
        }
        #[cfg(test)]
        CLEANUP_HOOK.with(|slot| {
            if let Some(hook) = slot.take() {
                if let Some(root) = &self.root {
                    hook(root.path());
                }
            }
        });
        #[cfg(test)]
        let event_uncertain = INJECT_MONITOR_UNCERTAINTY.replace(false);
        #[cfg(not(test))]
        let event_uncertain = false;
        let root = self
            .root
            .take()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        let binding_digest = root.cleanup(event_uncertain)?;
        #[cfg(test)]
        if let Some(cache_root) = self.test_cache_root.take() {
            #[cfg(unix)]
            restore_owner_directories(&cache_root, self.owner)?;
            std::fs::remove_dir_all(cache_root).map_err(Spec034ReleaseArtifactError::Io)?;
        }
        Ok(CompletedIsolationCleanup {
            binding_digest,
            raw_evidence_cleaned: true,
            leak_count: 0,
            leak_summary: Vec::new(),
        })
    }
}

impl Drop for RunnerIsolation {
    fn drop(&mut self) {
        #[cfg(test)]
        if let Some(cache_root) = &self.test_cache_root {
            #[cfg(unix)]
            let _ = restore_owner_directories(cache_root, self.owner);
            let _ = std::fs::remove_dir_all(cache_root);
        }
    }
}

#[cfg(all(test, unix))]
fn restore_owner_directories(path: &Path, owner: u32) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = std::fs::symlink_metadata(path).map_err(Spec034ReleaseArtifactError::Io)?;
    if !metadata.file_type().is_dir() {
        return Ok(());
    }
    if metadata.uid() != owner {
        return Err(Spec034ReleaseArtifactError::CleanupIdentityMismatch);
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(Spec034ReleaseArtifactError::Io)?;
    for entry in std::fs::read_dir(path).map_err(Spec034ReleaseArtifactError::Io)? {
        restore_owner_directories(
            &entry.map_err(Spec034ReleaseArtifactError::Io)?.path(),
            owner,
        )?;
    }
    Ok(())
}

#[cfg(all(test, not(unix)))]
fn restore_owner_directories(_path: &Path, _owner: u32) -> Result<(), Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}
