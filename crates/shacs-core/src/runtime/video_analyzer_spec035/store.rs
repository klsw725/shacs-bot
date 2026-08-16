use shacs_projection::Spec035MediaProjection;
use std::fmt::{Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_PROJECTION_BYTES: u64 = 64 * 1024;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum Spec035MediaProjectionStoreError {
    Io(io::Error),
    InvalidRecord,
    CommitStatusUnknown(Spec035MediaProjectionTransactionStage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035MediaProjectionTransactionStage {
    Renamed,
}

impl Display for Spec035MediaProjectionStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "media projection store I/O failed: {error}"),
            Self::InvalidRecord => formatter.write_str("media projection store record is invalid"),
            Self::CommitStatusUnknown(stage) => {
                write!(formatter, "media projection commit status unknown after {stage:?}")
            }
        }
    }
}

impl std::error::Error for Spec035MediaProjectionStoreError {}

impl From<io::Error> for Spec035MediaProjectionStoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct Spec035MediaProjectionStore {
    path: PathBuf,
}

impl Spec035MediaProjectionStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: data_dir
                .as_ref()
                .join("media")
                .join("projections")
                .join("current.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self) -> Result<Option<Spec035MediaProjection>, Spec035MediaProjectionStoreError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let file = match options.open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_PROJECTION_BYTES {
            return Err(Spec035MediaProjectionStoreError::InvalidRecord);
        }
        let capacity = usize::try_from(metadata.len())
            .map_err(|_| Spec035MediaProjectionStoreError::InvalidRecord)?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(MAX_PROJECTION_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len())
            .map_err(|_| Spec035MediaProjectionStoreError::InvalidRecord)?
            > MAX_PROJECTION_BYTES
        {
            return Err(Spec035MediaProjectionStoreError::InvalidRecord);
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Spec035MediaProjectionStoreError::InvalidRecord)?;
        Spec035MediaProjection::parse_json(text)
            .map(Some)
            .map_err(|_| Spec035MediaProjectionStoreError::InvalidRecord)
    }

    pub fn publish(
        &self,
        projection: &Spec035MediaProjection,
    ) -> Result<(), Spec035MediaProjectionStoreError> {
        self.publish_with_parent_sync(projection, sync_parent_directory)
    }

    pub(super) fn publish_with_parent_sync<S>(
        &self,
        projection: &Spec035MediaProjection,
        sync_parent: S,
    ) -> Result<(), Spec035MediaProjectionStoreError>
    where
        S: FnOnce(&Path) -> io::Result<()>,
    {
        let bytes = serde_json::to_vec(projection)
            .map_err(|_| Spec035MediaProjectionStoreError::InvalidRecord)?;
        if u64::try_from(bytes.len())
            .map_err(|_| Spec035MediaProjectionStoreError::InvalidRecord)?
            > MAX_PROJECTION_BYTES
        {
            return Err(Spec035MediaProjectionStoreError::InvalidRecord);
        }
        let parent = self
            .path
            .parent()
            .ok_or(Spec035MediaProjectionStoreError::InvalidRecord)?;
        create_safe_directory(parent)?;
        reject_non_regular_target(&self.path)?;
        let temp = parent.join(format!(
            "current.json.tmp-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let result = write_and_publish(&temp, &self.path, parent, &bytes, sync_parent);
        if result.is_err() {
            let _cleanup = fs::remove_file(&temp);
        }
        result
    }
}

fn create_safe_directory(path: &Path) -> Result<(), Spec035MediaProjectionStoreError> {
    if let Some(parent) = path.parent() {
        if parent.exists() {
            reject_symlink_or_non_directory(parent)?;
        }
    }
    fs::create_dir_all(path)?;
    reject_symlink_or_non_directory(path)
}

fn reject_symlink_or_non_directory(path: &Path) -> Result<(), Spec035MediaProjectionStoreError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Spec035MediaProjectionStoreError::InvalidRecord);
    }
    Ok(())
}

fn reject_non_regular_target(path: &Path) -> Result<(), Spec035MediaProjectionStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(Spec035MediaProjectionStoreError::InvalidRecord)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn write_and_publish(
    temp: &Path,
    target: &Path,
    parent: &Path,
    bytes: &[u8],
    sync_parent: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), Spec035MediaProjectionStoreError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, target)?;
    sync_parent(parent).map_err(|_| {
        Spec035MediaProjectionStoreError::CommitStatusUnknown(
            Spec035MediaProjectionTransactionStage::Renamed,
        )
    })?;
    Ok(())
}

fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}
