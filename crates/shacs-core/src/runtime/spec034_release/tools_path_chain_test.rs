use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn original_cargo_and_rustc_swap_does_not_change_controlled_snapshots(
) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["cargo", "rustc"] {
        let root = tempfile::tempdir()?;
        let path = root.path().join(name);
        let displaced = root.path().join(format!("{name}.original"));
        std::fs::write(&path, format!("#!/bin/sh\necho '{name} original'\n"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        let resolved = ResolvedTool::resolve_for_test(name, vec![path.clone()])?;

        std::fs::rename(&path, &displaced)?;
        std::fs::write(&path, format!("#!/bin/sh\necho '{name} replacement'\n"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;

        resolved.verify()?;
        let output = resolved.command(Path::new(".")).arg("--version").output()?;
        assert!(String::from_utf8(output.stdout)?.contains("original"));
    }
    Ok(())
}

#[test]
fn cargo_and_rustc_ancestor_a_b_a_break_path_chain_seals(
) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["cargo", "rustc"] {
        let root = tempfile::tempdir()?;
        let ancestor = root.path().join("toolchain");
        let displaced = root.path().join("toolchain-a");
        let replacement = root.path().join("toolchain-b");
        std::fs::create_dir_all(ancestor.join("bin"))?;
        std::fs::write(ancestor.join("bin").join(name), name.as_bytes())?;
        let sealed = super::super::path_chain::PathChainSeal::capture(
            &ancestor.join("bin").join(name),
            true,
        )?;

        std::fs::rename(&ancestor, &displaced)?;
        std::fs::create_dir_all(replacement.join("bin"))?;
        std::fs::write(replacement.join("bin").join(name), name.as_bytes())?;
        std::fs::rename(&replacement, &ancestor)?;
        std::fs::remove_dir_all(&ancestor)?;
        std::fs::rename(&displaced, &ancestor)?;

        assert!(matches!(sealed.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
    }
    Ok(())
}

#[test]
fn original_git_swap_does_not_change_controlled_snapshot(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let toolchain = root.path().join("toolchain");
    let displaced = root.path().join("toolchain-a");
    let replacement = root.path().join("toolchain-b");
    std::fs::create_dir_all(toolchain.join("bin"))?;
    let git = toolchain.join("bin/git");
    std::fs::write(&git, b"#!/bin/sh\necho 'git version fixture'\n")?;
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700))?;
    let resolved = ResolvedTool::resolve_for_test("git", vec![git])?;

    std::fs::rename(&toolchain, &displaced)?;
    std::fs::create_dir_all(replacement.join("bin"))?;
    std::fs::write(replacement.join("bin/git"), b"fake")?;
    std::fs::rename(&replacement, &toolchain)?;
    std::fs::remove_dir_all(&toolchain)?;
    std::fs::rename(&displaced, &toolchain)?;

    resolved.verify()?;
    let output = resolved.command(Path::new(".")).arg("--version").output()?;
    assert!(String::from_utf8(output.stdout)?.contains("fixture"));
    Ok(())
}

#[test]
fn controlled_root_entry_a_b_a_breaks_full_ancestor_metadata_seal(
) -> Result<(), Box<dyn std::error::Error>> {
    let toolchain = ResolvedToolchain::resolve()?;
    let controlled = toolchain.home.parent().ok_or("controlled root")?;
    let marker = controlled.join("marker");

    std::fs::write(&marker, b"mutation")?;
    std::fs::remove_file(&marker)?;

    assert!(matches!(toolchain.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}
