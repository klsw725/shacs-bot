use super::model::*;
use super::source::{validate_locator, ConfinedSourceReader};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{Metadata, OpenOptions};
use std::io::Write;
use std::path::Path;

const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ArtifactMetadata {
    device: u64,
    inode: u64,
    mode: u32,
    links: u64,
    size: u64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
}

struct CapturedArtifact {
    bytes: Vec<u8>,
    metadata: ArtifactMetadata,
}

pub struct ArtifactSnapshot {
    root: ArtifactMetadata,
    files: BTreeMap<String, CapturedArtifact>,
}

impl ArtifactSnapshot {
    pub fn capture(root: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::capture_with(root, || {}, || {}, |_| {})
    }

    fn capture_with(
        root: &Path,
        after_root_open: impl FnOnce(),
        after_first_entry: impl FnOnce(),
        mut after_file_open: impl FnMut(&str),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let reader = ConfinedSourceReader::open(root)?;
        let root = ArtifactMetadata::capture(&reader.root_metadata()?);
        after_root_open();
        let names = reader.entry_names_with(after_first_entry)?;
        for name in &names {
            validate_locator(name)?;
        }
        let mut total = 0_u64;
        let mut files = BTreeMap::new();
        for name in names {
            let remaining = MAX_TOTAL_BYTES
                .checked_sub(total)
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            let limit = remaining.min(MAX_ARTIFACT_BYTES);
            let (bytes, metadata) = reader
                .read_with_metadata_hooks(&name, limit, || {}, || after_file_open(&name))?
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            total = total
                .checked_add(bytes.len() as u64)
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            files.insert(
                name,
                CapturedArtifact {
                    bytes,
                    metadata: ArtifactMetadata::capture(&metadata),
                },
            );
        }
        Ok(Self { root, files })
    }

    pub fn bytes(&self, locator: &str) -> Result<&[u8], Spec034ReleaseArtifactError> {
        validate_locator(locator)?;
        self.files
            .get(locator)
            .map(|artifact| artifact.bytes.as_slice())
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)
    }

    pub fn json<T: serde::de::DeserializeOwned>(
        &self,
        locator: &str,
    ) -> Result<T, Spec034ReleaseArtifactError> {
        serde_json::from_slice(self.bytes(locator)?).map_err(Spec034ReleaseArtifactError::Json)
    }

    pub fn digest(&self, locator: &str) -> Result<String, Spec034ReleaseArtifactError> {
        Ok(digest_bytes(self.bytes(locator)?))
    }

    pub fn artifact_digests(&self) -> Vec<DigestRow> {
        self.files
            .iter()
            .filter(|(name, _)| {
                !matches!(name.as_str(), "manifest.json" | "publication-status.json")
                    && !name.starts_with(".publication-status.")
            })
            .map(|(locator, artifact)| DigestRow {
                locator: locator.clone(),
                digest: digest_bytes(&artifact.bytes),
            })
            .collect()
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(name, artifact)| (name.as_str(), artifact.bytes.as_slice()))
    }

    pub fn publication_digest(&self) -> String {
        let mut digest = Sha256::new();
        for (name, artifact) in &self.files {
            if name == "publication-status.json" || name.starts_with(".publication-status.") {
                continue;
            }
            digest.update(name.as_bytes());
            digest.update([0]);
            digest.update(digest_bytes(&artifact.bytes).as_bytes());
            digest.update(b"\n");
        }
        format!("sha256:{:x}", digest.finalize())
    }

    pub(super) fn root_metadata(&self) -> &ArtifactMetadata {
        &self.root
    }

    pub(super) fn sealed_files(
        &self,
    ) -> impl Iterator<Item = (&str, &[u8], &ArtifactMetadata)> {
        self.files.iter().map(|(name, artifact)| {
            (
                name.as_str(),
                artifact.bytes.as_slice(),
                &artifact.metadata,
            )
        })
    }

    #[cfg(test)]
    pub(super) fn capture_for_test(
        root: &Path,
        after_root_open: impl FnOnce(),
        after_first_entry: impl FnOnce(),
        after_file_open: impl FnMut(&str),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::capture_with(root, after_root_open, after_first_entry, after_file_open)
    }
}

impl ArtifactMetadata {
    pub(super) fn capture_from(metadata: &Self) -> Self {
        metadata.clone()
    }

    #[cfg(unix)]
    pub(super) fn capture(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            links: metadata.nlink(),
            size: metadata.size(),
            ctime_seconds: metadata.ctime(),
            ctime_nanoseconds: metadata.ctime_nsec(),
        }
    }

    #[cfg(not(unix))]
    pub(super) fn capture(metadata: &Metadata) -> Self {
        use std::time::UNIX_EPOCH;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        Self {
            device: 0,
            inode: 0,
            mode: u32::from(metadata.permissions().readonly()),
            links: 0,
            size: metadata.len(),
            ctime_seconds: i64::try_from(modified.as_secs()).unwrap_or(i64::MAX),
            ctime_nanoseconds: i64::from(modified.subsec_nanos()),
        }
    }

    pub(super) const fn device(&self) -> u64 {
        self.device
    }

    pub(super) const fn inode(&self) -> u64 {
        self.inode
    }
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn write_json(
    root: &Path,
    locator: &str,
    value: &impl serde::Serialize,
) -> Result<(), Spec034ReleaseArtifactError> {
    validate_locator(locator)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(Spec034ReleaseArtifactError::Json)?;
    durable_write(&root.join(locator), &bytes)
}

pub(super) fn durable_write(
    path: &Path,
    bytes: &[u8],
) -> Result<(), Spec034ReleaseArtifactError> {
    durable_write_with(path, bytes, |file| file.sync_all())
}

fn durable_write_with(
    path: &Path,
    bytes: &[u8],
    sync: impl FnOnce(&std::fs::File) -> std::io::Result<()>,
) -> Result<(), Spec034ReleaseArtifactError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    file.write_all(bytes)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    sync(&file).map_err(Spec034ReleaseArtifactError::Io)
}

pub fn collect_digests(root: &Path) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    Ok(ArtifactSnapshot::capture(root)?.artifact_digests())
}

pub fn validate_digest_rows(
    snapshot: &ArtifactSnapshot,
    rows: &[DigestRow],
) -> Result<(), Spec034ReleaseArtifactError> {
    (snapshot.artifact_digests() == rows)
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
}

#[cfg(test)]
#[path = "artifacts_test.rs"]
mod tests;
