use super::contracts::{
    artifact_schema, CommittedArtifact, GeneratedArtifactRecord, GeneratedProvenance,
    GeneratedProvenanceKind,
};
use super::{
    ArtifactId, ArtifactWriteRequest, CandidateId, GeneratedMediaContractError,
    MediaRootRelativePath, SafeModelId, SafeProviderId, Sha256Digest,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod read;
#[cfg(test)]
mod tests;
mod transaction;
mod types;
use transaction::{
    extension_for_mime, lock_store, reject_existing, reject_symlink, staging_path, sync_dir,
    sync_dir_io, write_new, StagedArtifact,
};
pub use types::{
    ArtifactReadStage, ArtifactStoreError, ArtifactTransactionStage, TransactionDecision,
};

const ARTIFACTS_DIR: &str = "artifacts";
const STAGE_PREFIX: &str = ".stage-";

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
    root_handle: Arc<std::fs::File>,
}

impl ArtifactStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ArtifactStoreError> {
        reject_symlink(root.as_ref())?;
        fs::create_dir_all(root.as_ref()).map_err(ArtifactStoreError::Io)?;
        reject_symlink(root.as_ref())?;
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(ArtifactStoreError::Io)?;
        let root_handle = read::open_root_handle(&root)?;
        let store = Self {
            root,
            root_handle: Arc::new(root_handle),
        };
        let lock = lock_store(&store.root)?;
        store.ensure_artifacts_dir()?;
        store.cleanup_staging()?;
        drop(lock);
        Ok(store)
    }

    pub fn persist(
        &self,
        request: ArtifactWriteRequest,
    ) -> Result<CommittedArtifact, ArtifactStoreError> {
        self.persist_with_observer(request, |_| TransactionDecision::Continue)
    }

    pub fn persist_with_observer<F>(
        &self,
        request: ArtifactWriteRequest,
        observer: F,
    ) -> Result<CommittedArtifact, ArtifactStoreError>
    where
        F: FnMut(ArtifactTransactionStage) -> TransactionDecision,
    {
        self.persist_transaction(request, observer, sync_dir_io)
    }

    fn persist_transaction<F, S>(
        &self,
        request: ArtifactWriteRequest,
        mut observer: F,
        sync_parent: S,
    ) -> Result<CommittedArtifact, ArtifactStoreError>
    where
        F: FnMut(ArtifactTransactionStage) -> TransactionDecision,
        S: FnOnce(&Path) -> std::io::Result<()>,
    {
        let ArtifactWriteRequest {
            candidate,
            metadata,
        } = request;
        let candidate = candidate
            .into_local_bytes()
            .map_err(|_| ArtifactStoreError::RemotePayloadRequiresPolicy)?;
        let (candidate_id, origin, media) = candidate.into_parts();
        let candidate_id = CandidateId::new(candidate_id.into_string())?;
        let (provider_id, model_id) = origin.into_parts();
        let provider_id = SafeProviderId::new(provider_id)?;
        let model_id = SafeModelId::new(model_id)?;
        let (mime_type, bytes) = media.into_parts();
        let lock = lock_store(&self.root)?;
        self.ensure_artifacts_dir()?;
        self.cleanup_staging()?;
        let artifacts = self.artifacts_dir();
        let final_dir = artifacts.join(metadata.artifact_id.as_str());
        reject_existing(&final_dir)?;
        let mut staging =
            StagedArtifact::create(staging_path(&self.artifacts_dir(), &metadata.artifact_id))?;
        let payload_name = format!("payload.{}", extension_for_mime(&mime_type));
        let payload_path = staging.path().join(&payload_name);
        write_new(&payload_path, &bytes)?;
        interrupt_before_publish(&mut observer, ArtifactTransactionStage::PayloadSynced)?;
        let relative_path = MediaRootRelativePath::new(format!(
            "{ARTIFACTS_DIR}/{}/{payload_name}",
            metadata.artifact_id.as_str()
        ))?;
        let record = GeneratedArtifactRecord {
            schema: artifact_schema(),
            artifact_id: metadata.artifact_id,
            candidate_id,
            kind: metadata.kind,
            media_root_relative_path: relative_path,
            mime_type,
            byte_len: u64::try_from(bytes.len()).map_err(|_| ArtifactStoreError::InvalidRecord)?,
            sha256: Sha256Digest::from_bytes(&bytes),
            provenance: GeneratedProvenance {
                kind: GeneratedProvenanceKind::Generated,
                provider_id,
                model_id,
                operation: metadata.operation,
                source_artifact_ids: metadata.source_artifact_ids,
            },
            generation_options_summary: metadata.generation_options_summary,
            created_at: metadata.created_at,
            retention: metadata.retention,
            disclosure: metadata.disclosure,
        };
        let record_bytes = serde_json::to_vec_pretty(&record).map_err(ArtifactStoreError::Json)?;
        write_new(&staging.path().join("record.json"), &record_bytes)?;
        interrupt_before_publish(&mut observer, ArtifactTransactionStage::RecordSynced)?;
        sync_dir(staging.path())?;
        interrupt_before_publish(
            &mut observer,
            ArtifactTransactionStage::StagingDirectorySynced,
        )?;
        fs::rename(staging.path(), &final_dir).map_err(ArtifactStoreError::Io)?;
        staging.mark_published();
        interrupt_after_publish(&mut observer, ArtifactTransactionStage::Renamed)?;
        sync_parent(&artifacts).map_err(|_| {
            ArtifactStoreError::CommitStatusUnknown(ArtifactTransactionStage::Renamed)
        })?;
        interrupt_after_publish(
            &mut observer,
            ArtifactTransactionStage::ParentDirectorySynced,
        )?;
        drop(lock);
        Ok(CommittedArtifact::new(record))
    }

    fn artifacts_dir(&self) -> PathBuf {
        self.root.join(ARTIFACTS_DIR)
    }

    fn ensure_artifacts_dir(&self) -> Result<(), ArtifactStoreError> {
        let artifacts = self.artifacts_dir();
        reject_symlink(&artifacts)?;
        match fs::create_dir(&artifacts) {
            Ok(()) => sync_dir(&self.root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if artifacts.is_dir() {
                    Ok(())
                } else {
                    Err(ArtifactStoreError::InvalidStore)
                }
            }
            Err(error) => Err(ArtifactStoreError::Io(error)),
        }
    }

    fn cleanup_staging(&self) -> Result<(), ArtifactStoreError> {
        let artifacts = self.artifacts_dir();
        let mut removed = false;
        for entry in fs::read_dir(&artifacts).map_err(ArtifactStoreError::Io)? {
            let entry = entry.map_err(ArtifactStoreError::Io)?;
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(STAGE_PREFIX)
            {
                continue;
            }
            let metadata = entry.file_type().map_err(ArtifactStoreError::Io)?;
            if metadata.is_symlink() || !metadata.is_dir() {
                return Err(ArtifactStoreError::SymlinkRejected);
            }
            fs::remove_dir_all(entry.path()).map_err(ArtifactStoreError::Io)?;
            removed = true;
        }
        if removed {
            sync_dir(&artifacts)?;
        }
        Ok(())
    }
}

fn interrupt_before_publish<F>(
    observer: &mut F,
    stage: ArtifactTransactionStage,
) -> Result<(), ArtifactStoreError>
where
    F: FnMut(ArtifactTransactionStage) -> TransactionDecision,
{
    match observer(stage) {
        TransactionDecision::Continue => Ok(()),
        TransactionDecision::Interrupt => Err(ArtifactStoreError::Interrupted(stage)),
    }
}

fn interrupt_after_publish<F>(
    observer: &mut F,
    stage: ArtifactTransactionStage,
) -> Result<(), ArtifactStoreError>
where
    F: FnMut(ArtifactTransactionStage) -> TransactionDecision,
{
    match observer(stage) {
        TransactionDecision::Continue => Ok(()),
        TransactionDecision::Interrupt => Err(ArtifactStoreError::CommitStatusUnknown(stage)),
    }
}
