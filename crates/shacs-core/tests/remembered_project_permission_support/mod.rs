use serde_json::{json, Map, Value};
use shacs_config::{RememberedPermissionFileStore, WorkspacePermissionId};
use shacs_core::runtime::{
    AgentLoop, AgentLoopConfig, ContextBuilder, MessageBus, PermissionMode, PermissionModeSnapshot,
    ProjectPermissionStoreConfig, SessionManager,
};
use shacs_core::tools::{JsonMap, SchemaFragment, Tool, ToolParameters, ToolRegistry, ToolResult};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest, ToolCallRequest,
};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct ProjectPermissionFixture {
    pub workspace: tempfile::TempDir,
    pub store: RememberedPermissionFileStore,
    pub workspace_id: WorkspacePermissionId,
}

pub struct MockProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
}

struct CountingTool {
    name: &'static str,
    parameter: &'static str,
    calls: Arc<AtomicUsize>,
}

impl Tool for CountingTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "count calls"
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property(
                self.parameter,
                shacs_core::tools::StringSchema::new(self.parameter),
            )
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        "executed".into()
    }
}

impl MockProvider {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

impl ProviderClient for MockProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

impl ProjectPermissionFixture {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("data");
        fs::create_dir_all(&data_dir)?;
        let workspace_id = WorkspacePermissionId::from_canonical_workspace_path(
            root.path().canonicalize()?.to_string_lossy().as_ref(),
        );
        Ok(Self {
            workspace: root,
            store: RememberedPermissionFileStore::from_path(data_dir.join("permissions.json")),
            workspace_id,
        })
    }
}

pub fn exec_tool_call_response(call_id: &str, command: &str) -> LlmResponse {
    tool_call_response(
        call_id,
        "exec",
        Map::from_iter([("command".to_owned(), json!(command))]),
    )
}

pub fn registry(calls: Arc<AtomicUsize>) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(CountingTool {
        name: "exec",
        parameter: "command",
        calls: calls.clone(),
    });
    registry.register(CountingTool {
        name: "write_file",
        parameter: "path",
        calls,
    });
    registry
}

pub fn runtime_with_project_permissions<'a>(
    workspace: &Path,
    bus: MessageBus,
    registry: &'a ToolRegistry,
    client: &'a dyn ProviderClient,
    store_path: PathBuf,
    workspace_id: WorkspacePermissionId,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    runtime_with_project_permissions_interactive(
        workspace,
        bus,
        registry,
        client,
        store_path,
        workspace_id,
        true,
    )
}

pub fn runtime_with_project_permissions_interactive<'a>(
    workspace: &Path,
    bus: MessageBus,
    registry: &'a ToolRegistry,
    client: &'a dyn ProviderClient,
    store_path: PathBuf,
    workspace_id: WorkspacePermissionId,
    interactive: bool,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.permission_mode_snapshot = PermissionModeSnapshot {
        mode: PermissionMode::Auto,
        source: Some("test".to_owned()),
        scope_ref: None,
    };
    config.permission_interactive = interactive;
    config.project_permission_store = Some(ProjectPermissionStoreConfig {
        store_path,
        workspace_id,
    });
    Ok(AgentLoop::new(
        bus,
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        registry,
        client,
        config,
    ))
}

fn tool_call_response(call_id: &str, name: &str, arguments: Map<String, Value>) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(call_id, name, arguments)],
        ..LlmResponse::default()
    }
}

fn provider_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
