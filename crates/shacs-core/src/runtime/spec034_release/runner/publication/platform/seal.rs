use super::*;
use crate::runtime::spec034_release::artifacts::{
    digest_bytes, ArtifactMetadata, ArtifactSnapshot,
};
use crate::runtime::spec034_release::source::validate_locator;
use rustix::fs::{openat, Mode, OFlags};
use std::io::Read;
use sha2::{Digest, Sha256};

const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(target_vendor = "apple")]
type DeviceId = i32;
#[cfg(target_os = "linux")]
type DeviceId = u64;

#[derive(PartialEq, Eq)]
struct SealedEntry {
    name: String,
    metadata: ArtifactMetadata,
    content_digest: String,
}

#[derive(PartialEq, Eq)]
pub(super) struct FinalStagingSeal {
    entries: Vec<SealedEntry>,
    root: ArtifactMetadata,
    content_digest: String,
}

impl FinalStagingSeal {
    pub(super) fn from_approved(
        handle: &File,
        approved: ArtifactSnapshot,
        marker: super::marker::FsyncedMarker,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::from_approved_with(handle, approved, marker, || {})
    }

    fn from_approved_with(
        handle: &File,
        approved: ArtifactSnapshot,
        marker: super::marker::FsyncedMarker,
        after_inventory: impl FnOnce(),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let status: PublicationStatusDocument = serde_json::from_slice(&marker.bytes)
            .map_err(Spec034ReleaseArtifactError::Json)?;
        let current = Self::capture_stable(handle, after_inventory)?;
        if current.root.device() != approved.root_metadata().device()
            || current.root.inode() != approved.root_metadata().inode()
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let mut expected = approved
            .sealed_files()
            .map(|(name, bytes, metadata)| SealedEntry {
                name: name.to_owned(),
                metadata: ArtifactMetadata::capture_from(metadata),
                content_digest: digest_bytes(bytes),
            })
            .collect::<Vec<_>>();
        expected.push(SealedEntry {
            name: super::marker::STATUS_FINAL.to_owned(),
            metadata: marker.metadata,
            content_digest: digest_bytes(&marker.bytes),
        });
        expected.sort_by(|left, right| left.name.cmp(&right.name));
        if current.entries != expected {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok(Self {
            content_digest: status.content_digest,
            ..current
        })
    }

    fn capture_stable(
        handle: &File,
        between_snapshots: impl FnOnce(),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let first = Self::capture_once(handle)?;
        between_snapshots();
        let second = Self::capture_once(handle)?;
        (first == second)
            .then_some(second)
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }

    fn capture_once(
        handle: &File,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let identity = handle.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        let mut names = cap_primitives::fs::read_base_dir(handle)
            .map_err(Spec034ReleaseArtifactError::Io)?
            .map(|entry| {
                entry
                    .map_err(Spec034ReleaseArtifactError::Io)?
                    .file_name()
                    .into_string()
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        names.sort();
        names.dedup();
        let mut total = 0_u64;
        let mut entries = Vec::with_capacity(names.len());
        let mut content_digest = None;
        for name in names {
            validate_locator(&name)?;
            let descriptor = openat(
                handle,
                name.as_str(),
                OFlags::RDONLY.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC),
                Mode::empty(),
            )
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
            let mut file: File = descriptor.into();
            let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
            if !metadata.is_file() || metadata.len() > MAX_ARTIFACT_BYTES {
                return Err(Spec034ReleaseArtifactError::InvalidEvidence);
            }
            total = total
                .checked_add(metadata.len())
                .filter(|total| *total <= MAX_TOTAL_BYTES)
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            let mut bytes = Vec::new();
            file.by_ref()
                .take(MAX_ARTIFACT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(Spec034ReleaseArtifactError::Io)?;
            if bytes.len() as u64 != metadata.len() {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
            if name == super::marker::STATUS_FINAL {
                let status: PublicationStatusDocument = serde_json::from_slice(&bytes)
                    .map_err(Spec034ReleaseArtifactError::Json)?;
                content_digest = Some(status.content_digest);
            }
            entries.push(SealedEntry {
                name,
                metadata: ArtifactMetadata::capture(&metadata),
                content_digest: digest_bytes(&bytes),
            });
        }
        entries
            .iter()
            .find(|entry| entry.name == super::marker::STATUS_FINAL)
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        Ok(Self {
            entries,
            root: ArtifactMetadata::capture(&identity),
            content_digest: content_digest.ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?,
        })
    }

    #[cfg(test)]
    pub(super) fn capture_for_test(
        handle: &File,
        approved: ArtifactSnapshot,
        marker: super::marker::FsyncedMarker,
        after_inventory: impl FnOnce(),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::from_approved_with(handle, approved, marker, after_inventory)
    }

    pub(super) fn verify(&self, handle: &File) -> Result<(), Spec034ReleaseArtifactError> {
        (Self::capture_stable(handle, || {})? == *self)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }

    pub(super) fn verify_destination(
        &self,
        parent: &File,
        name: &OsStr,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        let identity = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| unknown())?;
        (device_as_u64(identity.st_dev)? == self.root.device()
            && identity.st_ino == self.root.inode())
            .then_some(())
            .ok_or_else(unknown)
    }

    pub(super) fn verify_post_rename(
        &self,
        handle: &File,
        parent: &File,
        name: &OsStr,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.verify_destination(parent, name)?;
        let current = Self::capture_stable(handle, || {})?;
        if current.entries != self.entries
            || current.root.device() != self.root.device()
            || current.root.inode() != self.root.inode()
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok(())
    }

    pub(super) fn binding_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.root.device().to_le_bytes());
        digest.update(self.root.inode().to_le_bytes());
        for entry in &self.entries {
            digest.update(entry.name.as_bytes());
            digest.update([0]);
            digest.update(entry.content_digest.as_bytes());
        }
        format!("sha256:{:x}", digest.finalize())
    }

    pub(super) fn content_digest(&self) -> &str {
        &self.content_digest
    }
}

fn device_as_u64(device: DeviceId) -> Result<u64, Spec034ReleaseArtifactError> {
    u64::try_from(device).map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)
}

fn unknown() -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::CommitStatusUnknown(PublicationStage::DestinationIdentity)
}
