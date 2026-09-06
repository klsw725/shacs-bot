use super::staging::StagingDirectory;
use super::*;
use rustix::fs::{fsync, openat, renameat_with, Mode, OFlags, RenameFlags};
use std::io::Write;

use crate::runtime::generated_media_release::artifacts::{ArtifactMetadata, ArtifactSnapshot};

const STATUS_TEMP: &str = ".publication-status.validated";
pub(super) const STATUS_FINAL: &str = "publication-status.json";

pub(super) struct FsyncedMarker {
    pub(super) bytes: Vec<u8>,
    pub(super) metadata: ArtifactMetadata,
}

#[cfg(test)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum MarkerSyncFailure {
    BeforeRenameDirectory,
}

impl StagingDirectory {
    pub(super) fn write_marker(
        &mut self,
        run_id: &str,
        approved: &ArtifactSnapshot,
    ) -> Result<FsyncedMarker, Spec034ReleaseArtifactError> {
        let content_digest = approved.publication_digest();
        let bytes = serde_json::to_vec_pretty(&PublicationStatusDocument {
            schema: PUBLICATION_STATUS_SCHEMA.to_owned(),
            run_id: run_id.to_owned(),
            status: PublicationStatus::Validated,
            content_digest,
        })
        .map_err(Spec034ReleaseArtifactError::Json)?;
        let flags = OFlags::WRONLY
            .union(OFlags::CREATE)
            .union(OFlags::EXCL)
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC);
        let descriptor = openat(
            &self.handle,
            STATUS_TEMP,
            flags,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| unknown(PublicationStage::MarkerCreate))?;
        let mut file: File = descriptor.into();
        file.write_all(&bytes)
            .map_err(|_| unknown(PublicationStage::MarkerWrite))?;
        file.sync_all()
            .map_err(|_| unknown(PublicationStage::FileSync))?;
        #[cfg(test)]
        if self.failure == Some(MarkerSyncFailure::BeforeRenameDirectory) {
            return Err(unknown(PublicationStage::DirectorySync));
        }
        fsync(&self.handle).map_err(|_| unknown(PublicationStage::DirectorySync))?;
        renameat_with(
            &self.handle,
            STATUS_TEMP,
            &self.handle,
            STATUS_FINAL,
            RenameFlags::NOREPLACE,
        )
        .map_err(|_| unknown(PublicationStage::MarkerRename))?;
        fsync(&self.handle).map_err(|_| unknown(PublicationStage::DirectorySync))?;
        let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        Ok(FsyncedMarker {
            bytes,
            metadata: ArtifactMetadata::capture(&metadata),
        })
    }

    #[cfg(test)]
    pub(super) fn inject_marker_sync_failure(&mut self, failure: MarkerSyncFailure) {
        self.failure = Some(failure);
    }
}

fn unknown(stage: PublicationStage) -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::CommitStatusUnknown(stage)
}
