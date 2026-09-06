use super::*;
use crate::runtime::spec034_release::artifacts::ArtifactSnapshot;
use rustix::fd::AsFd;
use rustix::fs::{fstat, AtFlags};
use super::seal::FinalStagingSeal;
use std::path::PathBuf;

pub struct StagingDirectory {
    directory: Option<tempfile::TempDir>,
    parent: File,
    name: OsString,
    pub(super) handle: File,
    #[cfg(test)]
    pub(super) failure: Option<super::marker::MarkerSyncFailure>,
}

pub struct ValidatedStagingDirectory {
    staging: StagingDirectory,
    pub(super) seal: FinalStagingSeal,
    pub(super) binding: super::super::FinalSourceBinding,
}

impl StagingDirectory {
    pub fn path(&self) -> &Path {
        self.directory
            .as_ref()
            .map(tempfile::TempDir::path)
            .unwrap_or_else(|| Path::new(""))
    }

    pub(super) fn parent(&self) -> &File {
        &self.parent
    }

    pub(super) fn name(&self) -> &OsStr {
        &self.name
    }

    pub(super) fn verify_for(
        &self,
        destination_parent: &File,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        same_handle(&self.parent, destination_parent)?;
        if !same_handle_path(&self.parent, &self.name, &self.handle) {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let staging = fstat(&self.handle).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let parent = fstat(&self.parent).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        (staging.st_dev == parent.st_dev)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)
    }

    #[cfg(test)]
    pub(super) fn capture_for_test(
        directory: tempfile::TempDir,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let path = directory.path();
        let parent_path = path
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        let parent = File::open(parent_path).map_err(Spec034ReleaseArtifactError::Io)?;
        Self::capture(directory, parent)
    }

    fn capture(
        directory: tempfile::TempDir,
        parent: File,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let name = directory
            .path()
            .file_name()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
            .to_os_string();
        let handle: File = open_child(&parent, &name)
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?
            .into();
        let staging = Self {
            directory: Some(directory),
            parent,
            name,
            handle,
            #[cfg(test)]
            failure: None,
        };
        staging.verify_for(&staging.parent)?;
        Ok(staging)
    }

    fn disarm_cleanup(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = directory.keep();
        }
    }

    pub(in crate::runtime::spec034_release::runner) fn finalize_approved_marker(
        mut self,
        run_id: &str,
        approved: ArtifactSnapshot,
        binding: super::super::FinalSourceBinding,
    ) -> Result<ValidatedStagingDirectory, Spec034ReleaseArtifactError> {
        let marker = self.write_marker(run_id, &approved)?;
        let seal = FinalStagingSeal::from_approved(&self.handle, approved, marker)?;
        Ok(ValidatedStagingDirectory {
            staging: self,
            seal,
            binding,
        })
    }

    #[cfg(test)]
    pub(super) fn finalize_marker(
        self,
        run_id: &str,
    ) -> Result<ValidatedStagingDirectory, Spec034ReleaseArtifactError> {
        let approved = ArtifactSnapshot::capture(self.path())?;
        self.finalize_approved_marker(
            run_id,
            approved,
            super::super::FinalSourceBinding::fixture(),
        )
    }

    #[cfg(test)]
    pub(super) fn finalize_marker_for_test(
        mut self,
        run_id: &str,
        after_inventory: impl FnOnce(),
    ) -> Result<ValidatedStagingDirectory, Spec034ReleaseArtifactError> {
        let approved = ArtifactSnapshot::capture(self.path())?;
        let marker = self.write_marker(run_id, &approved)?;
        let seal = FinalStagingSeal::capture_for_test(
            &self.handle,
            approved,
            marker,
            after_inventory,
        )?;
        Ok(ValidatedStagingDirectory {
            staging: self,
            seal,
            binding: super::super::FinalSourceBinding::fixture(),
        })
    }
}

impl ValidatedStagingDirectory {
    pub fn path(&self) -> &Path {
        self.staging.path()
    }

    pub(super) fn parent(&self) -> &File {
        self.staging.parent()
    }

    pub(super) fn name(&self) -> &OsStr {
        self.staging.name()
    }

    pub(super) fn handle(&self) -> &File {
        &self.staging.handle
    }

    pub(super) fn verify_for(
        &self,
        destination_parent: &File,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.staging.verify_for(destination_parent)
    }

    pub(super) fn disarm_cleanup(&mut self) {
        self.staging.disarm_cleanup();
    }
}

impl EvidenceDestination {
    pub fn staging(&self) -> Result<StagingDirectory, Spec034ReleaseArtifactError> {
        self.verify_chain()?;
        let parent = self
            .parent()
            .try_clone()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let staging = tempfile::Builder::new()
            .prefix(".spec034-release-")
            .tempdir_in(self.parent_path())
            .map_err(Spec034ReleaseArtifactError::Io)?;
        self.verify_chain()?;
        let staging = StagingDirectory::capture(staging, parent)?;
        staging.verify_for(self.parent())?;
        Ok(staging)
    }

    fn parent_path(&self) -> PathBuf {
        let mut path = if self.absolute {
            PathBuf::from("/")
        } else {
            PathBuf::from(".")
        };
        for component in &self.components {
            path.push(component);
        }
        path
    }
}

pub(super) fn same_handle(
    left: impl AsFd,
    right: impl AsFd,
) -> Result<(), Spec034ReleaseArtifactError> {
    let left = fstat(left).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let right = fstat(right).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    if left.st_dev == right.st_dev && left.st_ino == right.st_ino {
        Ok(())
    } else {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}

pub(super) fn same_handle_path(parent: &File, name: &OsStr, handle: &File) -> bool {
    let Ok(path) = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
        return false;
    };
    let Ok(opened) = fstat(handle) else {
        return false;
    };
    path.st_dev == opened.st_dev && path.st_ino == opened.st_ino
}
