use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn cargo_replace_restore_after_hook_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    assert_tool_replace_restore("cargo")
}

#[test]
fn rustc_replace_restore_after_hook_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    assert_tool_replace_restore("rustc")
}

fn assert_tool_replace_restore(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let tools = root.join("tools");
    let home = root.join("home");
    let cargo_home = root.join("cargo-home");
    let target = root.join("target");
    for directory in [&tools, &home, &cargo_home, &target] {
        std::fs::create_dir(directory)?;
    }
    let cargo = tools.join("cargo");
    let rustc = tools.join("rustc");
    write_tool(&cargo, "cargo 1.0.0")?;
    write_tool(&rustc, "rustc 1.0.0")?;
    let toolchain = crate::runtime::generated_media_release::tools::ResolvedToolchain::resolve_tools_for_test(
        home,
        cargo_home,
        target,
        cargo.clone(),
        rustc,
    )?;
    let attacked = tools.join(name);
    let displaced = tools.join(format!("{name}-approved"));
    let evidence = root.join("evidence");
    let mut destination = EvidenceDestination::prepare(&evidence)?;
    let staging = destination.staging()?;
    std::fs::write(staging.path().join("manifest.json"), b"validated")?;
    let approved = crate::runtime::generated_media_release::artifacts::ArtifactSnapshot::capture(
        staging.path(),
    )?;
    let staging = staging.finalize_approved_marker(
        "run",
        approved,
        FinalSourceBinding::toolchain_fixture(toolchain),
    )?;

    let result = destination.publish_with(&staging, || {
        assert!(std::fs::rename(&attacked, &displaced).is_ok());
        assert!(std::fs::copy(&displaced, &attacked).is_ok());
        assert!(std::fs::remove_file(&attacked).is_ok());
        assert!(std::fs::rename(&displaced, &attacked).is_ok());
    });

    assert!(result.is_err());
    assert!(!evidence.exists());
    Ok(())
}

fn write_tool(path: &Path, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(path, format!("#!/bin/sh\necho '{version}'\n"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
