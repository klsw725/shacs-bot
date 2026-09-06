use super::*;
use crate::runtime::spec034_release::CommittedPublicationIdentity;
use sha2::{Digest, Sha256};

#[derive(PartialEq, Eq)]
pub(super) struct FinalAggregate {
    artifact: String,
    binding: String,
    destination: String,
}

impl FinalAggregate {
    pub(super) fn into_identity(self, content_digest: String) -> CommittedPublicationIdentity {
        CommittedPublicationIdentity {
            content_digest,
            artifact_digest: self.artifact,
            binding_digest: self.binding,
            destination_digest: self.destination,
        }
    }
}

impl EvidenceDestination {
    pub(super) fn capture_final_aggregate(
        &self,
        staging: &ValidatedStagingDirectory,
    ) -> Result<FinalAggregate, Spec034ReleaseArtifactError> {
        staging
            .seal
            .verify_post_rename(staging.handle(), self.parent(), &self.leaf)?;
        self.verify_chain()?;
        let binding = staging.binding.capture_digest()?;
        let mut destination = Sha256::new();
        let last = self.handles.len().saturating_sub(1);
        for (index, handle) in self.handles.iter().enumerate() {
            let metadata = handle.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                destination.update(metadata.dev().to_le_bytes());
                destination.update(metadata.ino().to_le_bytes());
                destination.update(metadata.mode().to_le_bytes());
                if index == last {
                    destination.update(metadata.ctime().to_le_bytes());
                    destination.update(metadata.ctime_nsec().to_le_bytes());
                    destination.update(metadata.nlink().to_le_bytes());
                    destination.update(metadata.size().to_le_bytes());
                }
            }
        }
        destination.update(self.leaf.as_encoded_bytes());
        Ok(FinalAggregate {
            artifact: staging.seal.binding_digest(),
            binding,
            destination: format!("sha256:{:x}", destination.finalize()),
        })
    }
}
