use super::*;

pub struct EvidenceDestination;

pub struct StagingDirectory {
    path: std::path::PathBuf,
}

pub struct ValidatedStagingDirectory;

impl StagingDirectory {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn finalize_approved_marker(
        self,
        _run_id: &str,
        _approved: crate::runtime::generated_media_release::artifacts::ArtifactSnapshot,
        _binding: super::FinalSourceBinding,
    ) -> Result<ValidatedStagingDirectory, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::MarkerRename,
        ))
    }
}

impl EvidenceDestination {
    pub fn prepare(_path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub fn publish(
        &mut self,
        _staging: ValidatedStagingDirectory,
    ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub(in crate::runtime::generated_media_release::runner) fn publish_with_runner_hooks(
        &mut self,
        _staging: ValidatedStagingDirectory,
        _before_final_verification: impl FnOnce(),
        _after_final_verification: impl FnOnce(),
    ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub fn staging(&self) -> Result<StagingDirectory, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
            PublicationStage::MarkerCreate,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_facing_api_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let snapshot = crate::runtime::generated_media_release::artifacts::ArtifactSnapshot::capture(
            &root,
        )?;
        let staging = StagingDirectory {
            path: root.clone(),
        };
        assert_eq!(staging.path(), root);
        assert!(staging
            .finalize_approved_marker(
                "run",
                snapshot,
                super::super::FinalSourceBinding::fixture(),
            )
            .is_err());
        assert!(EvidenceDestination::prepare(&root).is_err());
        let mut destination = EvidenceDestination;
        assert!(destination.staging().is_err());
        assert!(destination.publish(ValidatedStagingDirectory).is_err());
        assert!(destination
            .publish_with_runner_hooks(ValidatedStagingDirectory, || {}, || {})
            .is_err());
        Ok(())
    }
}
