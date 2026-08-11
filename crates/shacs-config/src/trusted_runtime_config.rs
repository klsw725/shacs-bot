use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecSandboxPolicyConfig {
    #[serde(default)]
    pub fallback: ExecSandboxFallbackConfig,
    #[serde(default)]
    pub deny_read: Vec<String>,
    #[serde(default)]
    pub allow_write: Vec<String>,
    #[serde(default)]
    pub network: ExecSandboxNetworkConfig,
}

impl Default for ExecSandboxPolicyConfig {
    fn default() -> Self {
        Self {
            fallback: ExecSandboxFallbackConfig::SandboxRequired,
            deny_read: Vec::new(),
            allow_write: Vec::new(),
            network: ExecSandboxNetworkConfig::Allow,
        }
    }
}

impl ExecSandboxPolicyConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecSandboxFallbackConfig {
    TrustedNativeFallback,
    #[default]
    SandboxRequired,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecSandboxNetworkConfig {
    #[default]
    Allow,
    Deny,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedRuntimeConfig {
    #[serde(default)]
    pub resources: Vec<TrustedResourceConfig>,
    #[serde(default)]
    pub trace: TrustedTraceConfig,
}

impl TrustedRuntimeConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedResourceConfig {
    pub resource_ref: String,
    pub kind: TrustedResourceKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<TrustedJavaScriptRuntime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustedResourceKind {
    Prompt,
    Context,
    Package,
    Python,
    JavaScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustedJavaScriptRuntime {
    Node,
    Bun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedTraceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub destination: TrustedTraceDestination,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_summary: Option<String>,
}

impl Default for TrustedTraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            destination: TrustedTraceDestination::LocalOnly,
            path: None,
            exporter: None,
            endpoint_summary: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustedTraceDestination {
    #[default]
    LocalOnly,
    ConfiguredRemote,
}
