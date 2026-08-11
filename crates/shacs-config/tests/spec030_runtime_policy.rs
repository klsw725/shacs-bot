use shacs_config::{
    Config, ExecSandboxFallbackConfig, ExecSandboxNetworkConfig, TrustedJavaScriptRuntime,
    TrustedResourceKind, TrustedTraceDestination,
};
use std::error::Error;

#[test]
fn spec030_runtime_policy_defaults_preserve_existing_exec_behavior() -> Result<(), Box<dyn Error>> {
    // Given / When
    let config: Config = serde_json::from_str("{}")?;

    // Then
    assert_eq!(
        config.tools.exec.sandbox_policy.fallback,
        ExecSandboxFallbackConfig::SandboxRequired
    );
    assert_eq!(
        config.tools.exec.sandbox_policy.network,
        ExecSandboxNetworkConfig::Allow
    );
    assert!(config.tools.exec.sandbox_policy.deny_read.is_empty());
    assert!(config.tools.exec.sandbox_policy.allow_write.is_empty());
    assert!(config.trusted_runtime.resources.is_empty());
    assert!(!config.trusted_runtime.trace.enabled);
    Ok(())
}

#[test]
fn spec030_runtime_policy_parses_sandbox_resources_and_trace() -> Result<(), Box<dyn Error>> {
    // Given
    let raw = r#"{
      "tools": {"exec": {"sandboxPolicy": {
        "fallback": "trustedNativeFallback",
        "denyRead": ["private"],
        "allowWrite": ["output"],
        "network": "deny"
      }}},
      "trustedRuntime": {
        "resources": [{
          "resourceRef": "extension:configured",
          "kind": "javaScript",
          "path": "extension.mjs",
          "program": "node",
          "runtime": "node"
        }],
        "trace": {"enabled": true, "destination": "configuredRemote", "path": "trace.jsonl",
          "exporter": "otlp", "endpointSummary": "https://collector.example"}
      }
    }"#;

    // When
    let config: Config = serde_json::from_str(raw)?;

    // Then
    assert_eq!(
        config.tools.exec.sandbox_policy.fallback,
        ExecSandboxFallbackConfig::TrustedNativeFallback
    );
    assert_eq!(
        config.trusted_runtime.trace.exporter.as_deref(),
        Some("otlp")
    );
    assert_eq!(
        config.trusted_runtime.trace.endpoint_summary.as_deref(),
        Some("https://collector.example")
    );
    assert_eq!(
        config.tools.exec.sandbox_policy.network,
        ExecSandboxNetworkConfig::Deny
    );
    assert_eq!(config.tools.exec.sandbox_policy.deny_read, ["private"]);
    assert_eq!(config.tools.exec.sandbox_policy.allow_write, ["output"]);
    assert_eq!(
        config.trusted_runtime.resources[0].kind,
        TrustedResourceKind::JavaScript
    );
    assert_eq!(
        config.trusted_runtime.resources[0].runtime,
        Some(TrustedJavaScriptRuntime::Node)
    );
    assert_eq!(
        config.trusted_runtime.trace.destination,
        TrustedTraceDestination::ConfiguredRemote
    );
    Ok(())
}
