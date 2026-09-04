use super::super::model::{
    PublicationStage, PublicationStatus, PublicationStatusDocument, Spec034ReleaseArtifactError,
    PUBLICATION_STATUS_SCHEMA,
};
use super::super::CommittedPublicationIdentity;
use std::path::Path;

#[path = "publication/binding.rs"]
mod binding;
pub(super) use binding::FinalSourceBinding;

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
mod platform {
    use super::*;
    use rustix::fs::{
        fsync, mkdirat, renameat_with, statat, unlinkat, AtFlags, Mode, OFlags, RenameFlags,
    };
    use std::ffi::{OsStr, OsString};
    use std::fs::File;

    mod aggregate;
    mod components;
    mod marker;
    mod path;
    mod quarantine;
    mod seal;
    mod staging;
    use path::{open_anchor, open_child, parse};
    use staging::{same_handle, same_handle_path};
    #[cfg(test)]
    pub use staging::StagingDirectory;
    pub use staging::ValidatedStagingDirectory;
    #[cfg(test)]
    use marker::MarkerSyncFailure;

    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    struct CreatedComponent {
        parent_index: usize,
        name: OsString,
    }

    pub struct EvidenceDestination {
        absolute: bool,
        components: Vec<OsString>,
        handles: Vec<File>,
        created: Vec<CreatedComponent>,
        leaf: OsString,
        published: bool,
        #[cfg(test)]
        destination_sync_failure: bool,
    }

    impl EvidenceDestination {
        pub fn prepare(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
            Self::prepare_with(path, |_| Ok(()))
        }

        fn prepare_with(
            path: &Path,
            mut after_mkdir: impl FnMut(&OsStr) -> Result<(), Spec034ReleaseArtifactError>,
        ) -> Result<Self, Spec034ReleaseArtifactError> {
            let (absolute, mut components) = parse(path)?;
            let leaf = components
                .pop()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            let anchor = open_anchor(absolute)?;
            let mut destination = Self {
                absolute,
                components,
                handles: vec![anchor.into()],
                created: Vec::new(),
                leaf,
                published: false,
                #[cfg(test)]
                destination_sync_failure: false,
            };
            destination.open_components(&mut after_mkdir)?;
            destination.require_leaf_missing()?;
            Ok(destination)
        }

        #[cfg(test)]
        pub fn publish(
            &mut self,
            staging: ValidatedStagingDirectory,
        ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
            self.publish_with_runner_hooks(staging, || {}, || {})
        }

        pub(in crate::runtime::spec034_release::runner) fn publish_with_runner_hooks(
            &mut self,
            mut staging: ValidatedStagingDirectory,
            before_final_verification: impl FnOnce(),
            after_final_verification: impl FnOnce(),
        ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
            let identity = self.publish_with_hooks(
                &staging,
                before_final_verification,
                after_final_verification,
                || {},
                || {},
            )?;
            staging.disarm_cleanup();
            Ok(identity)
        }

        #[cfg(test)]
        fn publish_with(
            &mut self,
            staging: &ValidatedStagingDirectory,
            after_final_verification: impl FnOnce(),
        ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
            self.publish_with_hooks(staging, || {}, after_final_verification, || {}, || {})
        }

        #[cfg(test)]
        fn publish_with_post_rename_hook(
            &mut self,
            staging: &ValidatedStagingDirectory,
            after_rename: impl FnOnce(),
        ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
            self.publish_with_hooks(staging, || {}, || {}, after_rename, || {})
        }

        #[cfg(test)]
        fn publish_with_post_fsync_hook(
            &mut self,
            staging: &ValidatedStagingDirectory,
            after_fsync: impl FnOnce(),
        ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
            self.publish_with_hooks(staging, || {}, || {}, || {}, after_fsync)
        }

        fn publish_with_hooks(
            &mut self,
            staging: &ValidatedStagingDirectory,
            before_final_verification: impl FnOnce(),
            after_final_verification: impl FnOnce(),
            after_rename: impl FnOnce(),
            after_fsync: impl FnOnce(),
        ) -> Result<CommittedPublicationIdentity, Spec034ReleaseArtifactError> {
            self.verify_chain()?;
            staging.verify_for(self.parent())?;
            before_final_verification();
            staging
                .seal
                .verify(staging.handle())
                .map_err(|_| unknown_commit())?;
            after_final_verification();
            staging.binding.verify()?;
            staging
                .seal
                .verify(staging.handle())
                .map_err(|_| unknown_commit())?;
            self.verify_chain().map_err(|_| unknown_commit())?;
            self.require_leaf_missing().map_err(|_| unknown_commit())?;
            staging
                .verify_for(self.parent())
                .map_err(|_| unknown_commit())?;
            self.rename_verified(staging)?;
            after_rename();
            let verified = staging
                .seal
                .verify_post_rename(staging.handle(), self.parent(), &self.leaf)
                .and_then(|()| self.verify_chain())
                .and_then(|()| staging.binding.verify());
            if verified.is_err() {
                quarantine::quarantine_visible(self.parent(), &self.leaf)?;
                return Err(unknown_commit());
            }
            #[cfg(test)]
            if self.destination_sync_failure {
                return Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
                    PublicationStage::DirectorySync,
                ));
            }
            fsync(self.parent()).map_err(|_| {
                Spec034ReleaseArtifactError::CommitStatusUnknown(PublicationStage::DirectorySync)
            })?;
            let first = self.capture_final_aggregate(staging);
            after_fsync();
            let second = self.capture_final_aggregate(staging);
            let aggregate = match (first, second) {
                (Ok(first), Ok(second)) if first == second => second,
                _ => {
                    quarantine::quarantine_visible(self.parent(), &self.leaf)?;
                    return Err(unknown_commit());
                }
            };
            Ok(aggregate.into_identity(staging.seal.content_digest().to_owned()))
        }

        fn rename_verified(
            &mut self,
            staging: &ValidatedStagingDirectory,
        ) -> Result<(), Spec034ReleaseArtifactError> {
            renameat_with(
                staging.parent(),
                staging.name(),
                self.parent(),
                &self.leaf,
                RenameFlags::NOREPLACE,
            )
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
            self.published = true;
            Ok(())
        }

        fn verify_chain(&self) -> Result<(), Spec034ReleaseArtifactError> {
            let anchor = open_anchor(self.absolute)?;
            same_handle(&anchor, &self.handles[0])?;
            let mut current: File = anchor.into();
            for (index, name) in self.components.iter().enumerate() {
                let child = open_child(&current, name)
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
                same_handle(&child, &self.handles[index + 1])?;
                current = child.into();
            }
            Ok(())
        }

        fn require_leaf_missing(&self) -> Result<(), Spec034ReleaseArtifactError> {
            match statat(self.parent(), &self.leaf, AtFlags::SYMLINK_NOFOLLOW) {
                Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
                _ => Err(Spec034ReleaseArtifactError::InvalidConfig),
            }
        }

        fn parent(&self) -> &File {
            &self.handles[self.handles.len() - 1]
        }

        #[cfg(test)]
        pub(super) fn inject_destination_sync_failure(&mut self) {
            self.destination_sync_failure = true;
        }

    }

    fn unknown_commit() -> Spec034ReleaseArtifactError {
        Spec034ReleaseArtifactError::CommitStatusUnknown(PublicationStage::DestinationIdentity)
    }

    #[cfg(all(test, unix))]
    mod durability_tests;
    #[cfg(all(test, unix))]
    mod race_tests;
    #[cfg(all(test, unix))]
    mod race_snapshot_tests;
    #[cfg(all(test, unix))]
    mod quarantine_tests;
    #[cfg(all(test, unix))]
    mod toolchain_binding_tests;
    #[cfg(all(test, unix))]
    mod approved_snapshot_tests;
    #[cfg(all(test, unix))]
    mod tests;
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub use platform::EvidenceDestination;

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
#[path = "publication/unsupported.rs"]
mod unsupported;
#[cfg(all(test, any(target_os = "linux", target_vendor = "apple")))]
#[path = "publication/unsupported.rs"]
mod unsupported_contract;
#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
pub use unsupported::EvidenceDestination;
