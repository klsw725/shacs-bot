use super::artifacts::{digest_bytes, durable_write};
use super::model::Spec034ReleaseArtifactError;
use super::source_descriptor::{
    copy_tree, digest_tree, open_anchored_directory, open_optional_directory,
    read_file, TreeKind,
};
use sha2::{Digest, Sha256};
use rustix::fs::{openat, Mode, OFlags};
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};

const MAX_CONTROL_FILE_BYTES: u64 = 1024 * 1024;
const NORMALIZED_CONFIG: &[u8] = b"[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = false\n";

pub(super) struct GitMetadataSnapshot {
    source_git: File,
    source_common: File,
    source_digest: String,
    controlled_git: File,
    controlled_digest: String,
    directory: PathBuf,
}

impl GitMetadataSnapshot {
    pub(super) fn capture(
        repository: &File,
        controlled_worktree: &Path,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let source_git = match open_git_directory(repository)? {
            Some(directory) => directory,
            None => {
                let bytes = read_file(repository, OsStr::new(".git"), MAX_CONTROL_FILE_BYTES)?
                    .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
                let path = text
                    .trim()
                    .strip_prefix("gitdir: ")
                    .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
                open_anchored_directory(repository, Path::new(path))?
            }
        };
        let source_common = match read_file(
            &source_git,
            OsStr::new("commondir"),
            MAX_CONTROL_FILE_BYTES,
        )? {
            Some(bytes) => {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
                open_anchored_directory(&source_git, Path::new(text.trim()))?
            }
            None => source_git
                .try_clone()
                .map_err(Spec034ReleaseArtifactError::Io)?,
        };
        let config_digest = config_digest(&source_common, &source_git)?;
        reject_alternates(&source_common)?;
        let directory = controlled_worktree.join(".git");
        let common_digest = copy_tree(&source_common, &directory, TreeKind::Git)?;
        let git_digest = digest_tree(&source_git, TreeKind::Git)?;
        if !same_directory(&source_git, &source_common)? {
            copy_tree(&source_git, &directory, TreeKind::Git)?;
        }
        for name in ["commondir", "gitdir", "config.worktree"] {
            match std::fs::remove_file(directory.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
            }
        }
        durable_write(&directory.join("config"), NORMALIZED_CONFIG)?;
        File::open(&directory)
            .and_then(|opened| opened.sync_all())
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let controlled_git = File::open(&directory).map_err(Spec034ReleaseArtifactError::Io)?;
        let controlled_digest = digest_tree(&controlled_git, TreeKind::ControlledGit)?;
        let source_digest = combine_digest(&combine_digest(&common_digest, &git_digest), &config_digest);
        Ok(Self {
            source_git,
            source_common,
            source_digest,
            controlled_git,
            controlled_digest,
            directory,
        })
    }

    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(super) fn source_digest(&self) -> &str {
        &self.source_digest
    }

    pub(super) fn controlled_digest(&self) -> &str {
        &self.controlled_digest
    }

    pub(super) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        let common = digest_tree(&self.source_common, TreeKind::Git)?;
        let git = digest_tree(&self.source_git, TreeKind::Git)?;
        let config = config_digest(&self.source_common, &self.source_git)?;
        if combine_digest(&combine_digest(&common, &git), &config) != self.source_digest
            || digest_tree(&self.controlled_git, TreeKind::ControlledGit)?
                != self.controlled_digest
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok(())
    }
}

fn open_git_directory(repository: &File) -> Result<Option<File>, Spec034ReleaseArtifactError> {
    let flags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    match openat(repository, OsStr::new(".git"), flags, Mode::empty()) {
        Ok(descriptor) => Ok(Some(descriptor.into())),
        Err(error) if error == rustix::io::Errno::NOTDIR => Ok(None),
        Err(_) => Err(Spec034ReleaseArtifactError::InvalidConfig),
    }
}

fn config_digest(
    common: &File,
    git: &File,
) -> Result<String, Spec034ReleaseArtifactError> {
    let mut digest = Sha256::new();
    for (directory, name) in [(common, "config"), (git, "config.worktree")] {
        digest.update(name.as_bytes());
        digest.update([0]);
        if let Some(bytes) = read_file(directory, OsStr::new(name), MAX_CONTROL_FILE_BYTES)? {
            super::source_git_config::reject_behavior_config(&bytes)?;
            digest.update(digest_bytes(&bytes).as_bytes());
        }
        digest.update([0]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn reject_alternates(common: &File) -> Result<(), Spec034ReleaseArtifactError> {
    let Some(objects) = open_optional_directory(common, OsStr::new("objects"))? else {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    };
    let Some(info) = open_optional_directory(&objects, OsStr::new("info"))? else {
        return Ok(());
    };
    match read_file(&info, OsStr::new("alternates"), MAX_CONTROL_FILE_BYTES)? {
        Some(_) => Err(Spec034ReleaseArtifactError::InvalidConfig),
        None => Ok(()),
    }
}

fn combine_digest(common: &str, git: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"spec034.git-metadata.v1\0");
    digest.update(common.as_bytes());
    digest.update([0]);
    digest.update(git.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

#[cfg(unix)]
fn same_directory(left: &File, right: &File) -> Result<bool, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let left = left.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let right = right.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(not(unix))]
fn same_directory(_left: &File, _right: &File) -> Result<bool, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}
