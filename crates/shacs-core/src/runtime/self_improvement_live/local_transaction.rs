use super::local_owner::{state_directory, state_path};
use super::{LocalApplyReceipt, LocalImprovementBlock, LocalRollbackReceipt};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalTransactionPhase {
    IntentDurable,
    Staged,
    TargetReplaced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub(crate) enum TransactionReceipt {
    Apply { receipt: LocalApplyReceipt },
    Rollback { receipt: LocalRollbackReceipt },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransactionJournal {
    pub schema_version: u32,
    pub proposal_id: String,
    pub target_ref: String,
    pub before_digest: String,
    pub after_digest: String,
    pub checkpoint: Vec<u8>,
    pub replacement: Vec<u8>,
    pub receipt: TransactionReceipt,
    pub phase: LocalTransactionPhase,
}

pub(crate) struct ProcessTransaction {
    root: PathBuf,
    _lock: File,
}

impl ProcessTransaction {
    pub fn acquire(root: &Path) -> Result<Self, LocalImprovementBlock> {
        let state = state_directory(root)?;
        sync_dir(root)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let lock = options
            .open(state_path(root, "transaction.lock")?)
            .map_err(|_| LocalImprovementBlock::Io)?;
        lock.lock_exclusive()
            .map_err(|_| LocalImprovementBlock::Io)?;
        Ok(Self {
            root: state,
            _lock: lock,
        })
    }

    pub fn journal(&self) -> Result<Option<TransactionJournal>, LocalImprovementBlock> {
        let path = self.journal_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|_| LocalImprovementBlock::Io)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| LocalImprovementBlock::RecoveryRequired)
    }

    pub fn persist(&self, journal: &TransactionJournal) -> Result<(), LocalImprovementBlock> {
        write_atomic(&self.journal_path()?, journal)
    }

    pub fn stage(&self, bytes: &[u8]) -> Result<PathBuf, LocalImprovementBlock> {
        let mut staged =
            tempfile::NamedTempFile::new_in(&self.root).map_err(|_| LocalImprovementBlock::Io)?;
        staged
            .write_all(bytes)
            .map_err(|_| LocalImprovementBlock::Io)?;
        staged
            .as_file()
            .sync_all()
            .map_err(|_| LocalImprovementBlock::Io)?;
        let (_, path) = staged.keep().map_err(|_| LocalImprovementBlock::Io)?;
        sync_dir(&self.root)?;
        Ok(path)
    }

    pub fn commit_target(&self, staged: &Path, target: &Path) -> Result<(), LocalImprovementBlock> {
        fs::rename(staged, target).map_err(|_| LocalImprovementBlock::Io)?;
        sync_dir(target.parent().ok_or(LocalImprovementBlock::UnsafeTarget)?)?;
        sync_dir(&self.root)
    }

    pub fn clear(&self) -> Result<(), LocalImprovementBlock> {
        for path in [self.root.join("replacement.stage"), self.journal_path()?] {
            if path.exists() {
                fs::remove_file(path).map_err(|_| LocalImprovementBlock::Io)?;
            }
        }
        sync_dir(&self.root)
    }

    fn journal_path(&self) -> Result<PathBuf, LocalImprovementBlock> {
        let root = self
            .root
            .parent()
            .ok_or(LocalImprovementBlock::UnsafeTarget)?;
        state_path(root, "transaction.json")
    }
}

fn write_atomic(path: &Path, value: &TransactionJournal) -> Result<(), LocalImprovementBlock> {
    let parent = path.parent().ok_or(LocalImprovementBlock::Io)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| LocalImprovementBlock::Io)?;
    serde_json::to_writer_pretty(&mut temporary, value).map_err(|_| LocalImprovementBlock::Io)?;
    temporary
        .write_all(b"\n")
        .map_err(|_| LocalImprovementBlock::Io)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| LocalImprovementBlock::Io)?;
    temporary
        .persist(path)
        .map_err(|_| LocalImprovementBlock::Io)?;
    sync_dir(parent)
}

fn sync_dir(path: &Path) -> Result<(), LocalImprovementBlock> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| LocalImprovementBlock::Io)
}
