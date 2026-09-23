use serde_json::{json, Value};
use shacs_core::runtime::{
    AgentLoopConfig, ContainerNetworkMode, ContainerRuntimeKind, ContainmentSnapshotRef,
    DockerContainmentSnapshot, PermissionMode, PermissionRuleInput, ProcExecSummary,
};
use shacs_core::tools::{JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolResult};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct LocalProvider(pub AtomicUsize);

impl ProviderClient for LocalProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        Ok(match self.0.fetch_add(1, Ordering::SeqCst) % 3 {
            0 => LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "retry-call",
                    "exec",
                    serde_json::Map::from_iter([("command".to_owned(), json!("cargo test"))]),
                )],
                ..LlmResponse::default()
            },
            1 => LlmResponse {
                content: Some(json!({
                    "verdict": "deny_candidate", "confidence": "high", "scope_match": "requested",
                    "risk_summary": "local classifier fixture", "evidence_refs": ["classifier:test"],
                    "evaluator_ref": "classifier:test"
                }).to_string()),
                ..LlmResponse::default()
            },
            _ => LlmResponse {
                content: Some("local continuation".to_owned()),
                ..LlmResponse::default()
            },
        })
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

pub struct CountedTool {
    pub count: Arc<AtomicUsize>,
    pub fatal: bool,
}

impl Tool for CountedTool {
    fn name(&self) -> &str {
        "exec"
    }
    fn description(&self) -> &str {
        "Local counter; no process or network execution."
    }
    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", StringSchema::new("Command"))
            .required(["command"])
            .to_json_schema()
    }
    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.count.fetch_add(1, Ordering::SeqCst);
        if self.fatal {
            "(MCP tool call failed: TimeoutError)".into()
        } else {
            "local counted execution".into()
        }
    }
}

pub fn config(workspace: &std::path::Path) -> AgentLoopConfig {
    let mut config = AgentLoopConfig::new(workspace, "local-fixture");
    config.permission_mode_snapshot.mode = PermissionMode::Auto;
    config.permission_interactive = true;
    config.fail_on_tool_error = true;
    config.permission_rule_input = PermissionRuleInput {
        containment: DockerContainmentSnapshot {
            contained: Some(true),
            runtime: ContainerRuntimeKind::Docker,
            root_user: Some(false),
            privileged: Some(false),
            host_mounts_summary: Vec::new(),
            network_mode: ContainerNetworkMode::None,
            digest: Some("test-contained".to_owned()),
            summary: Some("local fixture containment input".to_owned()),
        },
        protected_targets: Vec::new(),
        proc_exec_summary: Some(ProcExecSummary {
            command_family: "test".to_owned(),
            target_refs: Vec::new(),
            destructive: false,
            network: false,
            secret_exposure: false,
            summary_available: true,
        }),
    };
    config.containment_snapshot = Some(ContainmentSnapshotRef {
        contained: Some(true),
        backend: Some("official-container".to_owned()),
        digest: Some("test-contained".to_owned()),
        summary: None,
    });
    config.permission_auto_approval = shacs_config::AutoApprovalConfig {
        enabled: true,
        allow_proc_exec_verification: false,
        ..shacs_config::AutoApprovalConfig::default()
    };
    config
}
