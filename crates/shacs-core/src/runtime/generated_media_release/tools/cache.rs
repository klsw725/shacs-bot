use super::*;
use crate::runtime::generated_media_release::source_descriptor::{
    copy_tree, digest_tree, open_anchored_directory, read_file, TreeKind,
};
use rustix::fs::{fsync, renameat_with, RenameFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;

mod payload;
use payload::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CacheFile {
    locator: String,
    digest: String,
    mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CacheManifest {
    schema: String,
    source_closure_digest: String,
    tree_digest: String,
    files: Vec<CacheFile>,
}

pub(super) struct CacheBinding {
    pub(super) source_closure_digest: String,
    pub(super) manifest_digest: String,
    pub(super) tree_digest: String,
    root: File,
    manifest: CacheManifest,
    seal: PathChainSeal,
}

impl CacheBinding {
    pub(super) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.seal.verify()?;
        let bytes = read_file(
            &self.root,
            std::ffi::OsStr::new("cache-manifest.json"),
            4 * 1024 * 1024,
        )?
        .ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
        let manifest: CacheManifest =
            serde_json::from_slice(&bytes).map_err(Spec034ReleaseArtifactError::Json)?;
        if digest_bytes(&bytes) != self.manifest_digest
            || manifest != self.manifest
            || manifest.source_closure_digest != self.source_closure_digest
            || manifest.tree_digest != self.tree_digest
            || digest_tree(&self.root, TreeKind::Cache)? != self.tree_digest
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        verify_manifest_files(&self.root, &manifest)?;
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn resolve(
    tools: &Path,
) -> Result<(ResolvedTool, ResolvedTool, ResolvedTool, CacheBinding), Spec034ReleaseArtifactError> {
    static RUN_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run_tools = tools
        .parent()
        .and_then(Path::parent)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .join(format!(
            "run-toolchain-{}/tools",
            RUN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&run_tools).map_err(Spec034ReleaseArtifactError::Io)?;
    resolve_into(tools, &run_tools)
}

pub(super) fn resolve_into(
    cache_tools: &Path,
    run_tools: &Path,
) -> Result<(ResolvedTool, ResolvedTool, ResolvedTool, CacheBinding), Spec034ReleaseArtifactError> {
    resolve_with_hook(cache_tools, run_tools, || {})
}

fn resolve_with_hook(
    cache_tools: &Path,
    run_tools: &Path,
    after_stage: impl FnOnce(),
) -> Result<(ResolvedTool, ResolvedTool, ResolvedTool, CacheBinding), Spec034ReleaseArtifactError> {
    let final_root = cache_tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let parent = final_root
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    std::fs::create_dir_all(parent).map_err(Spec034ReleaseArtifactError::Io)?;
    let final_seal = final_root
        .exists()
        .then(|| PathChainSeal::capture_leaf(final_root))
        .transpose()?;
    let stage = tempfile::Builder::new()
        .prefix(".toolchain-stage-")
        .tempdir_in(parent)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let stage_root = stage.path().to_path_buf();
    let stage_tools = stage_root.join("tools");
    std::fs::create_dir(&stage_tools).map_err(Spec034ReleaseArtifactError::Io)?;
    let (cargo_candidates, rustc_candidates, rustdoc_candidates) = rust_tool_candidates();
    let cargo = ResolvedTool::resolve_into("cargo", cargo_candidates, &stage_tools)?;
    let rustc = ResolvedTool::resolve_into("rustc", rustc_candidates, &stage_tools)?;
    let rustdoc = ResolvedTool::resolve_into("rustdoc", rustdoc_candidates, &stage_tools)?;
    for entry in std::fs::read_dir(&stage_root).map_err(Spec034ReleaseArtifactError::Io)? {
        seal_cache_payload(&entry.map_err(Spec034ReleaseArtifactError::Io)?.path())?;
    }
    let manifest = CacheManifest::capture(&stage_root, [&cargo, &rustc, &rustdoc])?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(Spec034ReleaseArtifactError::Json)?;
    super::super::artifacts::durable_write(&stage_root.join("cache-manifest.json"), &manifest_bytes)?;
    seal_cache_payload(&stage_root)?;
    sync_tree(&stage_root)?;
    after_stage();
    if let Some(seal) = &final_seal {
        seal.verify()?;
    }
    publish_or_verify(stage, final_root, &manifest, &manifest_bytes)?;
    let cache_root = File::open(final_root).map_err(Spec034ReleaseArtifactError::Io)?;
    verify_manifest_files(&cache_root, &manifest)?;
    if digest_tree(&cache_root, TreeKind::Cache)? != manifest.tree_digest {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    let run_root = run_tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let copied_digest = copy_tree(&cache_root, run_root, TreeKind::Cache)?;
    if copied_digest != manifest.tree_digest {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    let mut rustc = ResolvedTool::resolve_into(
        "rustc",
        vec![run_tools.join("rustc")],
        run_tools,
    )?;
    let mut rustdoc = ResolvedTool::resolve_into(
        "rustdoc",
        vec![run_tools.join("rustdoc")],
        run_tools,
    )?;
    let mut cargo = ResolvedTool::resolve_into(
        "cargo",
        vec![run_tools.join("cargo")],
        run_tools,
    )?;
    cargo.reseal()?;
    rustc.reseal()?;
    rustdoc.reseal()?;
    Ok((
        cargo,
        rustc,
        rustdoc,
        CacheBinding {
            source_closure_digest: manifest.source_closure_digest.clone(),
            manifest_digest: digest_bytes(&manifest_bytes),
            tree_digest: manifest.tree_digest.clone(),
            root: cache_root,
            manifest,
            seal: PathChainSeal::capture_leaf(final_root)?,
        },
    ))
}

fn verify_manifest_files(
    root: &File,
    manifest: &CacheManifest,
) -> Result<(), Spec034ReleaseArtifactError> {
    for expected in &manifest.files {
        let locator = Path::new(&expected.locator);
        let name = locator
            .file_name()
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        let parent = locator.parent().unwrap_or_else(|| Path::new(""));
        let directory = open_anchored_directory(root, parent)?;
        let bytes = read_file(&directory, name, MAX_TOOL_BYTES)?
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
        if digest_bytes(&bytes) != expected.digest {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
    }
    Ok(())
}

impl CacheManifest {
    fn capture(
        root: &Path,
        tools: [&ResolvedTool; 3],
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let mut files = Vec::new();
        collect_files(root, root, &mut files)?;
        files.sort_by(|left, right| left.locator.cmp(&right.locator));
        let root_handle = File::open(root).map_err(Spec034ReleaseArtifactError::Io)?;
        let tree_digest = digest_tree(&root_handle, TreeKind::Cache)?;
        let mut source = Sha256::new();
        source.update(b"spec034.tool-source-closure.v1\0");
        for tool in tools {
            source.update(
                serde_json::to_vec(tool.identity()).map_err(Spec034ReleaseArtifactError::Json)?,
            );
            source.update([0]);
        }
        for file in &files {
            source.update(file.locator.as_bytes());
            source.update([0]);
            source.update(file.digest.as_bytes());
            source.update(file.mode.to_le_bytes());
        }
        Ok(Self {
            schema: "spec034.tool-cache.v3".to_owned(),
            source_closure_digest: format!("sha256:{:x}", source.finalize()),
            tree_digest,
            files,
        })
    }
}

#[cfg(test)]
#[path = "cache_test.rs"]
mod tests;
