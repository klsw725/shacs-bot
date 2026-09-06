use super::Spec034ReleaseArtifactError;
use std::fs::Metadata;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct RootIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
    pub(super) owner: u32,
    birth_seconds: i64,
    birth_nanoseconds: i64,
    start_seconds: i64,
    start_nanoseconds: i64,
}

impl RootIdentity {
    pub(super) fn capture(metadata: &Metadata) -> Result<Self, Spec034ReleaseArtifactError> {
        use std::os::unix::fs::MetadataExt;
        let (birth_seconds, birth_nanoseconds) = birth(metadata)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner: metadata.uid(),
            birth_seconds,
            birth_nanoseconds,
            start_seconds: metadata.ctime(),
            start_nanoseconds: metadata.ctime_nsec(),
        })
    }

    pub(super) fn bytes(self) -> [u8; 52] {
        let mut bytes = [0; 52];
        bytes[0..8].copy_from_slice(&self.device.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.inode.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.owner.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.birth_seconds.to_le_bytes());
        bytes[28..36].copy_from_slice(&self.birth_nanoseconds.to_le_bytes());
        bytes[36..44].copy_from_slice(&self.start_seconds.to_le_bytes());
        bytes[44..52].copy_from_slice(&self.start_nanoseconds.to_le_bytes());
        bytes
    }

    pub(super) fn same_object(self, other: Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.owner == other.owner
            && self.birth_seconds == other.birth_seconds
            && self.birth_nanoseconds == other.birth_nanoseconds
    }
}

#[cfg(target_vendor = "apple")]
fn birth(metadata: &Metadata) -> Result<(i64, i64), Spec034ReleaseArtifactError> {
    use std::os::macos::fs::MetadataExt;
    Ok((metadata.st_birthtime(), metadata.st_birthtime_nsec()))
}

#[cfg(not(target_vendor = "apple"))]
fn birth(metadata: &Metadata) -> Result<(i64, i64), Spec034ReleaseArtifactError> {
    use std::time::UNIX_EPOCH;
    let created = metadata
        .created()
        .map_err(Spec034ReleaseArtifactError::Io)?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    Ok((
        i64::try_from(created.as_secs())
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
        i64::from(created.subsec_nanos()),
    ))
}
