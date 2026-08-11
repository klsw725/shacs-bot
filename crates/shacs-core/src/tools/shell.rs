mod execution;
mod guard;
mod process;

use crate::runtime::sandbox_adapter::{
    SandboxExecutionFact, SandboxFallbackPolicy, SandboxMountPlan, SandboxNetworkPlan,
};
use crate::runtime::trusted_runtime::Spec030FactStore;
use crate::runtime::ProcessExecutionReceipt;
use crate::tools::filesystem::PathContext;
use crate::tools::SchemaFragment;
use crate::tools::{
    IntegerSchema, JsonMap, StringSchema, Tool, ToolCallExecutionContext, ToolParameters,
    ToolResult,
};
use serde_json::Value;
use shacs_config::{ExecSandboxFallbackConfig, ExecSandboxNetworkConfig, ExecSandboxPolicyConfig};
use shacs_security::NetworkGuard;
use std::collections::BTreeMap;
use std::path::PathBuf;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TIMEOUT_SECONDS: u64 = 600;
const MAX_OUTPUT: usize = 10_000;

#[derive(Debug, Clone)]
pub struct ExecConfig {
    pub timeout_seconds: u64,
    pub working_dir: Option<PathBuf>,
    pub deny_patterns: Vec<String>,
    pub allow_patterns: Vec<String>,
    pub restrict_to_workspace: bool,
    pub sandbox: Option<String>,
    pub sandbox_fallback: SandboxFallbackPolicy,
    pub sandbox_mounts: SandboxMountPlan,
    pub sandbox_network: SandboxNetworkPlan,
    pub path_append: Option<String>,
    pub allowed_env_keys: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub path_context: PathContext,
    pub network_guard: NetworkGuard,
}

impl ExecConfig {
    pub fn new(path_context: PathContext) -> Self {
        let sandbox_mounts = SandboxMountPlan {
            deny_read: Vec::new(),
            allow_write: path_context.workspace.iter().cloned().collect(),
        };
        Self {
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            working_dir: path_context.workspace.clone(),
            deny_patterns: guard::default_deny_patterns(),
            allow_patterns: Vec::new(),
            restrict_to_workspace: false,
            sandbox: None,
            sandbox_fallback: SandboxFallbackPolicy::SandboxRequired,
            sandbox_mounts,
            sandbox_network: SandboxNetworkPlan::Allow,
            path_append: None,
            allowed_env_keys: Vec::new(),
            env: BTreeMap::new(),
            path_context,
            network_guard: NetworkGuard::default(),
        }
    }

    pub fn apply_sandbox_policy(
        &mut self,
        policy: &ExecSandboxPolicyConfig,
        workspace: &std::path::Path,
    ) {
        self.sandbox_fallback = match policy.fallback {
            ExecSandboxFallbackConfig::TrustedNativeFallback => {
                SandboxFallbackPolicy::TrustedNativeFallback
            }
            ExecSandboxFallbackConfig::SandboxRequired => SandboxFallbackPolicy::SandboxRequired,
        };
        self.sandbox_network = match policy.network {
            ExecSandboxNetworkConfig::Allow => SandboxNetworkPlan::Allow,
            ExecSandboxNetworkConfig::Deny => SandboxNetworkPlan::Deny,
        };
        self.sandbox_mounts.deny_read = policy
            .deny_read
            .iter()
            .map(|path| resolve_policy_path(workspace, path))
            .collect();
        self.sandbox_mounts.allow_write.extend(
            policy
                .allow_write
                .iter()
                .map(|path| resolve_policy_path(workspace, path)),
        );
        self.sandbox_mounts.allow_write.sort();
        self.sandbox_mounts.allow_write.dedup();
    }
}

fn resolve_policy_path(workspace: &std::path::Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecToolProcessResult {
    pub output: String,
    pub receipt: ProcessExecutionReceipt,
    pub sandbox: Option<SandboxExecutionFact>,
    pub sandbox_warning: Option<String>,
}

#[derive(Clone)]
pub struct ExecTool {
    config: ExecConfig,
    spec030_facts: Option<Spec030FactStore>,
}

impl ExecTool {
    pub fn new(config: ExecConfig) -> Self {
        Self {
            config,
            spec030_facts: None,
        }
    }

    pub fn with_workspace(workspace: impl Into<PathBuf>) -> Self {
        Self::new(ExecConfig::new(PathContext::workspace(workspace)))
    }

    pub fn with_spec030_fact_store(mut self, facts: Spec030FactStore) -> Self {
        self.spec030_facts = Some(facts);
        self
    }
}

impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout/stderr plus exit code. Output is truncated at 10,000 chars."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", StringSchema::new("The shell command to execute"))
            .property(
                "working_dir",
                StringSchema::new("Optional working directory for the command"),
            )
            .property(
                "timeout",
                IntegerSchema::new("Timeout in seconds (default 60, max 600)")
                    .minimum(1)
                    .maximum(MAX_TIMEOUT_SECONDS as i64),
            )
            .required(["command"])
            .to_json_schema()
    }

    fn exclusive(&self) -> bool {
        true
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.execute_with_context(params, &ToolCallExecutionContext::default())
    }

    fn execute_with_context(
        &self,
        params: JsonMap,
        context: &ToolCallExecutionContext,
    ) -> ToolResult {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if command.trim().is_empty() {
            return "Error executing command: Unknown command".into();
        }
        let working_dir = params.get("working_dir").and_then(Value::as_str);
        let timeout = params
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(self.config.timeout_seconds)
            .min(MAX_TIMEOUT_SECONDS);
        match self.execute_command(command, working_dir, timeout, context) {
            Ok(output) => output.into(),
            Err(error) => format!("Error executing command: {error}").into(),
        }
    }
}
