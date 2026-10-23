use serde_json::Value;
use shacs_core::runtime::{
    PermissionMode, PermissionModeSnapshot, PermissionedAction, PermissionedActionInput,
    PermissionedActionOrigin, RuntimeToolCall,
};
use shacs_core::tools::{
    JsonMap, SchemaFragment, StringSchema, Tool, ToolParameters, ToolRegistry, ToolResult,
};

struct TestTool(&'static str);

impl Tool for TestTool {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "test tool"
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("command", StringSchema::new("command"))
            .property("path", StringSchema::new("path"))
            .property("url", StringSchema::new("url"))
            .property("query", StringSchema::new("query"))
            .to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Text("ok".to_owned())
    }
}

pub fn registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for name in [
        "exec",
        "read_file",
        "write_file",
        "edit_file",
        "notebook_edit",
        "list_dir",
        "glob",
        "grep",
        "web_fetch",
        "web_search",
        "mcp_server_tool_name",
    ] {
        registry.register(TestTool(name));
    }
    registry
}

pub fn action(
    registry: &ToolRegistry,
    id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> PermissionedAction {
    shacs_core::runtime::normalize_runtime_tool_call(
        registry,
        &RuntimeToolCall::new(id, tool_name, arguments),
        PermissionedActionInput {
            session_id: "session".to_owned(),
            turn_id: "turn".to_owned(),
            origin: PermissionedActionOrigin::UserTurn,
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: PermissionMode::Default,
                source: None,
                scope_ref: None,
            },
            containment_snapshot: None,
            intent_snapshot: None,
        },
    )
}
