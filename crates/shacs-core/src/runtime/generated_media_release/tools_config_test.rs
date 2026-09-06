use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn cargo_config_replace_restore_blocks_command_evidence(
) -> Result<(), Box<dyn std::error::Error>> {
    let toolchain = ResolvedToolchain::resolve()?;
    let config = toolchain.cargo_home_path().join("config.toml");
    let displaced = toolchain.cargo_home_path().join("config.approved");
    let approved = std::fs::read(&config)?;
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600))?;

    std::fs::rename(&config, &displaced)?;
    std::fs::write(&config, b"[build]\nrustc-wrapper='/malicious'\n")?;
    std::fs::remove_file(&config)?;
    std::fs::rename(&displaced, &config)?;
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o400))?;

    assert_eq!(std::fs::read(&config)?, approved);
    assert!(matches!(
        toolchain.command(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}

#[test]
fn cargo_config_absence_and_alternate_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let toolchain = ResolvedToolchain::resolve()?;
    let legacy = toolchain.cargo_home_path().join("config");
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o600))?;
    std::fs::remove_file(&legacy)?;
    assert!(toolchain.verify().is_err());
    drop(toolchain);

    let toolchain = ResolvedToolchain::resolve()?;
    std::fs::write(toolchain.cargo_home_path().join("config.local"), b"[build]\n")?;
    assert!(toolchain.verify().is_err());
    Ok(())
}
