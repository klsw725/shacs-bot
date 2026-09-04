use super::model::*;
use super::path_chain::PathChainSeal;
#[cfg(test)]
use super::tools::release_tempdir;
use std::path::{Component, Path, PathBuf};
use sha2::{Digest, Sha256};

#[path = "source_seal.rs"]
mod source_seal;
use source_seal::ExecutionSeal;

const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

#[path = "source_manifest.rs"]
mod source_manifest;
#[cfg(test)]
use source_manifest::nul_paths_for_test as nul_paths;

#[path = "source_reader.rs"]
mod source_reader;
pub(in crate::runtime::spec034_release) use source_reader::ConfinedSourceReader;

#[path = "source_fixture.rs"]
mod source_fixture;

#[path = "source_root.rs"]
mod source_root;
pub(in crate::runtime::spec034_release) use source_root::SourceRootContext;

#[derive(PartialEq, Eq)]
pub(super) struct SourceSnapshot {
    pub manifest: SourceManifest,
    files: Vec<(String, Vec<u8>)>,
}

pub(super) struct MaterializedSource {
    directory: PathBuf,
    _root: Option<tempfile::TempDir>,
    seal: ExecutionSeal,
    path_seal: PathChainSeal,
    remove_on_drop: bool,
}

impl MaterializedSource {
    pub fn path(&self) -> &Path {
        &self.directory
    }

    pub fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.path_seal.verify()?;
        self.seal.verify(self.path())
    }

    #[cfg(test)]
    fn task_parent_path(&self) -> &Path {
        self.directory.parent().unwrap_or_else(|| Path::new(""))
    }
}

impl Drop for MaterializedSource {
    fn drop(&mut self) {
        let _ = source_seal::set_read_only(&self.directory, false);
        if self.remove_on_drop {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

impl SourceSnapshot {
    pub fn include(
        &mut self,
        context: &SourceRootContext,
        locator: &str,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        if self.files.iter().any(|(name, _)| name == locator) {
            return Ok(());
        }
        validate_locator(locator)?;
        context.verify()?;
        let reader = ConfinedSourceReader::open(context.root())?;
        let bytes = reader
            .read(locator, MAX_SOURCE_BYTES)?
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        context.verify()?;
        self.files.push((locator.to_owned(), bytes));
        Ok(())
    }

    #[cfg(test)]
    pub fn materialize(&self) -> Result<MaterializedSource, Spec034ReleaseArtifactError> {
        let root = release_tempdir("source-root")?;
        let source_parent = root.path().join("source");
        std::fs::create_dir(&source_parent).map_err(Spec034ReleaseArtifactError::Io)?;
        let mut materialized = self.materialize_at(&source_parent)?;
        materialized._root = Some(root);
        materialized.remove_on_drop = true;
        Ok(materialized)
    }

    pub(super) fn materialize_at(
        &self,
        source_parent: &Path,
    ) -> Result<MaterializedSource, Spec034ReleaseArtifactError> {
        let materialization_digest = self.materialization_digest();
        let digest = materialization_digest
            .strip_prefix("sha256:")
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        let directory = source_parent.join(digest).join("snapshot");
        if directory.exists() {
            source_seal::set_read_only(&directory, true)?;
            let seal = ExecutionSeal::capture(&directory)?;
            if !seal.matches_files(&self.files) {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
            let path_seal = PathChainSeal::capture_controlled(&directory)?;
            return Ok(MaterializedSource {
                directory,
                _root: None,
                seal,
                path_seal,
                remove_on_drop: false,
            });
        }
        std::fs::create_dir_all(
            directory
                .parent()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?,
        )
        .map_err(Spec034ReleaseArtifactError::Io)?;
        std::fs::create_dir(&directory).map_err(Spec034ReleaseArtifactError::Io)?;
        for (locator, bytes) in &self.files {
            let path = directory.join(locator);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(Spec034ReleaseArtifactError::Io)?;
            }
            std::fs::write(path, bytes).map_err(Spec034ReleaseArtifactError::Io)?;
        }
        source_seal::set_read_only(&directory, true)?;
        let seal = ExecutionSeal::capture(&directory)?;
        let path_seal = PathChainSeal::capture_controlled(&directory)?;
        Ok(MaterializedSource {
            directory,
            _root: None,
            seal,
            path_seal,
            remove_on_drop: false,
        })
    }

    pub fn bytes(&self, locator: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(name, _)| name == locator)
            .map(|(_, bytes)| bytes.as_slice())
    }

    fn materialization_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"spec034.materialized-source.v1\0");
        digest.update(self.manifest.digest.as_bytes());
        for (locator, bytes) in &self.files {
            digest.update([0]);
            digest.update(locator.as_bytes());
            digest.update([0]);
            digest.update(super::artifacts::digest_bytes(bytes).as_bytes());
        }
        format!("sha256:{:x}", digest.finalize())
    }
}

#[cfg(test)]
pub fn collect(repo_root: &Path) -> Result<SourceManifest, Spec034ReleaseArtifactError> {
    let context = SourceRootContext::resolve(repo_root)?;
    Ok(capture_context(&context)?.manifest)
}

pub(super) fn collect_context(
    context: &SourceRootContext,
) -> Result<SourceManifest, Spec034ReleaseArtifactError> {
    Ok(source_manifest::capture_context(context)?.manifest)
}

#[cfg(test)]
pub(super) fn capture(repo_root: &Path) -> Result<SourceSnapshot, Spec034ReleaseArtifactError> {
    let context = SourceRootContext::resolve(repo_root)?;
    capture_context(&context)
}

pub(super) fn capture_context(
    context: &SourceRootContext,
) -> Result<SourceSnapshot, Spec034ReleaseArtifactError> {
    let first = source_manifest::capture_context(context)?;
    let second = source_manifest::capture_context(context)?;
    (first == second)
        .then_some(first)
        .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
}

#[cfg(test)]
fn capture_with_git_after_enumeration_for_test(
    repo_root: &Path,
    git: super::tools::ResolvedTool,
    after_enumeration: impl FnOnce(),
) -> Result<SourceSnapshot, Spec034ReleaseArtifactError> {
    let context = SourceRootContext::with_git_for_test(repo_root, git)?;
    source_manifest::capture_after_enumeration_for_test(&context, after_enumeration)
}

pub fn validate_locator(locator: &str) -> Result<(), Spec034ReleaseArtifactError> {
    if locator.is_empty()
        || Path::new(locator)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}

#[cfg(test)]
fn digest_source_for_test(
    root: &Path,
    locator: &str,
    after_ancestor_open: impl FnOnce(),
) -> Result<String, Spec034ReleaseArtifactError> {
    let reader = ConfinedSourceReader::open(root)?;
    let bytes = reader
        .read_with(locator, MAX_SOURCE_BYTES, after_ancestor_open)?
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    Ok(super::artifacts::digest_bytes(&bytes))
}

#[cfg(test)]
#[path = "source_test.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "source_ancestor_test.rs"]
mod ancestor_tests;

#[cfg(all(test, unix))]
#[path = "source_root_test.rs"]
mod root_tests;

#[cfg(all(test, unix))]
#[path = "source_snapshot_test.rs"]
mod snapshot_tests;
