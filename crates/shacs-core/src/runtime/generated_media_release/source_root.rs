use super::super::model::Spec034ReleaseArtifactError;
use super::super::path_chain::PathChainSeal;
use super::super::source_descriptor::{copy_tree, digest_tree, TreeKind};
use super::source_fixture::FixtureBinding;
use super::super::source_git_snapshot::GitMetadataSnapshot;
use super::super::tools::ResolvedTool;
use std::fs::File;
use std::path::{Path, PathBuf};

pub(in crate::runtime::generated_media_release) struct SourceRootContext {
    root: PathBuf,
    live_handle: File,
    live_identity: RootIdentity,
    live_digest: String,
    live_metadata_digest: String,
    controlled_handle: File,
    controlled_digest: String,
    seal: PathChainSeal,
    git: ResolvedTool,
    git_metadata: GitMetadataSnapshot,
    fixtures: FixtureBinding,
    _snapshot: tempfile::TempDir,
}

#[derive(PartialEq, Eq)]
struct RootIdentity {
    device: u64,
    inode: u64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
    mode: u32,
    links: u64,
    size: u64,
}

impl SourceRootContext {
    #[cfg(test)]
    pub(in crate::runtime::generated_media_release) fn resolve(
        root: &Path,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::with_git(root, ResolvedTool::git()?, &[])
    }

    pub(in crate::runtime::generated_media_release) fn resolve_release(
        root: &Path,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::with_git(root, ResolvedTool::git()?, &super::super::catalog::FIXTURES)
    }

    fn with_git(
        root: &Path,
        git: ResolvedTool,
        fixtures: &[&str],
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::with_git_and_hook(root, git, fixtures, || {})
    }

    fn with_git_and_hook(
        root: &Path,
        git: ResolvedTool,
        fixtures: &[&str],
        after_root_open: impl FnOnce(),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let reader = super::ConfinedSourceReader::open(root)?;
        let live_identity = RootIdentity::capture(&reader.root_metadata()?)?;
        after_root_open();
        let live_handle = reader.into_root();
        if RootIdentity::capture(&live_handle.metadata().map_err(Spec034ReleaseArtifactError::Io)?)?
            != live_identity
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let snapshot = tempfile::Builder::new()
            .prefix("shacs-spec034-repository-")
            .tempdir()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let root = snapshot
            .path()
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?
            .join("worktree");
        let live_digest = copy_tree(&live_handle, &root, TreeKind::Worktree)?;
        let live_metadata_digest = digest_tree(&live_handle, TreeKind::LiveWorktree)?;
        let fixtures = FixtureBinding::capture(&live_handle, &root, fixtures)?;
        let git_metadata = GitMetadataSnapshot::capture(&live_handle, &root)?;
        let controlled_handle = File::open(&root).map_err(Spec034ReleaseArtifactError::Io)?;
        let controlled_digest = digest_tree(&controlled_handle, TreeKind::Worktree)?;
        if controlled_digest != live_digest {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let context = Self {
            seal: PathChainSeal::capture_controlled(&root)?,
            root,
            live_handle,
            live_identity,
            live_digest,
            live_metadata_digest,
            controlled_handle,
            controlled_digest,
            git,
            git_metadata,
            fixtures,
            _snapshot: snapshot,
        };
        context.verify()?;
        Ok(context)
    }

    pub(in crate::runtime::generated_media_release) fn verify(
        &self,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.verify_identity()?;
        let live = digest_tree(&self.live_handle, TreeKind::Worktree)?;
        let live_metadata = digest_tree(&self.live_handle, TreeKind::LiveWorktree)?;
        let controlled = digest_tree(&self.controlled_handle, TreeKind::Worktree)?;
        if live != self.live_digest
            || live_metadata != self.live_metadata_digest
            || controlled != self.controlled_digest
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok(())
    }

    pub(in crate::runtime::generated_media_release) fn verify_identity(
        &self,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.seal.verify()?;
        self.git.verify()?;
        self.git_metadata.verify()?;
        self.fixtures.verify(&self.live_handle)?;
        if RootIdentity::capture(
            &self
                .live_handle
                .metadata()
                .map_err(Spec034ReleaseArtifactError::Io)?,
        )? != self.live_identity {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok(())
    }

    pub(in crate::runtime::generated_media_release) fn root(&self) -> &Path {
        &self.root
    }

    pub(in crate::runtime::generated_media_release) fn git(&self) -> &ResolvedTool {
        &self.git
    }

    pub(in crate::runtime::generated_media_release) fn git_dir(&self) -> &Path {
        self.git_metadata.directory()
    }

    pub(in crate::runtime::generated_media_release) fn binding_digest(&self) -> String {
        super::super::artifacts::digest_bytes(
            format!(
                "{}\0{}\0{}\0{}\0{}",
                self.live_digest,
                self.controlled_digest,
                self.git_metadata.source_digest(),
                self.git_metadata.controlled_digest()
                ,self.fixtures.digest()
            )
            .as_bytes(),
        )
    }

    #[cfg(test)]
    pub(super) fn with_git_for_test(
        root: &Path,
        git: ResolvedTool,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::with_git(root, git, &[])
    }

    #[cfg(test)]
    pub(super) fn with_git_hook_for_test(
        root: &Path,
        git: ResolvedTool,
        after_root_open: impl FnOnce(),
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Self::with_git_and_hook(root, git, &[], after_root_open)
    }
}

#[cfg(unix)]
impl RootIdentity {
    fn capture(metadata: &std::fs::Metadata) -> Result<Self, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
        if !metadata.is_dir() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            ctime_seconds: metadata.ctime(),
            ctime_nanoseconds: metadata.ctime_nsec(),
            mode: metadata.mode(),
            links: metadata.nlink(),
            size: metadata.size(),
        })
    }
}

#[cfg(not(unix))]
impl RootIdentity {
    fn capture(_metadata: &std::fs::Metadata) -> Result<Self, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}
