use super::ExecutionSnapshot;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use shacs_eval::evaluator::ReplayDatasetItem;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod trajectory_transaction;
mod trajectory_validation;
use trajectory_transaction::{
    create_safe_dir, reject_existing, reject_symlink, sync_dir, sync_tree, write_new,
    StagedTrajectory,
};
use trajectory_validation::{digest, locator_to_string, read_verified, record_digest, validate_id};

const TRAJECTORY_SCHEMA: &str = "shacs.recorded-trajectory.v2";
const STAGING_DIR: &str = ".trajectory-staging";
static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedBoundaryRequirement {
    RecordedOnly,
    LiveDestructive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedTrajectoryOrigin {
    AutomationOwnerReceipt,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSourceArtifactInput {
    pub source_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedTrajectoryInput {
    pub trajectory_id: String,
    pub snapshot: ExecutionSnapshot,
    pub sources: Vec<RecordedSourceArtifactInput>,
    pub owner_outcome: ReplayDatasetItem,
    pub boundary_requirement: RecordedBoundaryRequirement,
    pub origin: RecordedTrajectoryOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedArtifactRef {
    pub locator: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedSourceArtifact {
    pub source_id: String,
    pub locator: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedTrajectoryRecord {
    pub schema: String,
    pub trajectory_id: String,
    pub snapshot: RecordedArtifactRef,
    pub sources: Vec<RecordedSourceArtifact>,
    pub owner_outcome: ReplayDatasetItem,
    pub boundary_requirement: RecordedBoundaryRequirement,
    pub origin: RecordedTrajectoryOrigin,
    pub record_digest: String,
}

#[derive(Debug, Clone)]
pub struct RecordedTrajectoryStore {
    root: PathBuf,
}

#[derive(Debug)]
pub enum RecordedTrajectoryStoreError {
    InvalidId,
    InvalidRecord,
    DigestMismatch,
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl RecordedTrajectoryStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RecordedTrajectoryStoreError> {
        reject_symlink(root.as_ref())?;
        fs::create_dir_all(root.as_ref()).map_err(RecordedTrajectoryStoreError::Io)?;
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(RecordedTrajectoryStoreError::Io)?;
        let store = Self { root };
        let lock = store.lock()?;
        store.cleanup_staging()?;
        drop(lock);
        Ok(store)
    }

    pub fn write(
        &self,
        input: RecordedTrajectoryInput,
    ) -> Result<RecordedTrajectoryRecord, RecordedTrajectoryStoreError> {
        validate_id(&input.trajectory_id)?;
        let lock = self.lock()?;
        self.cleanup_staging()?;
        let base = PathBuf::from("trajectories").join(&input.trajectory_id);
        let final_dir = self.root.join(&base);
        reject_existing(&final_dir)?;
        let snapshot_locator = base.join("snapshot.json");
        let snapshot_bytes = serde_json::to_vec_pretty(&input.snapshot)
            .map_err(RecordedTrajectoryStoreError::Json)?;
        let mut staging = StagedTrajectory::create(self.staging_path(&input.trajectory_id))?;
        write_new(&staging.path().join("snapshot.json"), &snapshot_bytes)?;
        let mut sources = Vec::with_capacity(input.sources.len());
        for source in input.sources {
            validate_id(&source.source_id)?;
            let locator = base
                .join("sources")
                .join(format!("{}.bin", source.source_id));
            write_new(
                &staging
                    .path()
                    .join("sources")
                    .join(format!("{}.bin", source.source_id)),
                &source.bytes,
            )?;
            sources.push(RecordedSourceArtifact {
                source_id: source.source_id,
                locator: locator_to_string(&locator)?,
                digest: digest(&source.bytes),
            });
        }
        sources.sort_by(|left, right| left.source_id.cmp(&right.source_id));
        let mut record = RecordedTrajectoryRecord {
            schema: TRAJECTORY_SCHEMA.to_owned(),
            trajectory_id: input.trajectory_id,
            snapshot: RecordedArtifactRef {
                locator: locator_to_string(&snapshot_locator)?,
                digest: digest(&snapshot_bytes),
            },
            sources,
            owner_outcome: input.owner_outcome,
            boundary_requirement: input.boundary_requirement,
            origin: input.origin,
            record_digest: String::new(),
        };
        record.record_digest = record_digest(&record)?;
        let bytes =
            serde_json::to_vec_pretty(&record).map_err(RecordedTrajectoryStoreError::Json)?;
        write_new(&staging.path().join("record.json"), &bytes)?;
        sync_tree(staging.path())?;
        let trajectories = self.root.join("trajectories");
        create_safe_dir(&trajectories)?;
        fs::rename(staging.path(), trajectories.join(&record.trajectory_id))
            .map_err(RecordedTrajectoryStoreError::Io)?;
        staging.mark_published();
        sync_dir(&trajectories)?;
        self.cleanup_staging()?;
        drop(lock);
        Ok(record)
    }

    pub fn read(
        &self,
        trajectory_id: &str,
    ) -> Result<RecordedTrajectoryRecord, RecordedTrajectoryStoreError> {
        validate_id(trajectory_id)?;
        let bytes =
            fs::read(self.record_path(trajectory_id)).map_err(RecordedTrajectoryStoreError::Io)?;
        let record: RecordedTrajectoryRecord =
            serde_json::from_slice(&bytes).map_err(RecordedTrajectoryStoreError::Json)?;
        if record.schema != TRAJECTORY_SCHEMA
            || record.trajectory_id != trajectory_id
            || record_digest(&record)? != record.record_digest
        {
            return Err(RecordedTrajectoryStoreError::InvalidRecord);
        }
        Ok(record)
    }

    pub(crate) fn read_artifact(
        &self,
        reference: &RecordedArtifactRef,
    ) -> Result<Vec<u8>, RecordedTrajectoryStoreError> {
        read_verified(&self.root, &reference.locator, &reference.digest)
    }

    pub(crate) fn read_source(
        &self,
        reference: &RecordedSourceArtifact,
    ) -> Result<Vec<u8>, RecordedTrajectoryStoreError> {
        read_verified(&self.root, &reference.locator, &reference.digest)
    }

    fn record_path(&self, trajectory_id: &str) -> PathBuf {
        self.root
            .join("trajectories")
            .join(trajectory_id)
            .join("record.json")
    }

    fn staging_path(&self, trajectory_id: &str) -> PathBuf {
        self.root.join(STAGING_DIR).join(format!(
            "{trajectory_id}.{}.{}",
            std::process::id(),
            NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn lock(&self) -> Result<std::fs::File, RecordedTrajectoryStoreError> {
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(".trajectory.lock"))
            .map_err(RecordedTrajectoryStoreError::Io)?;
        lock.lock_exclusive()
            .map_err(RecordedTrajectoryStoreError::Io)?;
        Ok(lock)
    }

    fn cleanup_staging(&self) -> Result<(), RecordedTrajectoryStoreError> {
        let staging = self.root.join(STAGING_DIR);
        match fs::symlink_metadata(&staging) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(RecordedTrajectoryStoreError::InvalidRecord)
            }
            Ok(metadata) if metadata.is_dir() => {
                fs::remove_dir_all(staging).map_err(RecordedTrajectoryStoreError::Io)
            }
            Ok(_) => Err(RecordedTrajectoryStoreError::InvalidRecord),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(RecordedTrajectoryStoreError::Io(error)),
        }
    }
}

impl std::fmt::Display for RecordedTrajectoryStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RecordedTrajectoryStoreError {}
