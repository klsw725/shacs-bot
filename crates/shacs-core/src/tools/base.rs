use crate::controlled_child::ControlledChildAbort;
use crate::runtime::{ProcessAdapterKind, ProcessGateInput};
use serde_json::{json, Map, Value};
use shacs_providers::ProviderInvocation;

mod casting;
mod validation;

pub use casting::cast_value;
pub use validation::validate_json_schema_value;

pub type JsonMap = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn render(&self) -> String {
        if self.path.is_empty() {
            self.message.clone()
        } else {
            format!("{} {}", self.path, self.message)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolResult {
    Text(String),
    Json(Value),
    AskUserInterrupt {
        question: String,
        options: Vec<String>,
    },
}

impl ToolResult {
    pub fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Json(value) => value.to_string(),
            Self::AskUserInterrupt { question, options } => {
                if options.is_empty() {
                    question
                } else {
                    format!("{question}\n{}", options.join("\n"))
                }
            }
        }
    }
}

impl From<String> for ToolResult {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ToolResult {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ToolCallExecutionContext {
    pub(crate) process_gate_input: Option<ProcessGateInput>,
    pub(crate) process_abort: Option<ControlledChildAbort>,
    provider_invocation: Option<ProviderInvocation>,
}

impl ToolCallExecutionContext {
    pub fn new(process_gate_input: Option<ProcessGateInput>) -> Self {
        Self {
            process_gate_input,
            process_abort: None,
            provider_invocation: None,
        }
    }

    pub fn with_process_abort(mut self, process_abort: ControlledChildAbort) -> Self {
        self.process_abort = Some(process_abort);
        self
    }

    pub fn with_provider_invocation(mut self, provider_invocation: ProviderInvocation) -> Self {
        self.provider_invocation = Some(provider_invocation);
        self
    }

    pub fn provider_invocation(&self) -> Option<&ProviderInvocation> {
        self.provider_invocation.as_ref()
    }
}

impl ToolDefinition {
    pub fn to_openai_schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn parameters(&self) -> Value;

    fn read_only(&self) -> bool {
        false
    }

    fn exclusive(&self) -> bool {
        false
    }

    fn concurrency_safe(&self) -> bool {
        self.read_only() && !self.exclusive()
    }

    fn process_adapter_kind(&self) -> Option<ProcessAdapterKind> {
        None
    }

    fn execute(&self, params: JsonMap) -> ToolResult;

    fn execute_with_context(
        &self,
        params: JsonMap,
        _context: &ToolCallExecutionContext,
    ) -> ToolResult {
        self.execute(params)
    }

    fn cast_params(&self, params: JsonMap) -> JsonMap {
        let schema = self.parameters();
        if schema.get("type") != Some(&Value::String("object".to_owned())) {
            return params;
        }

        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        params
            .into_iter()
            .map(|(key, value)| {
                let cast = properties
                    .get(&key)
                    .map_or(value.clone(), |fragment| cast_value(value, fragment));
                (key, cast)
            })
            .collect()
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        let schema = self.parameters();
        validate_json_schema_value(&Value::Object(params.clone()), &schema, "")
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_owned(),
            description: self.description().to_owned(),
            parameters: self.parameters(),
        }
    }

    fn to_schema(&self) -> Value {
        self.definition().to_openai_schema()
    }
}
