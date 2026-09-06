use super::*;

pub(super) fn prepare(
    name: &str,
    original: &Path,
    tools: &Path,
) -> Result<Vec<PathChainSeal>, Spec034ReleaseArtifactError> {
    prepare_with_hook(name, original, tools, || {})
}

fn prepare_with_hook(
    name: &str,
    original: &Path,
    tools: &Path,
    after_copy: impl FnOnce(),
) -> Result<Vec<PathChainSeal>, Spec034ReleaseArtifactError> {
    if !matches!(name, "rustc" | "rustdoc") {
        return Ok(Vec::new());
    }
    let source = original
        .parent()
        .and_then(Path::parent)
        .map(|parent| parent.join("lib"))
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let target = tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .join("lib");
    if !source.is_dir() {
        #[cfg(test)]
        return Ok(Vec::new());
        #[cfg(not(test))]
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    let source_seal = PathChainSeal::capture_leaf(&source)?;
    let parent = target
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let verification = tempfile::Builder::new()
        .prefix(".runtime-verify-")
        .tempdir_in(parent)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let copied = verification.path().join("lib");
    copy_runtime_tree(&source, &copied, &source)?;
    if target.exists() {
        after_copy();
        source_seal.verify()?;
        if runtime_digest(&target)? != runtime_digest(&copied)? {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let mut seals = vec![source_seal];
        collect_seals(&target, &mut seals)?;
        return Ok(seals);
    }
    copy_runtime_tree(&source, &target, &source)?;
    after_copy();
    source_seal.verify()?;
    if runtime_digest(&target)? != runtime_digest(&copied)? {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    let mut seals = vec![source_seal];
    collect_seals(&target, &mut seals)?;
    Ok(seals)
}

fn runtime_digest(path: &Path) -> Result<String, Spec034ReleaseArtifactError> {
    let root = File::open(path).map_err(Spec034ReleaseArtifactError::Io)?;
    super::super::source_descriptor::digest_tree(
        &root,
        super::super::source_descriptor::TreeKind::Cache,
    )
}

fn copy_runtime_tree(
    source: &Path,
    target: &Path,
    boundary: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    std::fs::create_dir(target).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut entries = std::fs::read_dir(source)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        if source_path
            .strip_prefix(boundary)
            .is_ok_and(|relative| relative == Path::new("rustlib/src"))
        {
            continue;
        }
        let target_path = target.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&source_path)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.file_type().is_symlink() {
            let resolved = source_path
                .canonicalize()
                .map_err(Spec034ReleaseArtifactError::Io)?;
            copy_resolved(&resolved, &target_path, boundary)?;
        } else if metadata.is_dir() {
            copy_runtime_tree(&source_path, &target_path, boundary)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &target_path)?;
        } else {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
    }
    File::open(target)
        .and_then(|directory| directory.sync_all())
        .map_err(Spec034ReleaseArtifactError::Io)
}

fn copy_resolved(
    source: &Path,
    target: &Path,
    boundary: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    let metadata = std::fs::metadata(source).map_err(Spec034ReleaseArtifactError::Io)?;
    if metadata.is_dir() {
        if !source.starts_with(boundary) {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        copy_runtime_tree(source, target, boundary)
    } else if metadata.is_file() {
        copy_file(source, target)
    } else {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}

fn copy_file(source: &Path, target: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    let mut input = File::open(source).map_err(Spec034ReleaseArtifactError::Io)?;
    let before = input.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    std::io::copy(&mut input, &mut output).map_err(Spec034ReleaseArtifactError::Io)?;
    output.sync_all().map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::set_permissions(target, before.permissions())
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let after = input.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    if !super::resolved::same_file_snapshot(&before, &after) {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    Ok(())
}

fn collect_seals(
    path: &Path,
    seals: &mut Vec<PathChainSeal>,
) -> Result<(), Spec034ReleaseArtifactError> {
    seals.push(PathChainSeal::capture_leaf(path)?);
    let mut entries = std::fs::read_dir(path)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        if metadata.is_dir() {
            collect_seals(&path, seals)?;
        } else if metadata.is_file() {
            seals.push(PathChainSeal::capture_digest_leaf(&path)?);
        } else {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "runtime_libraries_test.rs"]
mod tests;
