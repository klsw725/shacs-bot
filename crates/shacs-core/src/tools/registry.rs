use crate::tools::{JsonMap, Tool, ToolResult};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

const ERROR_HINT: &str = "\n\n[Analyze the error above and try a different approach.]";

#[derive(Clone)]
pub struct PreparedToolCall {
    pub tool: Arc<dyn Tool>,
    pub params: JsonMap,
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        self.tools.insert(tool.name().to_owned(), Arc::new(tool));
    }

    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_owned(), tool);
    }

    pub fn unregister(&mut self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.remove(name)
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn definitions(&self) -> Vec<Value> {
        let (mut builtin, mut mcp): (Vec<_>, Vec<_>) = self
            .tools
            .values()
            .map(|tool| tool.to_schema())
            .partition(|schema| !schema_name(schema).starts_with("mcp_"));
        builtin.sort_by_key(schema_name);
        mcp.sort_by_key(schema_name);
        builtin.extend(mcp);
        builtin
    }

    pub fn prepare_call(&self, name: &str, params: Value) -> Result<PreparedToolCall, String> {
        let Value::Object(params) = params else {
            if matches!(name, "write_file" | "read_file" | "edit_file") {
                return Err(format!(
                    "Error: Tool '{name}' parameters must be a JSON object. Use named parameters: tool_name(param1=\"value1\", param2=\"value2\")"
                ));
            }
            return Err(format!(
                "Error: Tool '{name}' parameters must be a JSON object"
            ));
        };

        let Some(tool) = self.get(name) else {
            return Err(format!(
                "Error: Tool '{name}' not found. Available: {}",
                self.tool_names().join(", ")
            ));
        };
        let cast_params = tool.cast_params(params);
        let errors = tool.validate_params(&cast_params);
        if errors.is_empty() {
            Ok(PreparedToolCall {
                tool,
                params: cast_params,
            })
        } else {
            Err(format!(
                "Error: Invalid parameters for tool '{name}': {}",
                errors
                    .iter()
                    .map(|error| error.render())
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        }
    }

    pub fn execute(&self, name: &str, params: Value) -> ToolResult {
        match self.prepare_call(name, params) {
            Ok(call) => {
                let result = call.tool.execute(call.params);
                match result {
                    ToolResult::Text(text) if text.starts_with("Error") => {
                        ToolResult::Text(format!("{text}{ERROR_HINT}"))
                    }
                    other => other,
                }
            }
            Err(error) => ToolResult::Text(format!("{error}{ERROR_HINT}")),
        }
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

fn schema_name(schema: &Value) -> String {
    schema
        .get("function")
        .and_then(Value::as_object)
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .or_else(|| schema.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}
