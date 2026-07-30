use super::{
    RememberedPermissionRemoveByPrefixOutcome, RememberedPermissionStore,
    RememberedPermissionStoreError, RememberedPermissionStoreErrorKind, WorkspacePermissionId,
};
use crate::ConfigContext;
use fs2::FileExt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

const STORE_FILE_NAME: &str = "permissions.json";
const LOCK_FILE_NAME: &str = ".permissions.lock";
const MAX_STORE_BYTES: u64 = 1_048_576;
const MAX_RULES_PER_PROJECT: usize = 256;

#[derive(Debug, Clone)]
pub struct RememberedPermissionFileStore {
    path: PathBuf,
}

impl RememberedPermissionFileStore {
    pub fn for_context(context: &ConfigContext) -> Self {
        Self {
            path: context.remembered_permissions_path(),
        }
    }

    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<RememberedPermissionStore, RememberedPermissionStoreError> {
        load_store(&self.path)
    }

    pub fn mutate<F>(&self, mutation: F) -> Result<(), RememberedPermissionStoreError>
    where
        F: FnOnce(&mut RememberedPermissionStore) -> Result<(), RememberedPermissionStoreError>,
    {
        self.mutate_with(|store| {
            mutation(store)?;
            Ok(())
        })
    }

    pub fn mutate_with<F, T>(&self, mutation: F) -> Result<T, RememberedPermissionStoreError>
    where
        F: FnOnce(&mut RememberedPermissionStore) -> Result<T, RememberedPermissionStoreError>,
    {
        let parent = self.parent_dir()?;
        fs::create_dir_all(parent)?;
        let _lock = PermissionStoreLock::acquire(&parent.join(LOCK_FILE_NAME))?;
        let mut store = load_store(&self.path)?;
        let output = mutation(&mut store)?;
        store.enforce_project_rule_limit(MAX_RULES_PER_PROJECT)?;
        write_store_atomically(&self.path, &store)?;
        Ok(output)
    }

    pub fn remove_rule_by_prefix(
        &self,
        workspace_id: &WorkspacePermissionId,
        rule_id_prefix: &str,
    ) -> Result<RememberedPermissionRemoveByPrefixOutcome, RememberedPermissionStoreError> {
        let parent = self.parent_dir()?;
        fs::create_dir_all(parent)?;
        let _lock = PermissionStoreLock::acquire(&parent.join(LOCK_FILE_NAME))?;
        let mut store = load_store(&self.path)?;
        let output = store.remove_rule_by_prefix(workspace_id, rule_id_prefix);
        if matches!(
            output,
            RememberedPermissionRemoveByPrefixOutcome::Removed(_)
        ) {
            store.enforce_project_rule_limit(MAX_RULES_PER_PROJECT)?;
            write_store_atomically(&self.path, &store)?;
        }
        Ok(output)
    }

    fn parent_dir(&self) -> Result<&Path, RememberedPermissionStoreError> {
        self.path.parent().ok_or_else(|| {
            RememberedPermissionStoreError::new(RememberedPermissionStoreErrorKind::Io)
        })
    }
}

struct PermissionStoreLock {
    _file: File,
}

impl PermissionStoreLock {
    fn acquire(path: &Path) -> Result<Self, RememberedPermissionStoreError> {
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        ensure_regular_file(path, &file.metadata()?)?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }
}

fn load_store(path: &Path) -> Result<RememberedPermissionStore, RememberedPermissionStoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RememberedPermissionStore::default());
        }
        Err(error) => return Err(error.into()),
    };
    reject_symlink(&metadata)?;
    if !metadata.file_type().is_file() {
        return Err(RememberedPermissionStoreError::new(
            RememberedPermissionStoreErrorKind::NotRegularFile,
        ));
    }
    if metadata.len() > MAX_STORE_BYTES {
        return Err(RememberedPermissionStoreError::new(
            RememberedPermissionStoreErrorKind::Oversized,
        ));
    }
    let file = open_regular_for_read(path)?;
    let mut reader = file.take(MAX_STORE_BYTES + 1);
    let mut raw = String::new();
    reader.read_to_string(&mut raw)?;
    if raw.len() as u64 > MAX_STORE_BYTES {
        return Err(RememberedPermissionStoreError::new(
            RememberedPermissionStoreErrorKind::Oversized,
        ));
    }
    RememberedPermissionStore::from_json_str(&raw)
}

fn open_regular_for_read(path: &Path) -> Result<File, RememberedPermissionStoreError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    ensure_regular_file(path, &file.metadata()?)?;
    Ok(file)
}

fn write_store_atomically(
    path: &Path,
    store: &RememberedPermissionStore,
) -> Result<(), RememberedPermissionStoreError> {
    let parent = path.parent().ok_or_else(|| {
        RememberedPermissionStoreError::new(RememberedPermissionStoreErrorKind::Io)
    })?;
    let temp_path = unique_temp_path(path);
    let write_result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path)?;
        let text = store.to_json_string()?;
        file.write_all(text.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        sync_dir(parent)?;
        Ok(())
    })();
    match write_result {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            let _ = sync_dir(parent);
            Err(error)
        }
    }
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!(".{STORE_FILE_NAME}.tmp-{}-{nanos}", process::id()))
}

fn ensure_regular_file(
    _path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), RememberedPermissionStoreError> {
    reject_symlink(metadata)?;
    if !metadata.file_type().is_file() {
        return Err(RememberedPermissionStoreError::new(
            RememberedPermissionStoreErrorKind::NotRegularFile,
        ));
    }
    Ok(())
}

fn reject_symlink(metadata: &fs::Metadata) -> Result<(), RememberedPermissionStoreError> {
    #[cfg(unix)]
    if metadata.file_type().is_symlink() {
        return Err(RememberedPermissionStoreError::new(
            RememberedPermissionStoreErrorKind::Symlink,
        ));
    }
    Ok(())
}

fn sync_dir(path: &Path) -> Result<(), RememberedPermissionStoreError> {
    File::open(path)?.sync_all()?;
    Ok(())
}
