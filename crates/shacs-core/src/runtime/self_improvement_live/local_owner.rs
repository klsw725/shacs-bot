use super::local_types::digest;
use super::{LocalImprovementBlock, LocalImprovementProposal};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct LocalArtifactOwner {
    root: PathBuf,
    mutation_count: AtomicUsize,
}

impl LocalArtifactOwner {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, LocalImprovementBlock> {
        let root = root.as_ref();
        fs::create_dir_all(root).map_err(|_| LocalImprovementBlock::Io)?;
        Ok(Self {
            root: root.canonicalize().map_err(|_| LocalImprovementBlock::Io)?,
            mutation_count: AtomicUsize::new(0),
        })
    }

    pub(crate) fn read(
        &self,
        proposal: &LocalImprovementProposal,
    ) -> Result<Vec<u8>, LocalImprovementBlock> {
        fs::read(self.target(proposal.target_ref())?).map_err(|_| LocalImprovementBlock::Io)
    }

    pub fn mutation_count(&self) -> usize {
        self.mutation_count.load(Ordering::SeqCst)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn target_path(
        &self,
        proposal: &LocalImprovementProposal,
    ) -> Result<PathBuf, LocalImprovementBlock> {
        self.target(proposal.target_ref())
    }

    pub(crate) fn note_mutation(&self) {
        self.mutation_count.fetch_add(1, Ordering::SeqCst);
    }

    fn target(&self, target_ref: &str) -> Result<PathBuf, LocalImprovementBlock> {
        let relative = Path::new(target_ref);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(LocalImprovementBlock::UnsafeTarget);
        }
        let target = self.root.join(relative);
        let parent = target
            .parent()
            .ok_or(LocalImprovementBlock::UnsafeTarget)?
            .canonicalize()
            .map_err(|_| LocalImprovementBlock::UnsafeTarget)?;
        if !parent.starts_with(&self.root)
            || target
                .symlink_metadata()
                .map(|meta| meta.file_type().is_symlink())
                .unwrap_or(false)
        {
            return Err(LocalImprovementBlock::UnsafeTarget);
        }
        Ok(target)
    }
}

pub(crate) fn state_directory(root: &Path) -> Result<PathBuf, LocalImprovementBlock> {
    let state = root.join(".shacs-self-improvement");
    match fs::symlink_metadata(&state) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(LocalImprovementBlock::UnsafeTarget);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&state).map_err(|_| LocalImprovementBlock::Io)?;
        }
        Err(_) => return Err(LocalImprovementBlock::Io),
    }
    let canonical = state
        .canonicalize()
        .map_err(|_| LocalImprovementBlock::UnsafeTarget)?;
    if !canonical.starts_with(root) {
        return Err(LocalImprovementBlock::UnsafeTarget);
    }
    Ok(canonical)
}

pub(crate) fn state_path(root: &Path, name: &str) -> Result<PathBuf, LocalImprovementBlock> {
    let state = state_directory(root)?;
    let path = state.join(name);
    if fs::symlink_metadata(&path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(LocalImprovementBlock::UnsafeTarget);
    }
    Ok(path)
}

pub(crate) fn owner_evidence(
    action: &str,
    proposal: &LocalImprovementProposal,
    current: &str,
) -> String {
    digest(format!("local-owner:{action}:{}:{current}", proposal.proposal_id()).as_bytes())
}
