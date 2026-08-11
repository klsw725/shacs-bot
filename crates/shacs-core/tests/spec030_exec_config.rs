use shacs_config::{ExecSandboxFallbackConfig, ExecSandboxNetworkConfig, ExecSandboxPolicyConfig};
use shacs_core::runtime::sandbox_adapter::{SandboxFallbackPolicy, SandboxNetworkPlan};
use shacs_core::tools::{ExecConfig, PathContext};
use std::error::Error;

#[test]
fn production_exec_config_applies_fallback_mount_and_network_policy() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(workspace.join("private"))?;
    std::fs::create_dir_all(workspace.join("output"))?;
    let policy = ExecSandboxPolicyConfig {
        fallback: ExecSandboxFallbackConfig::TrustedNativeFallback,
        deny_read: vec!["private".to_owned()],
        allow_write: vec!["output".to_owned()],
        network: ExecSandboxNetworkConfig::Deny,
    };
    let mut config = ExecConfig::new(PathContext::workspace(&workspace));

    // When
    config.apply_sandbox_policy(&policy, &workspace);

    // Then
    assert_eq!(
        config.sandbox_fallback,
        SandboxFallbackPolicy::TrustedNativeFallback
    );
    assert_eq!(config.sandbox_network, SandboxNetworkPlan::Deny);
    assert_eq!(config.sandbox_mounts.deny_read, [workspace.join("private")]);
    assert!(config
        .sandbox_mounts
        .allow_write
        .contains(&workspace.join("output")));
    assert!(config.sandbox_mounts.allow_write.contains(&workspace));
    Ok(())
}

#[test]
fn production_exec_config_keeps_required_allow_defaults() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let mut config = ExecConfig::new(PathContext::workspace(root.path()));

    // When
    config.apply_sandbox_policy(&ExecSandboxPolicyConfig::default(), root.path());

    // Then
    assert_eq!(
        config.sandbox_fallback,
        SandboxFallbackPolicy::SandboxRequired
    );
    assert_eq!(config.sandbox_network, SandboxNetworkPlan::Allow);
    assert!(config.sandbox_mounts.deny_read.is_empty());
    Ok(())
}
