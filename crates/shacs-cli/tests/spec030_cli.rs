use serde_json::Value;
use std::error::Error;
use std::process::Command;

#[test]
fn trusted_runtime_cli_json_reports_unavailable_when_api_owner_is_down(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    let config = root.path().join("config.json");
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(&config, "{}")?;
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "runtime",
            "trusted-runtime",
            "--config",
            config.to_string_lossy().as_ref(),
            "--workspace",
            workspace.to_string_lossy().as_ref(),
            "--format",
            "json",
        ])
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(body["availability"], "unavailable");
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["unavailableReason"], "ownerUnavailable");
    Ok(())
}

#[test]
fn trusted_runtime_cli_accepts_workspace_before_command() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args([
            "--workspace",
            workspace.to_string_lossy().as_ref(),
            "runtime",
            "trusted-runtime",
            "--format",
            "json",
        ])
        .output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(body["availability"], "unavailable");
    Ok(())
}

#[test]
fn cli_help_lists_the_trusted_runtime_inspection_surface() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)?.contains("trusted-runtime"));
    Ok(())
}
