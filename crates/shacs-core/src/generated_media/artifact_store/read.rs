use super::{
    artifact_schema, ArtifactId, ArtifactReadStage, ArtifactStore, ArtifactStoreError,
    CommittedArtifact, GeneratedArtifactRecord, Sha256Digest, ARTIFACTS_DIR,
};
use crate::generated_media::GeneratedArtifactRef;
use cap_primitives::ambient_authority;
use cap_primitives::fs::{open, open_ambient, open_dir_nofollow, FollowSymlinks, OpenOptions};
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

impl ArtifactStore {
    pub fn read(&self, artifact_id: &ArtifactId) -> Result<CommittedArtifact, ArtifactStoreError> {
        self.read_with_observer(artifact_id, |_| {})
    }

    pub fn read_with_observer<F>(
        &self,
        artifact_id: &ArtifactId,
        mut observer: F,
    ) -> Result<CommittedArtifact, ArtifactStoreError>
    where
        F: FnMut(ArtifactReadStage),
    {
        let artifacts = open_child_directory(&self.root_handle, OsStr::new(ARTIFACTS_DIR))?;
        observer(ArtifactReadStage::BeforeArtifactDirectoryOpen);
        let artifact = open_child_directory(&artifacts, OsStr::new(artifact_id.as_str()))?;
        let bytes = read_regular_file(&artifact, OsStr::new("record.json"))?;
        let record: GeneratedArtifactRecord =
            serde_json::from_slice(&bytes).map_err(ArtifactStoreError::Json)?;
        if record.schema != artifact_schema() || record.artifact_id != *artifact_id {
            return Err(ArtifactStoreError::InvalidRecord);
        }
        read_record_payload(&artifact, &record)?;
        Ok(CommittedArtifact::new(record))
    }

    pub fn read_committed_ref(
        &self,
        artifact_ref: &GeneratedArtifactRef,
    ) -> Result<(CommittedArtifact, Vec<u8>), ArtifactStoreError> {
        let committed = self.read(&artifact_ref.artifact_id)?;
        if committed.artifact_ref() != *artifact_ref {
            return Err(ArtifactStoreError::ReferenceMismatch);
        }
        let bytes = self.read_payload(&committed)?;
        Ok((committed, bytes))
    }

    pub fn read_payload(
        &self,
        committed: &CommittedArtifact,
    ) -> Result<Vec<u8>, ArtifactStoreError> {
        let artifacts = open_child_directory(&self.root_handle, OsStr::new(ARTIFACTS_DIR))?;
        let artifact =
            open_child_directory(&artifacts, OsStr::new(committed.artifact_id.as_str()))?;
        read_record_payload(&artifact, committed.record())
    }
}

pub(super) fn open_root_handle(root: &Path) -> Result<File, ArtifactStoreError> {
    let Some(root_name) = root.file_name() else {
        let mut options = OpenOptions::new();
        options.read(true)._cap_fs_ext_follow(FollowSymlinks::No);
        let handle = open_ambient(root, &options, ambient_authority()).map_err(map_open_error)?;
        require_directory_handle(&handle)?;
        return Ok(handle);
    };
    let parent_path = root.parent().ok_or(ArtifactStoreError::InvalidStore)?;
    let parent = File::open(parent_path).map_err(ArtifactStoreError::Io)?;
    require_directory_handle(&parent)?;
    open_child_directory(&parent, root_name)
}

fn open_child_directory(parent: &File, name: &OsStr) -> Result<File, ArtifactStoreError> {
    open_dir_nofollow(parent, Path::new(name)).map_err(map_open_error)
}

fn read_regular_file(parent: &File, name: &OsStr) -> Result<Vec<u8>, ArtifactStoreError> {
    let mut options = OpenOptions::new();
    options.read(true)._cap_fs_ext_follow(FollowSymlinks::No);
    let mut file = open(parent, Path::new(name), &options).map_err(map_open_error)?;
    require_regular_handle(&file)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(ArtifactStoreError::Io)?;
    require_regular_handle(&file)?;
    Ok(bytes)
}

fn read_record_payload(
    artifact: &File,
    record: &GeneratedArtifactRecord,
) -> Result<Vec<u8>, ArtifactStoreError> {
    let payload_name = payload_name(record)?;
    let bytes = read_regular_file(artifact, payload_name)?;
    if u64::try_from(bytes.len()).map_err(|_| ArtifactStoreError::InvalidRecord)? != record.byte_len
        || Sha256Digest::from_bytes(&bytes) != record.sha256
    {
        return Err(ArtifactStoreError::DigestMismatch);
    }
    Ok(bytes)
}

fn payload_name(record: &GeneratedArtifactRecord) -> Result<&OsStr, ArtifactStoreError> {
    let expected_parent = PathBuf::from(ARTIFACTS_DIR).join(record.artifact_id.as_str());
    let path = record.media_root_relative_path.as_path();
    if path.parent() != Some(expected_parent.as_path()) {
        return Err(ArtifactStoreError::InvalidRecord);
    }
    path.file_name().ok_or(ArtifactStoreError::InvalidRecord)
}

fn require_directory_handle(file: &File) -> Result<(), ArtifactStoreError> {
    if file.metadata().map_err(ArtifactStoreError::Io)?.is_dir() {
        Ok(())
    } else {
        Err(ArtifactStoreError::InvalidStore)
    }
}

fn require_regular_handle(file: &File) -> Result<(), ArtifactStoreError> {
    if file.metadata().map_err(ArtifactStoreError::Io)?.is_file() {
        Ok(())
    } else {
        Err(ArtifactStoreError::NonRegularFile)
    }
}

fn map_open_error(error: std::io::Error) -> ArtifactStoreError {
    #[cfg(unix)]
    if matches!(
        error.raw_os_error(),
        Some(libc::ELOOP) | Some(libc::ENOTDIR)
    ) {
        return ArtifactStoreError::SymlinkRejected;
    }
    ArtifactStoreError::Io(error)
}
