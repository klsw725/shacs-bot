use super::*;

fn fixture() -> Result<(tempfile::TempDir, PathBuf, PathBuf), Spec034ReleaseArtifactError> {
    let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let bin = root.path().join("toolchain/bin");
    let lib = root.path().join("toolchain/lib");
    let tools = root.path().join("controlled/tools");
    std::fs::create_dir_all(&bin).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::create_dir_all(&lib).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::create_dir_all(&tools).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(bin.join("rustc"), b"rustc").map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(lib.join("driver"), b"approved").map_err(Spec034ReleaseArtifactError::Io)?;
    Ok((root, bin.join("rustc"), tools))
}

#[test]
fn copied_library_mutation_breaks_closure_seal() -> Result<(), Spec034ReleaseArtifactError> {
    let (_root, rustc, tools) = fixture()?;
    let seals = prepare("rustc", &rustc, &tools)?;
    let copied = tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .join("lib/driver");
    assert!(!std::fs::symlink_metadata(&copied)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .file_type()
        .is_symlink());
    std::fs::write(&copied, b"mutated").map_err(Spec034ReleaseArtifactError::Io)?;
    assert!(seals.iter().any(|seal| seal.verify().is_err()));
    Ok(())
}

#[test]
fn original_library_a_b_a_during_copy_is_rejected() -> Result<(), Spec034ReleaseArtifactError> {
    let (root, rustc, tools) = fixture()?;
    let library = root.path().join("toolchain/lib");
    let displaced = root.path().join("library-a");
    let replacement = root.path().join("library-b");
    let result = prepare_with_hook("rustc", &rustc, &tools, || {
        assert!(std::fs::rename(&library, &displaced).is_ok());
        assert!(std::fs::create_dir(&replacement).is_ok());
        assert!(std::fs::rename(&replacement, &library).is_ok());
        assert!(std::fs::remove_dir(&library).is_ok());
        assert!(std::fs::rename(&displaced, &library).is_ok());
    });
    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}

#[test]
fn rust_source_subtree_is_not_copied_into_runtime_cache(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = tempfile::tempdir().map_err(Spec034ReleaseArtifactError::Io)?;
    let source = root.path().join("lib");
    let target = root.path().join("cached-lib");
    std::fs::create_dir_all(source.join("rustlib/src/rust"))
        .map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(source.join("rustlib/src/rust/source.rs"), b"source")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(source.join("runtime.dylib"), b"runtime")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    copy_runtime_tree(&source, &target, &source)?;
    assert!(!target.join("rustlib/src").exists());
    assert_eq!(
        std::fs::read(target.join("runtime.dylib"))
            .map_err(Spec034ReleaseArtifactError::Io)?,
        b"runtime"
    );
    Ok(())
}

#[test]
fn malicious_preseeded_runtime_tree_never_becomes_cache_baseline(
) -> Result<(), Spec034ReleaseArtifactError> {
    let (root, rustc, tools) = fixture()?;
    let target = root.path().join("controlled/lib");
    std::fs::create_dir(&target).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::write(target.join("driver"), b"forged")
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let result = prepare("rustc", &rustc, &tools);
    assert!(matches!(
        result,
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}
